use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Duration, Utc};
use reqwest::{
    Client,
    header::{ACCEPT, USER_AGENT},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Serialize)]
pub struct Identity {
    pub handle: &'static str,
    pub aliases: &'static [&'static str],
    pub tagline: &'static str,
    pub location: &'static str,
    pub focus: &'static [&'static str],
    pub scope_note: &'static str,
    pub snapshot_date: &'static str,
}

#[derive(Clone, Serialize)]
pub struct Project {
    pub name: String,
    pub owner: String,
    pub url: String,
    pub description: String,
    pub primary_stack: String,
    pub team: ProjectTeam,
    pub context: ProjectContext,
    pub era: ProjectEra,
    pub featured: bool,
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectTeam {
    Solo,
    Team,
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectContext {
    Personal,
    University,
    Professional,
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectEra {
    Current,
    Legacy,
}

#[derive(Clone, Copy)]
struct ManualProject {
    name: &'static str,
    owner: &'static str,
    url: &'static str,
    description: &'static str,
    primary_stack: &'static str,
    team: ProjectTeam,
    context: ProjectContext,
    era: ProjectEra,
    featured: bool,
}

#[derive(Debug, Deserialize)]
struct GithubRepoOwner {
    login: String,
}

#[derive(Debug, Deserialize)]
struct GithubRepo {
    name: String,
    owner: GithubRepoOwner,
    html_url: String,
    description: Option<String>,
    language: Option<String>,
    pushed_at: String,
    #[serde(default)]
    topics: Vec<String>,
    #[serde(default)]
    private: bool,
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    fork: bool,
}

const SYNC_OWNERS: [&str; 2] = ["KaaldurSoftworks", "wowvain-dev"];
const LEGACY_THRESHOLD_YEARS: i64 = 2;
const DROP_THRESHOLD_YEARS: i64 = 4;

// Keep private/non-indexed projects here so they are always visible.
const MANUAL_PROJECTS: [ManualProject; 4] = [
    ManualProject {
        name: "albita_engine",
        owner: "KaaldurSoftworks",
        url: "",
        description: "Data-driven Odin game engine with a SOKOL backend and editor tooling.",
        primary_stack: "Odin",
        team: ProjectTeam::Solo,
        context: ProjectContext::Professional,
        era: ProjectEra::Current,
        featured: true,
    },
    ManualProject {
        name: "threnody_of_souls",
        owner: "KaaldurSoftworks",
        url: "",
        description: "Threnody of Souls is an isometric ARPG built as a roguelike with deep systems inspired by Path of Exile and Diablo 2.",
        primary_stack: "C# + Godot",
        team: ProjectTeam::Solo,
        context: ProjectContext::Professional,
        era: ProjectEra::Current,
        featured: true,
    },
    ManualProject {
        name: "net_note",
        owner: "wowvain-dev",
        url: "",
        description: "Distributed note-taking platform built in collaboration with a small team of students at TU Delft.",
        primary_stack: "Java + Spring Boot + JavaFX + WebSockets + REST",
        team: ProjectTeam::Team,
        context: ProjectContext::University,
        era: ProjectEra::Current,
        featured: true,
    },
    ManualProject {
        name: "cpu_raytracer",
        owner: "wowvain-dev",
        url: "",
        description: "CPU-based raytracer implemented in C++ with OpenGL. Built mainly for testing theoretical concepts studied in Computer Graphics.",
        primary_stack: "C++ + OpenGL",
        team: ProjectTeam::Solo,
        context: ProjectContext::University,
        era: ProjectEra::Current,
        featured: true,
    },
];

pub fn identity_data() -> Identity {
    Identity {
        handle: "thewowvain",
        aliases: &["kellenth", "vain"],
        tagline: "games + game engines + systems programming",
        location: "Delft, Netherlands",
        focus: &[
            "game engine architecture",
            "graphics programming",
            "systems-level tooling",
            "linux-centric dev environment",
        ],
        scope_note: "Private/manual projects are pinned, public projects are synced daily from wowvain-dev and KaaldurSoftworks.",
        snapshot_date: "auto-refresh (daily)",
    }
}

pub fn fallback_project_data() -> Vec<Project> {
    manual_project_data()
}

pub async fn synced_project_data() -> Result<Vec<Project>, String> {
    let manual = manual_project_data();
    let dynamic = fetch_dynamic_projects().await?;
    Ok(merge_projects(manual, dynamic))
}

fn manual_project_data() -> Vec<Project> {
    MANUAL_PROJECTS
        .iter()
        .map(|entry| Project {
            name: entry.name.to_string(),
            owner: entry.owner.to_string(),
            url: entry.url.to_string(),
            description: entry.description.to_string(),
            primary_stack: entry.primary_stack.to_string(),
            team: entry.team,
            context: entry.context,
            era: entry.era,
            featured: entry.featured,
        })
        .collect::<Vec<_>>()
}

fn merge_projects(mut manual: Vec<Project>, dynamic: Vec<Project>) -> Vec<Project> {
    let mut seen = HashSet::new();
    for project in &manual {
        seen.insert(project_key(project));
    }

    for project in dynamic {
        if seen.insert(project_key(&project)) {
            manual.push(project);
        }
    }

    manual.sort_by(|left, right| {
        right
            .featured
            .cmp(&left.featured)
            .then_with(|| left.owner.to_ascii_lowercase().cmp(&right.owner.to_ascii_lowercase()))
            .then_with(|| left.name.to_ascii_lowercase().cmp(&right.name.to_ascii_lowercase()))
    });
    manual
}

fn project_key(project: &Project) -> String {
    format!(
        "{}:{}",
        project.owner.to_ascii_lowercase(),
        project.name.to_ascii_lowercase()
    )
}

async fn fetch_dynamic_projects() -> Result<Vec<Project>, String> {
    let client = Client::builder()
        .build()
        .map_err(|error| format!("failed to create github client: {error}"))?;

    let now = Utc::now();
    let mut projects = Vec::new();
    let mut errors = Vec::new();

    for owner in SYNC_OWNERS {
        match fetch_owner_repos(&client, owner).await {
            Ok(repos) => {
                for repo in repos {
                    if let Some(project) = classify_repo(&client, repo, now).await {
                        projects.push(project);
                    }
                }
            }
            Err(error) => errors.push(format!("{owner}: {error}")),
        }
    }

    if projects.is_empty() && !errors.is_empty() {
        return Err(format!("github sync failed ({})", errors.join(" | ")));
    }

    if !errors.is_empty() {
        eprintln!("GitHub sync partial failure: {}", errors.join(" | "));
    }

    Ok(projects)
}

async fn fetch_owner_repos(client: &Client, owner: &str) -> Result<Vec<GithubRepo>, String> {
    let mut page = 1usize;
    let mut output = Vec::new();

    loop {
        let url = format!(
            "https://api.github.com/users/{owner}/repos?type=owner&sort=pushed&direction=desc&per_page=100&page={page}"
        );
        let response = client
            .get(url)
            .header(USER_AGENT, "wowvain-portfolio-sync")
            .header(ACCEPT, "application/vnd.github+json")
            .send()
            .await
            .map_err(|error| format!("request failed: {error}"))?;

        let status = response.status();
        if !status.is_success() {
            return Err(format!("github status {status}"));
        }

        let page_repos = response
            .json::<Vec<GithubRepo>>()
            .await
            .map_err(|error| format!("invalid github payload: {error}"))?;

        let fetched = page_repos.len();
        output.extend(page_repos);
        if fetched < 100 {
            break;
        }
        page += 1;
    }

    Ok(output)
}

async fn classify_repo(client: &Client, repo: GithubRepo, now: DateTime<Utc>) -> Option<Project> {
    if repo.private || repo.archived || repo.fork {
        return None;
    }
    if is_excluded_repo(repo.owner.login.as_str(), repo.name.as_str()) {
        return None;
    }

    let pushed_at = DateTime::parse_from_rfc3339(repo.pushed_at.as_str())
        .ok()?
        .with_timezone(&Utc);
    let age = now.signed_duration_since(pushed_at);
    if age > Duration::days(365 * DROP_THRESHOLD_YEARS) {
        return None;
    }

    let topics = repo
        .topics
        .iter()
        .map(|topic| topic.trim().to_ascii_lowercase())
        .filter(|topic| !topic.is_empty())
        .collect::<HashSet<_>>();

    let owner = repo.owner.login.clone();
    let team = classify_team(owner.as_str(), &topics);
    let context = classify_context(owner.as_str(), &topics);
    let era = if age >= Duration::days(365 * LEGACY_THRESHOLD_YEARS) {
        ProjectEra::Legacy
    } else {
        ProjectEra::Current
    };
    let description = resolve_repo_description(client, &repo).await;
    let primary_stack = resolve_repo_stack(client, &repo).await;

    Some(Project {
        name: repo.name,
        owner,
        url: repo.html_url,
        description,
        primary_stack,
        team,
        context,
        era,
        featured: false,
    })
}

async fn resolve_repo_description(client: &Client, repo: &GithubRepo) -> String {
    if let Some(description) = normalize_description(repo.description.as_deref().unwrap_or_default()) {
        return description;
    }

    if let Some(readme_summary) =
        fetch_repo_readme_summary(client, repo.owner.login.as_str(), repo.name.as_str()).await
    {
        return readme_summary;
    }

    String::from("Repository synced from GitHub.")
}

async fn resolve_repo_stack(client: &Client, repo: &GithubRepo) -> String {
    let primary_hint = normalize_language(repo.language.as_deref().unwrap_or_default());
    match fetch_repo_languages(client, repo.owner.login.as_str(), repo.name.as_str()).await {
        Ok(languages) if !languages.is_empty() => infer_stack_from_languages(languages),
        Ok(_) => primary_hint.unwrap_or_else(|| String::from("Unknown")),
        Err(error) => {
            eprintln!(
                "language sync failed for {}/{}: {error}",
                repo.owner.login, repo.name
            );
            primary_hint.unwrap_or_else(|| String::from("Unknown"))
        }
    }
}

async fn fetch_repo_languages(
    client: &Client,
    owner: &str,
    repo: &str,
) -> Result<Vec<(String, u64)>, String> {
    let url = format!("https://api.github.com/repos/{owner}/{repo}/languages");
    let response = client
        .get(url)
        .header(USER_AGENT, "wowvain-portfolio-sync")
        .header(ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|error| format!("request failed: {error}"))?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!("github status {status}"));
    }

    let payload = response
        .json::<HashMap<String, u64>>()
        .await
        .map_err(|error| format!("invalid github payload: {error}"))?;
    let mut languages = payload
        .into_iter()
        .filter(|(_, bytes)| *bytes > 0)
        .collect::<Vec<_>>();
    languages.sort_by(|left, right| right.1.cmp(&left.1));
    Ok(languages)
}

async fn fetch_repo_readme_summary(client: &Client, owner: &str, repo: &str) -> Option<String> {
    let url = format!("https://api.github.com/repos/{owner}/{repo}/readme");
    let response = client
        .get(url)
        .header(USER_AGENT, "wowvain-portfolio-sync")
        .header(ACCEPT, "application/vnd.github.raw+json")
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let markdown = response.text().await.ok()?;
    extract_readme_summary(markdown.as_str())
}

fn extract_readme_summary(markdown: &str) -> Option<String> {
    let mut lines = Vec::new();

    for raw_line in markdown.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            if !lines.is_empty() {
                break;
            }
            continue;
        }

