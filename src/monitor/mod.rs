use crate::OutputFormat;
use crate::toplevels::ToplevelJson;
use crate::workspaces::WorkspaceJson;
use serde::Serialize;

/// A single event emitted in monitor/subscribe mode.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum MonitorEvent {
    /// A workspace was created or became visible for the first time.
    WorkspaceAdded { workspace: WorkspaceJson },
    /// A workspace's state changed (name, tiling, active, pinned, …).
    WorkspaceUpdated { workspace: WorkspaceJson },
    /// A workspace was removed.
    WorkspaceRemoved { name: String, id: Option<String> },
    /// A new window appeared.
    WindowOpened { window: ToplevelJson },
    /// A window's state changed (title, state flags, workspace, …).
    WindowUpdated { window: ToplevelJson },
    /// A window was closed.
    WindowClosed {
        title: String,
        app_id: String,
        identifier: String,
    },
}

/// Print a single monitor event to stdout in the requested format.
/// Uses ndjson (one JSON object per line) for Json/PrettyJson so the stream
/// is trivially `jq`-filterable.
pub fn print_event(event: &MonitorEvent, format: OutputFormat) {
    match format {
        OutputFormat::Human => print_human(event),
        OutputFormat::Json => println!("{}", serde_json::to_string(event).unwrap()),
        OutputFormat::PrettyJson => println!("{}", serde_json::to_string_pretty(event).unwrap()),
    }
}

fn print_human(event: &MonitorEvent) {
    match event {
        MonitorEvent::WorkspaceAdded { workspace } => {
            let active = if workspace.active { " *" } else { "" };
            let tiling = workspace
                .tiling
                .as_deref()
                .map(|t| format!(" [tiling: {t}]"))
                .unwrap_or_default();
            let on = workspace
                .output
                .as_deref()
                .map(|o| format!(" on {o}"))
                .unwrap_or_default();
            println!(
                "workspace-added   {}{}{}{}",
                workspace.name, active, tiling, on
            );
        }
        MonitorEvent::WorkspaceUpdated { workspace } => {
            let active = if workspace.active { " *" } else { "" };
            let pinned = if workspace.pinned { " [pinned]" } else { "" };
            let tiling = workspace
                .tiling
                .as_deref()
                .map(|t| format!(" [tiling: {t}]"))
                .unwrap_or_default();
            let on = workspace
                .output
                .as_deref()
                .map(|o| format!(" on {o}"))
                .unwrap_or_default();
            println!(
                "workspace-updated {}{}{}{}{}",
                workspace.name, active, pinned, tiling, on
            );
        }
        MonitorEvent::WorkspaceRemoved { name, .. } => {
            println!("workspace-removed {name}");
        }
        MonitorEvent::WindowOpened { window } => {
            let state = if window.state.is_empty() {
                String::new()
            } else {
                format!(" [{}]", window.state.join(", "))
            };
            println!(
                "window-opened     \"{}\" ({}){}",
                window.title, window.app_id, state
            );
        }
        MonitorEvent::WindowUpdated { window } => {
            let state = if window.state.is_empty() {
                String::new()
            } else {
                format!(" [{}]", window.state.join(", "))
            };
            println!(
                "window-updated    \"{}\" ({}){}",
                window.title, window.app_id, state
            );
        }
        MonitorEvent::WindowClosed { title, app_id, .. } => {
            println!("window-closed     \"{title}\" ({app_id})");
        }
    }
}
