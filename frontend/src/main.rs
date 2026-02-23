use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use gloo_net::http::Request;
use gloo_timers::{callback::Interval, future::TimeoutFuture};
use serde::Deserialize;
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{
    Element, Event, HtmlElement, HtmlInputElement, HtmlTextAreaElement, KeyboardEvent, MouseEvent,
};
use yew::prelude::*;

type DirCache = HashMap<String, Vec<RepoEntry>>;
type FileCache = HashMap<String, String>;
type OverrideMap = HashMap<String, String>;
const MOBILE_NVIM_WARNING: &str = "nvim texteditor functionality is incompatible with phones";
const STUDIES_LINE: &str = "bachelor: computer science & engineering | year 2 @ tudelft";

#[derive(Clone, Debug, PartialEq, Deserialize)]
struct Identity {
    handle: String,
    aliases: Vec<String>,
    tagline: String,
    location: String,
    focus: Vec<String>,
    scope_note: String,
    snapshot_date: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
struct Project {
    name: String,
    owner: String,
    url: String,
    description: String,
    primary_stack: String,
    #[serde(default)]
    team: Option<ProjectTeam>,
    #[serde(default)]
    context: Option<ProjectContext>,
    #[serde(default)]
    source: Option<ProjectSourceLegacy>,
    era: ProjectEra,
    featured: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProjectTeam {
    Solo,
    Team,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProjectContext {
    Personal,
    University,
    Professional,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProjectSourceLegacy {
    University,
    Solo,
    Team,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProjectEra {
    Current,
    Legacy,
}

#[derive(Clone, Debug, PartialEq)]
enum LineKind {
    Command,
    Output,
    Identity,
    Error,
    Section,
    Muted,
    Ls,
    Project,
}

#[derive(Clone, Debug, PartialEq)]
struct LsToken {
    icon: String,
    name: String,
    class_name: &'static str,
    width_ch: usize,
}

#[derive(Clone, Debug, PartialEq)]
struct TerminalLine {
    kind: LineKind,
    text: String,
    ls_tokens: Vec<LsToken>,
    project: Option<Project>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ProjectFilters {
    team: Option<ProjectTeam>,
    context: Option<ProjectContext>,
    legacy_only: bool,
}

impl TerminalLine {
    fn command(text: impl Into<String>) -> Self {
        Self {
            kind: LineKind::Command,
            text: text.into(),
            ls_tokens: Vec::new(),
            project: None,
        }
    }

    fn output(text: impl Into<String>) -> Self {
        Self {
            kind: LineKind::Output,
            text: text.into(),
            ls_tokens: Vec::new(),
            project: None,
        }
    }

    fn identity(text: impl Into<String>) -> Self {
        Self {
            kind: LineKind::Identity,
            text: text.into(),
            ls_tokens: Vec::new(),
            project: None,
        }
    }

    fn error(text: impl Into<String>) -> Self {
        Self {
            kind: LineKind::Error,
            text: text.into(),
            ls_tokens: Vec::new(),
            project: None,
        }
    }

    fn section(text: impl Into<String>) -> Self {
        Self {
            kind: LineKind::Section,
            text: text.into(),
            ls_tokens: Vec::new(),
            project: None,
        }
    }

    fn muted(text: impl Into<String>) -> Self {
        Self {
            kind: LineKind::Muted,
            text: text.into(),
            ls_tokens: Vec::new(),
            project: None,
        }
    }

    fn ls(tokens: Vec<LsToken>) -> Self {
        Self {
            kind: LineKind::Ls,
            text: String::new(),
            ls_tokens: tokens,
            project: None,
        }
    }

    fn project(project: &Project) -> Self {
        Self {
            kind: LineKind::Project,
            text: String::new(),
            ls_tokens: Vec::new(),
            project: Some(project.clone()),
        }
    }
}

#[derive(Clone, PartialEq)]
struct TerminalState {
    lines: Vec<TerminalLine>,
}

enum TerminalAction {
    Append(Vec<TerminalLine>),
    Clear,
}

impl Reducible for TerminalState {
    type Action = TerminalAction;

    fn reduce(self: Rc<Self>, action: Self::Action) -> Rc<Self> {
        match action {
            TerminalAction::Append(mut new_lines) => {
                let mut lines = self.lines.clone();
                lines.append(&mut new_lines);
                Rc::new(Self { lines })
            }
            TerminalAction::Clear => Rc::new(Self { lines: Vec::new() }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum BootPhase {
    Blinking,
    Ready,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum EditorMode {
    Normal,
    Insert,
    Command,
}

#[derive(Clone, Debug, PartialEq)]
struct EditorState {
    path: Vec<String>,
    content: String,
    cursor: usize,
    preferred_column: Option<usize>,
    mode: EditorMode,
    command: String,
    status: String,
    dirty: bool,
    tree_visible: bool,
    tree_focused: bool,
    tree_dir: Vec<String>,
    tree_entries: Vec<EditorTreeEntry>,
    tree_selection: usize,
    show_keybinds_help: bool,
    leader_pending: bool,
    leader_window_pending: bool,
    g_pending: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct EditorTreeEntry {
    name: String,
    path: Vec<String>,
    depth: usize,
    is_dir: bool,
    private_repo: bool,
    expanded: bool,
    loading: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum RepoEntryKind {
    File,
    Dir,
}

#[derive(Clone, Debug, PartialEq)]
struct RepoEntry {
    name: String,
    path: String,
    kind: RepoEntryKind,
}

#[derive(Debug, Deserialize)]
struct GithubContentItem {
    name: String,
    path: String,
    #[serde(rename = "type")]
    kind: String,
    content: Option<String>,
    encoding: Option<String>,
    size: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct GithubApiError {
    message: String,
}

#[derive(Default)]
struct CommandOutcome {
    lines: Vec<TerminalLine>,
    next_cwd: Option<Vec<String>>,
    clear: bool,
    open_editor: Option<EditorState>,
}

#[derive(Default)]
struct AutocompleteOutcome {
    input: String,
    lines: Vec<TerminalLine>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LanguageKind {
    Plain,
    JavaScript,
    Cpp,
    C,
    Rust,
    Odin,
    CSharp,
    Java,
    Kotlin,
    TypeScript,
    Php,
    Zig,
    Markdown,
    Json,
    Xml,
    Html,
    Css,
    Dart,
    Toml,
    Yaml,
}

#[derive(Default)]
struct HighlightState {
    in_block_comment: bool,
    in_markdown_fence: bool,
}

#[derive(Clone)]
struct HlToken {
    class_name: &'static str,
    text: String,
}

#[function_component(App)]
fn app() -> Html {
    let is_phone_client = is_phone_device();
    let identity = use_state(|| None::<Identity>);
    let projects = use_state(Vec::<Project>::new);
    let terminal = use_reducer(|| TerminalState { lines: Vec::new() });
    let output_ref = use_node_ref();
    let prompt_input_ref = use_node_ref();
    let cwd = use_state(Vec::<String>::new);
    let boot_phase = use_state(|| BootPhase::Blinking);
    let auto_scroll_terminal = use_state(|| false);
    let font_loading = use_state(|| true);
    let prompt_spotlight = use_state(|| false);
    let phone_warning_popup_visible = use_state(|| is_phone_client);
    let cursor_on = use_state(|| true);
    let command_input = use_state(String::new);
    let completion_lines = use_state(Vec::<TerminalLine>::new);
    let busy = use_state(|| false);
    let dir_cache = use_state(HashMap::<String, Vec<RepoEntry>>::new);
    let file_cache = use_state(HashMap::<String, String>::new);
    let overrides = use_state(HashMap::<String, String>::new);
    let history = use_state(Vec::<String>::new);
    let project_filters = use_state(ProjectFilters::default);
    let editor = use_state(|| None::<EditorState>);
    let editor_ref = use_node_ref();
    let editor_highlight_ref = use_node_ref();
    let editor_gutter_ref = use_node_ref();
    let editor_tree_ref = use_node_ref();
    let editor_command_ref = use_node_ref();

    {
        let font_loading = font_loading.clone();
        use_effect_with((), move |_| {
            spawn_local(async move {
                wait_for_initial_fonts().await;
                font_loading.set(false);
            });
            || ()
        });
    }

    {
        let cursor_on = cursor_on.clone();
        use_effect(move || {
            let handle = Interval::new(520, move || {
                cursor_on.set(!*cursor_on);
            });
            move || drop(handle)
        });
    }

    {
        let identity = identity.clone();
        let projects = projects.clone();
        let terminal = terminal.clone();
        let boot_phase = boot_phase.clone();
        let prompt_spotlight = prompt_spotlight.clone();
        let cwd = cwd.clone();

        use_effect_with((), move |_| {
            spawn_local(async move {
                let fetched_identity = match fetch_identity().await {
                    Ok(data) => data,
                    Err(error) => {
                        terminal.dispatch(TerminalAction::Append(vec![TerminalLine::error(
                            format!("identity API error: {error}"),
                        )]));
                        default_identity()
                    }
                };

                let fetched_projects = match fetch_projects().await {
                    Ok(data) => data,
                    Err(error) => {
                        terminal.dispatch(TerminalAction::Append(vec![TerminalLine::error(
                            format!("projects API error: {error}"),
                        )]));
                        Vec::new()
                    }
                };

                identity.set(Some(fetched_identity.clone()));
                projects.set(fetched_projects.clone());
                cwd.set(vec![String::from("projects")]);

                boot_phase.set(BootPhase::Ready);
                TimeoutFuture::new(420).await;
                prompt_spotlight.set(true);
            });
            || ()
        });
    }

    {
        let output_ref = output_ref.clone();
        let line_count = terminal.lines.len();
        let auto_scroll = *auto_scroll_terminal;
        use_effect_with((line_count, auto_scroll), move |_| {
            if auto_scroll {
                if let Some(element) = output_ref.cast::<HtmlElement>() {
                    element.set_scroll_top(element.scroll_height());
                }
            }
            || ()
        });
    }

    {
        let prompt_input_ref = prompt_input_ref.clone();
        let boot_ready = *boot_phase == BootPhase::Ready;
        let busy_now = *busy;
        let editor_open = editor.is_some();
        let line_count = terminal.lines.len();
        use_effect_with((boot_ready, busy_now, editor_open, line_count), move |_| {
            if boot_ready && !busy_now && !editor_open {
                set_prompt_focus(&prompt_input_ref);
            }
            || ()
        });
    }

    {
        let editor_state = (*editor).clone();
        let editor_ref = editor_ref.clone();
        let editor_tree_ref = editor_tree_ref.clone();
        let editor_command_ref = editor_command_ref.clone();
        use_effect_with(editor_state, move |state| {
            if let Some(editor) = state {
                if editor.mode == EditorMode::Command {
                    if let Some(input) = editor_command_ref.cast::<HtmlInputElement>() {
                        let _ = input.focus();
                        let len = input.value().len() as u32;
                        let _ = input.set_selection_range(len, len);
                    }
                } else if editor.tree_visible && editor.tree_focused {
                    if let Some(tree) = editor_tree_ref.cast::<HtmlElement>() {
                        let _ = tree.focus();
                    }
                } else {
                    if let Some(area) = editor_ref.cast::<HtmlTextAreaElement>() {
                        let _ = area.focus();
                        let pos = editor.cursor.min(area.value().len()) as u32;
                        let _ = area.set_selection_range(pos, pos);
                    }
                }
            }
            || ()
        });
    }

    {
        let editor_state = (*editor).clone();
        let editor_tree_ref = editor_tree_ref.clone();
        use_effect_with(editor_state, move |state| {
            if let Some(editor_state) = state {
                if editor_state.tree_visible && editor_state.tree_focused {
                    if let Some(tree_list) = editor_tree_ref.cast::<HtmlElement>() {
                        let row_height = 22;
                        let selected_top = (editor_state.tree_selection as i32) * row_height;
                        let selected_bottom = selected_top + row_height;
                        let viewport_top = tree_list.scroll_top();
                        let viewport_bottom = viewport_top + tree_list.client_height();
                        if selected_top < viewport_top {
                            tree_list.set_scroll_top(selected_top);
                        } else if selected_bottom > viewport_bottom {
                            tree_list.set_scroll_top(selected_bottom - tree_list.client_height());
                        }
                    }
                }
            }
            || ()
        });
    }

    let oninput = {
        let command_input = command_input.clone();
        let completion_lines = completion_lines.clone();
        let prompt_spotlight = prompt_spotlight.clone();
        Callback::from(move |event: InputEvent| {
            let target: HtmlInputElement = event.target_unchecked_into();
            command_input.set(target.value());
            completion_lines.set(Vec::new());
            if *prompt_spotlight {
                prompt_spotlight.set(false);
            }
        })
    };

    let onkeydown = {
        let command_input = command_input.clone();
        let boot_phase = boot_phase.clone();
        let busy = busy.clone();
        let editor = editor.clone();
        let cwd = cwd.clone();
        let projects = projects.clone();
        let dir_cache = dir_cache.clone();
        let overrides = overrides.clone();
        let completion_lines = completion_lines.clone();

        Callback::from(move |event: KeyboardEvent| {
            if event.key() != "Tab" {
                return;
            }
            event.prevent_default();

            if *boot_phase != BootPhase::Ready || *busy || editor.is_some() {
                return;
            }

            let raw = (*command_input).clone();
            if raw.trim().is_empty() {
                return;
            }

            let cwd_now = (*cwd).clone();
            let projects_data = (*projects).clone();
            let dir_cache = dir_cache.clone();
            let overrides = overrides.clone();
            let command_input = command_input.clone();
            let completion_lines = completion_lines.clone();

            spawn_local(async move {
                let completion =
                    autocomplete_input(raw, cwd_now, projects_data, dir_cache, overrides).await;
                command_input.set(completion.input);
                completion_lines.set(completion.lines);
            });
        })
    };

    let onsubmit = {
        let command_input = command_input.clone();
        let boot_phase = boot_phase.clone();
        let terminal = terminal.clone();
        let cwd = cwd.clone();
        let identity = identity.clone();
        let projects = projects.clone();
        let busy = busy.clone();
        let dir_cache = dir_cache.clone();
        let file_cache = file_cache.clone();
        let overrides = overrides.clone();
        let history = history.clone();
        let auto_scroll_terminal = auto_scroll_terminal.clone();
        let output_ref = output_ref.clone();
        let prompt_spotlight = prompt_spotlight.clone();
        let editor = editor.clone();
        let completion_lines = completion_lines.clone();
        let is_phone_client = is_phone_client;

        Callback::from(move |event: SubmitEvent| {
            event.prevent_default();

            if *boot_phase != BootPhase::Ready || *busy || editor.is_some() {
                return;
            }

            let raw = command_input.trim().to_string();
            if raw.is_empty() {
                return;
            }

            let keep_top_for_about = raw
                .split_whitespace()
                .next()
                .map(|command| command == "about")
                .unwrap_or(false);

            auto_scroll_terminal.set(!keep_top_for_about);
            if *prompt_spotlight {
                prompt_spotlight.set(false);
            }
            let command_line = format!("{}{}", format_prompt(cwd.as_ref()), raw);
            terminal.dispatch(TerminalAction::Append(vec![TerminalLine::command(
                command_line,
            )]));
            command_input.set(String::new());
            completion_lines.set(Vec::new());
            busy.set(true);

            let identity_data = identity.as_ref().cloned().unwrap_or_else(default_identity);
            let project_data = (*projects).clone();
            let cwd_now = (*cwd).clone();
            let terminal_handle = terminal.clone();
            let cwd_handle = cwd.clone();
            let busy_handle = busy.clone();
            let dir_cache = dir_cache.clone();
            let file_cache = file_cache.clone();
            let overrides = overrides.clone();
            let output_ref = output_ref.clone();
            let editor = editor.clone();

            let mut next_history = (*history).clone();
            next_history.push(raw.clone());
            history.set(next_history.clone());

            spawn_local(async move {
                let outcome = run_command(
                    raw,
                    cwd_now,
                    identity_data,
                    project_data,
                    next_history,
                    dir_cache,
                    file_cache,
                    overrides,
                    is_phone_client,
                )
                .await;

                if outcome.clear {
                    terminal_handle.dispatch(TerminalAction::Clear);
                }

                if !outcome.lines.is_empty() {
                    terminal_handle.dispatch(TerminalAction::Append(outcome.lines));
                }

                if let Some(next_cwd) = outcome.next_cwd {
                    cwd_handle.set(next_cwd);
                }

                if let Some(editor_state) = outcome.open_editor {
                    editor.set(Some(editor_state));
                }

                if keep_top_for_about {
                    TimeoutFuture::new(16).await;
                    if let Some(element) = output_ref.cast::<HtmlElement>() {
                        element.set_scroll_top(0);
                    }
                }

                busy_handle.set(false);
            });
        })
    };
    let dismiss_prompt_spotlight = {
        let prompt_spotlight = prompt_spotlight.clone();
        let prompt_input_ref = prompt_input_ref.clone();
        Callback::from(move |event: MouseEvent| {
            event.prevent_default();
            event.stop_propagation();
            prompt_spotlight.set(false);
            set_prompt_focus(&prompt_input_ref);
        })
    };
    let dismiss_phone_warning_popup = {
        let phone_warning_popup_visible = phone_warning_popup_visible.clone();
        let prompt_input_ref = prompt_input_ref.clone();
        Callback::from(move |event: MouseEvent| {
            event.prevent_default();
            event.stop_propagation();
            phone_warning_popup_visible.set(false);
            set_prompt_focus(&prompt_input_ref);
        })
    };
    let open_editor_keybinds_popup = {
        let editor = editor.clone();
        Callback::from(move |event: MouseEvent| {
            event.prevent_default();
            event.stop_propagation();
            if let Some(mut state) = (*editor).clone() {
                state.show_keybinds_help = true;
                state.status = String::from("keybinds");
                editor.set(Some(state));
            }
        })
    };
    let dismiss_editor_keybinds_popup = {
        let editor = editor.clone();
        Callback::from(move |event: MouseEvent| {
            event.prevent_default();
            event.stop_propagation();
            if let Some(mut state) = (*editor).clone() {
                state.show_keybinds_help = false;
                state.status = String::from("-- NORMAL --");
                editor.set(Some(state));
            }
        })
    };
    let on_terminal_click = {
        let prompt_input_ref = prompt_input_ref.clone();
        let boot_phase = boot_phase.clone();
        let editor = editor.clone();
        Callback::from(move |event: MouseEvent| {
            if *boot_phase != BootPhase::Ready || editor.is_some() {
                return;
            }

            let Some(target) = event.target_dyn_into::<Element>() else {
                return;
            };

            if target.closest(".filter-chip").ok().flatten().is_some() {
                return;
            }

            set_prompt_focus(&prompt_input_ref);
        })
    };

    let editor_oninput = {
        let editor = editor.clone();
        Callback::from(move |event: InputEvent| {
            let target: HtmlTextAreaElement = event.target_unchecked_into();
            let Some(mut state) = (*editor).clone() else {
                return;
            };

            if state.mode != EditorMode::Insert {
                return;
            }

            state.content = target.value();
            state.cursor = target.selection_start().ok().flatten().unwrap_or(0) as usize;
            state.dirty = true;
            state.status = String::from("-- INSERT --");
            editor.set(Some(state));
        })
    };

    let editor_onscroll = {
        let editor_highlight_ref = editor_highlight_ref.clone();
        let editor_gutter_ref = editor_gutter_ref.clone();
        Callback::from(move |event: Event| {
            let area: HtmlTextAreaElement = event.target_unchecked_into();
            if let Some(layer) = editor_highlight_ref.cast::<HtmlElement>() {
                layer.set_scroll_top(area.scroll_top());
                layer.set_scroll_left(area.scroll_left());
            }
            if let Some(gutter) = editor_gutter_ref.cast::<HtmlElement>() {
                gutter.set_scroll_top(area.scroll_top());
            }
        })
    };

    let editor_onkeydown = {
        let editor = editor.clone();
        let editor_ref = editor_ref.clone();
        let identity = identity.clone();
        let projects = projects.clone();
        let dir_cache = dir_cache.clone();
        let file_cache = file_cache.clone();
        let overrides = overrides.clone();

        Callback::from(move |event: KeyboardEvent| {
            let Some(mut state) = (*editor).clone() else {
                return;
            };

            let cursor = if let Some(area) = editor_ref.cast::<HtmlTextAreaElement>() {
                clamp_boundary(
                    state.content.as_str(),
                    area.selection_start().ok().flatten().unwrap_or(0) as usize,
                )
            } else {
                state.cursor
            };

            match state.mode {
                EditorMode::Insert => {
                    if event.key() == "Escape" {
                        event.prevent_default();
                        state.mode = EditorMode::Normal;
                        state.cursor = cursor;
                        state.status = String::from("-- NORMAL --");
                        editor.set(Some(state));
                    }
                }
                EditorMode::Normal => {
                    let key = event.key();

                    if state.show_keybinds_help {
                        if key == "Escape" || key == "?" {
                            event.prevent_default();
                            state.show_keybinds_help = false;
                            state.status = String::from("-- NORMAL --");
                            editor.set(Some(state));
                        }
                        return;
                    }

                    if key == "?" {
                        event.prevent_default();
                        state.show_keybinds_help = true;
                        state.status = String::from("keybinds");
                        editor.set(Some(state));
                        return;
                    }

                    if state.leader_window_pending {
                        event.prevent_default();
                        state.leader_window_pending = false;
                        state.g_pending = false;

                        match key.as_str() {
                            "h" | "ArrowLeft" | "k" | "ArrowUp" => {
                                state.tree_visible = true;
                                state.tree_focused = true;
                                state.status = String::from("NvimTree: focused");

                                if state.tree_entries.is_empty() {
                                    let tree_dir = state.tree_dir.clone();
                                    state.status =
                                        format!("NvimTree: loading {}", display_path(&tree_dir));
                                    let projects_data = (*projects).clone();
                                    let dir_cache = dir_cache.clone();
                                    let overrides = overrides.clone();
                                    let editor = editor.clone();

                                    spawn_local(async move {
                                        match list_path(
                                            &tree_dir,
                                            &projects_data,
                                            dir_cache,
                                            overrides,
                                        )
                                        .await
                                        {
                                            Ok(entries) => {
                                                if let Some(mut live) = (*editor).clone() {
                                                    live.tree_entries = editor_tree_files(
                                                        &tree_dir,
                                                        entries,
                                                        0,
                                                        &projects_data,
                                                    );
                                                    live.tree_selection = live.tree_selection.min(
                                                        live.tree_entries.len().saturating_sub(1),
                                                    );
                                                    live.status = format!(
                                                        "NvimTree: {}",
                                                        display_path(&tree_dir)
                                                    );
                                                    editor.set(Some(live));
                                                }
                                            }
                                            Err(error) => {
                                                if let Some(mut live) = (*editor).clone() {
                                                    live.tree_entries.clear();
                                                    live.tree_selection = 0;
                                                    live.status = format!(
                                                        "NvimTree error ({}): {error}",
                                                        display_path(&tree_dir)
                                                    );
                                                    editor.set(Some(live));
                                                }
                                            }
                                        }
                                    });
                                }
                            }
                            "l" | "ArrowRight" | "j" | "ArrowDown" => {
                                state.tree_focused = false;
                                state.status = String::from("-- NORMAL --");
                            }
                            _ => {
                                state.status = String::from("-- NORMAL --");
                            }
                        }

                        editor.set(Some(state));
                        return;
                    }

                    if state.leader_pending {
                        event.prevent_default();
                        state.leader_pending = false;
                        state.g_pending = false;

                        match key.as_str() {
                            "e" => {
                                state.tree_visible = !state.tree_visible;
                                state.tree_focused = false;
                                if state.tree_visible {
                                    state.status =
                                        format!("NvimTree: {}", display_path(&state.tree_dir));

                                    if state.tree_entries.is_empty() {
                                        let tree_dir = state.tree_dir.clone();
                                        state.status = format!(
                                            "NvimTree: loading {}",
                                            display_path(&tree_dir)
                                        );
                                        let projects_data = (*projects).clone();
                                        let dir_cache = dir_cache.clone();
                                        let overrides = overrides.clone();
                                        let editor = editor.clone();

                                        spawn_local(async move {
                                            match list_path(
                                                &tree_dir,
                                                &projects_data,
                                                dir_cache,
                                                overrides,
                                            )
                                            .await
                                            {
                                                Ok(entries) => {
                                                    if let Some(mut live) = (*editor).clone() {
                                                        live.tree_entries = editor_tree_files(
                                                            &tree_dir,
                                                            entries,
                                                            0,
                                                            &projects_data,
                                                        );
                                                        live.tree_selection =
                                                            live.tree_selection.min(
                                                                live.tree_entries
                                                                    .len()
                                                                    .saturating_sub(1),
                                                            );
                                                        live.status = format!(
                                                            "NvimTree: {}",
                                                            display_path(&tree_dir)
                                                        );
                                                        editor.set(Some(live));
                                                    }
                                                }
                                                Err(error) => {
                                                    if let Some(mut live) = (*editor).clone() {
                                                        live.tree_entries.clear();
                                                        live.tree_selection = 0;
                                                        live.status = format!(
                                                            "NvimTree error ({}): {error}",
                                                            display_path(&tree_dir)
                                                        );
                                                        editor.set(Some(live));
                                                    }
                                                }
                                            }
                                        });
                                    }
                                } else {
                                    state.status = String::from("-- NORMAL --");
                                }
                            }
                            "w" | "W" => {
                                state.leader_window_pending = true;
                                state.status = String::from("<leader>w");
                            }
                            _ => {
                                state.status = String::from("-- NORMAL --");
                            }
                        }

                        editor.set(Some(state));
                        return;
                    }

                    if key == " " && !event.alt_key() && !event.ctrl_key() && !event.meta_key() {
                        event.prevent_default();
                        state.leader_pending = true;
                        state.leader_window_pending = false;
                        state.g_pending = false;
                        state.status = String::from("<leader>");
                        editor.set(Some(state));
                        return;
                    }

                    if state.g_pending {
                        state.g_pending = false;
                        if key == "g" {
                            event.prevent_default();
                            state.cursor = 0;
                            state.preferred_column = None;
                            state.status = String::from("-- NORMAL --");
                            editor.set(Some(state));
                            return;
                        }
                    }

                    if state.tree_focused {
                        event.prevent_default();
                        match key.as_str() {
                            ":" | ";" => {
                                state.mode = EditorMode::Command;
                                state.command.clear();
                                state.status = String::from(":");
                                editor.set(Some(state));
                            }
                            "Escape" => {
                                state.tree_focused = false;
                                state.status = String::from("-- NORMAL --");
                                editor.set(Some(state));
                            }
                            "q" => editor.set(Some(state)),
                            "j" | "ArrowDown" => {
                                if !state.tree_entries.is_empty() {
                                    state.tree_selection = (state.tree_selection + 1)
                                        .min(state.tree_entries.len().saturating_sub(1));
                                }
                                editor.set(Some(state));
                            }
                            "k" | "ArrowUp" => {
                                state.tree_selection = state.tree_selection.saturating_sub(1);
                                editor.set(Some(state));
                            }
                            "h" | "ArrowLeft" => {
                                let current_index = state
                                    .tree_selection
                                    .min(state.tree_entries.len().saturating_sub(1));
                                if let Some(current_entry) =
                                    state.tree_entries.get(current_index).cloned()
                                {
                                    if current_entry.is_dir && current_entry.expanded {
                                        clear_tree_children(&mut state.tree_entries, current_index);
                                        if let Some(entry) =
                                            state.tree_entries.get_mut(current_index)
                                        {
                                            entry.expanded = false;
                                            entry.loading = false;
                                        }
                                        state.status = format!(
                                            "NvimTree: {}",
                                            display_path(&current_entry.path)
                                        );
                                        editor.set(Some(state));
                                        return;
                                    }

                                    if let Some(parent_index) =
                                        parent_tree_index(&state.tree_entries, current_index)
                                    {
                                        state.tree_selection = parent_index;
                                        if let Some(parent) = state.tree_entries.get(parent_index) {
                                            state.status =
                                                format!("NvimTree: {}", display_path(&parent.path));
                                        }
                                        editor.set(Some(state));
                                        return;
                                    }
                                }

                                if state.tree_dir.is_empty() {
                                    state.status = String::from("NvimTree: /");
                                    editor.set(Some(state));
                                } else {
                                    let parent =
                                        state.tree_dir[..state.tree_dir.len() - 1].to_vec();
                                    state.status =
                                        format!("NvimTree: loading {}", display_path(&parent));
                                    let projects_data = (*projects).clone();
                                    let dir_cache = dir_cache.clone();
                                    let overrides = overrides.clone();
                                    let editor_for_async = editor.clone();
                                    let requested_dir = parent.clone();

                                    spawn_local(async move {
                                        match list_path(
                                            &requested_dir,
                                            &projects_data,
                                            dir_cache,
                                            overrides,
                                        )
                                        .await
                                        {
                                            Ok(entries) => {
                                                if let Some(mut live) = (*editor_for_async).clone()
                                                {
                                                    live.tree_dir = requested_dir.clone();
                                                    live.tree_entries = editor_tree_files(
                                                        &requested_dir,
                                                        entries,
                                                        0,
                                                        &projects_data,
                                                    );
                                                    live.tree_selection = 0;
                                                    live.tree_focused = true;
                                                    live.status = format!(
                                                        "NvimTree: {}",
                                                        display_path(&requested_dir)
                                                    );
                                                    editor_for_async.set(Some(live));
                                                }
                                            }
                                            Err(error) => {
                                                if let Some(mut live) = (*editor_for_async).clone()
                                                {
                                                    live.status = format!(
                                                        "NvimTree error ({}): {error}",
                                                        display_path(&requested_dir)
                                                    );
                                                    editor_for_async.set(Some(live));
                                                }
                                            }
                                        }
                                    });
                                    editor.set(Some(state));
                                }
                            }
                            "Enter" | "l" | "ArrowRight" => {
                                let identity_data =
                                    identity.as_ref().cloned().unwrap_or_else(default_identity);
                                let projects_data = (*projects).clone();
                                let selected_index = state
                                    .tree_selection
                                    .min(state.tree_entries.len().saturating_sub(1));
                                activate_tree_entry(
                                    &mut state,
                                    selected_index,
                                    identity_data,
                                    projects_data,
                                    dir_cache.clone(),
                                    file_cache.clone(),
                                    overrides.clone(),
                                    editor.clone(),
                                );
                                editor.set(Some(state));
                            }
                            _ => {
                                editor.set(Some(state));
                            }
                        }
                        return;
                    }

                    event.prevent_default();
                    let previous_preferred_column = state.preferred_column;
                    state.preferred_column = None;
                    match key.as_str() {
                        "h" | "ArrowLeft" => {
                            state.cursor = prev_boundary(state.content.as_str(), cursor)
                        }
                        "l" | "ArrowRight" => {
                            state.cursor = next_boundary(state.content.as_str(), cursor)
                        }
                        "j" | "ArrowDown" => {
                            let preferred = previous_preferred_column
                                .unwrap_or_else(|| line_column(state.content.as_str(), cursor));
                            state.cursor =
                                move_vertical(state.content.as_str(), cursor, 1, preferred);
                            state.preferred_column = Some(preferred);
                        }
                        "k" | "ArrowUp" => {
                            let preferred = previous_preferred_column
                                .unwrap_or_else(|| line_column(state.content.as_str(), cursor));
                            state.cursor =
                                move_vertical(state.content.as_str(), cursor, -1, preferred);
                            state.preferred_column = Some(preferred);
                        }
                        "w" => {
                            state.cursor = next_word_boundary(state.content.as_str(), cursor);
                        }
                        "b" => {
                            state.cursor = prev_word_boundary(state.content.as_str(), cursor);
                        }
                        "g" => {
                            state.g_pending = true;
                            state.status = String::from("g");
                        }
                        "G" => {
                            state.cursor = state.content.len();
                        }
                        "0" => state.cursor = line_start(state.content.as_str(), cursor),
                        "$" => state.cursor = line_end(state.content.as_str(), cursor),
                        "i" => {
                            state.mode = EditorMode::Insert;
                            state.status = String::from("-- INSERT --");
                            state.cursor = cursor;
                        }
                        "a" => {
                            state.mode = EditorMode::Insert;
                            state.status = String::from("-- INSERT --");
                            state.cursor = next_boundary(state.content.as_str(), cursor);
                        }
                        "A" => {
                            state.mode = EditorMode::Insert;
                            state.status = String::from("-- INSERT --");
                            state.cursor = line_end(state.content.as_str(), cursor);
                        }
                        "x" => {
                            let next = next_boundary(state.content.as_str(), cursor);
                            if cursor < next {
                                let mut content = state.content.clone();
                                content.replace_range(cursor..next, "");
                                state.content = content;
                                state.dirty = true;
                            }
                            state.cursor = cursor.min(state.content.len());
                        }
                        "o" => {
                            let insert_at = line_end(state.content.as_str(), cursor);
                            state.content.insert(insert_at, '\n');
                            state.cursor = insert_at + 1;
                            state.mode = EditorMode::Insert;
                            state.status = String::from("-- INSERT --");
                            state.dirty = true;
                        }
                        ":" => {
                            state.mode = EditorMode::Command;
                            state.command.clear();
                            state.status = String::from(":");
                        }
                        _ => {}
                    }

                    state.leader_pending = false;
                    state.leader_window_pending = false;
                    editor.set(Some(state));
                }
                EditorMode::Command => {}
            }
        })
    };

    let editor_command_input = {
        let editor = editor.clone();
        Callback::from(move |event: InputEvent| {
            let target: HtmlInputElement = event.target_unchecked_into();
            let Some(mut state) = (*editor).clone() else {
                return;
            };
            if state.mode != EditorMode::Command {
                return;
            }
            state.command = target.value();
            editor.set(Some(state));
        })
    };

    let editor_command_submit = {
        let editor = editor.clone();
        let overrides = overrides.clone();
        let terminal = terminal.clone();

        Callback::from(move |event: SubmitEvent| {
            event.prevent_default();

            let Some(mut state) = (*editor).clone() else {
                return;
            };

            if state.mode != EditorMode::Command {
                return;
            }

            let cmd = state.command.trim().to_string();
            let path_text = display_path(&state.path);

            match cmd.as_str() {
                "w" => {
                    save_override(&state.path, &state.content, overrides.clone());
                    state.dirty = false;
                    state.mode = EditorMode::Normal;
                    state.command.clear();
                    state.status = format!("{path_text} written");
                    editor.set(Some(state));
                }
                "q" => {
                    if state.dirty {
                        state.mode = EditorMode::Normal;
                        state.command.clear();
                        state.status = String::from("No write since last change (:q! to force)");
                        editor.set(Some(state));
                    } else {
                        editor.set(None);
                        terminal.dispatch(TerminalAction::Append(vec![TerminalLine::muted(
                            format!("closed {path_text}"),
                        )]));
                    }
                }
                "q!" => {
                    editor.set(None);
                    terminal.dispatch(TerminalAction::Append(vec![TerminalLine::muted(format!(
                        "discarded {path_text}"
                    ))]));
                }
                "wq" | "x" => {
                    save_override(&state.path, &state.content, overrides.clone());
                    editor.set(None);
                    terminal.dispatch(TerminalAction::Append(vec![TerminalLine::muted(format!(
                        "wrote and closed {path_text}"
                    ))]));
                }
                "help" => {
                    state.mode = EditorMode::Normal;
                    state.command.clear();
                    state.status = String::from(":w  :q  :q!  :wq");
                    editor.set(Some(state));
                }
                _ => {
                    state.mode = EditorMode::Normal;
                    state.command.clear();
                    state.status = format!("Unknown command: {cmd}");
                    editor.set(Some(state));
                }
            }
        })
    };

    let editor_command_keydown = {
        let editor = editor.clone();
        Callback::from(move |event: KeyboardEvent| {
            if event.key() != "Escape" {
                return;
            }

            let Some(mut state) = (*editor).clone() else {
                return;
            };

            if state.mode != EditorMode::Command {
                return;
            }

            event.prevent_default();
            state.mode = EditorMode::Normal;
            state.command.clear();
            state.status = String::from("-- NORMAL --");
            editor.set(Some(state));
        })
    };

    let clear_project_filters = {
        let project_filters = project_filters.clone();
        Callback::from(move |_| {
            project_filters.set(ProjectFilters::default());
        })
    };
    let toggle_team_solo = {
        let project_filters = project_filters.clone();
        Callback::from(move |_| {
            let mut next = *project_filters;
            next.team = if next.team == Some(ProjectTeam::Solo) {
                None
            } else {
                Some(ProjectTeam::Solo)
            };
            project_filters.set(next);
        })
    };
    let toggle_team_team = {
        let project_filters = project_filters.clone();
        Callback::from(move |_| {
            let mut next = *project_filters;
            next.team = if next.team == Some(ProjectTeam::Team) {
                None
            } else {
                Some(ProjectTeam::Team)
            };
            project_filters.set(next);
        })
    };
    let toggle_context_personal = {
        let project_filters = project_filters.clone();
        Callback::from(move |_| {
            let mut next = *project_filters;
            next.context = if next.context == Some(ProjectContext::Personal) {
                None
            } else {
                Some(ProjectContext::Personal)
            };
            project_filters.set(next);
        })
    };
    let toggle_context_uni = {
        let project_filters = project_filters.clone();
        Callback::from(move |_| {
            let mut next = *project_filters;
            next.context = if next.context == Some(ProjectContext::University) {
                None
            } else {
                Some(ProjectContext::University)
            };
            project_filters.set(next);
        })
    };
    let toggle_context_professional = {
        let project_filters = project_filters.clone();
        Callback::from(move |_| {
            let mut next = *project_filters;
            next.context = if next.context == Some(ProjectContext::Professional) {
                None
            } else {
                Some(ProjectContext::Professional)
            };
            project_filters.set(next);
        })
    };
    let toggle_legacy_only = {
        let project_filters = project_filters.clone();
        Callback::from(move |_| {
            let mut next = *project_filters;
            next.legacy_only = !next.legacy_only;
            project_filters.set(next);
        })
    };
    let filters = *project_filters;
    let terminal_lines_view = {
        let mut filters_inserted = false;
        let render_filters_bar = || {
            html! {
                <div class="line line-filter-row">
                    <section class="project-filters">
                        <span class="filters-label">{"project filters:"}</span>
                        <span class="filter-group">
                            <button
                                class={classes!(
                                    "filter-chip",
                                    if filters == ProjectFilters::default() { "is-active" } else { "" }
                                )}
                                type="button"
                                onclick={clear_project_filters.clone()}
                            >
                                {"ALL"}
                            </button>
                        </span>
                        <span class="filter-sep"></span>
                        <span class="filter-group">
                            <button
                                class={classes!(
                                    "filter-chip",
                                    "badge-team-solo",
                                    if filters.team == Some(ProjectTeam::Solo) { "is-active" } else { "" }
                                )}
                                type="button"
                                onclick={toggle_team_solo.clone()}
                            >
                                {"SOLO"}
                            </button>
                            <button
                                class={classes!(
                                    "filter-chip",
                                    "badge-team-team",
                                    if filters.team == Some(ProjectTeam::Team) { "is-active" } else { "" }
                                )}
                                type="button"
                                onclick={toggle_team_team.clone()}
                            >
                                {"TEAM"}
                            </button>
                        </span>
                        <span class="filter-sep"></span>
                        <span class="filter-group">
                            <button
                                class={classes!(
                                    "filter-chip",
                                    "badge-context-personal",
                                    if filters.context == Some(ProjectContext::Personal) { "is-active" } else { "" }
                                )}
                                type="button"
                                onclick={toggle_context_personal.clone()}
                            >
                                {"PERSONAL"}
                            </button>
                            <button
                                class={classes!(
                                    "filter-chip",
                                    "badge-context-uni",
                                    if filters.context == Some(ProjectContext::University) { "is-active" } else { "" }
                                )}
                                type="button"
                                onclick={toggle_context_uni.clone()}
                            >
                                {"UNI"}
                            </button>
                            <button
                                class={classes!(
                                    "filter-chip",
                                    "badge-context-professional",
                                    if filters.context == Some(ProjectContext::Professional) { "is-active" } else { "" }
                                )}
                                type="button"
                                onclick={toggle_context_professional.clone()}
                            >
                                {"PROFESSIONAL"}
                            </button>
                        </span>
                        <span class="filter-sep"></span>
                        <span class="filter-group">
                            <button
                                class={classes!(
                                    "filter-chip",
                                    "badge-era-legacy",
                                    if filters.legacy_only { "is-active" } else { "" }
                                )}
                                type="button"
                                onclick={toggle_legacy_only.clone()}
                            >
                                {"LEGACY"}
                            </button>
                        </span>
                    </section>
                </div>
            }
        };

        html! {
            <>
                {for terminal.lines.iter().enumerate().filter_map(|(index, line)| {
                    match line.kind {
                        LineKind::Project => {
                            let project = line.project.as_ref()?;
                            if !project_matches_filters(project, &filters) {
                                return None;
                            }
                            Some(render_line(line))
                        }
                        LineKind::Section => {
                            if line.text == "[projects]" && !filters_inserted {
                                filters_inserted = true;
                                return Some(html! {
                                    <>
                                        {render_line(line)}
                                        {render_filters_bar()}
                                    </>
                                });
                            }
                            if line.text == "[other projects]" {
                                if filters != ProjectFilters::default() {
                                    return None;
                                }
                                let has_project_after = terminal
                                    .lines
                                    .iter()
                                    .skip(index + 1)
                                    .any(|next| {
                                        matches!(next.kind, LineKind::Project)
                                            && next
                                                .project
                                                .as_ref()
                                                .is_some_and(|project| {
                                                    project_matches_filters(project, &filters)
                                                })
                                    });
                                if !has_project_after {
                                    return None;
                                }
                            }
                            Some(render_line(line))
                        }
                        _ => Some(render_line(line)),
                    }
                })}
            </>
        }
    };

    html! {
        <main
            class={classes!("frame", if *prompt_spotlight { "is-spotlight" } else { "" })}
            onclick={on_terminal_click}
        >
            {
                if *font_loading {
                    html! {
                        <section class="font-loader">
                            <div class="font-loader-spinner"></div>
                            <span class="font-loader-text">{"loading terminal font..."}</span>
                        </section>
                    }
                } else {
                    html! {}
                }
            }
            {
                if *boot_phase != BootPhase::Ready {
                    html! {
                        <header class="boot-header">
                            <span class="hint">{"loading terminal..."}</span>
                        </header>
                    }
                } else {
                    html! {
                        <header class="boot-header">
                            <span class="hint">{"terminal ready - type `help`"}</span>
                        </header>
                    }
                }
            }

            {
                if let Some(editor_state) = editor.as_ref() {
                    let language = detect_language(&editor_state.path);
                    let editor_main_class = classes!(
                        "editor-main",
                        if editor_state.tree_visible {
                            "tree-open"
                        } else {
                            "tree-closed"
                        }
                    );
                    html! {
                        <section class="editor-screen">
                            <div class="editor-header">
                                {format!("nvim {} [{}]", display_path(&editor_state.path), language_label(language))}
                            </div>
                            <div class={editor_main_class}>
                                {
                                    if editor_state.tree_visible {
                                        let tree_path = display_path(&editor_state.tree_dir);
                                        html! {
                                            <aside class={classes!(
                                                "editor-tree",
                                                if editor_state.tree_focused { "is-focused" } else { "" }
                                            )}>
                                                <div class="editor-tree-path">
                                                    <span class="editor-tree-path-text">{tree_path}</span>
                                                    <button
                                                        class="editor-tree-help-trigger"
                                                        type="button"
                                                        onclick={open_editor_keybinds_popup.clone()}
                                                    >
                                                        {"press '?' for keybinds"}
                                                    </button>
                                                </div>
                                                <div
                                                    class="editor-tree-list"
                                                    ref={editor_tree_ref.clone()}
                                                    tabindex="0"
                                                    onkeydown={editor_onkeydown.clone()}
                                                >
                                                    {
                                                        for editor_state.tree_entries.iter().enumerate().map(|(index, entry)| {
                                                            let editor = editor.clone();
                                                            let identity = identity.clone();
                                                            let projects = projects.clone();
                                                            let dir_cache = dir_cache.clone();
                                                            let file_cache = file_cache.clone();
                                                            let overrides = overrides.clone();
                                                            let on_click = Callback::from(move |event: MouseEvent| {
                                                                event.prevent_default();
                                                                event.stop_propagation();
                                                                if let Some(mut state) = (*editor).clone() {
                                                                    if state.tree_entries.is_empty() {
                                                                        return;
                                                                    }
                                                                    let selected_index = index.min(
                                                                        state.tree_entries.len().saturating_sub(1),
                                                                    );
                                                                    state.tree_selection = selected_index;
                                                                    state.tree_focused = true;
                                                                    let identity_data = identity
                                                                        .as_ref()
                                                                        .cloned()
                                                                        .unwrap_or_else(default_identity);
                                                                    let projects_data = (*projects).clone();
                                                                    activate_tree_entry(
                                                                        &mut state,
                                                                        selected_index,
                                                                        identity_data,
                                                                        projects_data,
                                                                        dir_cache.clone(),
                                                                        file_cache.clone(),
                                                                        overrides.clone(),
                                                                        editor.clone(),
                                                                    );
                                                                    editor.set(Some(state));
                                                                }
                                                            });
                                                            render_tree_entry(
                                                                index,
                                                                entry,
                                                                index == editor_state.tree_selection,
                                                                on_click,
                                                            )
                                                        })
                                                    }
                                                </div>
                                            </aside>
                                        }
                                    } else {
                                        html! {}
                                    }
                                }
                                <div class="editor-buffer">
                                    <pre class="editor-gutter" ref={editor_gutter_ref.clone()}>
                                        {render_line_numbers(&editor_state.content)}
                                    </pre>
                                    <pre class="editor-highlight" ref={editor_highlight_ref.clone()}>
                                        {render_highlighted(&editor_state.content, language)}
                                    </pre>
                                    <textarea
                                        class="editor-area"
                                        ref={editor_ref}
                                        value={editor_state.content.clone()}
                                        oninput={editor_oninput}
                                        onkeydown={editor_onkeydown}
                                        onscroll={editor_onscroll}
                                        spellcheck="false"
                                    />
                                </div>
                            </div>
                            <div class="editor-status">
                                <span>{if editor_state.dirty { "[+]" } else { "[ ]" }}</span>
                                <span>{display_path(&editor_state.path)}</span>
                                <span>{language_label(language)}</span>
                                <span>{editor_mode_label(editor_state.mode)}</span>
                                <span>{if editor_state.tree_focused { "TREE" } else { "-" }}</span>
                                <span>{editor_state.status.clone()}</span>
                            </div>
                            {
                                if editor_state.mode == EditorMode::Command {
                                    html! {
                                    <form class="editor-cmd-row" onsubmit={editor_command_submit.clone()}>
                                            <span>{":"}</span>
                                            <input
                                                class="editor-cmd-input"
                                                type="text"
                                                ref={editor_command_ref}
                                                value={editor_state.command.clone()}
                                                oninput={editor_command_input.clone()}
                                                onkeydown={editor_command_keydown}
                                                autocomplete="off"
                                                autocorrect="off"
                                                spellcheck="false"
                                            />
                                        </form>
                                    }
                                } else {
                                    html! {}
                                }
                            }
                            {
                                if editor_state.show_keybinds_help {
                                    html! {
                                        <div class={classes!("center-popup-overlay", "is-active")}>
                                            <div class="center-popup-card keybinds-popup-card">
                                                <button
                                                    class="center-popup-close"
                                                    type="button"
                                                    onclick={dismiss_editor_keybinds_popup.clone()}
                                                    aria-label="Close keybinds help"
                                                ></button>
                                                <div class="center-popup-title">{"nvim keybinds"}</div>
                                                <div class="keybind-groups">
                                                    <div class="keybind-group">
                                                        <div class="keybind-group-label">{"navigation"}</div>
                                                        <div>{"h / j / k / l  move cursor"}</div>
                                                        <div>{"w / b          next / previous word"}</div>
                                                        <div>{"gg / G         top / bottom of file"}</div>
                                                    </div>
                                                    <div class="keybind-group">
                                                        <div class="keybind-group-label">{"explorer"}</div>
                                                        <div>{"<Space>e       toggle explorer"}</div>
                                                        <div>{"Enter / l      open file or expand folder"}</div>
                                                        <div>{"h             collapse folder / select parent"}</div>
                                                    </div>
                                                    <div class="keybind-group">
                                                        <div class="keybind-group-label">{"windows & modes"}</div>
                                                        <div>{"<Space>w h/j/k/l switch pane"}</div>
                                                        <div>{"i / Esc        insert / normal mode"}</div>
                                                        <div>{":w / :q / :wq  save / close / save+close"}</div>
                                                    </div>
                                                </div>
                                            </div>
                                        </div>
                                    }
                                } else {
                                    html! {}
                                }
                            }
                        </section>
                    }
                } else {
                    html! {
                        <>
                            <section class="terminal-output" ref={output_ref}>
                                {terminal_lines_view.clone()}
                            </section>
                            {
                                if *boot_phase == BootPhase::Ready {
                                    html! {
                                        <>
                                            <div class={classes!(
                                                "prompt-focus",
                                                if *prompt_spotlight { "is-active" } else { "" }
                                            )}>
                                                <div class="prompt-spotlight-msg">
                                                    <span>{"Try writing `about`"}</span>
                                                    <button
                                                        class="prompt-spotlight-close"
                                                        type="button"
                                                        onclick={dismiss_prompt_spotlight.clone()}
                                                        aria-label="Dismiss onboarding hint"
                                                    ></button>
                                                </div>
                                                <form class="prompt-row" {onsubmit}>
                                                    <span class="prompt-label">{format_prompt(cwd.as_ref())}</span>
                                                    <input
                                                        class="prompt-input"
                                                        type="text"
                                                        ref={prompt_input_ref}
                                                        value={(*command_input).clone()}
                                                        {oninput}
                                                        onkeydown={onkeydown}
                                                        autocomplete="off"
                                                        autocorrect="off"
                                                        spellcheck="false"
                                                        disabled={*busy}
                                                    />
                                                </form>
                                            </div>
                                            {
                                                if !completion_lines.is_empty() {
                                                    html! {
                                                        <section class="completion-panel">
                                                            {for completion_lines.iter().map(render_line)}
                                                        </section>
                                                    }
                                                } else {
                                                    html! {}
                                                }
                                            }
                                        </>
                                    }
                                } else {
                                    html! {}
                                }
                            }
                        </>
                    }
                }
            }
            {
                if is_phone_client && *boot_phase == BootPhase::Ready && *phone_warning_popup_visible {
                    html! {
                        <div class={classes!("center-popup-overlay", "is-active")}>
                            <div class="center-popup-card phone-warning-popup">
                                <button
                                    class="center-popup-close"
                                    type="button"
                                    onclick={dismiss_phone_warning_popup.clone()}
                                    aria-label="Dismiss mobile notice"
                                ></button>
                                <div class="center-popup-title">{"mobile notice"}</div>
                                <div class="center-popup-body">
                                    {MOBILE_NVIM_WARNING}
                                </div>
                            </div>
                        </div>
                    }
                } else {
                    html! {}
                }
            }
        </main>
    }
}

async fn run_command(
    raw: String,
    cwd: Vec<String>,
    identity: Identity,
    projects: Vec<Project>,
    history: Vec<String>,
    dir_cache: UseStateHandle<DirCache>,
    file_cache: UseStateHandle<FileCache>,
    overrides: UseStateHandle<OverrideMap>,
    is_phone_client: bool,
) -> CommandOutcome {
    let mut outcome = CommandOutcome::default();
    let mut parts = raw.split_whitespace();
    let command = parts.next().unwrap_or_default();
    let args: Vec<String> = parts.map(ToOwned::to_owned).collect();

    match command {
        "help" => outcome.lines = help_lines(),
        "about" => outcome.lines = about_lines(&identity, &projects),
        "pwd" => outcome
            .lines
            .push(TerminalLine::output(display_path(cwd.as_ref()))),
        "whoami" => outcome
            .lines
            .push(TerminalLine::output(identity.handle.clone())),
        "history" => {
            if history.is_empty() {
                outcome.lines.push(TerminalLine::muted("(empty history)"));
            } else {
                let start = history.len().saturating_sub(80);
                for (index, item) in history.iter().enumerate().skip(start) {
                    outcome
                        .lines
                        .push(TerminalLine::output(format!("{:>4}  {}", index + 1, item)));
                }
            }
        }
        "clear" => outcome.clear = true,
        "ls" => {
            let mut path_arg = None::<String>;
            for arg in &args {
                match arg.as_str() {
                    "-1" | "--oneline" => {}
                    _ if arg.starts_with('-') => {
                        outcome
                            .lines
                            .push(TerminalLine::error(format!("ls: unknown option '{arg}'")));
                        return outcome;
                    }
                    _ => {
                        if path_arg.is_some() {
                            outcome
                                .lines
                                .push(TerminalLine::error("ls: too many path operands"));
                            return outcome;
                        }
                        path_arg = Some(arg.clone());
                    }
                }
            }

            let target = path_arg
                .as_ref()
                .map(|path| normalize_path(cwd.as_ref(), path))
                .unwrap_or_else(|| cwd.clone());

            match list_path(&target, &projects, dir_cache, overrides).await {
                Ok(entries) => {
                    let rows = entries
                        .iter()
                        .map(|entry| {
                            vec![ls_token_for_entry(entry.as_str(), 0, &target, &projects)]
                        })
                        .collect::<Vec<_>>();
                    if rows.is_empty() {
                        outcome.lines.push(TerminalLine::output("(empty)"));
                    } else {
                        for row in rows {
                            outcome.lines.push(TerminalLine::ls(row));
                        }
                    }
                }
                Err(error) => outcome.lines.push(TerminalLine::error(error)),
            }
        }
        "cd" => {
            let target = args
                .first()
                .map(|path| normalize_path(cwd.as_ref(), path))
                .unwrap_or_default();

            match ensure_directory(&target, &projects, dir_cache, overrides).await {
                Ok(()) => outcome.next_cwd = Some(target),
                Err(error) => outcome.lines.push(TerminalLine::error(error)),
            }
        }
        "cat" => {
            let Some(path_arg) = args.first() else {
                outcome
                    .lines
                    .push(TerminalLine::error("cat: missing file operand"));
                return outcome;
            };

            let target = normalize_path(cwd.as_ref(), path_arg);
            match cat_path(&target, &identity, &projects, file_cache, overrides).await {
                Ok(content) => {
                    for line in content.lines() {
                        outcome.lines.push(TerminalLine::output(line.to_string()));
                    }
                }
                Err(error) => outcome.lines.push(TerminalLine::error(error)),
            }
        }
        "head" => match parse_count_and_file(&args, "head") {
            Ok((count, file_arg)) => {
                let target = normalize_path(cwd.as_ref(), file_arg.as_str());
                match cat_path(&target, &identity, &projects, file_cache, overrides).await {
                    Ok(content) => {
                        for line in content.lines().take(count) {
                            outcome.lines.push(TerminalLine::output(line.to_string()));
                        }
                    }
                    Err(error) => outcome.lines.push(TerminalLine::error(error)),
                }
            }
            Err(error) => outcome.lines.push(TerminalLine::error(error)),
        },
        "tail" => match parse_count_and_file(&args, "tail") {
            Ok((count, file_arg)) => {
                let target = normalize_path(cwd.as_ref(), file_arg.as_str());
                match cat_path(&target, &identity, &projects, file_cache, overrides).await {
                    Ok(content) => {
                        let all = content.lines().collect::<Vec<_>>();
                        let skip = all.len().saturating_sub(count);
                        for line in all.into_iter().skip(skip) {
                            outcome.lines.push(TerminalLine::output(line.to_string()));
                        }
                    }
                    Err(error) => outcome.lines.push(TerminalLine::error(error)),
                }
            }
            Err(error) => outcome.lines.push(TerminalLine::error(error)),
        },
        "grep" => match parse_grep_args(&args) {
            Ok((pattern, case_insensitive, file_arg)) => {
                let target = normalize_path(cwd.as_ref(), file_arg.as_str());
                match cat_path(
                    &target,
                    &identity,
                    &projects,
                    file_cache.clone(),
                    overrides.clone(),
                )
                .await
                {
                    Ok(content) => {
                        let mut hits = 0usize;
                        for (index, line) in content.lines().enumerate() {
                            let is_match = if case_insensitive {
                                line.to_ascii_lowercase().contains(pattern.as_str())
                            } else {
                                line.contains(pattern.as_str())
                            };
                            if is_match {
                                hits += 1;
                                outcome.lines.push(TerminalLine::output(format!(
                                    "{:>4}: {}",
                                    index + 1,
                                    line
                                )));
                            }
                        }
                        if hits == 0 {
                            outcome.lines.push(TerminalLine::muted("grep: no matches"));
                        }
                    }
                    Err(error) => outcome.lines.push(TerminalLine::error(error)),
                }
            }
            Err(error) => outcome.lines.push(TerminalLine::error(error)),
        },
        "wc" => {
            let Some(path_arg) = args.first() else {
                outcome
                    .lines
                    .push(TerminalLine::error("wc: missing file operand"));
                return outcome;
            };
            let target = normalize_path(cwd.as_ref(), path_arg);
            match cat_path(
                &target,
                &identity,
                &projects,
                file_cache.clone(),
                overrides.clone(),
            )
            .await
            {
                Ok(content) => {
                    let lines = content.lines().count();
                    let words = content.split_whitespace().count();
                    let bytes = content.len();
                    outcome.lines.push(TerminalLine::output(format!(
                        "{:>7} {:>7} {:>7} {}",
                        lines,
                        words,
                        bytes,
                        display_path(&target)
                    )));
                }
                Err(error) => outcome.lines.push(TerminalLine::error(error)),
            }
        }
        "mkdir" => match parse_mkdir_args(&args) {
            Ok((target, recursive)) => {
                let final_path = normalize_path(cwd.as_ref(), target.as_str());
                if final_path.is_empty() {
                    outcome
                        .lines
                        .push(TerminalLine::error("mkdir: cannot create root directory"));
                    return outcome;
                }

                if ensure_directory(&final_path, &projects, dir_cache.clone(), overrides.clone())
                    .await
                    .is_ok()
                {
                    outcome
                        .lines
                        .push(TerminalLine::error("mkdir: directory already exists"));
                    return outcome;
                }

                let exists_as_file = cat_path(
                    &final_path,
                    &identity,
                    &projects,
                    file_cache.clone(),
                    overrides.clone(),
                )
                .await
                .is_ok();
                if exists_as_file {
                    outcome
                        .lines
                        .push(TerminalLine::error("mkdir: path exists as file"));
                    return outcome;
                }

                if recursive {
                    for depth in 1..=final_path.len() {
                        let partial = final_path[..depth].to_vec();
                        if ensure_directory(
                            &partial,
                            &projects,
                            dir_cache.clone(),
                            overrides.clone(),
                        )
                        .await
                        .is_err()
                        {
                            create_virtual_dir(&partial, overrides.clone());
                        }
                    }
                } else {
                    let parent = final_path[..final_path.len().saturating_sub(1)].to_vec();
                    if ensure_directory(&parent, &projects, dir_cache, overrides.clone())
                        .await
                        .is_err()
                    {
                        outcome.lines.push(TerminalLine::error(
                            "mkdir: parent directory does not exist (use mkdir -p)",
                        ));
                        return outcome;
                    }
                    create_virtual_dir(&final_path, overrides.clone());
                }

                outcome.lines.push(TerminalLine::muted(format!(
                    "created {}",
                    display_path(&final_path)
                )));
            }
            Err(error) => outcome.lines.push(TerminalLine::error(error)),
        },
        "touch" => {
            let Some(path_arg) = args.first() else {
                outcome
                    .lines
                    .push(TerminalLine::error("touch: missing file operand"));
                return outcome;
            };
            let target = normalize_path(cwd.as_ref(), path_arg);
            if target.is_empty() {
                outcome
                    .lines
                    .push(TerminalLine::error("touch: cannot touch root"));
                return outcome;
            }

            if ensure_directory(&target, &projects, dir_cache.clone(), overrides.clone())
                .await
                .is_ok()
            {
                outcome
                    .lines
                    .push(TerminalLine::error("touch: is a directory"));
                return outcome;
            }

            let parent = target[..target.len().saturating_sub(1)].to_vec();
            if ensure_directory(&parent, &projects, dir_cache, overrides.clone())
                .await
                .is_err()
            {
                outcome
                    .lines
                    .push(TerminalLine::error("touch: no such parent directory"));
                return outcome;
            }

            let key = absolute_path(&target);
            if overrides.contains_key(key.as_str()) {
                outcome.lines.push(TerminalLine::muted(format!(
                    "updated {}",
                    display_path(&target)
                )));
                return outcome;
            }

            if cat_path(
                &target,
                &identity,
                &projects,
                file_cache.clone(),
                overrides.clone(),
            )
            .await
            .is_ok()
            {
                outcome.lines.push(TerminalLine::muted(format!(
                    "{} exists (read-only source)",
                    display_path(&target)
                )));
                return outcome;
            }

            save_override(&target, "", overrides.clone());
            outcome.lines.push(TerminalLine::muted(format!(
                "created {}",
                display_path(&target)
            )));
        }
        "rm" => {
            let Some(path_arg) = args.first() else {
                outcome
                    .lines
                    .push(TerminalLine::error("rm: missing operand"));
                return outcome;
            };
            let target = normalize_path(cwd.as_ref(), path_arg);
            if target.is_empty() {
                outcome
                    .lines
                    .push(TerminalLine::error("rm: cannot remove root"));
                return outcome;
            }

            if remove_override(&target, overrides.clone()) {
                outcome.lines.push(TerminalLine::muted(format!(
                    "removed {}",
                    display_path(&target)
                )));
                return outcome;
            }

            let dir_key = virtual_dir_key(&target);
            let has_nested = overrides
                .keys()
                .any(|entry| entry.starts_with(dir_key.as_str()) && entry != &dir_key);
            if overrides.contains_key(dir_key.as_str()) {
                if has_nested {
                    outcome
                        .lines
                        .push(TerminalLine::error("rm: directory not empty"));
                } else {
                    let mut next = (*overrides).clone();
                    next.remove(dir_key.as_str());
                    overrides.set(next);
                    outcome.lines.push(TerminalLine::muted(format!(
                        "removed {}",
                        display_path(&target)
                    )));
                }
                return outcome;
            }

            outcome.lines.push(TerminalLine::error(
                "rm: can only remove virtual files/directories created in this terminal",
            ));
        }
        "stat" => {
            let target = args
                .first()
                .map(|path| normalize_path(cwd.as_ref(), path))
                .unwrap_or_else(|| cwd.clone());

            if ensure_directory(&target, &projects, dir_cache.clone(), overrides.clone())
                .await
                .is_ok()
            {
                let count = list_path(&target, &projects, dir_cache, overrides)
                    .await
                    .map(|entries| entries.len())
                    .unwrap_or(0);
                outcome.lines.push(TerminalLine::output(format!(
                    "  File: {}",
                    display_path(&target)
                )));
                outcome
                    .lines
                    .push(TerminalLine::output("  Type: directory"));
                outcome
                    .lines
                    .push(TerminalLine::output(format!("  Items: {count}")));
            } else {
                match cat_path(&target, &identity, &projects, file_cache, overrides).await {
                    Ok(content) => {
                        outcome.lines.push(TerminalLine::output(format!(
                            "  File: {}",
                            display_path(&target)
                        )));
                        outcome.lines.push(TerminalLine::output("  Type: file"));
                        outcome.lines.push(TerminalLine::output(format!(
                            "  Size: {} bytes",
                            content.len()
                        )));
                        outcome.lines.push(TerminalLine::output(format!(
                            "  Lines: {}",
                            content.lines().count()
                        )));
                    }
                    Err(error) => outcome.lines.push(TerminalLine::error(error)),
                }
            }
        }
        "tree" => {
            let target = args
                .first()
                .map(|path| normalize_path(cwd.as_ref(), path))
                .unwrap_or_else(|| cwd.clone());

            match tree_path(&target, &projects, dir_cache, overrides).await {
                Ok(lines) => {
                    for line in lines {
                        outcome.lines.push(TerminalLine::output(line));
                    }
                }
                Err(error) => outcome.lines.push(TerminalLine::error(error)),
            }
        }
        "nvim" | "vim" => {
            if is_phone_client {
                outcome.lines.push(TerminalLine::error(MOBILE_NVIM_WARNING));
                return outcome;
            }

            let Some(path_arg) = args.first() else {
                outcome.lines.push(TerminalLine::error(
                    "nvim: missing file path (usage: nvim <path>)",
                ));
                return outcome;
            };

            let target = normalize_path(cwd.as_ref(), path_arg);
            if target.is_empty() {
                outcome
                    .lines
                    .push(TerminalLine::error("nvim: cannot open root directory"));
                return outcome;
            }

            if ensure_directory(&target, &projects, dir_cache.clone(), overrides.clone())
                .await
                .is_ok()
            {
                let tree_entries = editor_tree_files(
                    &target,
                    list_path(&target, &projects, dir_cache.clone(), overrides.clone())
                        .await
                        .unwrap_or_default(),
                    0,
                    &projects,
                );
                outcome.open_editor = Some(EditorState {
                    path: target.clone(),
                    content: String::new(),
                    cursor: 0,
                    preferred_column: None,
                    mode: EditorMode::Normal,
                    command: String::new(),
                    status: format!("NvimTree: {}", display_path(&target)),
                    dirty: false,
                    tree_visible: true,
                    tree_focused: true,
                    tree_dir: target,
                    tree_entries,
                    tree_selection: 0,
                    show_keybinds_help: false,
                    leader_pending: false,
                    leader_window_pending: false,
                    g_pending: false,
                });
                return outcome;
            }

            let abs = absolute_path(&target);
            let existing_override = overrides.get(abs.as_str()).cloned();
            let mut new_file = false;

            let content = if let Some(content) = existing_override.clone() {
                content
            } else {
                match cat_path(
                    &target,
                    &identity,
                    &projects,
                    file_cache.clone(),
                    overrides.clone(),
                )
                .await
                {
                    Ok(content) => content,
                    Err(error) => {
                        if error.contains("no such file") {
                            let parent = target[..target.len().saturating_sub(1)].to_vec();
                            if ensure_directory(
                                &parent,
                                &projects,
                                dir_cache.clone(),
                                overrides.clone(),
                            )
                            .await
                            .is_ok()
                            {
                                new_file = true;
                                String::new()
                            } else {
                                outcome.lines.push(TerminalLine::error(error));
                                return outcome;
                            }
                        } else {
                            outcome.lines.push(TerminalLine::error(error));
                            return outcome;
                        }
                    }
                }
            };

            let tree_dir = target[..target.len().saturating_sub(1)].to_vec();
            let tree_entries = editor_tree_files(
                &tree_dir,
                list_path(&tree_dir, &projects, dir_cache.clone(), overrides.clone())
                    .await
                    .unwrap_or_default(),
                0,
                &projects,
            );
            let tree_selection = tree_entries
                .iter()
                .position(|entry| !entry.is_dir && entry.path == target)
                .unwrap_or(0);

            outcome.open_editor = Some(EditorState {
                path: target,
                content,
                cursor: 0,
                preferred_column: None,
                mode: EditorMode::Normal,
                command: String::new(),
                status: if new_file {
                    String::from("[New File] -- NORMAL --")
                } else if existing_override.is_some() {
                    String::from("[override] -- NORMAL --")
                } else {
                    String::from("-- NORMAL --")
                },
                dirty: false,
                tree_visible: false,
                tree_focused: false,
                tree_dir,
                tree_entries,
                tree_selection,
                show_keybinds_help: false,
                leader_pending: false,
                leader_window_pending: false,
                g_pending: false,
            });
        }
        "echo" => outcome.lines.push(TerminalLine::output(args.join(" "))),
        _ => outcome.lines.push(TerminalLine::error(format!(
            "{command}: command not found (use `help`)"
        ))),
    }

    outcome
}

async fn fetch_identity() -> Result<Identity, String> {
    let response = Request::get("/api/identity")
        .send()
        .await
        .map_err(|error| error.to_string())?;

    if !response.ok() {
        return Err(format!("status {}", response.status()));
    }

    response
        .json::<Identity>()
        .await
        .map_err(|error| error.to_string())
}

async fn fetch_projects() -> Result<Vec<Project>, String> {
    let response = Request::get("/api/projects")
        .send()
        .await
        .map_err(|error| error.to_string())?;

    if !response.ok() {
        return Err(format!("status {}", response.status()));
    }

    response
        .json::<Vec<Project>>()
        .await
        .map_err(|error| error.to_string())
}

async fn list_path(
    path: &[String],
    projects: &[Project],
    dir_cache: UseStateHandle<DirCache>,
    overrides: UseStateHandle<OverrideMap>,
) -> Result<Vec<String>, String> {
    if path.is_empty() {
        let mut entries = vec![String::from("about.txt"), String::from("projects/")];
        merge_virtual_entries(&mut entries, path, &*overrides);
        sort_entries(&mut entries);
        entries.dedup();
        return Ok(entries);
    }

    if path.len() == 1 {
        if path[0] == "projects" {
            let mut names = projects
                .iter()
                .map(|project| format!("{}/", project.name))
                .collect::<Vec<_>>();
            merge_virtual_entries(&mut names, path, &*overrides);
            sort_entries(&mut names);
            names.dedup();
            return Ok(names);
        }

        if path[0] == "about.txt" {
            return Ok(vec![String::from("about.txt")]);
        }

        if is_virtual_dir(path, &*overrides) {
            let mut entries = Vec::new();
            merge_virtual_entries(&mut entries, path, &*overrides);
            sort_entries(&mut entries);
            entries.dedup();
            return Ok(entries);
        }

        if overrides.contains_key(absolute_path(path).as_str()) {
            return Ok(vec![path[0].clone()]);
        }

        return Err(String::from("ls: no such file or directory"));
    }

    if path.first().is_some_and(|head| head == "projects") {
        let Some((project, project_index)) = find_project_in_path(projects, path) else {
            return Err(String::from("ls: unknown project"));
        };

        if path.len() == project_index + 1 {
            let mut output = vec![String::from("META.txt")];
            match get_repo_dir(project.owner.as_str(), project_repo(project), "", dir_cache).await {
                Ok(entries) => output.extend(entries.into_iter().map(format_repo_entry)),
                Err(error) => {
                    if !error.contains("status 404") {
                        return Err(error);
                    }
                }
            }
            merge_virtual_entries(&mut output, path, &*overrides);
            sort_entries(&mut output);
            output.dedup();
            return Ok(output);
        }

        let repo_path = path[(project_index + 1)..].join("/");
        if repo_path == "META.txt" {
            return Ok(vec![String::from("META.txt")]);
        }

        let mut output = Vec::new();
        match get_repo_dir(
            project.owner.as_str(),
            project_repo(project),
            repo_path.as_str(),
            dir_cache,
        )
        .await
        {
            Ok(entries) => output.extend(entries.into_iter().map(format_repo_entry)),
            Err(error) => {
                if !error.contains("status 404") {
                    return Err(error);
                }
            }
        }

        merge_virtual_entries(&mut output, path, &*overrides);
        sort_entries(&mut output);
        output.dedup();

        if output.is_empty() {
            let absolute = absolute_path(path);
            if overrides.contains_key(absolute.as_str()) {
                if let Some(name) = path.last() {
                    return Ok(vec![name.clone()]);
                }
            }
            if is_virtual_dir(path, &*overrides) {
                return Ok(Vec::new());
            }
            return Err(String::from("ls: no such file or directory"));
        }

        return Ok(output);
    }

    if is_virtual_dir(path, &*overrides) {
        let mut entries = Vec::new();
        merge_virtual_entries(&mut entries, path, &*overrides);
        sort_entries(&mut entries);
        entries.dedup();
        return Ok(entries);
    }

    let absolute = absolute_path(path);
    if overrides.contains_key(absolute.as_str()) {
        if let Some(name) = path.last() {
            return Ok(vec![name.clone()]);
        }
    }

    Err(String::from("ls: no such file or directory"))
}

async fn ensure_directory(
    path: &[String],
    projects: &[Project],
    dir_cache: UseStateHandle<DirCache>,
    overrides: UseStateHandle<OverrideMap>,
) -> Result<(), String> {
    if path.is_empty() {
        return Ok(());
    }

    if path.len() == 1 {
        if path[0] == "projects" {
            return Ok(());
        }
        if is_virtual_dir(path, &*overrides) {
            return Ok(());
        }
        return Err(String::from("cd: not a directory"));
    }

    if path.first().is_some_and(|head| head == "projects") {
        let Some((project, project_index)) = find_project_in_path(projects, path) else {
            return Err(String::from("cd: unknown project"));
        };

        if path.len() == project_index + 1 {
            return Ok(());
        }

        let repo_path = path[(project_index + 1)..].join("/");
        if repo_path == "META.txt" {
            return Err(String::from("cd: not a directory"));
        }

        if get_repo_dir(
            project.owner.as_str(),
            project_repo(project),
            repo_path.as_str(),
            dir_cache.clone(),
        )
        .await
        .is_ok()
        {
            return Ok(());
        }

        let mut virtual_entries = Vec::new();
        merge_virtual_entries(&mut virtual_entries, path, &*overrides);
        if !virtual_entries.is_empty() {
            return Ok(());
        }
        if is_virtual_dir(path, &*overrides) {
            return Ok(());
        }

        return Err(String::from("cd: no such directory"));
    }

    if is_virtual_dir(path, &*overrides) {
        return Ok(());
    }

    Err(String::from("cd: no such file or directory"))
}

async fn cat_path(
    path: &[String],
    identity: &Identity,
    projects: &[Project],
    file_cache: UseStateHandle<FileCache>,
    overrides: UseStateHandle<OverrideMap>,
) -> Result<String, String> {
    let absolute = absolute_path(path);
    if let Some(content) = overrides.get(absolute.as_str()).cloned() {
        return Ok(content);
    }

    if is_virtual_dir(path, &*overrides) {
        return Err(String::from("cat: is a directory"));
    }

    if path.len() == 1 && path[0] == "about.txt" {
        return Ok(about_text(identity, projects));
    }

    if path.first().is_some_and(|head| head == "projects") && path.len() >= 2 {
        let Some((project, project_index)) = find_project_in_path(projects, path) else {
            return Err(String::from("cat: unknown project"));
        };

        if path.len() == project_index + 2 && path[project_index + 1] == "META.txt" {
            return Ok(project_meta(project));
        }

        if path.len() <= project_index + 1 {
            return Err(String::from("cat: is a directory"));
        }

        let repo_path = path[(project_index + 1)..].join("/");
        if repo_path.ends_with('/') {
            return Err(String::from("cat: is a directory"));
        }

        return get_repo_file(
            project.owner.as_str(),
            project_repo(project),
            repo_path.as_str(),
            file_cache,
        )
        .await;
    }

    Err(String::from("cat: no such file"))
}

async fn tree_path(
    path: &[String],
    projects: &[Project],
    dir_cache: UseStateHandle<DirCache>,
    overrides: UseStateHandle<OverrideMap>,
) -> Result<Vec<String>, String> {
    if ensure_directory(path, projects, dir_cache.clone(), overrides.clone())
        .await
        .is_err()
        && !path.is_empty()
    {
        return Err(String::from("tree: no such directory"));
    }

    let entries = list_path(path, projects, dir_cache, overrides).await?;
    let mut lines = vec![String::from(".")];

    for (index, item) in entries.iter().enumerate() {
        let prefix = if index + 1 == entries.len() {
            "`--"
        } else {
            "|--"
        };
        lines.push(format!("{prefix} {item}"));
    }

    Ok(lines)
}

async fn get_repo_dir(
    owner: &str,
    repo: &str,
    repo_path: &str,
    dir_cache: UseStateHandle<DirCache>,
) -> Result<Vec<RepoEntry>, String> {
    let key = format!("{owner}/{repo}:{repo_path}");
    if let Some(entries) = dir_cache.get(&key).cloned() {
        return Ok(entries);
    }

    let url = github_contents_url(owner, repo, repo_path);
    let response = Request::get(url.as_str())
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|error| format!("github request failed: {error}"))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("github response decode failed: {error}"))?;

    if !(200..300).contains(&status) {
        if let Ok(parsed) = serde_json::from_str::<GithubApiError>(body.as_str()) {
            return Err(format!("github: {}", parsed.message));
        }
        return Err(format!("github: status {status}"));
    }

    let parsed = serde_json::from_str::<Vec<GithubContentItem>>(body.as_str())
        .map_err(|_| String::from("path is not a directory or is unavailable"))?;

    let mut entries = parsed
        .into_iter()
        .map(|item| RepoEntry {
            name: item.name,
            path: item.path,
            kind: if item.kind == "dir" {
                RepoEntryKind::Dir
            } else {
                RepoEntryKind::File
            },
        })
        .collect::<Vec<_>>();

    entries.sort_by(|left, right| match (&left.kind, &right.kind) {
        (RepoEntryKind::Dir, RepoEntryKind::File) => std::cmp::Ordering::Less,
        (RepoEntryKind::File, RepoEntryKind::Dir) => std::cmp::Ordering::Greater,
        _ => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
    });

    let mut next = (*dir_cache).clone();
    next.insert(key, entries.clone());
    dir_cache.set(next);

    Ok(entries)
}

async fn get_repo_file(
    owner: &str,
    repo: &str,
    repo_path: &str,
    file_cache: UseStateHandle<FileCache>,
) -> Result<String, String> {
    let key = format!("{owner}/{repo}:{repo_path}");
    if let Some(content) = file_cache.get(&key).cloned() {
        return Ok(content);
    }

    let url = github_contents_url(owner, repo, repo_path);
    let response = Request::get(url.as_str())
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|error| format!("github request failed: {error}"))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("github response decode failed: {error}"))?;

    if !(200..300).contains(&status) {
        if let Ok(parsed) = serde_json::from_str::<GithubApiError>(body.as_str()) {
            return Err(format!("github: {}", parsed.message));
        }
        return Err(format!("github: status {status}"));
    }

    let parsed = serde_json::from_str::<GithubContentItem>(body.as_str())
        .map_err(|_| String::from("cat: unable to decode file metadata"))?;

    if parsed.kind != "file" {
        return Err(String::from("cat: not a file"));
    }

    if parsed.size.unwrap_or(0) > 700_000 {
        return Err(String::from("cat: file too large for terminal preview"));
    }

    let Some(content) = parsed.content else {
        return Err(String::from("cat: no textual content available"));
    };

    let encoding = parsed.encoding.unwrap_or_default();
    let decoded = if encoding == "base64" {
        let bytes = STANDARD
            .decode(content.replace('\n', "").as_bytes())
            .map_err(|_| String::from("cat: failed to decode base64 content"))?;
        String::from_utf8(bytes).map_err(|_| String::from("cat: binary or non-utf8 file"))?
    } else {
        content
    };

    let final_content = if decoded.len() > 50_000 {
        let mut trimmed = decoded.chars().take(50_000).collect::<String>();
        trimmed.push_str("\n\n[truncated]");
        trimmed
    } else {
        decoded
    };

    let mut next = (*file_cache).clone();
    next.insert(key, final_content.clone());
    file_cache.set(next);

    Ok(final_content)
}

fn github_contents_url(owner: &str, repo: &str, path: &str) -> String {
    if path.is_empty() {
        return format!("https://api.github.com/repos/{owner}/{repo}/contents");
    }

    let encoded = path
        .split('/')
        .map(|segment| urlencoding::encode(segment).to_string())
        .collect::<Vec<_>>()
        .join("/");

    format!("https://api.github.com/repos/{owner}/{repo}/contents/{encoded}")
}

fn parse_count_and_file(args: &[String], command: &str) -> Result<(usize, String), String> {
    if args.is_empty() {
        return Err(format!("{command}: missing file operand"));
    }

    if args.len() >= 3 && args[0] == "-n" {
        let count = args[1]
            .parse::<usize>()
            .map_err(|_| format!("{command}: invalid number '{}'", args[1]))?;
        return Ok((count.max(1), args[2].clone()));
    }

    Ok((10, args[0].clone()))
}

fn parse_grep_args(args: &[String]) -> Result<(String, bool, String), String> {
    if args.is_empty() {
        return Err(String::from("grep: missing pattern"));
    }

    let mut case_insensitive = false;
    let mut index = 0usize;
    if args[0] == "-i" {
        case_insensitive = true;
        index += 1;
    }

    if args.len() < index + 2 {
        return Err(String::from("grep: usage grep [-i] <pattern> <file>"));
    }
    if args.len() > index + 2 {
        return Err(String::from("grep: too many arguments"));
    }

    let mut pattern = args[index].clone();
    if case_insensitive {
        pattern = pattern.to_ascii_lowercase();
    }
    let file = args[index + 1].clone();
    Ok((pattern, case_insensitive, file))
}

fn parse_mkdir_args(args: &[String]) -> Result<(String, bool), String> {
    if args.is_empty() {
        return Err(String::from("mkdir: missing operand"));
    }

    if args[0] == "-p" {
        let Some(path) = args.get(1) else {
            return Err(String::from("mkdir: missing path after -p"));
        };
        if args.len() > 2 {
            return Err(String::from("mkdir: too many arguments"));
        }
        return Ok((path.clone(), true));
    }

    if args[0].starts_with('-') {
        return Err(format!("mkdir: unsupported option '{}'", args[0]));
    }

    if args.len() > 1 {
        return Err(String::from("mkdir: too many arguments"));
    }

    Ok((args[0].clone(), false))
}

async fn autocomplete_input(
    raw: String,
    cwd: Vec<String>,
    projects: Vec<Project>,
    dir_cache: UseStateHandle<DirCache>,
    overrides: UseStateHandle<OverrideMap>,
) -> AutocompleteOutcome {
    let mut outcome = AutocompleteOutcome {
        input: raw.clone(),
        lines: Vec::new(),
    };

    let spans = token_spans(raw.as_str());
    if spans.is_empty() {
        return outcome;
    }

    let trailing_space = raw.chars().last().is_some_and(char::is_whitespace);

    if spans.len() == 1 && !trailing_space {
        let (start, end) = spans[0];
        let prefix = &raw[start..end];
        let matches = shell_commands()
            .iter()
            .filter(|candidate| candidate.starts_with(prefix))
            .map(|candidate| (*candidate).to_string())
            .collect::<Vec<_>>();

        if matches.is_empty() {
            return outcome;
        }

        if matches.len() == 1 {
            outcome.input = replace_span(
                raw.as_str(),
                start,
                end,
                format!("{} ", matches[0]).as_str(),
            );
            return outcome;
        }

        let common = longest_common_prefix(&matches);
        if common.len() > prefix.len() {
            outcome.input = replace_span(raw.as_str(), start, end, common.as_str());
        }
        outcome.lines.push(TerminalLine::muted(matches.join("  ")));
        return outcome;
    }

    let command = &raw[spans[0].0..spans[0].1];
    let args = spans
        .iter()
        .skip(1)
        .map(|(start, end)| raw[*start..*end].to_string())
        .collect::<Vec<_>>();

    let (arg_index, replace_start, replace_end, raw_prefix) = if trailing_space {
        (
            spans.len().saturating_sub(1),
            raw.len(),
            raw.len(),
            String::new(),
        )
    } else {
        let (start, end) = *spans.last().expect("spans has at least one item");
        (
            spans.len().saturating_sub(2),
            start,
            end,
            raw[start..end].to_string(),
        )
    };

    if !command_supports_path_completion(command, &args, arg_index) {
        return outcome;
    }

    let (dir_prefix, name_prefix) = split_completion_prefix(raw_prefix.as_str());
    let target_dir = if dir_prefix.is_empty() {
        cwd.clone()
    } else {
        normalize_path(cwd.as_ref(), dir_prefix.as_str())
    };

    let mut matches = match list_path(&target_dir, &projects, dir_cache, overrides).await {
        Ok(entries) => entries,
        Err(_) => return outcome,
    };

    matches.retain(|entry| entry.starts_with(name_prefix.as_str()));

    if command == "cd" {
        matches.retain(|entry| entry.ends_with('/'));
    }

    sort_entries(&mut matches);
    matches.dedup();

    if matches.is_empty() {
        return outcome;
    }

    if matches.len() == 1 {
        let is_dir = matches[0].ends_with('/');
        let mut completed = format!("{}{}", dir_prefix, matches[0]);
        if !is_dir {
            completed.push(' ');
        }
        outcome.input = replace_span(raw.as_str(), replace_start, replace_end, completed.as_str());
        return outcome;
    }

    let common = longest_common_prefix(&matches);
    if common.len() > name_prefix.len() {
        let completed = format!("{dir_prefix}{common}");
        outcome.input = replace_span(raw.as_str(), replace_start, replace_end, completed.as_str());
    }

    for row in format_ls_grid(&matches) {
        outcome.lines.push(TerminalLine::ls(row));
    }

    outcome
}

fn shell_commands() -> &'static [&'static str] {
    &[
        "help", "about", "pwd", "ls", "cd", "cat", "head", "tail", "grep", "wc", "mkdir", "touch",
        "rm", "stat", "tree", "nvim", "vim", "history", "whoami", "echo", "clear",
    ]
}

fn token_spans(raw: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = None::<usize>;
    for (index, character) in raw.char_indices() {
        if character.is_whitespace() {
            if let Some(open) = start.take() {
                spans.push((open, index));
            }
        } else if start.is_none() {
            start = Some(index);
        }
    }

    if let Some(open) = start {
        spans.push((open, raw.len()));
    }

    spans
}

fn replace_span(raw: &str, start: usize, end: usize, replacement: &str) -> String {
    let mut output = String::with_capacity(raw.len() + replacement.len() + 4);
    output.push_str(&raw[..start]);
    output.push_str(replacement);
    output.push_str(&raw[end..]);
    output
}

fn longest_common_prefix(words: &[String]) -> String {
    let Some(first) = words.first() else {
        return String::new();
    };

    let mut prefix = first.clone();
    for word in words.iter().skip(1) {
        let mut length = 0usize;
        for (left, right) in prefix.chars().zip(word.chars()) {
            if left != right {
                break;
            }
            length += left.len_utf8();
        }
        prefix.truncate(length);
        if prefix.is_empty() {
            break;
        }
    }

    prefix
}

fn split_completion_prefix(prefix: &str) -> (String, String) {
    if let Some(index) = prefix.rfind('/') {
        return (
            prefix[..=index].to_string(),
            prefix[index + 1..].to_string(),
        );
    }

    (String::new(), prefix.to_string())
}

fn command_supports_path_completion(command: &str, args: &[String], arg_index: usize) -> bool {
    match command {
        "cd" | "cat" | "nvim" | "vim" | "touch" | "rm" | "wc" | "stat" | "tree" => arg_index == 0,
        "head" | "tail" => {
            if args.first().is_some_and(|arg| arg == "-n") {
                arg_index == 2
            } else {
                arg_index == 0
            }
        }
        "grep" => {
            if args.first().is_some_and(|arg| arg == "-i") {
                arg_index == 2
            } else {
                arg_index == 1
            }
        }
        "mkdir" => {
            if args.first().is_some_and(|arg| arg == "-p") {
                arg_index == 1
            } else {
                arg_index == 0
            }
        }
        "ls" => {
            let mut existing_paths = 0usize;
            for (index, arg) in args.iter().enumerate() {
                if index == arg_index {
                    continue;
                }
                if !arg.starts_with('-') {
                    existing_paths += 1;
                }
            }
            if existing_paths >= 1 {
                return false;
            }

            if let Some(current) = args.get(arg_index) {
                !current.starts_with('-')
            } else {
                true
            }
        }
        _ => false,
    }
}

fn format_ls_grid(entries: &[String]) -> Vec<Vec<LsToken>> {
    if entries.is_empty() {
        return Vec::new();
    }

    let columns = if entries.len() >= 22 {
        4
    } else if entries.len() >= 12 {
        3
    } else if entries.len() >= 5 {
        2
    } else {
        1
    };

    let rows = entries.len().div_ceil(columns);
    let mut column_widths = vec![0usize; columns];
    for column in 0..columns {
        for row in 0..rows {
            let index = column * rows + row;
            if index >= entries.len() {
                continue;
            }
            let width = entries[index].chars().count();
            column_widths[column] = column_widths[column].max(width);
        }
    }

    let mut output = Vec::new();

    for row in 0..rows {
        let mut row_tokens = Vec::new();
        for column in 0..columns {
            let index = column * rows + row;
            if index >= entries.len() {
                continue;
            }
            let width_ch = if column + 1 == columns {
                0
            } else {
                column_widths[column] + 4
            };
            row_tokens.push(ls_token(entries[index].as_str(), width_ch));
        }
        output.push(row_tokens);
    }

    output
}

fn ls_token(name: &str, width_ch: usize) -> LsToken {
    let is_dir = name.ends_with('/');
    let base = name.trim_end_matches('/');
    let lower = base.to_ascii_lowercase();
    let ext = lower.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("");

    if is_dir {
        return LsToken {
            icon: String::from("󰉋"),
            name: name.to_string(),
            class_name: "ls-dir",
            width_ch,
        };
    }

    let (icon, class_name) = if ["rs"].contains(&ext) {
        ("", "ls-src")
    } else if ["c", "cc", "cpp", "cxx", "h", "hpp", "hh", "cs", "odin"].contains(&ext) {
        ("󰙱", "ls-src")
    } else if ["md", "txt", "adoc"].contains(&ext) || lower.contains("readme") {
        ("󰈙", "ls-doc")
    } else if ["json", "toml", "yaml", "yml", "ini", "cfg", "lock"].contains(&ext) {
        ("", "ls-config")
    } else if ["png", "jpg", "jpeg", "gif", "webp", "svg", "ico"].contains(&ext) {
        ("󰈟", "ls-img")
    } else if ["sh", "bash", "zsh", "ps1", "bat"].contains(&ext) {
        ("", "ls-bin")
    } else {
        ("󰈔", "ls-file")
    };

    LsToken {
        icon: icon.to_string(),
        name: name.to_string(),
        class_name,
        width_ch,
    }
}

fn editor_tree_files(
    base_path: &[String],
    entries: Vec<String>,
    depth: usize,
    projects: &[Project],
) -> Vec<EditorTreeEntry> {
    entries
        .into_iter()
        .map(|entry| {
            let is_dir = entry.ends_with('/');
            let name = entry.trim_end_matches('/').to_string();
            let mut path = base_path.to_vec();
            path.push(name.clone());
            let private_repo = is_private_repo_path(&path, projects);
            EditorTreeEntry {
                name,
                path,
                depth,
                is_dir,
                private_repo,
                expanded: false,
                loading: false,
            }
        })
        .collect::<Vec<_>>()
}

fn tree_entry_label(entry: &EditorTreeEntry) -> String {
    if entry.is_dir {
        format!("{}/", entry.name)
    } else {
        entry.name.clone()
    }
}

fn clear_tree_children(entries: &mut Vec<EditorTreeEntry>, parent_index: usize) {
    let parent_depth = entries[parent_index].depth;
    let mut end = parent_index + 1;
    while end < entries.len() && entries[end].depth > parent_depth {
        end += 1;
    }
    entries.drain(parent_index + 1..end);
}

fn parent_tree_index(entries: &[EditorTreeEntry], index: usize) -> Option<usize> {
    if entries.is_empty() || index >= entries.len() {
        return None;
    }

    let depth = entries[index].depth;
    if depth == 0 {
        return None;
    }

    let mut cursor = index;
    while cursor > 0 {
        cursor -= 1;
        if entries[cursor].depth + 1 == depth {
            return Some(cursor);
        }
    }

    None
}

fn project_is_private(project: &Project) -> bool {
    !project_has_valid_link(project)
}

fn is_private_repo_path(path: &[String], projects: &[Project]) -> bool {
    if path.len() != 2 || path.first().map(|segment| segment.as_str()) != Some("projects") {
        return false;
    }

    path.get(1)
        .and_then(|repo_name| find_project(projects, repo_name.as_str()))
        .map(project_is_private)
        .unwrap_or(false)
}

fn is_private_repo_ls_entry(
    parent_path: &[String],
    entry_name: &str,
    projects: &[Project],
) -> bool {
    if parent_path.len() != 1
        || parent_path.first().map(|segment| segment.as_str()) != Some("projects")
    {
        return false;
    }
    if !entry_name.ends_with('/') {
        return false;
    }
    let repo_name = entry_name.trim_end_matches('/');
    if repo_name.is_empty() {
        return false;
    }

    find_project(projects, repo_name)
        .map(project_is_private)
        .unwrap_or(false)
}

fn ls_token_for_entry(
    name: &str,
    width_ch: usize,
    parent_path: &[String],
    projects: &[Project],
) -> LsToken {
    let mut token = ls_token(name, width_ch);
    if is_private_repo_ls_entry(parent_path, name, projects) {
        token.class_name = "ls-private-repo";
    }
    token
}

fn set_prompt_focus(prompt_input_ref: &NodeRef) {
    if let Some(input) = prompt_input_ref.cast::<HtmlInputElement>() {
        let _ = input.focus();
        let len = input.value().len() as u32;
        let _ = input.set_selection_range(len, len);
    }
}

fn activate_tree_entry(
    state: &mut EditorState,
    selected_index: usize,
    identity: Identity,
    projects: Vec<Project>,
    dir_cache: UseStateHandle<DirCache>,
    file_cache: UseStateHandle<FileCache>,
    overrides: UseStateHandle<OverrideMap>,
    editor: UseStateHandle<Option<EditorState>>,
) {
    let Some(selected) = state.tree_entries.get(selected_index).cloned() else {
        return;
    };

    state.tree_selection = selected_index.min(state.tree_entries.len().saturating_sub(1));

    if selected.is_dir {
        if selected.expanded {
            clear_tree_children(&mut state.tree_entries, selected_index);
            if let Some(parent) = state.tree_entries.get_mut(selected_index) {
                parent.expanded = false;
                parent.loading = false;
            }
            state.status = format!("NvimTree: {}", display_path(&selected.path));
            return;
        }

        if let Some(parent) = state.tree_entries.get_mut(selected_index) {
            parent.loading = true;
        }
        state.status = format!("NvimTree: loading {}", display_path(&selected.path));
        let selected_path = selected.path.clone();
        let dir_cache = dir_cache.clone();
        let overrides = overrides.clone();
        let editor_for_async = editor.clone();

        spawn_local(async move {
            match list_path(&selected_path, &projects, dir_cache, overrides).await {
                Ok(entries) => {
                    if let Some(mut live) = (*editor_for_async).clone() {
                        let Some(index) = live
                            .tree_entries
                            .iter()
                            .position(|entry| entry.path == selected_path)
                        else {
                            return;
                        };

                        clear_tree_children(&mut live.tree_entries, index);
                        let child_depth = live.tree_entries[index].depth + 1;
                        let children =
                            editor_tree_files(&selected_path, entries, child_depth, &projects);
                        live.tree_entries.splice(index + 1..index + 1, children);
                        if let Some(entry) = live.tree_entries.get_mut(index) {
                            entry.expanded = true;
                            entry.loading = false;
                        }
                        live.tree_selection = index.min(live.tree_entries.len().saturating_sub(1));
                        live.tree_focused = true;
                        live.status = format!("NvimTree: {}", display_path(&selected_path));
                        editor_for_async.set(Some(live));
                    }
                }
                Err(error) => {
                    if let Some(mut live) = (*editor_for_async).clone() {
                        if let Some(index) = live
                            .tree_entries
                            .iter()
                            .position(|entry| entry.path == selected_path)
                        {
                            if let Some(entry) = live.tree_entries.get_mut(index) {
                                entry.loading = false;
                            }
                        }
                        live.status =
                            format!("NvimTree error ({}): {error}", display_path(&selected_path));
                        editor_for_async.set(Some(live));
                    }
                }
            }
        });
        return;
    }

    let requested_file = selected.path.clone();
    state.status = format!("opening {}", display_path(&requested_file));

    let editor_for_async = editor.clone();
    let file_cache = file_cache.clone();
    let overrides = overrides.clone();
    let dir_cache_for_async = dir_cache.clone();
    spawn_local(async move {
        match cat_path(
            &requested_file,
            &identity,
            &projects,
            file_cache,
            overrides.clone(),
        )
        .await
        {
            Ok(content) => {
                if let Some(mut live) = (*editor_for_async).clone() {
                    let Some(index) = live
                        .tree_entries
                        .iter()
                        .position(|entry| entry.path == requested_file)
                    else {
                        return;
                    };
                    live.path = requested_file.clone();
                    live.content = content;
                    live.cursor = 0;
                    live.preferred_column = None;
                    live.mode = EditorMode::Normal;
                    live.status = format!("opened {}", display_path(&live.path));
                    live.dirty = false;
                    live.tree_selection = index;
                    live.tree_focused = false;
                    live.leader_pending = false;
                    live.leader_window_pending = false;
                    live.g_pending = false;
                    editor_for_async.set(Some(live));
                }
            }
            Err(error) => {
                if error.contains("is a directory") {
                    match list_path(
                        &requested_file,
                        &projects,
                        dir_cache_for_async,
                        overrides.clone(),
                    )
                    .await
                    {
                        Ok(entries) => {
                            if let Some(mut live) = (*editor_for_async).clone() {
                                let Some(index) = live
                                    .tree_entries
                                    .iter()
                                    .position(|entry| entry.path == requested_file)
                                else {
                                    return;
                                };

                                if let Some(current) = live.tree_entries.get_mut(index) {
                                    current.is_dir = true;
                                }
                                clear_tree_children(&mut live.tree_entries, index);
                                let child_depth = live.tree_entries[index].depth + 1;
                                let children = editor_tree_files(
                                    &requested_file,
                                    entries,
                                    child_depth,
                                    &projects,
                                );
                                live.tree_entries.splice(index + 1..index + 1, children);
                                if let Some(current) = live.tree_entries.get_mut(index) {
                                    current.expanded = true;
                                    current.loading = false;
                                }
                                live.tree_selection = index;
                                live.tree_focused = true;
                                live.status =
                                    format!("NvimTree: {}", display_path(&requested_file));
                                editor_for_async.set(Some(live));
                            }
                        }
                        Err(list_error) => {
                            if let Some(mut live) = (*editor_for_async).clone() {
                                live.status = format!(
                                    "NvimTree error ({}): {list_error}",
                                    display_path(&requested_file)
                                );
                                editor_for_async.set(Some(live));
                            }
                        }
                    }
                    return;
                }
                if let Some(mut live) = (*editor_for_async).clone() {
                    live.status = format!(
                        "NvimTree error ({}): {error}",
                        display_path(&requested_file)
                    );
                    editor_for_async.set(Some(live));
                }
            }
        }
    });
}

fn merge_virtual_entries(entries: &mut Vec<String>, dir_path: &[String], overrides: &OverrideMap) {
    let mut seen = entries.iter().cloned().collect::<HashSet<_>>();
    for item in immediate_virtual_entries(dir_path, overrides) {
        if seen.insert(item.clone()) {
            entries.push(item);
        }
    }
}

fn immediate_virtual_entries(dir_path: &[String], overrides: &OverrideMap) -> Vec<String> {
    let base = absolute_path(dir_path);
    let prefix = if base == "/" {
        String::from("/")
    } else {
        format!("{base}/")
    };

    let mut output = HashSet::new();
    for key in overrides.keys() {
        let Some(rest) = key.strip_prefix(prefix.as_str()) else {
            continue;
        };

        if rest.is_empty() {
            continue;
        }

        if let Some((head, _)) = rest.split_once('/') {
            output.insert(format!("{head}/"));
        } else {
            output.insert(rest.to_string());
        }
    }

    let mut final_entries = output.into_iter().collect::<Vec<_>>();
    sort_entries(&mut final_entries);
    final_entries
}

fn sort_entries(entries: &mut [String]) {
    entries.sort_by(|left, right| {
        let left_dir = left.ends_with('/');
        let right_dir = right.ends_with('/');

        match (left_dir, right_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => left.to_ascii_lowercase().cmp(&right.to_ascii_lowercase()),
        }
    });
}

fn save_override(path: &[String], content: &str, overrides: UseStateHandle<OverrideMap>) {
    let mut next = (*overrides).clone();
    next.insert(absolute_path(path), content.to_string());
    overrides.set(next);
}

fn remove_override(path: &[String], overrides: UseStateHandle<OverrideMap>) -> bool {
    let key = absolute_path(path);
    let mut next = (*overrides).clone();
    let removed = next.remove(key.as_str()).is_some();
    if removed {
        overrides.set(next);
    }
    removed
}

fn create_virtual_dir(path: &[String], overrides: UseStateHandle<OverrideMap>) {
    if path.is_empty() {
        return;
    }
    let mut next = (*overrides).clone();
    next.insert(virtual_dir_key(path), String::new());
    overrides.set(next);
}

fn virtual_dir_key(path: &[String]) -> String {
    format!("{}/", absolute_path(path))
}

fn is_virtual_dir(path: &[String], overrides: &OverrideMap) -> bool {
    if path.is_empty() {
        return true;
    }

    let key = virtual_dir_key(path);
    if overrides.contains_key(key.as_str()) {
        return true;
    }

    let prefix = format!("{}/", absolute_path(path));
    overrides
        .keys()
        .any(|entry| entry.starts_with(prefix.as_str()))
}

fn absolute_path(path: &[String]) -> String {
    if path.is_empty() {
        String::from("/")
    } else {
        format!("/{}", path.join("/"))
    }
}

fn help_lines() -> Vec<TerminalLine> {
    vec![
        TerminalLine::section("[commands]"),
        TerminalLine::output("help                         show command list"),
        TerminalLine::output("about                        print profile summary"),
        TerminalLine::output("pwd                          print current directory"),
        TerminalLine::output("ls [-1] [path]               list files/directories"),
        TerminalLine::output("cd [path]                    change directory"),
        TerminalLine::output("cat <file>                   print file contents"),
        TerminalLine::output("head [-n N] <file>           first N lines"),
        TerminalLine::output("tail [-n N] <file>           last N lines"),
        TerminalLine::output("grep [-i] <pattern> <file>   search file contents"),
        TerminalLine::output("wc <file>                    line/word/byte counts"),
        TerminalLine::output("mkdir [-p] <dir>             create virtual directory"),
        TerminalLine::output("touch <file>                 create virtual file"),
        TerminalLine::output("rm <path>                    remove virtual file/dir"),
        TerminalLine::output("stat [path]                  file/directory metadata"),
        TerminalLine::output("tree [path]                  one-level tree"),
        TerminalLine::output("nvim <file>                  open modal editor"),
        TerminalLine::output("history                      command history"),
        TerminalLine::output("whoami                       print handle"),
        TerminalLine::output("echo <text>                  print text"),
        TerminalLine::output("clear                        clear terminal"),
        TerminalLine::muted("filesystem root: /about.txt and /projects"),
    ]
}

fn about_lines(identity: &Identity, projects: &[Project]) -> Vec<TerminalLine> {
    let mut lines = Vec::new();
    lines.push(TerminalLine::section("[identity]"));
    lines.push(TerminalLine::identity(format!("handle: {}", identity.handle)));
    lines.push(TerminalLine::identity(format!(
        "aliases: {}",
        identity.aliases.join(", ")
    )));
    lines.push(TerminalLine::identity(format!(
        "tagline: {}",
        identity.tagline
    )));
    lines.push(TerminalLine::identity(format!(
        "location: {}",
        identity.location
    )));
    lines.push(TerminalLine::identity(STUDIES_LINE));
    lines.push(TerminalLine::identity(format!(
        "focus: {}",
        identity.focus.join(" | ")
    )));

    let (featured, remaining) = prioritized_project_sections(projects);

    lines.push(TerminalLine::section("[projects]"));
    if featured.is_empty() {
        lines.push(TerminalLine::muted("(none)"));
    } else {
        for project in featured {
            lines.push(TerminalLine::project(project));
        }
    }

    if !remaining.is_empty() {
        lines.push(TerminalLine::section("[other projects]"));
        for project in remaining {
            lines.push(TerminalLine::project(project));
        }
    }

    lines.push(TerminalLine::muted(format!(
        "scope: {}",
        identity.scope_note
    )));
    lines.push(TerminalLine::muted(format!(
        "snapshot: {}",
        identity.snapshot_date
    )));
    lines
}

fn about_text(identity: &Identity, projects: &[Project]) -> String {
    let (featured, remaining) = prioritized_project_sections(projects);

    let mut output = Vec::new();
    output.push(format!("handle: {}", identity.handle));
    output.push(format!("aliases: {}", identity.aliases.join(", ")));
    output.push(format!("tagline: {}", identity.tagline));
    output.push(format!("location: {}", identity.location));
    output.push(String::from(STUDIES_LINE));
    output.push(format!("focus: {}", identity.focus.join(" | ")));
    output.push(String::new());
    output.push(String::from("projects:"));

    for project in featured {
        output.push(format!(
            "- {}{} [{}] - {}",
            project.name,
            project_badge_text(project),
            project.primary_stack,
            project.description
        ));
    }
    if !remaining.is_empty() {
        output.push(String::new());
        output.push(String::from("other projects:"));
        for project in remaining {
            output.push(format!(
                "- {}{} [{}] - {}",
                project.name,
                project_badge_text(project),
                project.primary_stack,
                project.description
            ));
        }
    }
    output.push(String::new());
    output.push(format!("scope: {}", identity.scope_note));
    output.push(format!("snapshot: {}", identity.snapshot_date));
    output.join("\n")
}

fn prioritized_project_sections<'a>(
    projects: &'a [Project],
) -> (Vec<&'a Project>, Vec<&'a Project>) {
    let mut featured = Vec::new();
    let mut selected = HashSet::new();

    for project in projects
        .iter()
        .filter(|project| resolved_project_context(project) == ProjectContext::Professional)
    {
        if selected.insert(project_list_key(project)) {
            featured.push(project);
        }
    }

    for project in projects
        .iter()
        .filter(|project| project_has_valid_link(project) && project.era != ProjectEra::Legacy)
    {
        if selected.insert(project_list_key(project)) {
            featured.push(project);
        }
    }

    let mut remaining = Vec::new();
    for project in projects {
        if !selected.contains(&project_list_key(project)) {
            remaining.push(project);
        }
    }

    (featured, remaining)
}

fn project_list_key(project: &Project) -> String {
    format!(
        "{}:{}",
        project.owner.to_ascii_lowercase(),
        project.name.to_ascii_lowercase()
    )
}

fn project_meta(project: &Project) -> String {
    let team = resolved_project_team(project);
    let context = resolved_project_context(project);
    format!(
        "name: {}\nowner: {}\nstack: {}\nteam: {}\ncontext: {}\nera: {}\nmarkers: {}\ndescription: {}\nurl: {}",
        project.name,
        project.owner,
        project.primary_stack,
        project_team_label(team),
        project_context_label(context),
        project_era_label(project.era),
        project_badges(project).join("|"),
        project.description,
        project.url
    )
}

fn project_badges(project: &Project) -> Vec<&'static str> {
    let mut badges = vec![
        project_team_tag(resolved_project_team(project)),
        project_context_tag(resolved_project_context(project)),
    ];

    if project.era == ProjectEra::Legacy {
        badges.push("LEGACY");
    }

    badges
}

fn resolved_project_team(project: &Project) -> ProjectTeam {
    if let Some(team) = project.team {
        return team;
    }

    match project.source {
        Some(ProjectSourceLegacy::Team) => ProjectTeam::Team,
        _ => ProjectTeam::Solo,
    }
}

fn resolved_project_context(project: &Project) -> ProjectContext {
    if let Some(context) = project.context {
        return context;
    }

    match project.source {
        Some(ProjectSourceLegacy::University) => ProjectContext::University,
        Some(ProjectSourceLegacy::Team) => ProjectContext::Professional,
        _ => ProjectContext::Personal,
    }
}

fn project_badge_text(project: &Project) -> String {
    let badges = project_badges(project);
    if badges.is_empty() {
        String::new()
    } else {
        format!(" [{}]", badges.join("|"))
    }
}

fn project_team_tag(team: ProjectTeam) -> &'static str {
    match team {
        ProjectTeam::Solo => "SOLO",
        ProjectTeam::Team => "TEAM",
    }
}

fn project_context_tag(context: ProjectContext) -> &'static str {
    match context {
        ProjectContext::Personal => "PERSONAL",
        ProjectContext::University => "UNI",
        ProjectContext::Professional => "PROFESSIONAL",
    }
}

fn project_team_label(team: ProjectTeam) -> &'static str {
    match team {
        ProjectTeam::Solo => "solo",
        ProjectTeam::Team => "team",
    }
}

fn project_context_label(context: ProjectContext) -> &'static str {
    match context {
        ProjectContext::Personal => "personal",
        ProjectContext::University => "university",
        ProjectContext::Professional => "professional",
    }
}

fn project_era_label(era: ProjectEra) -> &'static str {
    match era {
        ProjectEra::Current => "current",
        ProjectEra::Legacy => "legacy",
    }
}

fn render_project_line(project: &Project) -> Html {
    let badges = project_badges(project);
    let has_link = project_has_valid_link(project);

    html! {
        <div class="line line-project">
            <div class="project-head">
                {
                    if has_link {
                        html! {
                            <a class="project-name" href={project.url.clone()} target="_blank" rel="noopener noreferrer">
                                {project.name.clone()}
                            </a>
                        }
                    } else {
                        html! { <span class="project-name">{project.name.clone()}</span> }
                    }
                }
                <span class="project-badges">
                    {for badges.into_iter().map(render_project_badge)}
                </span>
            </div>
            <div class="project-stack">{project.primary_stack.clone()}</div>
            <div class="project-description">{project.description.clone()}</div>
        </div>
    }
}

fn project_has_valid_link(project: &Project) -> bool {
    let url = project.url.trim();
    url.starts_with("https://") || url.starts_with("http://")
}

fn render_project_badge(badge: &str) -> Html {
    let class = match badge {
        "SOLO" => "badge-team-solo",
        "TEAM" => "badge-team-team",
        "PERSONAL" => "badge-context-personal",
        "UNI" => "badge-context-uni",
        "PROFESSIONAL" => "badge-context-professional",
        "LEGACY" => "badge-era-legacy",
        _ => "badge-generic",
    };

    html! { <span class={classes!("project-badge", class)}>{badge}</span> }
}

fn project_matches_filters(project: &Project, filters: &ProjectFilters) -> bool {
    let team = resolved_project_team(project);
    let context = resolved_project_context(project);

    if let Some(filter_team) = filters.team {
        if filter_team != team {
            return false;
        }
    }

    if let Some(filter_context) = filters.context {
        if filter_context != context {
            return false;
        }
    }

    !filters.legacy_only || project.era == ProjectEra::Legacy
}

fn render_line(line: &TerminalLine) -> Html {
    match line.kind {
        LineKind::Ls => html! {
            <div class="line line-ls-list">
                {for line.ls_tokens.iter().map(|token| html! {
                    <div class="line-ls-row">{render_ls_token(token)}</div>
                })}
            </div>
        },
        LineKind::Section => {
            if line.text == "[other projects]" {
                html! {
                    <div class="line line-section-divider">
                        <span>{"other projects"}</span>
                    </div>
                }
            } else {
                html! { <p class="line line-section">{line.text.clone()}</p> }
            }
        }
        LineKind::Project => {
            if let Some(project) = line.project.as_ref() {
                render_project_line(project)
            } else {
                html! { <p class="line line-muted">{"[invalid project line]"}</p> }
            }
        }
        LineKind::Identity => render_identity_line(line.text.as_str()),
        _ => {
            let class = match line.kind {
                LineKind::Command => "line line-command",
                LineKind::Output => "line line-output",
                LineKind::Identity => "line line-identity",
                LineKind::Error => "line line-error",
                LineKind::Muted => "line line-muted",
                LineKind::Ls => "line",
                LineKind::Project => "line",
                LineKind::Section => "line",
            };

            html! { <p class={class}>{line.text.clone()}</p> }
        }
    }
}

fn render_identity_line(text: &str) -> Html {
    if let Some((label, value)) = text.split_once(':') {
        let tone_class = identity_tone_class(label);
        html! {
            <p class={classes!("line", "line-identity", tone_class)}>
                <span class="identity-label">{format!("{label}:")}</span>
                <span class="identity-value">{format!(" {}", value.trim_start())}</span>
            </p>
        }
    } else {
        html! { <p class={classes!("line", "line-identity", "identity-tone-default")}>{text.to_string()}</p> }
    }
}

fn identity_tone_class(label: &str) -> &'static str {
    let normalized = label.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "handle" => "identity-tone-1",
        "aliases" => "identity-tone-2",
        "tagline" => "identity-tone-3",
        "location" => "identity-tone-4",
        "bachelor" => "identity-tone-5",
        "focus" => "identity-tone-6",
        _ => "identity-tone-default",
    }
}

fn render_ls_token(token: &LsToken) -> Html {
    let style = if token.width_ch == 0 {
        AttrValue::from("")
    } else {
        AttrValue::from(format!("min-width: {}ch;", token.width_ch))
    };

    html! {
        <span class={classes!("ls-entry", token.class_name)} style={style}>
            <span class="ls-icon">{token.icon.clone()}</span>
            <span class="ls-name">{token.name.clone()}</span>
        </span>
    }
}

fn render_tree_entry(
    index: usize,
    entry: &EditorTreeEntry,
    selected: bool,
    on_click: Callback<MouseEvent>,
) -> Html {
    let token = ls_token(tree_entry_label(entry).as_str(), 0);
    let depth_style = AttrValue::from(format!("padding-left: {}ch;", entry.depth * 2));
    let fold_marker = if entry.is_dir {
        if entry.loading {
            "~"
        } else if entry.expanded {
            "v"
        } else {
            ">"
        }
    } else {
        " "
    };
    html! {
        <div
            class={classes!(
                "tree-entry",
                token.class_name,
                if entry.private_repo { "tree-private-repo" } else { "" },
                if selected { "is-selected" } else { "" }
            )}
            data-index={index.to_string()}
            onclick={on_click}
        >
            <span class="tree-cursor">{if selected { ">" } else { " " }}</span>
            <span class="tree-fold">{fold_marker}</span>
            <span class="tree-icon">{token.icon}</span>
            <span class="tree-name" style={depth_style}>{token.name}</span>
        </div>
    }
}

fn detect_language(path: &[String]) -> LanguageKind {
    let Some(name) = path.last() else {
        return LanguageKind::Plain;
    };

    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "cargo.toml" => return LanguageKind::Toml,
        "package.json" | "tsconfig.json" | "composer.json" => return LanguageKind::Json,
        _ => {}
    }

