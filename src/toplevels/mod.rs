use crate::{Error, OutputFormat, state::AppData};
use cosmic_client_toolkit::toplevel_info::ToplevelInfo;
use cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_handle_v1;
use serde::Serialize;

#[derive(Serialize)]
pub struct ToplevelJson {
    pub title: String,
    pub app_id: String,
    pub identifier: String,
    pub state: Vec<String>,
    pub outputs: Vec<String>,
    pub workspaces: Vec<String>,
}

pub fn toplevel_to_json(t: &ToplevelInfo, state: &AppData) -> ToplevelJson {
    let tl_states: Vec<String> = t
        .state
        .iter()
        .map(|s| match s {
            zcosmic_toplevel_handle_v1::State::Maximized => "maximized",
            zcosmic_toplevel_handle_v1::State::Minimized => "minimized",
            zcosmic_toplevel_handle_v1::State::Activated => "activated",
            zcosmic_toplevel_handle_v1::State::Fullscreen => "fullscreen",
            zcosmic_toplevel_handle_v1::State::Sticky => "sticky",
            _ => "unknown",
        })
        .map(str::to_string)
        .collect();

    let outputs: Vec<String> = t
        .output
        .iter()
        .filter_map(|o| {
            state
                .output_state
                .info(o)
                .and_then(|info| info.name.clone())
        })
        .collect();

    let workspaces: Vec<String> = t
        .workspace
        .iter()
        .filter_map(|ws_handle| {
            state
                .workspace_state
                .workspace_info(ws_handle)
                .map(|ws| ws.name.clone())
        })
        .collect();

    ToplevelJson {
        title: t.title.clone(),
        app_id: t.app_id.clone(),
        identifier: t.identifier.clone(),
        state: tl_states,
        outputs,
        workspaces,
    }
}

pub fn cmd_get_toplevels(state: &AppData, format: OutputFormat) {
    let toplevels: Vec<ToplevelJson> = state
        .toplevel_info_state
        .toplevels()
        .map(|t| toplevel_to_json(t, state))
        .collect();

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string(&toplevels).unwrap());
        }
        OutputFormat::PrettyJson => {
            println!("{}", serde_json::to_string_pretty(&toplevels).unwrap());
        }
        OutputFormat::Human => {
            for t in &toplevels {
                let state_str = if t.state.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", t.state.join(", "))
                };
                let ws_str = if t.workspaces.is_empty() {
                    String::new()
                } else {
                    format!(" on {}", t.workspaces.join(", "))
                };
                println!("\"{}\" ({}){}{}", t.title, t.app_id, state_str, ws_str);
            }
        }
    }
}

// ── Window resolution ────────────────────────────────────────────────────────

/// Resolve a window selector to a ToplevelInfo.
/// Public re-export for use by capture module.
pub fn resolve_toplevel_pub<'a>(
    state: &'a AppData,
    selector: &str,
) -> Result<&'a ToplevelInfo, Error> {
    resolve_toplevel(state, selector)
}

/// Matching priority:
///   1. Exact identifier match
///   2. Exact title match
///   3. Exact app_id match
///   4. Case-insensitive substring match on title
///   5. Case-insensitive substring match on app_id
fn resolve_toplevel<'a>(state: &'a AppData, selector: &str) -> Result<&'a ToplevelInfo, Error> {
    let toplevels: Vec<&ToplevelInfo> = state.toplevel_info_state.toplevels().collect();

    macro_rules! match_unique {
        ($filter:expr, $label:expr) => {{
            let matched: Vec<_> = toplevels.iter().filter(|t| $filter(t)).collect();
            match matched.len() {
                1 => return Ok(matched[0]),
                n if n > 1 => {
                    return Err(Error::AmbiguousMatch(format!(
                        "selector '{}' matches {} windows by {} (titles: {})",
                        selector,
                        n,
                        $label,
                        matched
                            .iter()
                            .map(|t| t.title.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )));
                }
                _ => {}
            }
        }};
    }

    match_unique!(|t: &&ToplevelInfo| t.identifier == selector, "identifier");
    match_unique!(|t: &&ToplevelInfo| t.title == selector, "title");
    match_unique!(|t: &&ToplevelInfo| t.app_id == selector, "app_id");

    let lower = selector.to_lowercase();
    match_unique!(
        |t: &&ToplevelInfo| t.title.to_lowercase().contains(&lower),
        "title substring"
    );
    match_unique!(
        |t: &&ToplevelInfo| t.app_id.to_lowercase().contains(&lower),
        "app_id substring"
    );

    Err(Error::NotFound(format!("no window matching '{selector}'")))
}

// (cmd_window is defined in main.rs — it's CLI-only and references WindowCommand)

// ── Window mutations ──────────────────────────────────────────────────────────

fn toplevel_manager(
    state: &AppData,
) -> Result<cosmic_protocols::toplevel_management::v1::client::zcosmic_toplevel_manager_v1::ZcosmicToplevelManagerV1, crate::Error>{
    Ok(state
        .toplevel_manager_state
        .as_ref()
        .ok_or_else(|| {
            crate::Error::ProtocolNotAvailable(
                "zcosmic-toplevel-management-v1 not available".into(),
            )
        })?
        .manager
        .clone())
}

