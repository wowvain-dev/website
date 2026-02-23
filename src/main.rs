mod portfolio;

use std::sync::Arc;

use axum::{Json, Router, extract::State, routing::get};
use portfolio::{Identity, Project, identity_data, project_data};
use tower_http::services::{ServeDir, ServeFile};

#[derive(Clone)]
struct AppState {
    identity: Identity,
    projects: Arc<Vec<Project>>,
}

#[tokio::main]
async fn main() {
    let state = AppState {
        identity: identity_data(),
        projects: Arc::new(project_data()),
    };

    let app = Router::new()
        .route("/api/identity", get(identity))
        .route("/api/projects", get(projects))
        .fallback_service(
            ServeDir::new("frontend/dist")
                .append_index_html_on_directories(true)
                .not_found_service(ServeFile::new("frontend/dist/index.html")),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("failed to bind server to 127.0.0.1:3000");

    println!("Server running on http://127.0.0.1:3000");
    println!("Build frontend with: trunk build --release (from frontend/)");

    axum::serve(listener, app)
        .await
        .expect("server terminated unexpectedly");
}

async fn identity(State(state): State<AppState>) -> Json<Identity> {
    Json(state.identity)
}

async fn projects(State(state): State<AppState>) -> Json<Vec<Project>> {
    Json(state.projects.as_ref().clone())
}
