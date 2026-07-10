use base64::prelude::*;
use rmcp::{
    ServerHandler, handler::server::wrapper::Parameters, model::*, service::RequestContext, tool,
    tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;

// ── Parameter structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
struct WorkspaceSelector {
    /// Workspace name or id (exact match preferred, substring fallback)
    workspace: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct WorkspaceRenameParams {
    /// Workspace to rename (name or id)
    workspace: String,
    /// New name
    new_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct WorkspaceTilingParams {
    /// Workspace to change ("active" targets the currently active workspace)
    workspace: String,
    /// true to enable tiling, false to disable
    enabled: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TilingDefaultParams {
    /// true to enable tiling on all new (empty) workspaces
    enabled: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct WorkspaceReorderParams {
    /// Workspace to move
    workspace: String,
    /// Reference workspace
    reference: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct WindowSelector {
    /// Window title, app-id, or identifier (exact match preferred, substring fallback)
    selector: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct WindowMoveParams {
    /// Window to move (title, app-id, or identifier)
    selector: String,
    /// Destination workspace (name or id)
    workspace: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CaptureOutputParams {
    /// Output name (e.g. DP-1). Omit for the first available output.
    output: Option<String>,
    /// Paint the cursor onto the image
    #[serde(default)]
    cursor: bool,
    /// Scale factor: 0.0–1.0 or percentage string (e.g. "50%"). Omit for 1:1.
    scale: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CaptureWindowParams {
    /// Window title, app-id, or identifier (substring match)
    selector: String,
    /// Paint the cursor onto the image
    #[serde(default)]
    cursor: bool,
    /// Scale factor (0.0–1.0). Omit for 1:1.
    scale: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CaptureWorkspaceParams {
    /// Workspace name or id
    workspace: String,
    /// Paint the cursor onto the image
    #[serde(default)]
    cursor: bool,
    /// Scale factor (0.0–1.0). Omit for 1:1.
    scale: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct MouseMoveParams {
    /// Horizontal delta in pixels (positive = right)
    dx: f64,
    /// Vertical delta in pixels (positive = down)
    dy: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct MouseMoveAbsParams {
    /// X coordinate in screen pixels
    x: f64,
    /// Y coordinate in screen pixels
    y: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct MouseClickParams {
    /// Button: "left", "right", or "middle" (default: "left")
    #[serde(default = "default_left")]
    button: String,
    /// Action: "tap" (press+release), "press", or "release" (default: "tap")
    #[serde(default = "default_tap")]
    action: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct MouseClickCodeParams {
    /// Raw Linux evdev button code (e.g. 0x113 for side button)
    code: u32,
    /// Action: "tap", "press", or "release" (default: "tap")
    #[serde(default = "default_tap")]
    action: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct MouseScrollParams {
    /// Horizontal scroll amount (negative = left)
    #[serde(default)]
    dx: f64,
    /// Vertical scroll amount (negative = up)
    #[serde(default)]
    dy: f64,
    /// Use discrete (wheel-click) mode; dx/dy are detent counts
    #[serde(default)]
    discrete: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct KeyParams {
    /// Linux evdev keycode (1=Esc, 28=Enter, 57=Space, 29=Ctrl, 42=Shift, 56=Alt, 125=Super)
    keycode: i32,
    /// Action: "tap", "press", or "release" (default: "tap")
    #[serde(default = "default_tap")]
    action: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct KeysymParams {
    /// X11 keysym (e.g. 0xff0d=Return, 0xff1b=Escape, 0x61='a', 0xffe3=Ctrl)
    keysym: u32,
    /// Action: "tap", "press", or "release" (default: "tap")
    #[serde(default = "default_tap")]
    action: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TypeTextParams {
    /// UTF-8 text to inject directly (no layout mapping)
    text: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TouchDownParams {
    /// Finger slot index (0-based)
    slot: u32,
    /// X coordinate
    x: f64,
    /// Y coordinate
    y: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TouchMotionParams {
    /// Finger slot index (0-based)
    slot: u32,
    /// New X coordinate
    x: f64,
    /// New Y coordinate
    y: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TouchSlotParams {
    /// Finger slot index (0-based)
    slot: u32,
}

fn default_left() -> String {
    "left".to_string()
}
fn default_tap() -> String {
    "tap".to_string()
}

// ── Server ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CosmicMsgServer;

// ── Tool implementations ──────────────────────────────────────────────────────

#[tool_router]
impl CosmicMsgServer {
    // ── Query ─────────────────────────────────────────────────────────────────

    /// List all COSMIC workspaces with their state (active, tiling, pinned, coordinates).
    #[tool(
        description = "List all COSMIC workspaces with their state (active, tiling, pinned, coordinates)."
    )]
    async fn get_workspaces(&self) -> Result<CallToolResult, ErrorData> {
        let json = wayland_blocking(|state| {
            serde_json::to_string_pretty(&state.workspaces()).map_err(|e| e.to_string())
        })
        .await?;
        Ok(text_ok(json))
    }

    /// List all open windows (toplevels) with title, app-id, state, workspace, and output.
    #[tool(
        description = "List all open windows (toplevels) with title, app-id, state, workspace, and output."
    )]
    async fn get_toplevels(&self) -> Result<CallToolResult, ErrorData> {
        let json = wayland_blocking(|state| {
            serde_json::to_string_pretty(&state.toplevels()).map_err(|e| e.to_string())
        })
        .await?;
        Ok(text_ok(json))
    }

    /// List all connected monitors with name, resolution, refresh rate, and position.
    #[tool(
        description = "List all connected monitors with name, resolution, refresh rate, and position."
    )]
    async fn get_outputs(&self) -> Result<CallToolResult, ErrorData> {
        let json = wayland_blocking(|state| {
            serde_json::to_string_pretty(&state.outputs()).map_err(|e| e.to_string())
        })
        .await?;
        Ok(text_ok(json))
    }

    /// Show the full compositor tree: outputs → workspaces → windows.
    #[tool(description = "Show the full compositor tree: outputs → workspaces → windows.")]
    async fn get_tree(&self) -> Result<CallToolResult, ErrorData> {
        let json = wayland_blocking(|state| {
            serde_json::to_string_pretty(&state.tree()).map_err(|e| e.to_string())
        })
        .await?;
        Ok(text_ok(json))
    }

    // ── Workspaces ────────────────────────────────────────────────────────────

    /// Switch to (activate) a workspace by name or id.
    #[tool(description = "Switch to (activate) a workspace by name or id.")]
    async fn workspace_activate(
        &self,
        params: Parameters<WorkspaceSelector>,
    ) -> Result<CallToolResult, ErrorData> {
        let w = params.0.workspace;
        wayland_mutation(move |state, eq| crate::workspaces::workspace_activate(state, eq, &w))
            .await?;
        Ok(text_ok("workspace activated"))
    }

    /// Rename a workspace.
    #[tool(description = "Rename a workspace.")]
    async fn workspace_rename(
        &self,
        params: Parameters<WorkspaceRenameParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let p = params.0;
        wayland_mutation(move |state, eq| {
            crate::workspaces::workspace_rename(state, eq, &p.workspace, &p.new_name)
        })
        .await?;
        Ok(text_ok("workspace renamed"))
    }

    /// Enable or disable tiling on a workspace. Use workspace="active" for the current workspace.
    #[tool(
        description = "Enable or disable tiling on a workspace. Use workspace=\"active\" for the current workspace."
    )]
    async fn workspace_set_tiling(
        &self,
        params: Parameters<WorkspaceTilingParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let p = params.0;
        let msg = if p.enabled {
            "tiling enabled"
        } else {
            "tiling disabled"
        };
        wayland_mutation(move |state, eq| {
            crate::workspaces::workspace_set_tiling(state, eq, &p.workspace, p.enabled)
        })
        .await?;
        Ok(text_ok(msg))
    }

    /// Set the default tiling state for all currently empty (new) workspaces.
    #[tool(description = "Set the default tiling state for all currently empty (new) workspaces.")]
    async fn workspace_set_tiling_default(
        &self,
        params: Parameters<TilingDefaultParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let enabled = params.0.enabled;
        wayland_mutation(move |state, eq| {
            crate::workspaces::workspace_set_tiling_default(state, eq, enabled)
        })
        .await?;
        Ok(text_ok("tiling default updated"))
    }

    /// Pin a workspace so it persists across output changes.
    #[tool(description = "Pin a workspace so it persists across output changes.")]
    async fn workspace_pin(
        &self,
        params: Parameters<WorkspaceSelector>,
    ) -> Result<CallToolResult, ErrorData> {
        let w = params.0.workspace;
        wayland_mutation(move |state, eq| crate::workspaces::workspace_pin(state, eq, &w)).await?;
        Ok(text_ok("workspace pinned"))
    }

    /// Unpin a workspace.
    #[tool(description = "Unpin a workspace.")]
    async fn workspace_unpin(
        &self,
        params: Parameters<WorkspaceSelector>,
    ) -> Result<CallToolResult, ErrorData> {
        let w = params.0.workspace;
        wayland_mutation(move |state, eq| crate::workspaces::workspace_unpin(state, eq, &w))
            .await?;
        Ok(text_ok("workspace unpinned"))
    }

    /// Move a workspace to appear immediately before another workspace.
    #[tool(description = "Move a workspace to appear immediately before another workspace.")]
    async fn workspace_move_before(
        &self,
        params: Parameters<WorkspaceReorderParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let p = params.0;
        wayland_mutation(move |state, eq| {
            crate::workspaces::workspace_move_before(state, eq, &p.workspace, &p.reference)
        })
        .await?;
        Ok(text_ok("workspace moved"))
    }

    /// Move a workspace to appear immediately after another workspace.
    #[tool(description = "Move a workspace to appear immediately after another workspace.")]
    async fn workspace_move_after(
        &self,
        params: Parameters<WorkspaceReorderParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let p = params.0;
        wayland_mutation(move |state, eq| {
            crate::workspaces::workspace_move_after(state, eq, &p.workspace, &p.reference)
        })
        .await?;
        Ok(text_ok("workspace moved"))
    }

    // ── Windows ───────────────────────────────────────────────────────────────

    /// Focus (activate) a window. Selector matches by title, app-id, or identifier.
    #[tool(
        description = "Focus (activate) a window. Selector matches by title, app-id, or identifier."
    )]
    async fn window_activate(
        &self,
        params: Parameters<WindowSelector>,
    ) -> Result<CallToolResult, ErrorData> {
        let s = params.0.selector;
        wayland_mutation_mut(move |state, eq| crate::toplevels::window_activate(state, eq, &s))
            .await?;
        Ok(text_ok("window activated"))
    }

    /// Close a window.
    #[tool(description = "Close a window.")]
    async fn window_close(
        &self,
        params: Parameters<WindowSelector>,
    ) -> Result<CallToolResult, ErrorData> {
        let s = params.0.selector;
        wayland_mutation_mut(move |state, eq| crate::toplevels::window_close(state, eq, &s))
            .await?;
        Ok(text_ok("window closed"))
    }

    /// Maximize a window.
    #[tool(description = "Maximize a window.")]
    async fn window_maximize(
        &self,
        params: Parameters<WindowSelector>,
    ) -> Result<CallToolResult, ErrorData> {
        let s = params.0.selector;
        wayland_mutation_mut(move |state, eq| crate::toplevels::window_maximize(state, eq, &s))
            .await?;
        Ok(text_ok("window maximized"))
    }

    /// Restore a maximized window.
    #[tool(description = "Restore a maximized window.")]
    async fn window_unmaximize(
        &self,
        params: Parameters<WindowSelector>,
    ) -> Result<CallToolResult, ErrorData> {
        let s = params.0.selector;
        wayland_mutation_mut(move |state, eq| crate::toplevels::window_unmaximize(state, eq, &s))
            .await?;
        Ok(text_ok("window unmaximized"))
    }

    /// Minimize a window.
    #[tool(description = "Minimize a window.")]
    async fn window_minimize(
        &self,
        params: Parameters<WindowSelector>,
    ) -> Result<CallToolResult, ErrorData> {
        let s = params.0.selector;
        wayland_mutation_mut(move |state, eq| crate::toplevels::window_minimize(state, eq, &s))
            .await?;
        Ok(text_ok("window minimized"))
    }

    /// Restore a minimized window.
    #[tool(description = "Restore a minimized window.")]
    async fn window_unminimize(
        &self,
        params: Parameters<WindowSelector>,
    ) -> Result<CallToolResult, ErrorData> {
        let s = params.0.selector;
        wayland_mutation_mut(move |state, eq| crate::toplevels::window_unminimize(state, eq, &s))
            .await?;
        Ok(text_ok("window unminimized"))
    }

    /// Make a window fullscreen.
    #[tool(description = "Make a window fullscreen.")]
    async fn window_fullscreen(
        &self,
        params: Parameters<WindowSelector>,
    ) -> Result<CallToolResult, ErrorData> {
        let s = params.0.selector;
        wayland_mutation_mut(move |state, eq| crate::toplevels::window_fullscreen(state, eq, &s))
            .await?;
        Ok(text_ok("window fullscreened"))
    }

    /// Exit fullscreen on a window.
    #[tool(description = "Exit fullscreen on a window.")]
    async fn window_unfullscreen(
        &self,
        params: Parameters<WindowSelector>,
    ) -> Result<CallToolResult, ErrorData> {
        let s = params.0.selector;
        wayland_mutation_mut(move |state, eq| crate::toplevels::window_unfullscreen(state, eq, &s))
            .await?;
        Ok(text_ok("window unfullscreened"))
    }

    /// Make a window sticky so it appears on all workspaces.
    #[tool(description = "Make a window sticky so it appears on all workspaces.")]
    async fn window_set_sticky(
        &self,
        params: Parameters<WindowSelector>,
    ) -> Result<CallToolResult, ErrorData> {
        let s = params.0.selector;
        wayland_mutation_mut(move |state, eq| crate::toplevels::window_set_sticky(state, eq, &s))
            .await?;
        Ok(text_ok("window set sticky"))
    }

    /// Remove sticky from a window.
    #[tool(description = "Remove sticky from a window.")]
    async fn window_unset_sticky(
        &self,
        params: Parameters<WindowSelector>,
    ) -> Result<CallToolResult, ErrorData> {
        let s = params.0.selector;
        wayland_mutation_mut(move |state, eq| crate::toplevels::window_unset_sticky(state, eq, &s))
            .await?;
        Ok(text_ok("window unset sticky"))
    }

    /// Move a window to a different workspace.
    #[tool(description = "Move a window to a different workspace.")]
    async fn window_move_to_workspace(
        &self,
        params: Parameters<WindowMoveParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let p = params.0;
        wayland_mutation_mut(move |state, eq| {
            crate::toplevels::window_move_to_workspace(state, eq, &p.selector, &p.workspace)
        })
        .await?;
        Ok(text_ok("window moved"))
    }

    // ── Capture ───────────────────────────────────────────────────────────────

    /// Capture a monitor as a PNG image. Returns base64-encoded image/png.
    #[tool(description = "Capture a monitor as a PNG image. Returns base64-encoded image/png.")]
    async fn capture_output(
        &self,
        params: Parameters<CaptureOutputParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let p = params.0;
        let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
            let (mut state, mut eq) =
                crate::connection::collect_state().map_err(|e| e.to_string())?;
            let output = match &p.output {
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
                    .ok_or_else(|| format!("output '{}' not found", name))?,
                None => state
                    .output_state
                    .outputs()
                    .next()
                    .ok_or("no outputs available")?,
            };
            let mut buf = Vec::new();
            crate::capture::capture_output(
                &mut state, &mut eq, output, &mut buf, p.cursor, p.scale,
            )
            .map_err(|e| e.to_string())?;
            Ok(buf)
        })
        .await
        .map_err(|e| internal(e.to_string()))?
        .map_err(|e| internal(e))?;
        Ok(image_ok(bytes))
    }

    /// Capture a window as a PNG image. Selector matches by title, app-id, or identifier.
    #[tool(
        description = "Capture a window as a PNG image. Selector matches by title, app-id, or identifier."
    )]
    async fn capture_window(
        &self,
        params: Parameters<CaptureWindowParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let p = params.0;
        let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
            let (mut state, mut eq) =
                crate::connection::collect_state().map_err(|e| e.to_string())?;
            let mut buf = Vec::new();
            crate::capture::capture_window(
                &mut state,
                &mut eq,
                &p.selector,
                &mut buf,
                p.cursor,
                p.scale,
            )
            .map_err(|e| e.to_string())?;
            Ok(buf)
        })
        .await
        .map_err(|e| internal(e.to_string()))?
        .map_err(|e| internal(e))?;
        Ok(image_ok(bytes))
    }

    /// Capture a workspace as a PNG image.
    #[tool(description = "Capture a workspace as a PNG image.")]
    async fn capture_workspace(
        &self,
        params: Parameters<CaptureWorkspaceParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let p = params.0;
        let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
            let (mut state, mut eq) =
                crate::connection::collect_state().map_err(|e| e.to_string())?;
            let mut buf = Vec::new();
            crate::capture::capture_workspace(
                &mut state,
                &mut eq,
                &p.workspace,
                &mut buf,
                p.cursor,
                p.scale,
            )
            .map_err(|e| e.to_string())?;
            Ok(buf)
        })
        .await
        .map_err(|e| internal(e.to_string()))?
        .map_err(|e| internal(e))?;
        Ok(image_ok(bytes))
    }

    // ── Input — pointer ───────────────────────────────────────────────────────

    /// Move the pointer by a relative offset in pixels.
    #[tool(description = "Move the pointer by a relative offset in pixels.")]
    async fn input_mouse_move(
        &self,
        params: Parameters<MouseMoveParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let p = params.0;
        run_input(crate::input::InputCommand::MouseMove { dx: p.dx, dy: p.dy }).await?;
        Ok(text_ok(format!("mouse moved by ({}, {})", p.dx, p.dy)))
    }

    /// Move the pointer to an absolute screen position.
    #[tool(description = "Move the pointer to an absolute screen position.")]
    async fn input_mouse_move_abs(
        &self,
        params: Parameters<MouseMoveAbsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let p = params.0;
        run_input(crate::input::InputCommand::MouseMoveAbs { x: p.x, y: p.y }).await?;
        Ok(text_ok(format!("mouse moved to ({}, {})", p.x, p.y)))
    }

    /// Press, release, or tap a mouse button.
    /// button: "left" | "right" | "middle" (default: "left")
    /// action: "tap" | "press" | "release" (default: "tap")
    #[tool(
        description = "Press, release, or tap a mouse button. button: left|right|middle. action: tap|press|release."
    )]
    async fn input_mouse_click(
        &self,
        params: Parameters<MouseClickParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let p = params.0;
        let button = parse_button(&p.button)?;
        let action = parse_action(&p.action)?;
        run_input(crate::input::InputCommand::MouseClick {
            button,
            code: None,
            action,
        })
        .await?;
        Ok(text_ok(format!("{} {} clicked", p.action, p.button)))
    }

    /// Press, release, or tap a mouse button by raw Linux evdev code.
    /// Common codes: 0x110=left, 0x111=right, 0x112=middle, 0x113=side, 0x114=extra.
    #[tool(
        description = "Press/release/tap a mouse button by raw evdev code. 0x110=left, 0x111=right, 0x112=middle, 0x113=side, 0x114=extra."
    )]
    async fn input_mouse_click_code(
        &self,
        params: Parameters<MouseClickCodeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let p = params.0;
        let action = parse_action(&p.action)?;
        run_input(crate::input::InputCommand::MouseClick {
            button: crate::input::MouseButton::Left, // ignored when code is Some
            code: Some(p.code),
            action,
        })
        .await?;
        Ok(text_ok(format!("button 0x{:x} {}", p.code, p.action)))
    }

    /// Smooth or discrete scroll. Set discrete=true for wheel-click scroll (dy=-1 = one click up).
    #[tool(
        description = "Scroll the pointer. dx/dy in pixels (smooth) or detent counts (discrete=true). Negative dy = up."
    )]
    async fn input_mouse_scroll(
        &self,
        params: Parameters<MouseScrollParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let p = params.0;
        run_input(crate::input::InputCommand::MouseScroll {
            dx: p.dx,
            dy: p.dy,
            discrete: p.discrete,
        })
        .await?;
        Ok(text_ok("scrolled"))
    }

    // ── Input — keyboard ──────────────────────────────────────────────────────

    /// Send a keyboard key by Linux evdev keycode.
    /// Common: 1=Esc, 14=Backspace, 28=Enter, 57=Space, 29=Ctrl, 42=Shift, 56=Alt, 125=Super.
    #[tool(
        description = "Send a key by evdev keycode. 1=Esc, 14=Backspace, 28=Enter, 57=Space, 29=Ctrl, 42=Shift, 56=Alt, 125=Super. action: tap|press|release."
    )]
    async fn input_key(&self, params: Parameters<KeyParams>) -> Result<CallToolResult, ErrorData> {
        let p = params.0;
        let action = parse_action(&p.action)?;
        run_input(crate::input::InputCommand::Key {
            keycode: p.keycode,
            action,
        })
        .await?;
        Ok(text_ok(format!("keycode {} {}", p.keycode, p.action)))
    }

    /// Send a key by X11 keysym value.
    /// Common: 0xff0d=Return, 0xff1b=Escape, 0x20=space, 0x61-0x7a=a-z, 0xffe3=Ctrl.
    #[tool(
        description = "Send a key by X11 keysym. 0xff0d=Return, 0xff1b=Esc, 0x20=space, 0x61='a', 0xffe3=Ctrl. action: tap|press|release."
    )]
    async fn input_keysym(
        &self,
        params: Parameters<KeysymParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let p = params.0;
        let action = parse_action(&p.action)?;
        run_input(crate::input::InputCommand::Keysym {
            keysym: p.keysym,
            action,
        })
        .await?;
        Ok(text_ok(format!("keysym 0x{:x} {}", p.keysym, p.action)))
    }

    /// Inject a UTF-8 string directly into the focused application. Full Unicode supported.
    #[tool(
        description = "Type a UTF-8 string into the focused application. Full Unicode supported. No layout mapping needed."
    )]
    async fn input_type(
        &self,
        params: Parameters<TypeTextParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let text = params.0.text;
        run_input(crate::input::InputCommand::TypeText { text: text.clone() }).await?;
        Ok(text_ok(format!("typed: {text}")))
    }

    // ── Input — touch ─────────────────────────────────────────────────────────

    /// Touch-down event (first contact). slot is the finger index (0-based).
    #[tool(description = "Touch-down event. slot=finger index (0-based), x/y=coordinates.")]
    async fn input_touch_down(
        &self,
        params: Parameters<TouchDownParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let p = params.0;
        run_input(crate::input::InputCommand::TouchDown {
            slot: p.slot,
            x: p.x,
            y: p.y,
        })
        .await?;
        Ok(text_ok(format!(
            "touch down slot {} at ({}, {})",
            p.slot, p.x, p.y
        )))
    }

    /// Touch-motion event (finger moved while in contact).
    #[tool(
        description = "Touch-motion event (finger moved). slot=finger index, x/y=new coordinates."
    )]
    async fn input_touch_motion(
        &self,
        params: Parameters<TouchMotionParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let p = params.0;
        run_input(crate::input::InputCommand::TouchMotion {
            slot: p.slot,
            x: p.x,
            y: p.y,
        })
        .await?;
        Ok(text_ok(format!(
            "touch motion slot {} to ({}, {})",
            p.slot, p.x, p.y
        )))
    }

    /// Touch-up event (finger lifted).
    #[tool(description = "Touch-up event (finger lifted). slot=finger index.")]
    async fn input_touch_up(
        &self,
        params: Parameters<TouchSlotParams>,
    ) -> Result<CallToolResult, ErrorData> {
        run_input(crate::input::InputCommand::TouchUp {
            slot: params.0.slot,
        })
        .await?;
        Ok(text_ok(format!("touch up slot {}", params.0.slot)))
    }

    /// Touch-cancel event (gesture aborted, touch point cleared).
    #[tool(description = "Touch-cancel event (gesture aborted). slot=finger index.")]
    async fn input_touch_cancel(
        &self,
        params: Parameters<TouchSlotParams>,
    ) -> Result<CallToolResult, ErrorData> {
        run_input(crate::input::InputCommand::TouchCancel {
            slot: params.0.slot,
        })
        .await?;
        Ok(text_ok(format!("touch cancel slot {}", params.0.slot)))
    }
}

