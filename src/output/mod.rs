use crate::{OutputFormat, state::AppData};
use serde::Serialize;
use smithay_client_toolkit::output::OutputInfo;
use wayland_client::protocol::wl_output::Transform;

#[derive(Serialize)]
pub struct OutputJson {
    pub name: Option<String>,
    pub description: Option<String>,
    pub make: String,
    pub model: String,
    pub width: i32,
    pub height: i32,
    pub refresh: i32,
    pub x: i32,
    pub y: i32,
    pub scale: i32,
    pub transform: String,
    pub current_mode: Option<ModeJson>,
}

#[derive(Serialize)]
pub struct ModeJson {
    pub width: i32,
    pub height: i32,
    pub refresh: i32,
}

fn transform_name(t: Transform) -> &'static str {
    match t {
        Transform::Normal => "normal",
        Transform::_90 => "90",
        Transform::_180 => "180",
        Transform::_270 => "270",
        Transform::Flipped => "flipped",
        Transform::Flipped90 => "flipped-90",
        Transform::Flipped180 => "flipped-180",
        Transform::Flipped270 => "flipped-270",
        _ => "unknown",
    }
}

pub fn output_info_to_json(info: &OutputInfo) -> OutputJson {
    let current_mode = info.modes.iter().find(|m| m.current).map(|m| ModeJson {
        width: m.dimensions.0,
        height: m.dimensions.1,
        refresh: m.refresh_rate,
    });

    let (width, height, refresh) = current_mode
        .as_ref()
        .map(|m| (m.width, m.height, m.refresh))
        .unwrap_or((0, 0, 0));

    let (x, y) = info.location;

    OutputJson {
        name: info.name.clone(),
        description: info.description.clone(),
        make: info.make.clone(),
        model: info.model.clone(),
        width,
        height,
        refresh,
        x,
        y,
        scale: info.scale_factor,
        transform: transform_name(info.transform).to_string(),
        current_mode,
    }
}

pub fn cmd_get_outputs(state: &AppData, format: OutputFormat) {
    let outputs: Vec<OutputJson> = state
        .output_state
        .outputs()
        .filter_map(|o| state.output_state.info(&o))
        .map(|info| output_info_to_json(&info))
        .collect();

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string(&outputs).unwrap());
        }
        OutputFormat::PrettyJson => {
            println!("{}", serde_json::to_string_pretty(&outputs).unwrap());
        }
        OutputFormat::Human => {
            for o in &outputs {
                let name = o.name.as_deref().unwrap_or("unknown");
                let desc = o.description.as_deref().unwrap_or("");
                println!(
                    "{} \"{}\" {}x{}@{:.3}Hz at ({}, {})",
                    name,
                    desc,
                    o.width,
                    o.height,
                    o.refresh as f64 / 1000.0,
                    o.x,
                    o.y
                );
            }
        }
    }
}

// ── get-tree ─────────────────────────────────────────────────────────────────

use crate::toplevels::ToplevelJson;

#[derive(Serialize)]
pub struct TreeOutput {
    pub name: Option<String>,
    pub description: Option<String>,
    pub width: i32,
    pub height: i32,
    pub x: i32,
    pub y: i32,
    pub workspaces: Vec<TreeWorkspace>,
    /// Windows not associated with any workspace on this output
    pub floating: Vec<ToplevelJson>,
}

#[derive(Serialize)]
pub struct TreeWorkspace {
    pub name: String,
    pub id: Option<String>,
    pub active: bool,
    pub tiling: Option<String>,
    pub windows: Vec<ToplevelJson>,
}