        if line.starts_with('#')
            || line.starts_with("![")
            || line.starts_with("[![")
            || line.starts_with("<!")
        {
            continue;
        }

        lines.push(line);
        if lines.len() >= 4 {
            break;
        }
    }

    if lines.is_empty() {
        return None;
    }

    let joined = lines.join(" ");
    let cleaned = normalize_description(joined.as_str())?;
    Some(truncate_with_ellipsis(cleaned, 220))
}

fn truncate_with_ellipsis(text: String, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text;
    }
    let mut trimmed = text.chars().take(max_chars.saturating_sub(1)).collect::<String>();
    trimmed.push('…');
    trimmed
}

fn normalize_description(text: &str) -> Option<String> {
    let cleaned = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn normalize_language(text: &str) -> Option<String> {
    let cleaned = text.trim();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned.to_string())
    }
}

fn infer_stack_from_languages(languages: Vec<(String, u64)>) -> String {
    if languages.is_empty() {
        return String::from("Unknown");
    }

    let total_bytes = languages.iter().map(|(_, bytes)| *bytes).sum::<u64>().max(1);
    let mut selected = Vec::new();
    let mut covered = 0.0f64;

    for (index, (language, bytes)) in languages.into_iter().enumerate() {
        let ratio = bytes as f64 / total_bytes as f64;
        let include = index == 0 || (selected.len() < 3 && (ratio >= 0.12 || covered < 0.78));
        if include {
            selected.push(language);
            covered += ratio;
        }
        if selected.len() >= 3 || covered >= 0.9 {
            break;
        }
    }

    if selected.is_empty() {
        String::from("Unknown")
    } else {
        selected.join(" + ")
    }
}