// ── ServerHandler impl ────────────────────────────────────────────────────────

#[tool_handler]
impl ServerHandler for CosmicMsgServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new("cosmicmsg", env!("CARGO_PKG_VERSION")))
        .with_instructions(
            "MCP server for the COSMIC desktop. \
             Provides tools for querying and controlling workspaces, windows, and outputs; \
             capturing screenshots; and injecting mouse, keyboard, and touch input via the \
             XDG Remote Desktop portal (first use requires a permission dialog). \
             Selectors for windows and workspaces accept exact names or substrings.",
        )
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        Ok(ListResourcesResult {
            result_type: Some(ResultType::COMPLETE),
            resources: vec![
                Resource::new("cosmic://workspaces", "workspaces")
                    .with_description("All COSMIC workspaces with their current state")
                    .with_mime_type("application/json"),
                Resource::new("cosmic://toplevels", "toplevels")
                    .with_description("All open windows (toplevels) with their current state")
                    .with_mime_type("application/json"),
                Resource::new("cosmic://outputs", "outputs")
                    .with_description("All connected monitors with resolution and position")
                    .with_mime_type("application/json"),
                Resource::new("cosmic://tree", "tree")
                    .with_description("Full compositor tree: outputs → workspaces → windows")
                    .with_mime_type("application/json"),
            ],
            next_cursor: None,
            meta: None,
            ttl_ms: Some(2_000), // 2s: compositor state changes infrequently
            cache_scope: Some(CacheScope::Private),
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        let uri = request.uri.clone();
        let json = wayland_blocking(move |state| match uri.as_str() {
            "cosmic://workspaces" => {
                serde_json::to_string_pretty(&state.workspaces()).map_err(|e| e.to_string())
            }
            "cosmic://toplevels" => {
                serde_json::to_string_pretty(&state.toplevels()).map_err(|e| e.to_string())
            }
            "cosmic://outputs" => {
                serde_json::to_string_pretty(&state.outputs()).map_err(|e| e.to_string())
            }
            "cosmic://tree" => {
                serde_json::to_string_pretty(&state.tree()).map_err(|e| e.to_string())
            }
            _ => Err(format!("unknown resource: {uri}")),
        })
        .await?;

        Ok(ReadResourceResult::new(vec![ResourceContents::text(request.uri, json)]).into())
    }
}

