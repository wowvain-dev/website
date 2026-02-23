use serde::Serialize;

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

#[derive(Clone, Copy, Serialize)]
pub struct Project {
    pub name: &'static str,
    pub owner: &'static str,
    pub url: &'static str,
    pub description: &'static str,
    pub primary_stack: &'static str,
    pub team: ProjectTeam,
    pub context: ProjectContext,
    pub era: ProjectEra,
    pub featured: bool,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectTeam {
    Solo,
    Team,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectContext {
    Personal,
    University,
    Professional,
}

#[derive(Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectEra {
    Current,
    Legacy,
}

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
        scope_note: "Project set is curated from wowvain-dev and KaaldurSoftworks repositories.",
        snapshot_date: "2026-02-20",
    }
}

const PROJECTS: [Project; 8] = [
    Project {
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
    Project {
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
    Project {
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
    Project {
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
    Project {
        name: "othello_nasm",
        owner: "wowvain-dev",
        url: "https://github.com/wowvain-dev/othello-nasm",
        description: "Terminal Othello implementation in NASM with ncurses.",
        primary_stack: "Assembly",
        team: ProjectTeam::Solo,
        context: ProjectContext::Personal,
        era: ProjectEra::Current,
        featured: true,
    },
    Project {
        name: "tebo",
        owner: "wowvain-dev",
        url: "https://github.com/wowvain-dev/TeBo",
        description: "Interactive educational desktop application for children with learning difficulties.",
        primary_stack: "TypeScript + Electron + React",
        team: ProjectTeam::Team,
        context: ProjectContext::University,
        era: ProjectEra::Legacy,
        featured: true,
    },
    Project {
        name: "vainos_config",
        owner: "wowvain-dev",
        url: "https://github.com/wowvain-dev/vainos-config",
        description: "NixOS configuration repository with flakes and home-manager progression.",
        primary_stack: "Nix",
        team: ProjectTeam::Solo,
        context: ProjectContext::Personal,
        era: ProjectEra::Current,
        featured: false,
    },
    Project {
        name: "db_cli-rs",
        owner: "wowvain-dev",
        url: "https://github.com/wowvain-dev/db_cli-rs",
        description: "Playing around with a rust TUI library. It's a primitive file I/O example.",
        primary_stack: "Rust",
        team: ProjectTeam::Solo,
        context: ProjectContext::Personal,
        era: ProjectEra::Legacy,
        featured: false,
    },
];

pub fn project_data() -> Vec<Project> {
    PROJECTS.to_vec()
}
