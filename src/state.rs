use cosmic_client_toolkit::{
    screencopy::{
        CaptureFrame, CaptureSession, FailureReason, Formats, Frame, ScreencopyHandler,
        ScreencopyState,
    },
    toplevel_info::{ToplevelInfoHandler, ToplevelInfoState},
    toplevel_management::ToplevelManagerState,
    workspace::{WorkspaceHandler, WorkspaceState},
};
use cosmic_protocols::toplevel_management::v1::client::zcosmic_toplevel_manager_v1;
use smithay_client_toolkit::{
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    seat::{Capability, SeatHandler, SeatState},
    shm::{Shm, ShmHandler},
};
use wayland_client::{
    Connection, QueueHandle, WEnum,
    protocol::{wl_output, wl_seat},
};
use wayland_protocols::ext::{
    foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1,
    workspace::v1::client::ext_workspace_handle_v1,
};

use crate::OutputFormat;
use crate::monitor::{MonitorEvent, print_event};
use crate::toplevels::toplevel_to_json;
use crate::workspaces::workspace_to_json;

// ── Monitor config ─────────────────────────────────────────────────────────────

pub struct MonitorConfig {
    pub format: OutputFormat,
    pub known_workspaces: std::collections::HashSet<ext_workspace_handle_v1::ExtWorkspaceHandleV1>,
    pub workspace_snapshots: std::collections::HashMap<
        ext_workspace_handle_v1::ExtWorkspaceHandleV1,
        (String, Option<String>),
    >,
}

impl MonitorConfig {
    pub fn new(format: OutputFormat) -> Self {
        Self {
            format,
            known_workspaces: std::collections::HashSet::new(),
            workspace_snapshots: std::collections::HashMap::new(),
        }
    }
}

// ── Capture state ─────────────────────────────────────────────────────────────

pub enum CaptureResult {
    /// Frame is ready; data was written into the shm buffer (caller reads it directly).
    Ready,
    Failed(String),
    Stopped,
}

// ── AppData ───────────────────────────────────────────────────────────────────

pub struct AppData {
    pub registry_state: RegistryState,
    pub output_state: OutputState,
    pub seat_state: SeatState,
    pub shm_state: Shm,
    pub workspace_state: WorkspaceState,
    pub toplevel_info_state: ToplevelInfoState,
    pub toplevel_manager_state: Option<ToplevelManagerState>,
    pub screencopy_state: ScreencopyState,

    /// Set when running in subscribe/monitor mode via the CLI (prints to stdout).
    pub monitor: Option<MonitorConfig>,
    /// Set when running in subscribe/monitor mode via the library API (calls a handler).
    pub event_handler: Option<Box<dyn FnMut(MonitorEvent) -> Result<(), crate::Error>>>,
    /// Sticky error set by the event handler so the monitor loop can surface it.
    pub handler_error: Option<crate::Error>,

    pub workspaces_done: bool,
    pub toplevels_done: bool,

    // Screencopy signals
    pub capture_result: Option<CaptureResult>,
    pub capture_formats: Option<Formats>,
}

impl AppData {
    pub fn new(
        registry_state: RegistryState,
        output_state: OutputState,
        seat_state: SeatState,
        shm_state: Shm,
        workspace_state: WorkspaceState,
        toplevel_info_state: ToplevelInfoState,
        toplevel_manager_state: Option<ToplevelManagerState>,
        screencopy_state: ScreencopyState,
    ) -> Self {
        Self {
            registry_state,
            output_state,
            seat_state,
            shm_state,
            workspace_state,
            toplevel_info_state,
            toplevel_manager_state,
            screencopy_state,
            monitor: None,
            event_handler: None,
            handler_error: None,
            workspaces_done: false,
            toplevels_done: false,
            capture_result: None,
            capture_formats: None,
        }
    }
}

impl AppData {
    /// Dispatch a monitor event to whichever sink is configured:
    /// - CLI mode: print via `monitor::print_event`
    /// - Library mode: call the user's `event_handler`
    pub(crate) fn emit(&mut self, event: MonitorEvent) {
        if let Some(fmt) = self.monitor.as_ref().map(|m| m.format) {
            print_event(&event, fmt);
        }
        if let Some(handler) = self.event_handler.as_mut() {
            if let Err(e) = handler(event) {
                self.handler_error = Some(e);
            }
        }
    }
}

