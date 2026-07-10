use crate::{Error, OutputFormat, state::AppData};
use clap::ValueEnum;
use cosmic_client_toolkit::workspace::Workspace;
use cosmic_protocols::workspace::v2::client::zcosmic_workspace_handle_v2;
use serde::Serialize;
use wayland_client::{Proxy, WEnum};
use wayland_protocols::ext::workspace::v1::client::ext_workspace_handle_v1;
#[derive(Debug, Clone, ValueEnum)]
pub enum TilingArg {
    Enabled,
    Disabled,
}

#[derive(Serialize)]
pub struct WorkspaceJson {
    pub name: String,
    pub id: Option<String>,
    pub coordinates: Vec<u32>,
    pub active: bool,
    pub urgent: bool,
    pub hidden: bool,
    pub pinned: bool,
    pub tiling: Option<String>,
    pub output: Option<String>,
    pub group_id: String,
}

pub fn workspace_to_json(ws: &Workspace, state: &AppData) -> WorkspaceJson {
    let active = ws.state.contains(ext_workspace_handle_v1::State::Active);
    let urgent = ws.state.contains(ext_workspace_handle_v1::State::Urgent);
    let hidden = ws.state.contains(ext_workspace_handle_v1::State::Hidden);
    let pinned = ws
        .cosmic_state
        .contains(zcosmic_workspace_handle_v2::State::Pinned);

    let tiling = ws.tiling.as_ref().map(|t| match t {
        WEnum::Value(zcosmic_workspace_handle_v2::TilingState::TilingEnabled) => {
            "enabled".to_string()
        }
        WEnum::Value(zcosmic_workspace_handle_v2::TilingState::FloatingOnly) => {
            "disabled".to_string()
        }
        _ => "unknown".to_string(),
    });

    // Find the output this workspace is on via its workspace group
    let output = state
        .workspace_state
        .workspace_groups()
        .find(|g| g.workspaces.contains(&ws.handle))
        .and_then(|g| {
            g.outputs.first().and_then(|o| {
                state
                    .output_state
                    .info(o)
                    .and_then(|info| info.name.clone())
            })
        });

    let group_id = state
        .workspace_state
        .workspace_groups()
        .find(|g| g.workspaces.contains(&ws.handle))
        .map(|g| g.handle.id().to_string())
        .unwrap_or_default();

    WorkspaceJson {
        name: ws.name.clone(),
        id: ws.id.clone(),
        coordinates: ws.coordinates.clone(),
        active,
        urgent,
        hidden,
        pinned,
        tiling,
        output,
        group_id,
    }
}

pub fn cmd_get_workspaces(state: &AppData, format: OutputFormat) {
    let mut workspaces: Vec<WorkspaceJson> = state
        .workspace_state
        .workspaces()
        .map(|ws| workspace_to_json(ws, state))
        .collect();

    // Sort by group then coordinates
    workspaces.sort_by(|a, b| {
        a.group_id
            .cmp(&b.group_id)
            .then(a.coordinates.cmp(&b.coordinates))
    });

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string(&workspaces).unwrap());
        }
        OutputFormat::PrettyJson => {
            println!("{}", serde_json::to_string_pretty(&workspaces).unwrap());
        }
        OutputFormat::Human => {
            for ws in &workspaces {
                let active = if ws.active { " *" } else { "" };
                let pinned = if ws.pinned { " [pinned]" } else { "" };
                let tiling = ws
                    .tiling
                    .as_deref()
                    .map(|t| format!(" [tiling: {t}]"))
                    .unwrap_or_default();
                let output = ws
                    .output
                    .as_deref()
                    .map(|o| format!(" on {o}"))
                    .unwrap_or_default();
                println!("{}{}{}{}{}", ws.name, active, pinned, tiling, output);
            }
        }
    }
}

// ── Workspace resolution ──────────────────────────────────────────────────────

/// Find a workspace by name or id. Public wrapper used by the toplevels module.
pub fn resolve_workspace_pub<'a>(
    state: &'a AppData,
    selector: &str,
) -> Result<&'a Workspace, Error> {
    resolve_workspace(state, selector)
}