    let extension = lower.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("");
    match extension {
        "js" | "mjs" | "cjs" => LanguageKind::JavaScript,
        "cpp" | "cxx" | "cc" | "hpp" | "hh" | "hxx" => LanguageKind::Cpp,
        "c" | "h" => LanguageKind::C,
        "rs" => LanguageKind::Rust,
        "odin" => LanguageKind::Odin,
        "cs" => LanguageKind::CSharp,
        "java" => LanguageKind::Java,
        "kt" | "kts" => LanguageKind::Kotlin,
        "ts" | "tsx" => LanguageKind::TypeScript,
        "php" | "phtml" => LanguageKind::Php,
        "zig" => LanguageKind::Zig,
        "md" | "markdown" => LanguageKind::Markdown,
        "json" | "jsonc" => LanguageKind::Json,
        "xml" => LanguageKind::Xml,
        "html" | "htm" | "xhtml" => LanguageKind::Html,
        "css" | "scss" => LanguageKind::Css,
        "dart" => LanguageKind::Dart,
        "toml" => LanguageKind::Toml,
        "yaml" | "yml" => LanguageKind::Yaml,
        _ => LanguageKind::Plain,
    }
}

fn language_label(language: LanguageKind) -> &'static str {
    match language {
        LanguageKind::Plain => "plain",
        LanguageKind::JavaScript => "javascript",
        LanguageKind::Cpp => "c++",
        LanguageKind::C => "c",
        LanguageKind::Rust => "rust",
        LanguageKind::Odin => "odin",
        LanguageKind::CSharp => "c#",
        LanguageKind::Java => "java",
        LanguageKind::Kotlin => "kotlin",
        LanguageKind::TypeScript => "typescript",
        LanguageKind::Php => "php",
        LanguageKind::Zig => "zig",
        LanguageKind::Markdown => "markdown",
        LanguageKind::Json => "json",
        LanguageKind::Xml => "xml",
        LanguageKind::Html => "html",
        LanguageKind::Css => "css",
        LanguageKind::Dart => "dart",
        LanguageKind::Toml => "toml",
        LanguageKind::Yaml => "yaml",
    }
}

