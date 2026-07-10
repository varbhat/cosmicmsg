mod capture;
mod connection;
mod input;
mod mcp;
mod monitor;
mod output;
mod state;
mod toplevels;
mod types;
mod workspaces;

// Re-export at crate root so `crate::Error` / `crate::OutputFormat` resolve
// everywhere (all submodules use `crate::Error` and `crate::OutputFormat`).
pub use types::{Error, OutputFormat};

use clap::{Parser, Subcommand};
use std::io::{self, BufWriter};

#[derive(Parser, Debug)]
#[command(
    name = "cosmicmsg",
    version,
    about = "Query and control the COSMIC desktop",
    long_about = "cosmicmsg communicates with the COSMIC desktop via Wayland protocols \
                  to query and control workspaces, windows, and outputs."
)]
pub struct Cli {
    /// Output raw JSON (machine-readable, compact)
    #[arg(short, long, conflicts_with = "pretty")]
    pub json: bool,

    /// Output pretty-printed JSON
    #[arg(short, long, conflicts_with = "json")]
    pub pretty: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// List all connected outputs
    GetOutputs,

    /// List all workspaces with their state
    GetWorkspaces,

    /// List all open windows (toplevels) with their state
    GetToplevels,

    /// Show a tree of outputs → workspaces → windows
    GetTree,

    /// Subscribe to compositor events and print them as they arrive.
    ///
    /// Prints one JSON object per line (ndjson). Runs until interrupted (Ctrl-C).
    /// Event types: workspace-added, workspace-updated, workspace-removed,
    ///              window-opened, window-updated, window-closed
    Subscribe,

    /// Screen/window/workspace capture
    #[command(subcommand)]
    Capture(CaptureCommand),

    /// Workspace management commands
    #[command(subcommand)]
    Workspace(WorkspaceCommand),

    /// Window management commands
    #[command(subcommand)]
    Window(WindowCommand),

    /// Start a cosmicmsg MCP server over stdio (MCP spec 2026-07-28)
    Serve,

    /// Remote input via the XDG Remote Desktop portal
    ///
    /// The first invocation shows a system permission dialog. Subsequent
    /// invocations reuse the saved restore token and proceed silently.
    #[command(subcommand)]
    Input(input::InputCommand),
}