// ── Serve entry point ─────────────────────────────────────────────────────────

/// Start the cosmicmsg MCP server over stdio.
pub async fn serve() -> anyhow::Result<()> {
    use rmcp::ServiceExt;
    use rmcp::transport::stdio;

    eprintln!("cosmicmsg MCP server started (stdio)");
    CosmicMsgServer.serve(stdio()).await?.waiting().await?;
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn internal(msg: impl Into<String>) -> ErrorData {
    ErrorData::internal_error(msg.into(), None)
}

fn text_ok(s: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(s.into())])
}

fn image_ok(png: Vec<u8>) -> CallToolResult {
    let data = BASE64_STANDARD.encode(&png);
    CallToolResult::success(vec![ContentBlock::Image(ImageContent::new(
        data,
        "image/png",
    ))])
}

/// Run a read-only closure against a fresh Wayland state snapshot.
async fn wayland_blocking<F, T>(f: F) -> Result<T, ErrorData>
where
    F: FnOnce(&crate::state::AppData) -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let (state, _eq) =
            crate::connection::collect_state().map_err(|e| internal(e.to_string()))?;
        f(&state).map_err(|e| internal(e))
    })
    .await
    .map_err(|e| internal(e.to_string()))?
}

/// Run a mutation that takes `(&AppData, &mut EventQueue)`.
async fn wayland_mutation<F>(f: F) -> Result<(), ErrorData>
where
    F: FnOnce(
            &crate::state::AppData,
            &mut wayland_client::EventQueue<crate::state::AppData>,
        ) -> Result<(), crate::Error>
        + Send
        + 'static,
{
    tokio::task::spawn_blocking(move || {
        let (state, mut eq) =
            crate::connection::collect_state().map_err(|e| internal(e.to_string()))?;
        f(&state, &mut eq).map_err(|e| internal(e.to_string()))
    })
    .await
    .map_err(|e| internal(e.to_string()))?
}