fn render_line_numbers(content: &str) -> Html {
    let line_count = content.split('\n').count();
    html! {
        <>
            {for (1..=line_count).map(|number| html! {
                <div class="hl-line gutter-line">{number}</div>
            })}
        </>
    }
}

fn render_highlighted(content: &str, language: LanguageKind) -> Html {
    const MAX_HIGHLIGHT_BYTES: usize = 28_000;
    const MAX_HIGHLIGHT_LINES: usize = 1_600;

    let mut truncated = false;
    let mut limited = content.to_string();
    if limited.len() > MAX_HIGHLIGHT_BYTES {
        let cut = previous_char_boundary(limited.as_str(), MAX_HIGHLIGHT_BYTES);
        limited.truncate(cut);
        truncated = true;
    }

    let mut state = HighlightState::default();
    let mut lines = Vec::new();

    for (index, line) in limited.split('\n').enumerate() {
        if index >= MAX_HIGHLIGHT_LINES {
            truncated = true;
            break;
        }
        let mut tokens = highlight_line(line, language, &mut state);
        if tokens.is_empty() {
            tokens.push(HlToken {
                class_name: "tok-plain",
                text: String::new(),
            });
        }
        lines.push(html! {
            <div class="hl-line">
                {for tokens.into_iter().map(render_highlight_token)}
            </div>
        });
    }

    if truncated {
        lines.push(html! {
            <div class="hl-line">
                <span class="tok tok-comment">{"[highlight truncated for stability]"}</span>
            </div>
        });
    }

    html! { <>{for lines}</> }
}