#[derive(Subcommand, Debug)]
pub enum CaptureCommand {
    /// Capture an output (monitor) to a PNG file or stdout
    Output {
        /// Output name (e.g. DP-1). Defaults to the first output.
        #[arg(short, long)]
        output: Option<String>,
        /// Write PNG to this file path instead of stdout
        #[arg(short = 'f', long)]
        file: Option<String>,
        /// Paint the cursor onto the captured image
        #[arg(long)]
        cursor: bool,
        /// Scale factor: 0.0–1.0 or percentage (e.g. 0.5 or 50%)
        #[arg(short, long)]
        scale: Option<String>,
    },
    /// Capture a window to a PNG file or stdout
    Window {
        /// Window title, app-id, or identifier (substring match)
        selector: String,
        /// Write PNG to this file path instead of stdout
        #[arg(short = 'f', long)]
        file: Option<String>,
        /// Paint the cursor onto the captured image
        #[arg(long)]
        cursor: bool,
        /// Scale factor: 0.0–1.0 or percentage (e.g. 0.5 or 50%)
        #[arg(short, long)]
        scale: Option<String>,
    },
    /// Capture a workspace to a PNG file or stdout
    Workspace {
        /// Workspace name or id
        workspace: String,
        /// Write PNG to this file path instead of stdout
        #[arg(short = 'f', long)]
        file: Option<String>,
        /// Paint the cursor onto the captured image
        #[arg(long)]
        cursor: bool,
        /// Scale factor: 0.0–1.0 or percentage (e.g. 0.5 or 50%)
        #[arg(short, long)]
        scale: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum WorkspaceCommand {
    /// Switch to a workspace (by name or id)
    Activate { workspace: String },
    /// Rename a workspace
    Rename { workspace: String, new_name: String },
    /// Get the tiling state of a workspace (or the active workspace if omitted)
    GetTiling {
        /// Workspace name or id. Defaults to the currently active workspace.
        workspace: Option<String>,
    },
    /// Enable or disable tiling on a specific workspace
    SetTiling {
        /// Workspace name or id, or "active" to target the currently active workspace
        workspace: String,
        /// enabled or disabled
        state: workspaces::TilingArg,
    },
    /// Set the tiling default for all workspaces that have never had any windows.
    /// Mirrors the "new workspace" behavior toggle in the tiling applet.
    SetTilingDefault {
        /// enabled or disabled
        state: workspaces::TilingArg,
    },
    /// Pin a workspace (make it persistent across outputs)
    Pin { workspace: String },
    /// Unpin a workspace
    Unpin { workspace: String },
    /// Move a workspace to be before another workspace
    MoveBefore { workspace: String, before: String },
    /// Move a workspace to be after another workspace
    MoveAfter { workspace: String, after: String },
}

#[derive(Subcommand, Debug)]
pub enum WindowCommand {
    /// Activate (focus) a window
    Activate { selector: String },
    /// Close a window
    Close { selector: String },
    /// Maximize a window
    Maximize { selector: String },
    /// Unmaximize a window
    Unmaximize { selector: String },
    /// Minimize a window
    Minimize { selector: String },
    /// Unminimize a window
    Unminimize { selector: String },
    /// Fullscreen a window
    Fullscreen { selector: String },
    /// Exit fullscreen on a window
    Unfullscreen { selector: String },
    /// Make a window sticky (visible on all workspaces)
    SetSticky { selector: String },
    /// Remove sticky from a window
    UnsetSticky { selector: String },
    /// Move a window to a workspace
    MoveToWorkspace { selector: String, workspace: String },
}

fn main() {
    let cli = Cli::parse();

    let format = if cli.json {
        OutputFormat::Json
    } else if cli.pretty {
        OutputFormat::PrettyJson
    } else {
        OutputFormat::Human
    };

    if let Err(e) = run(cli.command, format) {
        eprintln!("error: {e}");
        std::process::exit(match e {
            Error::Connect(_) => 1,
            Error::ProtocolNotAvailable(_) => 2,
            Error::NotFound(_) => 3,
            Error::AmbiguousMatch(_) => 4,
            Error::Other(_) => 5,
        });
    }
}

fn run(command: Command, format: OutputFormat) -> Result<(), Error> {
    match command {
        Command::GetOutputs => {
            let (state, _eq) = connection::collect_state()?;
            output::cmd_get_outputs(&state, format);
        }
        Command::GetWorkspaces => {
            let (state, _eq) = connection::collect_state()?;
            workspaces::cmd_get_workspaces(&state, format);
        }
        Command::GetToplevels => {
            let (state, _eq) = connection::collect_state()?;
            toplevels::cmd_get_toplevels(&state, format);
        }
        Command::GetTree => {
            let (state, _eq) = connection::collect_state()?;
            output::cmd_get_tree(&state, format);
        }
        Command::Subscribe => {
            connection::run_monitor(format)?;
        }
        Command::Capture(cap_cmd) => {
            run_capture(cap_cmd)?;
        }
        Command::Workspace(ws_cmd) => {
            let (state, eq) = connection::collect_state()?;
            cmd_workspace(&state, ws_cmd)?;
            eq.flush().map_err(|e| Error::Other(e.to_string()))?;
        }
        Command::Window(win_cmd) => {
            let (mut state, eq) = connection::collect_state()?;
            cmd_window(&mut state, win_cmd)?;
            eq.flush().map_err(|e| Error::Other(e.to_string()))?;
        }
        Command::Input(cmd) => {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| Error::Other(e.to_string()))?
                .block_on(input::dispatch(cmd))
                .map_err(|e| Error::Other(e.to_string()))?;
        }
        Command::Serve => {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|e| Error::Other(e.to_string()))?
                .block_on(mcp::serve())
                .map_err(|e| Error::Other(e.to_string()))?;
        }
    }
    Ok(())
}