// ── ProvidesRegistryState ─────────────────────────────────────────────────────

impl ProvidesRegistryState for AppData {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    smithay_client_toolkit::registry_handlers!(OutputState, SeatState);
}

// ── OutputHandler ─────────────────────────────────────────────────────────────

impl OutputHandler for AppData {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

// ── SeatHandler ───────────────────────────────────────────────────────────────

impl SeatHandler for AppData {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        _: Capability,
    ) {
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        _: Capability,
    ) {
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

// ── ShmHandler ────────────────────────────────────────────────────────────────

impl ShmHandler for AppData {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm_state
    }
}

// ── WorkspaceHandler ──────────────────────────────────────────────────────────

impl WorkspaceHandler for AppData {
    fn workspace_state(&mut self) -> &mut WorkspaceState {
        &mut self.workspace_state
    }

    fn done(&mut self) {
        self.workspaces_done = true;

        if self.monitor.is_none() && self.event_handler.is_none() {
            return;
        }

        let current: std::collections::HashSet<ext_workspace_handle_v1::ExtWorkspaceHandleV1> =
            self.workspace_state
                .workspaces()
                .map(|w| w.handle.clone())
                .collect();

        let known: std::collections::HashSet<_> = self
            .monitor
            .as_ref()
            .map(|m| m.known_workspaces.clone())
            .unwrap_or_default();

        let mut added: Vec<MonitorEvent> = Vec::new();
        let mut updated: Vec<MonitorEvent> = Vec::new();
        let mut removed: Vec<MonitorEvent> = Vec::new();

        for ws in self.workspace_state.workspaces() {
            let json = workspace_to_json(ws, self);
            if known.contains(&ws.handle) {
                updated.push(MonitorEvent::WorkspaceUpdated { workspace: json });
            } else {
                added.push(MonitorEvent::WorkspaceAdded { workspace: json });
            }
        }

        let snapshots = self
            .monitor
            .as_ref()
            .map(|m| m.workspace_snapshots.clone())
            .unwrap_or_default();
        for gone_handle in known.difference(&current) {
            let (name, wid) = snapshots
                .get(gone_handle)
                .cloned()
                .unwrap_or_else(|| ("<unknown>".to_string(), None));
            removed.push(MonitorEvent::WorkspaceRemoved { name, id: wid });
        }

        let events: Vec<MonitorEvent> = added.into_iter().chain(updated).chain(removed).collect();
        for ev in events {
            self.emit(ev);
        }

        if let Some(mon) = self.monitor.as_mut() {
            mon.known_workspaces = current;
            mon.workspace_snapshots.clear();
            for ws in self.workspace_state.workspaces() {
                mon.workspace_snapshots
                    .insert(ws.handle.clone(), (ws.name.clone(), ws.id.clone()));
            }
        }
    }
}

// ── ToplevelInfoHandler ───────────────────────────────────────────────────────

impl ToplevelInfoHandler for AppData {
    fn toplevel_info_state(&mut self) -> &mut ToplevelInfoState {
        &mut self.toplevel_info_state
    }

    fn new_toplevel(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        handle: &ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
    ) {
        if self.monitor.is_some() || self.event_handler.is_some() {
            if let Some(info) = self.toplevel_info_state.info(handle) {
                let json = toplevel_to_json(info, self);
                self.emit(MonitorEvent::WindowOpened { window: json });
            }
        }
    }

    fn update_toplevel(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        handle: &ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
    ) {
        if self.monitor.is_some() || self.event_handler.is_some() {
            if let Some(info) = self.toplevel_info_state.info(handle) {
                let json = toplevel_to_json(info, self);
                self.emit(MonitorEvent::WindowUpdated { window: json });
            }
        }
    }