fn classify_team(owner: &str, topics: &HashSet<String>) -> ProjectTeam {
    if topics.contains("team") {
        return ProjectTeam::Team;
    }
    if topics.contains("solo") {
        return ProjectTeam::Solo;
    }
    if owner.eq_ignore_ascii_case("KaaldurSoftworks") {
        return ProjectTeam::Team;
    }
    ProjectTeam::Solo
}

fn classify_context(owner: &str, topics: &HashSet<String>) -> ProjectContext {
    if topics.contains("professional") || topics.contains("work") || topics.contains("pro") {
        return ProjectContext::Professional;
    }
    if topics.contains("uni")
        || topics.contains("university")
        || topics.contains("academic")
        || topics.contains("education")
    {
        return ProjectContext::University;
    }
    if topics.contains("personal") {
        return ProjectContext::Personal;
    }
    if owner.eq_ignore_ascii_case("KaaldurSoftworks") {
        return ProjectContext::Professional;
    }
    ProjectContext::Personal
}

fn is_excluded_repo(owner: &str, repo_name: &str) -> bool {
    if !owner.eq_ignore_ascii_case("wowvain-dev") {
        return false;
    }

    matches!(
        repo_name.trim().to_ascii_lowercase().as_str(),
        "wowvai-dev" | "wowvain-dev"
    )
}