fn run_capture(cmd: CaptureCommand) -> Result<(), Error> {
    use cosmic_client_toolkit::screencopy::CaptureSource;

    let (mut state, mut eq) = connection::collect_state()?;

    let (source, file, with_cursor, scale_str) = match cmd {
        CaptureCommand::Output {
            output,
            file,
            cursor,
            scale,
        } => {
            let wl_output = match &output {
                Some(name) => state
                    .output_state
                    .outputs()
                    .find(|o| {
                        state
                            .output_state
                            .info(o)
                            .and_then(|i| i.name.clone())
                            .as_deref()
                            == Some(name.as_str())
                    })
                    .ok_or_else(|| Error::NotFound(format!("output '{}'", name.as_str())))?,
                None => state
                    .output_state
                    .outputs()
                    .next()
                    .ok_or_else(|| Error::NotFound("no outputs available".into()))?,
            };
            (CaptureSource::Output(wl_output), file, cursor, scale)
        }
        CaptureCommand::Window {
            selector,
            file,
            cursor,
            scale,
        } => {
            let info = toplevels::resolve_toplevel_pub(&state, &selector)?;
            let handle = info.foreign_toplevel.clone();
            (CaptureSource::Toplevel(handle), file, cursor, scale)
        }
        CaptureCommand::Workspace {
            workspace,
            file,
            cursor,
            scale,
        } => {
            let ws = workspaces::resolve_workspace_pub(&state, &workspace)?;
            let handle = ws.handle.clone();
            (CaptureSource::Workspace(handle), file, cursor, scale)
        }
    };

    // Parse scale factor: accepts "0.5", "50%", "2x", etc.
    let scale = scale_str.as_deref().map(parse_scale).transpose()?;

    let mut out: Box<dyn std::io::Write> = match &file {
        Some(path) => Box::new(BufWriter::new(
            std::fs::File::create(path)
                .map_err(|e| Error::Other(format!("cannot create file '{path}': {e}")))?,
        )),
        None => Box::new(BufWriter::new(io::stdout())),
    };

    capture::capture_to_png(&mut state, &mut eq, source, &mut *out, with_cursor, scale)?;

    if let Some(path) = &file {
        eprintln!("saved to {path}");
    }

    Ok(())
}

/// Parse a scale string into a positive float.
/// Accepts: "0.5", "50%", "2x", "2X", "200%"
fn parse_scale(s: &str) -> Result<f64, Error> {
    let s = s.trim();
    let v: f64 = if let Some(pct) = s.strip_suffix('%') {
        pct.trim()
            .parse::<f64>()
            .map_err(|_| Error::Other(format!("invalid scale '{s}'")))?
            / 100.0
    } else if let Some(mult) = s.strip_suffix('x').or_else(|| s.strip_suffix('X')) {
        mult.trim()
            .parse::<f64>()
            .map_err(|_| Error::Other(format!("invalid scale '{s}'")))?
    } else {
        s.parse::<f64>()
            .map_err(|_| Error::Other(format!("invalid scale '{s}'")))?
    };
    if v <= 0.0 {
        return Err(Error::Other(format!("scale must be positive, got {s}")));
    }
    Ok(v)
}

// ── CLI command dispatchers (use WorkspaceCommand / WindowCommand enums) ───────