    fn toplevel_closed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        handle: &ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
    ) {
        if self.monitor.is_some() || self.event_handler.is_some() {
            if let Some(info) = self.toplevel_info_state.info(handle) {
                let ev = MonitorEvent::WindowClosed {
                    title: info.title.clone(),
                    app_id: info.app_id.clone(),
                    identifier: info.identifier.clone(),
                };
                self.emit(ev);
            }
        }
    }

    fn info_done(&mut self, _: &Connection, _: &QueueHandle<Self>) {
        self.toplevels_done = true;
    }
}

// ── ToplevelManagerHandler ────────────────────────────────────────────────────

impl cosmic_client_toolkit::toplevel_management::ToplevelManagerHandler for AppData {
    fn toplevel_manager_state(
        &mut self,
    ) -> &mut cosmic_client_toolkit::toplevel_management::ToplevelManagerState {
        self.toplevel_manager_state.as_mut().unwrap()
    }

    fn capabilities(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: Vec<WEnum<zcosmic_toplevel_manager_v1::ZcosmicToplelevelManagementCapabilitiesV1>>,
    ) {
    }
}

// ── ScreencopyHandler ─────────────────────────────────────────────────────────

impl ScreencopyHandler for AppData {
    fn screencopy_state(&mut self) -> &mut ScreencopyState {
        &mut self.screencopy_state
    }

    fn init_done(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _session: &CaptureSession,
        formats: &Formats,
    ) {
        self.capture_formats = Some(formats.clone());
    }

    fn stopped(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &CaptureSession) {
        self.capture_result = Some(CaptureResult::Stopped);
    }

    fn ready(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _frame: &CaptureFrame,
        _meta: Frame,
    ) {
        self.capture_result = Some(CaptureResult::Ready);
    }

    fn failed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _frame: &CaptureFrame,
        reason: WEnum<FailureReason>,
    ) {
        let msg = match reason {
            WEnum::Value(FailureReason::BufferConstraints) => "buffer constraints mismatch",
            WEnum::Value(FailureReason::Stopped) => "session stopped",
            _ => "unknown failure",
        };
        self.capture_result = Some(CaptureResult::Failed(msg.to_string()));
    }
}

// ── Delegate macros ───────────────────────────────────────────────────────────

smithay_client_toolkit::delegate_output!(AppData);
smithay_client_toolkit::delegate_registry!(AppData);
smithay_client_toolkit::delegate_seat!(AppData);
smithay_client_toolkit::delegate_shm!(AppData);

cosmic_client_toolkit::delegate_workspace!(AppData);
cosmic_client_toolkit::delegate_toplevel_info!(AppData);
cosmic_client_toolkit::delegate_toplevel_manager!(AppData);
cosmic_client_toolkit::delegate_screencopy!(AppData);

// ── AppData query helpers ─────────────────────────────────────────────────────
//
// These methods are used by the MCP server and CLI to read the compositor
// state as serialisable JSON-ready types.

impl AppData {
    /// All workspaces across all outputs, sorted by group then coordinates.
    pub fn workspaces(&self) -> Vec<crate::workspaces::WorkspaceJson> {
        let mut ws: Vec<crate::workspaces::WorkspaceJson> = self
            .workspace_state
            .workspaces()
            .map(|w| crate::workspaces::workspace_to_json(w, self))
            .collect();
        ws.sort_by(|a, b| {
            a.group_id
                .cmp(&b.group_id)
                .then(a.coordinates.cmp(&b.coordinates))
        });
        ws
    }

    /// All open windows (toplevels).
    pub fn toplevels(&self) -> Vec<crate::toplevels::ToplevelJson> {
        self.toplevel_info_state
            .toplevels()
            .map(|t| crate::toplevels::toplevel_to_json(t, self))
            .collect()
    }

    /// All connected outputs (monitors).
    pub fn outputs(&self) -> Vec<crate::output::OutputJson> {
        self.output_state
            .outputs()
            .filter_map(|o| self.output_state.info(&o))
            .map(|info| crate::output::output_info_to_json(&info))
            .collect()
    }

    /// The full output → workspace → window tree.
    pub fn tree(&self) -> Vec<crate::output::TreeOutput> {
        crate::output::build_tree(self)
    }
}