fn render_highlight_token(token: HlToken) -> Html {
    html! {
        <span class={classes!("tok", token.class_name)}>{token.text}</span>
    }
}

fn highlight_line(line: &str, language: LanguageKind, state: &mut HighlightState) -> Vec<HlToken> {
    match language {
        LanguageKind::JavaScript
        | LanguageKind::Cpp
        | LanguageKind::C
        | LanguageKind::Rust
        | LanguageKind::Odin
        | LanguageKind::CSharp
        | LanguageKind::Java
        | LanguageKind::Kotlin
        | LanguageKind::TypeScript
        | LanguageKind::Php
        | LanguageKind::Zig
        | LanguageKind::Dart => highlight_c_family_line(line, language, state),
        LanguageKind::Markdown => highlight_markdown_line(line, state),
        LanguageKind::Json => highlight_json_line(line),
        LanguageKind::Xml | LanguageKind::Html => highlight_markup_line(line),
        LanguageKind::Css => highlight_css_line(line, state),
        LanguageKind::Toml => highlight_key_value_line(line, '=', true),
        LanguageKind::Yaml => highlight_key_value_line(line, ':', true),
        LanguageKind::Plain => vec![HlToken {
            class_name: "tok-plain",
            text: line.to_string(),
        }],
    }
}