fn cmd_workspace(state: &state::AppData, cmd: WorkspaceCommand) -> Result<(), Error> {
    use cosmic_protocols::workspace::v2::client::zcosmic_workspace_handle_v2;
    use wayland_client::WEnum;
    use workspaces::{resolve_active_workspace, resolve_workspace_pub};

    match cmd {
        WorkspaceCommand::Activate { workspace } => {
            let ws = resolve_workspace_pub(state, &workspace)?;
            ws.handle.activate();
            if let Ok(mgr) = state.workspace_state.workspace_manager().get() {
                mgr.commit();
            }
        }
        WorkspaceCommand::Rename {
            workspace,
            new_name,
        } => {
            let ws = resolve_workspace_pub(state, &workspace)?;
            ws.cosmic_handle
                .as_ref()
                .ok_or_else(|| {
                    Error::ProtocolNotAvailable("zcosmic-workspace-v2 not available".into())
                })?
                .rename(new_name);
            if let Ok(mgr) = state.workspace_state.workspace_manager().get() {
                mgr.commit();
            }
        }
        WorkspaceCommand::GetTiling { workspace } => {
            let ws = match workspace {
                Some(sel) => resolve_workspace_pub(state, &sel)?,
                None => resolve_active_workspace(state)?,
            };
            let tiling = ws
                .tiling
                .as_ref()
                .map(|t| match t {
                    WEnum::Value(zcosmic_workspace_handle_v2::TilingState::TilingEnabled) => {
                        "enabled"
                    }
                    WEnum::Value(zcosmic_workspace_handle_v2::TilingState::FloatingOnly) => {
                        "disabled"
                    }
                    _ => "unknown",
                })
                .unwrap_or("unknown");
            println!("{}: tiling={}", ws.name, tiling);
        }
        WorkspaceCommand::SetTiling {
            workspace,
            state: tiling_arg,
        } => {
            let ws = if workspace.eq_ignore_ascii_case("active") {
                resolve_active_workspace(state)?
            } else {
                resolve_workspace_pub(state, &workspace)?
            };
            let cosmic = ws.cosmic_handle.as_ref().ok_or_else(|| {
                Error::ProtocolNotAvailable("zcosmic-workspace-v2 not available".into())
            })?;
            cosmic.set_tiling_state(workspaces::tiling_arg_to_protocol(&tiling_arg));
            if let Ok(mgr) = state.workspace_state.workspace_manager().get() {
                mgr.commit();
            }
        }
        WorkspaceCommand::SetTilingDefault { state: tiling_arg } => {
            let occupied: std::collections::HashSet<_> = state
                .toplevel_info_state
                .toplevels()
                .flat_map(|t| t.workspace.iter().cloned())
                .collect();
            let tiling_state = workspaces::tiling_arg_to_protocol(&tiling_arg);
            let mut changed = 0;
            for ws in state.workspace_state.workspaces() {
                if !occupied.contains(&ws.handle) {
                    if let Some(cosmic) = &ws.cosmic_handle {
                        cosmic.set_tiling_state(tiling_state);
                        changed += 1;
                    }
                }
            }
            if changed > 0 {
                if let Ok(mgr) = state.workspace_state.workspace_manager().get() {
                    mgr.commit();
                }
            }
            eprintln!("set tiling default on {changed} empty workspace(s)");
        }
        WorkspaceCommand::Pin { workspace } => {
            let ws = resolve_workspace_pub(state, &workspace)?;
            ws.cosmic_handle
                .as_ref()
                .ok_or_else(|| {
                    Error::ProtocolNotAvailable("zcosmic-workspace-v2 not available".into())
                })?
                .pin();
            if let Ok(mgr) = state.workspace_state.workspace_manager().get() {
                mgr.commit();
            }
        }
        WorkspaceCommand::Unpin { workspace } => {
            let ws = resolve_workspace_pub(state, &workspace)?;
            ws.cosmic_handle
                .as_ref()
                .ok_or_else(|| {
                    Error::ProtocolNotAvailable("zcosmic-workspace-v2 not available".into())
                })?
                .unpin();
            if let Ok(mgr) = state.workspace_state.workspace_manager().get() {
                mgr.commit();
            }
        }
        WorkspaceCommand::MoveBefore { workspace, before } => {
            let ws = resolve_workspace_pub(state, &workspace)?;
            let other = resolve_workspace_pub(state, &before)?;
            let cosmic = ws.cosmic_handle.as_ref().ok_or_else(|| {
                Error::ProtocolNotAvailable("zcosmic-workspace-v2 not available".into())
            })?;
            cosmic.move_before(&other.handle, 0);
            if let Ok(mgr) = state.workspace_state.workspace_manager().get() {
                mgr.commit();
            }
        }
        WorkspaceCommand::MoveAfter { workspace, after } => {
            let ws = resolve_workspace_pub(state, &workspace)?;
            let other = resolve_workspace_pub(state, &after)?;
            let cosmic = ws.cosmic_handle.as_ref().ok_or_else(|| {
                Error::ProtocolNotAvailable("zcosmic-workspace-v2 not available".into())
            })?;
            cosmic.move_after(&other.handle, 0);
            if let Ok(mgr) = state.workspace_state.workspace_manager().get() {
                mgr.commit();
            }
        }
    }
    Ok(())
}

