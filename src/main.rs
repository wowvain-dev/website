mod portfolio;

use std::{sync::Arc, time::Duration};

use axum::{
    Json, Router,
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
    routing::get,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use dotenvy::dotenv;
use portfolio::{Identity, Project, fallback_project_data, identity_data, synced_project_data};
use serde::Serialize;
use tokio::sync::{Mutex, RwLock};
use tower_http::services::{ServeDir, ServeFile};
use std::{
    env,
    net::{IpAddr, SocketAddr},
};

#[derive(Clone)]
struct AppState {
    identity: Identity,
    projects: Arc<ProjectCache>,
    allowed_refresh_ip: Option<IpAddr>,
}

struct ProjectCache {
    projects: RwLock<Vec<Project>>,
    last_refresh: RwLock<Option<DateTime<Utc>>>,
    last_error: RwLock<Option<String>>,
    refresh_lock: Mutex<()>,
}

#[derive(Serialize)]
struct RefreshResponse {
    ok: bool,
    refreshed: bool,
    project_count: usize,
    last_refresh: Option<String>,
    error: Option<String>,
}

impl ProjectCache {
    fn new(
        initial_projects: Vec<Project>,
        initial_refresh: Option<DateTime<Utc>>,
        initial_error: Option<String>,
    ) -> Self {
        Self {
            projects: RwLock::new(initial_projects),
            last_refresh: RwLock::new(initial_refresh),
            last_error: RwLock::new(initial_error),
            refresh_lock: Mutex::new(()),
        }
    }

    async fn snapshot(&self) -> Vec<Project> {
        self.projects.read().await.clone()
    }

    async fn refresh(&self, force: bool) -> RefreshResponse {
        let _refresh_guard = self.refresh_lock.lock().await;
        let now = Utc::now();

        if !force {
            let last = *self.last_refresh.read().await;
            if let Some(last_refresh) = last {
                if now.signed_duration_since(last_refresh) < ChronoDuration::hours(24) {
                    let project_count = self.projects.read().await.len();
                    let error = self.last_error.read().await.clone();
                    return RefreshResponse {
                        ok: error.is_none(),
                        refreshed: false,
                        project_count,
                        last_refresh: Some(last_refresh.to_rfc3339()),
                        error,
                    };
                }
            }
        }

        match synced_project_data().await {
            Ok(next_projects) => {
                let project_count = next_projects.len();
                *self.projects.write().await = next_projects;
                *self.last_refresh.write().await = Some(now);
                *self.last_error.write().await = None;
                RefreshResponse {
                    ok: true,
                    refreshed: true,
                    project_count,
                    last_refresh: Some(now.to_rfc3339()),
                    error: None,
                }
            }
            Err(error) => {
                *self.last_error.write().await = Some(error.clone());
                let project_count = self.projects.read().await.len();
                let last_refresh = self.last_refresh.read().await.map(|stamp| stamp.to_rfc3339());
                RefreshResponse {
                    ok: false,
                    refreshed: false,
                    project_count,
                    last_refresh,
                    error: Some(error),
                }
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let _ = dotenv();
    let allowed_refresh_ip = load_allowed_refresh_ip();

    let (initial_projects, initial_refresh, initial_error) = match synced_project_data().await {
        Ok(projects) => (projects, Some(Utc::now()), None),
        Err(error) => {
            eprintln!("Initial GitHub sync failed, using manual projects only: {error}");
            (fallback_project_data(), None, Some(error))
        }
    };

    let cache = Arc::new(ProjectCache::new(
        initial_projects,
        initial_refresh,
        initial_error,
    ));
    let cache_for_task = cache.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60 * 60 * 24));
        interval.tick().await;
        loop {
            interval.tick().await;
            let report = cache_for_task.refresh(true).await;
            if !report.ok {
                eprintln!(
                    "Scheduled project refresh failed: {}",
                    report.error.as_deref().unwrap_or("unknown error")
                );
            }
        }
    });

    let state = AppState {
        identity: identity_data(),
        projects: cache,
        allowed_refresh_ip,
    };

    let app = Router::new()
        .route("/api/identity", get(identity))
        .route("/api/projects", get(projects))
        .route("/api/projects/refresh", get(force_refresh).post(force_refresh))
        .fallback_service(
            ServeDir::new("frontend/dist")
                .append_index_html_on_directories(true)
                .not_found_service(ServeFile::new("frontend/dist/index.html")),
        )
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("failed to bind server to 127.0.0.1:3000");

    println!("Server running on http://127.0.0.1:3000");
    println!("Build frontend with: trunk build --release (from frontend/)");
    println!("Force refresh endpoint: POST /api/projects/refresh (restricted by MVPS_SERVER_IP in .env)");
    if let Some(ip) = state.allowed_refresh_ip {
        println!("Force refresh allowed IP: {ip}");
    } else {
        eprintln!("Force refresh disabled: MVPS_SERVER_IP is missing or invalid");
    }

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
        .await
        .expect("server terminated unexpectedly");
}

async fn identity(State(state): State<AppState>) -> Json<Identity> {
    Json(state.identity)
}

async fn projects(State(state): State<AppState>) -> Json<Vec<Project>> {
    Json(state.projects.snapshot().await)
}

async fn force_refresh(
    State(state): State<AppState>,
    ConnectInfo(source_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> (StatusCode, Json<RefreshResponse>) {
    let project_count = state.projects.projects.read().await.len();
    let last_refresh = state
        .projects
        .last_refresh
        .read()
        .await
        .map(|stamp| stamp.to_rfc3339());
    let request_ip = resolve_request_ip(&headers, source_addr);

    let Some(allowed_ip) = state.allowed_refresh_ip else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(RefreshResponse {
                ok: false,
                refreshed: false,
                project_count,
                last_refresh,
                error: Some(String::from(
                    "refresh denied: MVPS_SERVER_IP is not configured on server",
                )),
            }),
        );
    };

    if request_ip != allowed_ip {
        return (
            StatusCode::FORBIDDEN,
            Json(RefreshResponse {
                ok: false,
                refreshed: false,
                project_count,
                last_refresh,
                error: Some(format!(
                    "refresh denied: request source {request_ip} is not allowed"
                )),
            }),
        );
    }

    let result = state.projects.refresh(true).await;
    let status = if result.ok {
        StatusCode::OK
    } else {
        StatusCode::BAD_GATEWAY
    };
    (status, Json(result))
}

fn load_allowed_refresh_ip() -> Option<IpAddr> {
    let Ok(raw) = env::var("MVPS_SERVER_IP") else {
        return None;
    };
    let candidate = raw.trim();
    if candidate.is_empty() {
        return None;
    }
    match candidate.parse::<IpAddr>() {
        Ok(ip) => Some(ip),
        Err(error) => {
            eprintln!("Invalid MVPS_SERVER_IP '{candidate}': {error}");
            None
        }
    }
}

fn resolve_request_ip(headers: &HeaderMap, source_addr: SocketAddr) -> IpAddr {
    for header_name in ["x-forwarded-for", "x-real-ip"] {
        let Some(raw) = headers.get(header_name).and_then(|value| value.to_str().ok()) else {
            continue;
        };
        let Some(first) = raw.split(',').next().map(str::trim) else {
            continue;
        };
        if let Ok(ip) = first.parse::<IpAddr>() {
            return ip;
        }
    }

    source_addr.ip()
}