fn highlight_c_family_line(
    line: &str,
    language: LanguageKind,
    state: &mut HighlightState,
) -> Vec<HlToken> {
    let mut tokens = Vec::new();
    let keywords = language_keywords(language);
    let types = language_types(language);
    let mut index = 0usize;
    let mut guard = 0usize;

    while index < line.len() {
        guard += 1;
        if guard > line.len().saturating_mul(4).saturating_add(64) {
            tokens.push(HlToken {
                class_name: "tok-plain",
                text: line[index..].to_string(),
            });
            break;
        }

        if state.in_block_comment {
            if let Some(end_rel) = line[index..].find("*/") {
                let end = index + end_rel + 2;
                tokens.push(HlToken {
                    class_name: "tok-comment",
                    text: line[index..end].to_string(),
                });
                index = end;
                state.in_block_comment = false;
            } else {
                tokens.push(HlToken {
                    class_name: "tok-comment",
                    text: line[index..].to_string(),
                });
                return tokens;
            }
            continue;
        }

        let rest = &line[index..];
        if rest.starts_with("//") {
            tokens.push(HlToken {
                class_name: "tok-comment",
                text: rest.to_string(),
            });
            return tokens;
        }
        if rest.starts_with("/*") {
            if let Some(end_rel) = rest.find("*/") {
                let end = index + end_rel + 2;
                tokens.push(HlToken {
                    class_name: "tok-comment",
                    text: line[index..end].to_string(),
                });
                index = end;
            } else {
                tokens.push(HlToken {
                    class_name: "tok-comment",
                    text: rest.to_string(),
                });
                state.in_block_comment = true;
                return tokens;
            }
            continue;
        }

        let character = line[index..].chars().next().unwrap_or_default();
        if character == '"' || character == '\'' {
            let end = consume_quoted(line, index, character);
            tokens.push(HlToken {
                class_name: "tok-string",
                text: line[index..end].to_string(),
            });
            index = end;
            continue;
        }

        if character.is_ascii_digit() {
            let end = consume_number(line, index);
            tokens.push(HlToken {
                class_name: "tok-number",
                text: line[index..end].to_string(),
            });
            index = end;
            continue;
        }

        if is_identifier_start(character) {
            let end = consume_identifier(line, index);
            let word = &line[index..end];
            let class_name = if keywords.contains(&word) {
                "tok-keyword"
            } else if types.contains(&word) {
                "tok-type"
            } else {
                "tok-plain"
            };
            tokens.push(HlToken {
                class_name,
                text: word.to_string(),
            });
            index = end;
            continue;
        }

        if character.is_whitespace() {
            let end = consume_while(line, index, char::is_whitespace);
            tokens.push(HlToken {
                class_name: "tok-plain",
                text: line[index..end].to_string(),
            });
            index = end;
            continue;
        }

        let class_name = if "{}[]();:,.<>+-*/=%!&|^~?#".contains(character) {
            "tok-punct"
        } else {
            "tok-plain"
        };
        let end = index + character.len_utf8();
        tokens.push(HlToken {
            class_name,
            text: line[index..end].to_string(),
        });
        index = end;
    }

    tokens
}