fn cmd_window(state: &mut state::AppData, cmd: WindowCommand) -> Result<(), Error> {
    use toplevels::resolve_toplevel_pub;

    let manager = state
        .toplevel_manager_state
        .as_ref()
        .ok_or_else(|| {
            Error::ProtocolNotAvailable("zcosmic-toplevel-management-v1 not available".into())
        })?
        .manager
        .clone();

    macro_rules! cosmic {
        ($t:expr) => {
            $t.cosmic_toplevel.as_ref().ok_or_else(|| {
                Error::ProtocolNotAvailable("zcosmic-toplevel-info-v1 not available".into())
            })?
        };
    }

    match cmd {
        WindowCommand::Activate { selector } => {
            let t = resolve_toplevel_pub(state, &selector)?;
            let seat = state
                .seat_state
                .seats()
                .next()
                .ok_or_else(|| Error::Other("no seat available".into()))?;
            manager.activate(cosmic!(t), &seat);
        }
        WindowCommand::Close { selector } => {
            manager.close(cosmic!(resolve_toplevel_pub(state, &selector)?));
        }
        WindowCommand::Maximize { selector } => {
            manager.set_maximized(cosmic!(resolve_toplevel_pub(state, &selector)?));
        }
        WindowCommand::Unmaximize { selector } => {
            manager.unset_maximized(cosmic!(resolve_toplevel_pub(state, &selector)?));
        }
        WindowCommand::Minimize { selector } => {
            manager.set_minimized(cosmic!(resolve_toplevel_pub(state, &selector)?));
        }
        WindowCommand::Unminimize { selector } => {
            manager.unset_minimized(cosmic!(resolve_toplevel_pub(state, &selector)?));
        }
        WindowCommand::Fullscreen { selector } => {
            manager.set_fullscreen(cosmic!(resolve_toplevel_pub(state, &selector)?), None);
        }
        WindowCommand::Unfullscreen { selector } => {
            manager.unset_fullscreen(cosmic!(resolve_toplevel_pub(state, &selector)?));
        }
        WindowCommand::SetSticky { selector } => {
            manager.set_sticky(cosmic!(resolve_toplevel_pub(state, &selector)?));
        }
        WindowCommand::UnsetSticky { selector } => {
            manager.unset_sticky(cosmic!(resolve_toplevel_pub(state, &selector)?));
        }
        WindowCommand::MoveToWorkspace {
            selector,
            workspace,
        } => {
            let t = resolve_toplevel_pub(state, &selector)?;
            let cosmic = cosmic!(t);
            let ws = workspaces::resolve_workspace_pub(state, &workspace)?;
            let ws_handle = ws.handle.clone();
            let output = state
                .workspace_state
                .workspace_groups()
                .find(|g| g.workspaces.contains(&ws_handle))
                .and_then(|g| g.outputs.first().cloned())
                .ok_or_else(|| Error::Other("target workspace has no associated output".into()))?;
            manager.move_to_ext_workspace(cosmic, &ws_handle, &output);
        }
    }
    Ok(())
}