/// Build the output→workspace→window tree without printing.
pub fn build_tree(state: &AppData) -> Vec<TreeOutput> {
    use cosmic_protocols::workspace::v2::client::zcosmic_workspace_handle_v2::TilingState;
    use wayland_client::WEnum;
    use wayland_protocols::ext::workspace::v1::client::ext_workspace_handle_v1;

    let outputs: Vec<_> = state
        .output_state
        .outputs()
        .filter_map(|o| state.output_state.info(&o).map(|i| (o, i)))
        .collect();

    let workspaces: Vec<_> = state.workspace_state.workspaces().collect();
    let groups: Vec<_> = state.workspace_state.workspace_groups().collect();
    let toplevels: Vec<_> = state.toplevel_info_state.toplevels().collect();

    let mut tree: Vec<TreeOutput> = Vec::new();

    for (wl_output, info) in &outputs {
        let group_handles_on_output: Vec<_> = groups
            .iter()
            .filter(|g| g.outputs.contains(wl_output))
            .collect();

        let mut tree_workspaces: Vec<TreeWorkspace> = Vec::new();

        for group in &group_handles_on_output {
            let mut wss_in_group: Vec<_> = workspaces
                .iter()
                .filter(|w| group.workspaces.contains(&w.handle))
                .collect();
            wss_in_group.sort_by(|a, b| a.coordinates.cmp(&b.coordinates));

            for ws in wss_in_group {
                let ws_windows: Vec<ToplevelJson> = toplevels
                    .iter()
                    .filter(|t| t.workspace.contains(&ws.handle))
                    .map(|t| crate::toplevels::toplevel_to_json(t, state))
                    .collect();

                let active = ws.state.contains(ext_workspace_handle_v1::State::Active);
                let tiling = ws.tiling.as_ref().map(|t| match t {
                    WEnum::Value(TilingState::TilingEnabled) => "enabled".to_string(),
                    WEnum::Value(TilingState::FloatingOnly) => "disabled".to_string(),
                    _ => "unknown".to_string(),
                });

                tree_workspaces.push(TreeWorkspace {
                    name: ws.name.clone(),
                    id: ws.id.clone(),
                    active,
                    tiling,
                    windows: ws_windows,
                });
            }
        }

        let floating: Vec<ToplevelJson> = toplevels
            .iter()
            .filter(|t| {
                t.output.contains(wl_output)
                    && !t
                        .workspace
                        .iter()
                        .any(|ws_handle| workspaces.iter().any(|ws| &ws.handle == ws_handle))
            })
            .map(|t| crate::toplevels::toplevel_to_json(t, state))
            .collect();

        let (w, h) = info
            .modes
            .iter()
            .find(|m| m.current)
            .map(|m| (m.dimensions.0, m.dimensions.1))
            .unwrap_or((0, 0));

        let (x, y) = info.location;

        tree.push(TreeOutput {
            name: info.name.clone(),
            description: info.description.clone(),
            width: w,
            height: h,
            x,
            y,
            workspaces: tree_workspaces,
            floating,
        });
    }

    tree
}

pub fn cmd_get_tree(state: &AppData, format: OutputFormat) {
    let tree = build_tree(state);

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string(&tree).unwrap());
        }
        OutputFormat::PrettyJson => {
            println!("{}", serde_json::to_string_pretty(&tree).unwrap());
        }
        OutputFormat::Human => {
            for output in &tree {
                let name = output.name.as_deref().unwrap_or("unknown");
                let desc = output.description.as_deref().unwrap_or("");
                println!(
                    "Output: {} \"{}\" {}x{} at ({}, {})",
                    name, desc, output.width, output.height, output.x, output.y
                );
                for ws in &output.workspaces {
                    let active_mark = if ws.active { " *" } else { "" };
                    let tiling_mark = ws
                        .tiling
                        .as_deref()
                        .map(|t| format!(" [tiling: {t}]"))
                        .unwrap_or_default();
                    println!("  Workspace: {}{}{}", ws.name, active_mark, tiling_mark);
                    for win in &ws.windows {
                        println!("    - \"{}\" ({})", win.title, win.app_id);
                    }
                }
                if !output.floating.is_empty() {
                    println!("  Floating/sticky:");
                    for win in &output.floating {
                        println!("    - \"{}\" ({})", win.title, win.app_id);
                    }
                }
            }
        }
    }
}