fn resolve_workspace<'a>(state: &'a AppData, selector: &str) -> Result<&'a Workspace, Error> {
    let workspaces: Vec<_> = state.workspace_state.workspaces().collect();

    // 1. Exact match on name
    let exact: Vec<_> = workspaces.iter().filter(|ws| ws.name == selector).collect();
    if exact.len() == 1 {
        return Ok(exact[0]);
    }
    if exact.len() > 1 {
        return Err(Error::AmbiguousMatch(format!(
            "multiple workspaces named '{selector}'"
        )));
    }

    // 2. Exact match on id
    let by_id: Vec<_> = workspaces
        .iter()
        .filter(|ws| ws.id.as_deref() == Some(selector))
        .collect();
    if by_id.len() == 1 {
        return Ok(by_id[0]);
    }

    // 3. Case-insensitive substring match on name
    let lower = selector.to_lowercase();
    let partial: Vec<_> = workspaces
        .iter()
        .filter(|ws| ws.name.to_lowercase().contains(&lower))
        .collect();
    if partial.len() == 1 {
        return Ok(partial[0]);
    }
    if partial.len() > 1 {
        return Err(Error::AmbiguousMatch(format!(
            "selector '{selector}' matches multiple workspaces: {}",
            partial
                .iter()
                .map(|ws| ws.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }

    Err(Error::NotFound(format!(
        "no workspace matching '{selector}'"
    )))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

pub fn tiling_arg_to_protocol(arg: &TilingArg) -> zcosmic_workspace_handle_v2::TilingState {
    match arg {
        TilingArg::Enabled => zcosmic_workspace_handle_v2::TilingState::TilingEnabled,
        TilingArg::Disabled => zcosmic_workspace_handle_v2::TilingState::FloatingOnly,
    }
}

/// Find the currently active workspace.
pub fn resolve_active_workspace(state: &AppData) -> Result<&Workspace, Error> {
    state
        .workspace_state
        .workspaces()
        .find(|ws| ws.state.contains(ext_workspace_handle_v1::State::Active))
        .ok_or_else(|| Error::NotFound("no active workspace found".to_string()))
}

// ── Workspace mutations ───────────────────────────────────────────────────────

fn commit_workspaces(
    state: &AppData,
    eq: &mut wayland_client::EventQueue<AppData>,
) -> Result<(), crate::Error> {
    if let Ok(mgr) = state.workspace_state.workspace_manager().get() {
        mgr.commit();
    }
    eq.flush().map_err(|e| crate::Error::Other(e.to_string()))
}

pub fn workspace_activate(
    state: &AppData,
    eq: &mut wayland_client::EventQueue<AppData>,
    selector: &str,
) -> Result<(), crate::Error> {
    let ws = resolve_workspace_pub(state, selector)?;
    ws.handle.activate();
    commit_workspaces(state, eq)
}

pub fn workspace_rename(
    state: &AppData,
    eq: &mut wayland_client::EventQueue<AppData>,
    selector: &str,
    new_name: &str,
) -> Result<(), crate::Error> {
    let ws = resolve_workspace_pub(state, selector)?;
    ws.cosmic_handle
        .as_ref()
        .ok_or_else(|| {
            crate::Error::ProtocolNotAvailable("zcosmic-workspace-v2 not available".into())
        })?
        .rename(new_name.to_string());
    commit_workspaces(state, eq)
}

pub fn workspace_set_tiling(
    state: &AppData,
    eq: &mut wayland_client::EventQueue<AppData>,
    selector: &str,
    enabled: bool,
) -> Result<(), crate::Error> {
    use cosmic_protocols::workspace::v2::client::zcosmic_workspace_handle_v2::TilingState;
    let ws = if selector.eq_ignore_ascii_case("active") {
        resolve_active_workspace(state)?
    } else {
        resolve_workspace_pub(state, selector)?
    };
    let cosmic = ws.cosmic_handle.as_ref().ok_or_else(|| {
        crate::Error::ProtocolNotAvailable("zcosmic-workspace-v2 not available".into())
    })?;
    cosmic.set_tiling_state(if enabled {
        TilingState::TilingEnabled
    } else {
        TilingState::FloatingOnly
    });
    commit_workspaces(state, eq)
}

pub fn workspace_set_tiling_default(
    state: &AppData,
    eq: &mut wayland_client::EventQueue<AppData>,
    enabled: bool,
) -> Result<(), crate::Error> {
    use cosmic_protocols::workspace::v2::client::zcosmic_workspace_handle_v2::TilingState;
    let occupied: std::collections::HashSet<_> = state
        .toplevel_info_state
        .toplevels()
        .flat_map(|t| t.workspace.iter().cloned())
        .collect();
    let tiling = if enabled {
        TilingState::TilingEnabled
    } else {
        TilingState::FloatingOnly
    };
    for ws in state.workspace_state.workspaces() {
        if !occupied.contains(&ws.handle) {
            if let Some(cosmic) = &ws.cosmic_handle {
                cosmic.set_tiling_state(tiling);
            }
        }
    }
    commit_workspaces(state, eq)
}

pub fn workspace_pin(
    state: &AppData,
    eq: &mut wayland_client::EventQueue<AppData>,
    selector: &str,
) -> Result<(), crate::Error> {
    let ws = resolve_workspace_pub(state, selector)?;
    ws.cosmic_handle
        .as_ref()
        .ok_or_else(|| {
            crate::Error::ProtocolNotAvailable("zcosmic-workspace-v2 not available".into())
        })?
        .pin();
    commit_workspaces(state, eq)
}

pub fn workspace_unpin(
    state: &AppData,
    eq: &mut wayland_client::EventQueue<AppData>,
    selector: &str,
) -> Result<(), crate::Error> {
    let ws = resolve_workspace_pub(state, selector)?;
    ws.cosmic_handle
        .as_ref()
        .ok_or_else(|| {
            crate::Error::ProtocolNotAvailable("zcosmic-workspace-v2 not available".into())
        })?
        .unpin();
    commit_workspaces(state, eq)
}

pub fn workspace_move_before(
    state: &AppData,
    eq: &mut wayland_client::EventQueue<AppData>,
    workspace: &str,
    before: &str,
) -> Result<(), crate::Error> {
    let ws = resolve_workspace_pub(state, workspace)?;
    let other = resolve_workspace_pub(state, before)?;
    ws.cosmic_handle
        .as_ref()
        .ok_or_else(|| {
            crate::Error::ProtocolNotAvailable("zcosmic-workspace-v2 not available".into())
        })?
        .move_before(&other.handle, 0);
    commit_workspaces(state, eq)
}

pub fn workspace_move_after(
    state: &AppData,
    eq: &mut wayland_client::EventQueue<AppData>,
    workspace: &str,
    after: &str,
) -> Result<(), crate::Error> {
    let ws = resolve_workspace_pub(state, workspace)?;
    let other = resolve_workspace_pub(state, after)?;
    ws.cosmic_handle
        .as_ref()
        .ok_or_else(|| {
            crate::Error::ProtocolNotAvailable("zcosmic-workspace-v2 not available".into())
        })?
        .move_after(&other.handle, 0);
    commit_workspaces(state, eq)
}