fn highlight_markdown_line(line: &str, state: &mut HighlightState) -> Vec<HlToken> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("```") {
        state.in_markdown_fence = !state.in_markdown_fence;
        return vec![HlToken {
            class_name: "tok-keyword",
            text: line.to_string(),
        }];
    }

    if state.in_markdown_fence {
        return vec![HlToken {
            class_name: "tok-plain",
            text: line.to_string(),
        }];
    }

    if trimmed.starts_with('#') {
        return vec![HlToken {
            class_name: "tok-heading",
            text: line.to_string(),
        }];
    }

    if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("> ") {
        let lead = line.len() - trimmed.len();
        let marker_end = lead + 2;
        return vec![
            HlToken {
                class_name: "tok-plain",
                text: line[..lead].to_string(),
            },
            HlToken {
                class_name: "tok-keyword",
                text: line[lead..marker_end].to_string(),
            },
            HlToken {
                class_name: "tok-plain",
                text: line[marker_end..].to_string(),
            },
        ];
    }

    highlight_inline_backticks(line)
}

fn highlight_json_line(line: &str) -> Vec<HlToken> {
    let mut tokens = Vec::new();
    let mut index = 0usize;
    let mut guard = 0usize;

    while index < line.len() {
        guard += 1;
        if guard > line.len().saturating_mul(4).saturating_add(64) {
            tokens.push(HlToken {
                class_name: "tok-plain",
                text: line[index..].to_string(),
            });
            break;
        }
        let character = line[index..].chars().next().unwrap_or_default();
        if character.is_whitespace() {
            let end = consume_while(line, index, char::is_whitespace);
            tokens.push(HlToken {
                class_name: "tok-plain",
                text: line[index..end].to_string(),
            });
            index = end;
            continue;
        }

        if line[index..].starts_with("//") {
            tokens.push(HlToken {
                class_name: "tok-comment",
                text: line[index..].to_string(),
            });
            break;
        }

        if character == '"' {
            let end = consume_quoted(line, index, '"');
            let mut class_name = "tok-string";
            let mut cursor = end;
            while cursor < line.len()
                && line[cursor..].chars().next().unwrap_or(' ').is_whitespace()
            {
                cursor = consume_while(line, cursor, char::is_whitespace);
            }
            if cursor < line.len() && line[cursor..].starts_with(':') {
                class_name = "tok-key";
            }

            tokens.push(HlToken {
                class_name,
                text: line[index..end].to_string(),
            });
            index = end;
            continue;
        }

        if character.is_ascii_digit() || character == '-' {
            let end = consume_number(line, index);
            tokens.push(HlToken {
                class_name: "tok-number",
                text: line[index..end].to_string(),
            });
            index = end;
            continue;
        }

        if is_identifier_start(character) {
            let end = consume_identifier(line, index);
            let word = &line[index..end];
            let class_name = if ["true", "false", "null"].contains(&word) {
                "tok-keyword"
            } else {
                "tok-plain"
            };
            tokens.push(HlToken {
                class_name,
                text: word.to_string(),
            });
            index = end;
            continue;
        }

        let end = index + character.len_utf8();
        let class_name = if "{}[],:".contains(character) {
            "tok-punct"
        } else {
            "tok-plain"
        };
        tokens.push(HlToken {
            class_name,
            text: line[index..end].to_string(),
        });
        index = end;
    }

    tokens
}