/// Run a mutation that takes `(&mut AppData, &mut EventQueue)`.
async fn wayland_mutation_mut<F>(f: F) -> Result<(), ErrorData>
where
    F: FnOnce(
            &mut crate::state::AppData,
            &mut wayland_client::EventQueue<crate::state::AppData>,
        ) -> Result<(), crate::Error>
        + Send
        + 'static,
{
    tokio::task::spawn_blocking(move || {
        let (mut state, mut eq) =
            crate::connection::collect_state().map_err(|e| internal(e.to_string()))?;
        f(&mut state, &mut eq).map_err(|e| internal(e.to_string()))
    })
    .await
    .map_err(|e| internal(e.to_string()))?
}

/// Dispatch a cosmicmsg input command.
///
/// `input::dispatch` holds non-Send zbus/EIS internals across await points, so
/// we run it on a fresh single-thread runtime inside `spawn_blocking` rather
/// than awaiting it directly in the multi-thread MCP executor.
async fn run_input(cmd: crate::input::InputCommand) -> Result<(), ErrorData> {
    tokio::task::spawn_blocking(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| internal(e.to_string()))?
            .block_on(crate::input::dispatch(cmd))
            .map_err(|e| internal(format!("{e:#}")))
    })
    .await
    .map_err(|e| internal(e.to_string()))?
}

fn parse_button(s: &str) -> Result<crate::input::MouseButton, ErrorData> {
    match s {
        "left" | "Left" => Ok(crate::input::MouseButton::Left),
        "right" | "Right" => Ok(crate::input::MouseButton::Right),
        "middle" | "Middle" => Ok(crate::input::MouseButton::Middle),
        _ => Err(ErrorData::invalid_params(
            format!("unknown button '{s}'; use left, right, or middle"),
            None,
        )),
    }
}

fn parse_action(s: &str) -> Result<crate::input::KeyAction, ErrorData> {
    match s {
        "tap" => Ok(crate::input::KeyAction::Tap),
        "press" => Ok(crate::input::KeyAction::Press),
        "release" => Ok(crate::input::KeyAction::Release),
        _ => Err(ErrorData::invalid_params(
            format!("unknown action '{s}'; use tap, press, or release"),
            None,
        )),
    }
}