fn cosmic_handle(
    t: &cosmic_client_toolkit::toplevel_info::ToplevelInfo,
) -> Result<&cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1, crate::Error>{
    t.cosmic_toplevel.as_ref().ok_or_else(|| {
        crate::Error::ProtocolNotAvailable("zcosmic-toplevel-info-v1 not available".into())
    })
}

fn flush(eq: &mut wayland_client::EventQueue<AppData>) -> Result<(), crate::Error> {
    eq.flush().map_err(|e| crate::Error::Other(e.to_string()))
}

pub fn window_activate(
    state: &mut AppData,
    eq: &mut wayland_client::EventQueue<AppData>,
    selector: &str,
) -> Result<(), crate::Error> {
    let manager = toplevel_manager(state)?;
    let t = resolve_toplevel_pub(state, selector)?;
    let cosmic = cosmic_handle(t)?;
    let seat = state
        .seat_state
        .seats()
        .next()
        .ok_or_else(|| crate::Error::Other("no seat available".into()))?;
    manager.activate(cosmic, &seat);
    flush(eq)
}

pub fn window_close(
    state: &mut AppData,
    eq: &mut wayland_client::EventQueue<AppData>,
    selector: &str,
) -> Result<(), crate::Error> {
    let manager = toplevel_manager(state)?;
    let t = resolve_toplevel_pub(state, selector)?;
    manager.close(cosmic_handle(t)?);
    flush(eq)
}

pub fn window_maximize(
    state: &mut AppData,
    eq: &mut wayland_client::EventQueue<AppData>,
    selector: &str,
) -> Result<(), crate::Error> {
    let manager = toplevel_manager(state)?;
    let t = resolve_toplevel_pub(state, selector)?;
    manager.set_maximized(cosmic_handle(t)?);
    flush(eq)
}

pub fn window_unmaximize(
    state: &mut AppData,
    eq: &mut wayland_client::EventQueue<AppData>,
    selector: &str,
) -> Result<(), crate::Error> {
    let manager = toplevel_manager(state)?;
    let t = resolve_toplevel_pub(state, selector)?;
    manager.unset_maximized(cosmic_handle(t)?);
    flush(eq)
}

pub fn window_minimize(
    state: &mut AppData,
    eq: &mut wayland_client::EventQueue<AppData>,
    selector: &str,
) -> Result<(), crate::Error> {
    let manager = toplevel_manager(state)?;
    let t = resolve_toplevel_pub(state, selector)?;
    manager.set_minimized(cosmic_handle(t)?);
    flush(eq)
}

pub fn window_unminimize(
    state: &mut AppData,
    eq: &mut wayland_client::EventQueue<AppData>,
    selector: &str,
) -> Result<(), crate::Error> {
    let manager = toplevel_manager(state)?;
    let t = resolve_toplevel_pub(state, selector)?;
    manager.unset_minimized(cosmic_handle(t)?);
    flush(eq)
}

pub fn window_fullscreen(
    state: &mut AppData,
    eq: &mut wayland_client::EventQueue<AppData>,
    selector: &str,
) -> Result<(), crate::Error> {
    let manager = toplevel_manager(state)?;
    let t = resolve_toplevel_pub(state, selector)?;
    manager.set_fullscreen(cosmic_handle(t)?, None);
    flush(eq)
}

pub fn window_unfullscreen(
    state: &mut AppData,
    eq: &mut wayland_client::EventQueue<AppData>,
    selector: &str,
) -> Result<(), crate::Error> {
    let manager = toplevel_manager(state)?;
    let t = resolve_toplevel_pub(state, selector)?;
    manager.unset_fullscreen(cosmic_handle(t)?);
    flush(eq)
}

pub fn window_set_sticky(
    state: &mut AppData,
    eq: &mut wayland_client::EventQueue<AppData>,
    selector: &str,
) -> Result<(), crate::Error> {
    let manager = toplevel_manager(state)?;
    let t = resolve_toplevel_pub(state, selector)?;
    manager.set_sticky(cosmic_handle(t)?);
    flush(eq)
}

pub fn window_unset_sticky(
    state: &mut AppData,
    eq: &mut wayland_client::EventQueue<AppData>,
    selector: &str,
) -> Result<(), crate::Error> {
    let manager = toplevel_manager(state)?;
    let t = resolve_toplevel_pub(state, selector)?;
    manager.unset_sticky(cosmic_handle(t)?);
    flush(eq)
}

pub fn window_move_to_workspace(
    state: &mut AppData,
    eq: &mut wayland_client::EventQueue<AppData>,
    window_selector: &str,
    workspace_selector: &str,
) -> Result<(), crate::Error> {
    let manager = toplevel_manager(state)?;
    let t = resolve_toplevel_pub(state, window_selector)?;
    let cosmic = cosmic_handle(t)?;
    let ws = crate::workspaces::resolve_workspace_pub(state, workspace_selector)?;
    let ws_handle = ws.handle.clone();
    let output = state
        .workspace_state
        .workspace_groups()
        .find(|g| g.workspaces.contains(&ws_handle))
        .and_then(|g| g.outputs.first().cloned())
        .ok_or_else(|| crate::Error::Other("target workspace has no associated output".into()))?;
    manager.move_to_ext_workspace(cosmic, &ws_handle, &output);
    flush(eq)
}