fn highlight_key_value_line(line: &str, separator: char, allow_hash_comment: bool) -> Vec<HlToken> {
    let (code_part, comment_part) = if allow_hash_comment {
        split_comment(line, '#')
    } else {
        (line.to_string(), String::new())
    };

    let mut tokens = Vec::new();
    if let Some(separator_index) = find_separator_outside_quotes(code_part.as_str(), separator) {
        let left = &code_part[..separator_index];
        let right = &code_part[separator_index + separator.len_utf8()..];

        let leading = left.len() - left.trim_start().len();
        let trailing = left.trim_end().len();
        if leading > 0 {
            tokens.push(HlToken {
                class_name: "tok-plain",
                text: left[..leading].to_string(),
            });
        }
        if trailing > leading {
            tokens.push(HlToken {
                class_name: "tok-key",
                text: left[leading..trailing].to_string(),
            });
        }
        if trailing < left.len() {
            tokens.push(HlToken {
                class_name: "tok-plain",
                text: left[trailing..].to_string(),
            });
        }
        tokens.push(HlToken {
            class_name: "tok-punct",
            text: separator.to_string(),
        });
        tokens.extend(highlight_inline_value(right));
    } else {
        tokens.extend(highlight_inline_value(code_part.as_str()));
    }

    if !comment_part.is_empty() {
        tokens.push(HlToken {
            class_name: "tok-comment",
            text: comment_part,
        });
    }

    tokens
}

fn highlight_inline_value(line: &str) -> Vec<HlToken> {
    let mut tokens = Vec::new();
    let mut index = 0usize;
    let mut guard = 0usize;
    while index < line.len() {
        guard += 1;
        if guard > line.len().saturating_mul(4).saturating_add(64) {
            tokens.push(HlToken {
                class_name: "tok-plain",
                text: line[index..].to_string(),
            });
            break;
        }
        let character = line[index..].chars().next().unwrap_or_default();
        if character == '"' || character == '\'' {
            let end = consume_quoted(line, index, character);
            tokens.push(HlToken {
                class_name: "tok-string",
                text: line[index..end].to_string(),
            });
            index = end;
            continue;
        }

        if character.is_ascii_digit() || character == '-' {
            let end = consume_number(line, index);
            tokens.push(HlToken {
                class_name: "tok-number",
                text: line[index..end].to_string(),
            });
            index = end;
            continue;
        }

        if is_identifier_start(character) {
            let end = consume_identifier(line, index);
            let word = &line[index..end];
            let class_name = if ["true", "false", "null", "yes", "no"].contains(&word) {
                "tok-keyword"
            } else {
                "tok-plain"
            };
            tokens.push(HlToken {
                class_name,
                text: word.to_string(),
            });
            index = end;
            continue;
        }

        if character.is_whitespace() {
            let end = consume_while(line, index, char::is_whitespace);
            tokens.push(HlToken {
                class_name: "tok-plain",
                text: line[index..end].to_string(),
            });
            index = end;
            continue;
        }

        let end = index + character.len_utf8();
        let class_name = if "{}[](),:=".contains(character) {
            "tok-punct"
        } else {
            "tok-plain"
        };
        tokens.push(HlToken {
            class_name,
            text: line[index..end].to_string(),
        });
        index = end;
    }
    tokens
}

fn highlight_markup_line(line: &str) -> Vec<HlToken> {
    let mut tokens = Vec::new();
    let mut index = 0usize;

    while index < line.len() {
        let Some(open_rel) = line[index..].find('<') else {
            tokens.push(HlToken {
                class_name: "tok-plain",
                text: line[index..].to_string(),
            });
            break;
        };

        let open = index + open_rel;
        if open > index {
            tokens.push(HlToken {
                class_name: "tok-plain",
                text: line[index..open].to_string(),
            });
        }

        let Some(close_rel) = line[open..].find('>') else {
            tokens.push(HlToken {
                class_name: "tok-plain",
                text: line[open..].to_string(),
            });
            break;
        };

        let close = open + close_rel + 1;
        tokens.extend(highlight_tag(&line[open..close]));
        index = close;
    }

    if tokens.is_empty() {
        tokens.push(HlToken {
            class_name: "tok-plain",
            text: String::new(),
        });
    }

    tokens
}

fn highlight_tag(tag: &str) -> Vec<HlToken> {
    if tag.starts_with("<!--") {
        return vec![HlToken {
            class_name: "tok-comment",
            text: tag.to_string(),
        }];
    }

    let mut tokens = Vec::new();
    let mut index = 0usize;

    if tag.starts_with('<') {
        tokens.push(HlToken {
            class_name: "tok-punct",
            text: "<".to_string(),
        });
        index = 1;
    }

    if index < tag.len() && tag[index..].starts_with('/') {
        tokens.push(HlToken {
            class_name: "tok-punct",
            text: "/".to_string(),
        });
        index += 1;
    }

    let name_end = consume_while(tag, index, |character| {
        character.is_ascii_alphanumeric()
            || character == '-'
            || character == '_'
            || character == ':'
    });
    if name_end > index {
        tokens.push(HlToken {
            class_name: "tok-tag",
            text: tag[index..name_end].to_string(),
        });
        index = name_end;
    }

    while index < tag.len() {
        let character = tag[index..].chars().next().unwrap_or_default();
        if character == '>' {
            tokens.push(HlToken {
                class_name: "tok-punct",
                text: ">".to_string(),
            });
            index += 1;
            continue;
        }
        if character == '"' || character == '\'' {
            let end = consume_quoted(tag, index, character);
            tokens.push(HlToken {
                class_name: "tok-string",
                text: tag[index..end].to_string(),
            });
            index = end;
            continue;
        }
        if character.is_whitespace() {
            let end = consume_while(tag, index, char::is_whitespace);
            tokens.push(HlToken {
                class_name: "tok-plain",
                text: tag[index..end].to_string(),
            });
            index = end;
            continue;
        }
        if character == '/' || character == '=' {
            let end = index + character.len_utf8();
            tokens.push(HlToken {
                class_name: "tok-punct",
                text: tag[index..end].to_string(),
            });
            index = end;
            continue;
        }
        if is_identifier_start(character) {
            let end = consume_while(tag, index, |char| {
                char.is_ascii_alphanumeric() || char == '-' || char == '_' || char == ':'
            });
            tokens.push(HlToken {
                class_name: "tok-attr",
                text: tag[index..end].to_string(),
            });
            index = end;
            continue;
        }

        let end = index + character.len_utf8();
        tokens.push(HlToken {
            class_name: "tok-plain",
            text: tag[index..end].to_string(),
        });
        index = end;
    }

    tokens
}

fn highlight_css_line(line: &str, state: &mut HighlightState) -> Vec<HlToken> {
    let mut tokens = Vec::new();
    let mut index = 0usize;
    let mut guard = 0usize;

    while index < line.len() {
        guard += 1;
        if guard > line.len().saturating_mul(4).saturating_add(64) {
            tokens.push(HlToken {
                class_name: "tok-plain",
                text: line[index..].to_string(),
            });
            break;
        }

        let before = index;
        if state.in_block_comment {
            if let Some(end_rel) = line[index..].find("*/") {
                let end = index + end_rel + 2;
                tokens.push(HlToken {
                    class_name: "tok-comment",
                    text: line[index..end].to_string(),
                });
                index = end;
                state.in_block_comment = false;
            } else {
                tokens.push(HlToken {
                    class_name: "tok-comment",
                    text: line[index..].to_string(),
                });
                return tokens;
            }
            continue;
        }

        let rest = &line[index..];
        if rest.starts_with("/*") {
            if let Some(end_rel) = rest.find("*/") {
                let end = index + end_rel + 2;
                tokens.push(HlToken {
                    class_name: "tok-comment",
                    text: line[index..end].to_string(),
                });
                index = end;
            } else {
                tokens.push(HlToken {
                    class_name: "tok-comment",
                    text: rest.to_string(),
                });
                state.in_block_comment = true;
                return tokens;
            }
            continue;
        }

        let character = line[index..].chars().next().unwrap_or_default();
        if character == '"' || character == '\'' {
            let end = consume_quoted(line, index, character);
            tokens.push(HlToken {
                class_name: "tok-string",
                text: line[index..end].to_string(),
            });
            index = end;
            continue;
        }

        if character == '@' {
            let end = consume_while(line, index + 1, |char| {
                char.is_ascii_alphanumeric() || char == '-'
            });
            tokens.push(HlToken {
                class_name: "tok-keyword",
                text: line[index..end].to_string(),
            });
            index = end;
            continue;
        }

        if character.is_ascii_digit() {
            let end = consume_number(line, index);
            tokens.push(HlToken {
                class_name: "tok-number",
                text: line[index..end].to_string(),
            });
            index = end;
            continue;
        }

        if is_identifier_start(character)
            || character == '-'
            || character == '#'
            || character == '.'
        {
            let end = consume_while(line, index, |char| {
                char.is_ascii_alphanumeric() || ['-', '_', '#', '.'].contains(&char)
            });
            let word = &line[index..end];
            let class_name = if next_non_whitespace_char(line, end) == Some(':') {
                "tok-key"
            } else {
                "tok-plain"
            };
            tokens.push(HlToken {
                class_name,
                text: word.to_string(),
            });
            index = end;
            continue;
        }

        if character.is_whitespace() {
            let end = consume_while(line, index, char::is_whitespace);
            tokens.push(HlToken {
                class_name: "tok-plain",
                text: line[index..end].to_string(),
            });
            index = end;
            continue;
        }

        let end = index + character.len_utf8();
        let class_name = if "{}[]();:,.>+".contains(character) {
            "tok-punct"
        } else {
            "tok-plain"
        };
        tokens.push(HlToken {
            class_name,
            text: line[index..end].to_string(),
        });
        index = end;

        if index <= before {
            let next = next_boundary(line, before);
            if next <= before {
                break;
            }
            tokens.push(HlToken {
                class_name: "tok-plain",
                text: line[before..next].to_string(),
            });
            index = next;
        }
    }

    tokens
}

fn highlight_inline_backticks(line: &str) -> Vec<HlToken> {
    let mut tokens = Vec::new();
    let mut index = 0usize;

    while index < line.len() {
        let Some(open_rel) = line[index..].find('`') else {
            tokens.push(HlToken {
                class_name: "tok-plain",
                text: line[index..].to_string(),
            });
            break;
        };
        let open = index + open_rel;
        if open > index {
            tokens.push(HlToken {
                class_name: "tok-plain",
                text: line[index..open].to_string(),
            });
        }
        let Some(close_rel) = line[open + 1..].find('`') else {
            tokens.push(HlToken {
                class_name: "tok-plain",
                text: line[open..].to_string(),
            });
            break;
        };
        let close = open + 1 + close_rel + 1;
        tokens.push(HlToken {
            class_name: "tok-string",
            text: line[open..close].to_string(),
        });
        index = close;
    }

    if tokens.is_empty() {
        tokens.push(HlToken {
            class_name: "tok-plain",
            text: String::new(),
        });
    }

    tokens
}

fn split_comment(line: &str, marker: char) -> (String, String) {
    let mut in_single = false;
    let mut in_double = false;
    for (index, character) in line.char_indices() {
        match character {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            _ => {}
        }
        if !in_single && !in_double && character == marker {
            return (line[..index].to_string(), line[index..].to_string());
        }
    }
    (line.to_string(), String::new())
}

fn find_separator_outside_quotes(line: &str, separator: char) -> Option<usize> {
    let mut in_single = false;
    let mut in_double = false;
    for (index, character) in line.char_indices() {
        match character {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            _ => {}
        }
        if !in_single && !in_double && character == separator {
            return Some(index);
        }
    }
    None
}

fn consume_quoted(line: &str, start: usize, quote: char) -> usize {
    let mut escaped = false;
    let mut index = start + quote.len_utf8();
    while index < line.len() {
        let character = line[index..].chars().next().unwrap_or_default();
        let next = index + character.len_utf8();
        if escaped {
            escaped = false;
            index = next;
            continue;
        }
        if character == '\\' {
            escaped = true;
            index = next;
            continue;
        }
        if character == quote {
            return next;
        }
        index = next;
    }
    line.len()
}

fn consume_number(line: &str, start: usize) -> usize {
    consume_while(line, start, |character| {
        character.is_ascii_alphanumeric() || "._+-xXobOB".contains(character)
    })
}

fn consume_identifier(line: &str, start: usize) -> usize {
    consume_while(line, start, |character| {
        character.is_ascii_alphanumeric() || character == '_' || character == '$'
    })
}

fn consume_while<F>(line: &str, start: usize, predicate: F) -> usize
where
    F: Fn(char) -> bool,
{
    let mut index = start;
    while index < line.len() {
        let character = line[index..].chars().next().unwrap_or_default();
        if !predicate(character) {
            break;
        }
        index += character.len_utf8();
    }
    index
}

fn is_identifier_start(character: char) -> bool {
    character.is_ascii_alphabetic() || character == '_' || character == '$'
}

fn next_non_whitespace_char(line: &str, from: usize) -> Option<char> {
    let mut index = from;
    while index < line.len() {
        let character = line[index..].chars().next()?;
        if !character.is_whitespace() {
            return Some(character);
        }
        index += character.len_utf8();
    }
    None
}

fn language_keywords(language: LanguageKind) -> &'static [&'static str] {
    match language {
        LanguageKind::JavaScript => &[
            "let",
            "const",
            "var",
            "function",
            "return",
            "if",
            "else",
            "switch",
            "case",
            "break",
            "continue",
            "for",
            "while",
            "do",
            "class",
            "extends",
            "new",
            "import",
            "from",
            "export",
            "default",
            "async",
            "await",
            "try",
            "catch",
            "finally",
            "throw",
            "this",
            "super",
            "true",
            "false",
            "null",
            "undefined",
            "in",
            "of",
            "instanceof",
        ],
        LanguageKind::TypeScript => &[
            "let",
            "const",
            "var",
            "function",
            "return",
            "if",
            "else",
            "switch",
            "case",
            "break",
            "continue",
            "for",
            "while",
            "do",
            "class",
            "extends",
            "new",
            "import",
            "from",
            "export",
            "default",
            "async",
            "await",
            "try",
            "catch",
            "finally",
            "throw",
            "interface",
            "type",
            "enum",
            "implements",
            "readonly",
            "public",
            "private",
            "protected",
            "declare",
            "namespace",
            "as",
            "unknown",
            "never",
            "true",
            "false",
            "null",
            "undefined",
            "this",
            "super",
        ],
        LanguageKind::Rust => &[
            "fn", "let", "mut", "pub", "impl", "trait", "struct", "enum", "match", "if", "else",
            "loop", "while", "for", "in", "where", "async", "await", "move", "use", "mod", "crate",
            "self", "super", "return", "break", "continue", "const", "static", "unsafe", "dyn",
            "ref", "as", "true", "false",
        ],
        LanguageKind::Cpp | LanguageKind::C => &[
            "if",
            "else",
            "switch",
            "case",
            "default",
            "for",
            "while",
            "do",
            "break",
            "continue",
            "return",
            "try",
            "catch",
            "throw",
            "struct",
            "class",
            "enum",
            "namespace",
            "using",
            "template",
            "typename",
            "public",
            "private",
            "protected",
            "virtual",
            "override",
            "constexpr",
            "const",
            "static",
            "inline",
            "auto",
            "new",
            "delete",
            "this",
            "nullptr",
            "true",
            "false",
        ],
        LanguageKind::Odin => &[
            "package",
            "import",
            "proc",
            "struct",
            "enum",
            "union",
            "if",
            "else",
            "switch",
            "case",
            "for",
            "when",
            "defer",
            "return",
            "break",
            "continue",
            "cast",
            "auto_cast",
            "transmute",
            "true",
            "false",
            "nil",
        ],
        LanguageKind::CSharp => &[
            "using",
            "namespace",
            "class",
            "struct",
            "interface",
            "enum",
            "public",
            "private",
            "protected",
            "internal",
            "static",
            "void",
            "var",
            "new",
            "this",
            "base",
            "if",
            "else",
            "switch",
            "case",
            "default",
            "for",
            "foreach",
            "while",
            "do",
            "break",
            "continue",
            "return",
            "async",
            "await",
            "try",
            "catch",
            "finally",
            "throw",
            "null",
            "true",
            "false",
        ],
        LanguageKind::Java => &[
            "package",
            "import",
            "class",
            "interface",
            "enum",
            "public",
            "private",
            "protected",
            "static",
            "final",
            "void",
            "new",
            "this",
            "super",
            "if",
            "else",
            "switch",
            "case",
            "default",
            "for",
            "while",
            "do",
            "break",
            "continue",
            "return",
            "try",
            "catch",
            "finally",
            "throw",
            "null",
            "true",
            "false",
        ],
        LanguageKind::Kotlin => &[
            "fun",
            "val",
            "var",
            "class",
            "object",
            "interface",
            "data",
            "sealed",
            "enum",
            "companion",
            "public",
            "private",
            "protected",
            "internal",
            "open",
            "override",
            "abstract",
            "if",
            "else",
            "when",
            "for",
            "while",
            "do",
            "break",
            "continue",
            "return",
            "try",
            "catch",
            "finally",
            "throw",
            "null",
            "true",
            "false",
            "this",
            "super",
        ],
        LanguageKind::Php => &[
            "function",
            "echo",
            "if",
            "else",
            "elseif",
            "switch",
            "case",
            "default",
            "for",
            "foreach",
            "while",
            "do",
            "break",
            "continue",
            "return",
            "class",
            "interface",
            "trait",
            "public",
            "private",
            "protected",
            "static",
            "new",
            "this",
            "null",
            "true",
            "false",
        ],
        LanguageKind::Zig => &[
            "const",
            "var",
            "fn",
            "pub",
            "extern",
            "struct",
            "enum",
            "union",
            "if",
            "else",
            "switch",
            "for",
            "while",
            "break",
            "continue",
            "return",
            "defer",
            "errdefer",
            "try",
            "catch",
            "comptime",
            "usingnamespace",
            "null",
            "true",
            "false",
        ],
        LanguageKind::Dart => &[
            "import",
            "library",
            "class",
            "mixin",
            "enum",
            "extension",
            "abstract",
            "implements",
            "extends",
            "with",
            "if",
            "else",
            "switch",
            "case",
            "for",
            "while",
            "do",
            "break",
            "continue",
            "return",
            "try",
            "catch",
            "finally",
            "throw",
            "var",
            "final",
            "const",
            "new",
            "this",
            "super",
            "true",
            "false",
            "null",
            "async",
            "await",
        ],
        _ => &[],
    }
}

fn language_types(language: LanguageKind) -> &'static [&'static str] {
    match language {
        LanguageKind::Rust => &[
            "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize",
            "f32", "f64", "bool", "str", "String", "Result", "Option", "Self",
        ],
        LanguageKind::JavaScript | LanguageKind::TypeScript => &[
            "string", "number", "boolean", "object", "any", "void", "unknown",
        ],
        LanguageKind::Cpp | LanguageKind::C => &[
            "void", "int", "char", "float", "double", "short", "long", "bool", "size_t",
        ],
        LanguageKind::CSharp => &[
            "void", "int", "string", "bool", "double", "float", "decimal", "object",
        ],
        LanguageKind::Java => &[
            "void", "int", "long", "double", "float", "boolean", "char", "String",
        ],
        LanguageKind::Kotlin => &[
            "Int", "Long", "Double", "Float", "Boolean", "Char", "String", "Unit",
        ],
        LanguageKind::Php => &["int", "float", "bool", "string", "array", "object", "void"],
        LanguageKind::Zig => &[
            "u8", "u16", "u32", "u64", "usize", "i8", "i16", "i32", "i64", "isize", "f32", "f64",
            "bool",
        ],
        LanguageKind::Dart => &["int", "double", "bool", "String", "dynamic", "void"],
        _ => &[],
    }
}

fn editor_mode_label(mode: EditorMode) -> &'static str {
    match mode {
        EditorMode::Normal => "NORMAL",
        EditorMode::Insert => "INSERT",
        EditorMode::Command => "COMMAND",
    }
}

fn format_prompt(cwd: &[String]) -> String {
    format!("wowvain@kaaldur:{}$ ", display_path(cwd))
}

fn display_path(cwd: &[String]) -> String {
    if cwd.is_empty() {
        String::from("/")
    } else {
        format!("/{}", cwd.join("/"))
    }
}

async fn wait_for_initial_fonts() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let fonts = document.fonts();
    let Ok(ready) = fonts.ready() else {
        return;
    };
    let _ = JsFuture::from(ready).await;
}

fn is_phone_device() -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    let Ok(user_agent) = window.navigator().user_agent() else {
        return false;
    };
    let normalized = user_agent.to_ascii_lowercase();
    normalized.contains("iphone")
        || normalized.contains("android")
        || normalized.contains("mobile")
        || normalized.contains("ipod")
        || normalized.contains("windows phone")
}

fn format_repo_entry(entry: RepoEntry) -> String {
    match entry.kind {
        RepoEntryKind::Dir => format!("{}/", entry.name),
        RepoEntryKind::File => entry.name,
    }
}

fn normalize_path(cwd: &[String], raw: &str) -> Vec<String> {
    let mut output = if raw.starts_with('/') {
        Vec::new()
    } else {
        cwd.to_vec()
    };

    for segment in raw.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                output.pop();
            }
            _ => output.push(segment.to_string()),
        }
    }

    output
}

fn project_repo(project: &Project) -> &str {
    repo_slug_from_url(project.url.as_str()).unwrap_or(project.name.as_str())
}

fn repo_slug_from_url(url: &str) -> Option<&str> {
    let trimmed = url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }

    let repo = trimmed.rsplit('/').next()?;
    let repo = repo.trim_end_matches(".git");
    if repo.is_empty() { None } else { Some(repo) }
}

fn normalize_project_key(value: &str) -> String {
    value
        .trim()
        .trim_matches('/')
        .trim_matches('\\')
        .trim_end_matches(".git")
        .chars()
        .filter_map(|character| {
            if character.is_ascii_alphanumeric() {
                Some(character.to_ascii_lowercase())
            } else if character == '-' || character == '_' {
                Some('_')
            } else {
                None
            }
        })
        .collect::<String>()
}

fn find_project<'a>(projects: &'a [Project], name: &str) -> Option<&'a Project> {
    let mut candidates = Vec::new();
    let trimmed = name.trim();
    if !trimmed.is_empty() {
        candidates.push(trimmed.to_string());
    }

    for segment in trimmed.split(['/', '\\']) {
        if !segment.trim().is_empty() {
            candidates.push(segment.trim().to_string());
        }
    }

    candidates.sort();
    candidates.dedup();

    projects.iter().find(|project| {
        let project_name = normalize_project_key(project.name.as_str());
        let project_repo_name = normalize_project_key(project_repo(project));
        candidates.iter().any(|candidate| {
            let needle = normalize_project_key(candidate);
            !needle.is_empty() && (project_name == needle || project_repo_name == needle)
        })
    })
}

fn find_project_in_path<'a>(
    projects: &'a [Project],
    path: &[String],
) -> Option<(&'a Project, usize)> {
    if !path.first().is_some_and(|head| head == "projects") {
        return None;
    }

    if let Some(project_name) = path.get(1) {
        if let Some(project) = find_project(projects, project_name.as_str()) {
            return Some((project, 1));
        }
    }

    let should_try_third_segment = path
        .get(1)
        .is_some_and(|segment| segment.eq_ignore_ascii_case("projects"))
        || path.get(1).is_some_and(|segment| {
            projects
                .iter()
                .any(|project| project.owner.eq_ignore_ascii_case(segment))
        });

    if should_try_third_segment {
        if let Some(project_name) = path.get(2) {
            if let Some(project) = find_project(projects, project_name.as_str()) {
                return Some((project, 2));
            }
        }
    }

    None
}

fn previous_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn clamp_boundary(text: &str, position: usize) -> usize {
    let mut pos = position.min(text.len());
    while pos > 0 && !text.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

fn prev_boundary(text: &str, position: usize) -> usize {
    let pos = clamp_boundary(text, position);
    if pos == 0 {
        return 0;
    }

    let mut previous = 0;
    for (index, _) in text[..pos].char_indices() {
        previous = index;
    }
    previous
}

fn next_boundary(text: &str, position: usize) -> usize {
    let pos = clamp_boundary(text, position);
    if pos >= text.len() {
        return text.len();
    }

    let Some(character) = text[pos..].chars().next() else {
        return text.len();
    };

    pos + character.len_utf8()
}

fn line_start(text: &str, position: usize) -> usize {
    let pos = clamp_boundary(text, position);
    text[..pos].rfind('\n').map(|index| index + 1).unwrap_or(0)
}

fn line_end(text: &str, position: usize) -> usize {
    let pos = clamp_boundary(text, position);
    text[pos..]
        .find('\n')
        .map(|index| pos + index)
        .unwrap_or(text.len())
}

fn line_column(text: &str, position: usize) -> usize {
    let pos = clamp_boundary(text, position);
    let start = line_start(text, pos);
    text[start..pos].chars().count()
}

fn is_word_char(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

fn next_word_boundary(text: &str, position: usize) -> usize {
    let mut pos = clamp_boundary(text, position);
    if pos >= text.len() {
        return text.len();
    }

    if text[pos..].chars().next().is_some_and(is_word_char) {
        while pos < text.len() {
            let Some(character) = text[pos..].chars().next() else {
                break;
            };
            if !is_word_char(character) {
                break;
            }
            pos += character.len_utf8();
        }
    }

    while pos < text.len() {
        let Some(character) = text[pos..].chars().next() else {
            break;
        };
        if is_word_char(character) {
            break;
        }
        pos += character.len_utf8();
    }

    pos
}

fn prev_word_boundary(text: &str, position: usize) -> usize {
    let mut pos = clamp_boundary(text, position);
    if pos == 0 {
        return 0;
    }
    pos = prev_boundary(text, pos);

    loop {
        let Some(character) = text[pos..].chars().next() else {
            return 0;
        };
        if is_word_char(character) {
            break;
        }
        if pos == 0 {
            return 0;
        }
        pos = prev_boundary(text, pos);
    }

    while pos > 0 {
        let previous = prev_boundary(text, pos);
        let Some(character) = text[previous..].chars().next() else {
            break;
        };
        if !is_word_char(character) {
            break;
        }
        pos = previous;
    }

    pos
}

fn nth_char_byte(text: &str, column: usize) -> usize {
    if column == 0 {
        return 0;
    }

    text.char_indices()
        .nth(column)
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}

fn move_vertical(text: &str, position: usize, delta: isize, preferred_column: usize) -> usize {
    let pos = clamp_boundary(text, position);
    let current_start = line_start(text, pos);

    if delta < 0 {
        if current_start == 0 {
            return pos;
        }

        let prev_end = current_start.saturating_sub(1);
        let prev_start = line_start(text, prev_end);
        let prev_line = &text[prev_start..prev_end];
        let target_column = preferred_column.min(prev_line.chars().count());
        return prev_start + nth_char_byte(prev_line, target_column);
    }

    let current_end = line_end(text, pos);
    if current_end >= text.len() {
        return pos;
    }

    let next_start = current_end + 1;
    let next_end = line_end(text, next_start);
    let next_line = &text[next_start..next_end];
    let target_column = preferred_column.min(next_line.chars().count());
    next_start + nth_char_byte(next_line, target_column)
}

fn default_identity() -> Identity {
    Identity {
        handle: String::from("wowvain-dev"),
        aliases: vec![
            String::from("wowvain"),
            String::from("thewowvain"),
            String::from("wowvain-dev"),
        ],
        tagline: String::from("engine + tools + linux workflows"),
        location: String::from("Delft, Netherlands"),
        focus: vec![
            String::from("game engine architecture"),
            String::from("graphics programming"),
            String::from("systems-level tooling"),
            String::from("linux-centric dev environment"),
        ],
        scope_note: String::from(
            "Project set is curated from wowvain-dev and KaaldurSoftworks repositories.",
        ),
        snapshot_date: String::from("2026-02-20"),
    }
}

fn main() {
    yew::Renderer::<App>::new().render();
}
