use crate::{
    Error, OutputFormat,
    monitor::MonitorEvent,
    state::{AppData, MonitorConfig},
};
use cosmic_client_toolkit::{
    screencopy::ScreencopyState, toplevel_management::ToplevelManagerState,
    workspace::WorkspaceState,
};
use smithay_client_toolkit::{
    output::OutputState, registry::RegistryState, seat::SeatState, shm::Shm,
};
use wayland_client::{Connection, EventQueue, globals::registry_queue_init};

fn make_app_data(
    globals: &wayland_client::globals::GlobalList,
    qh: &wayland_client::QueueHandle<AppData>,
) -> Result<AppData, Error> {
    let registry_state = RegistryState::new(globals);
    let output_state = OutputState::new(globals, qh);
    let seat_state = SeatState::new(globals, qh);
    let shm_state =
        Shm::bind(globals, qh).map_err(|e| Error::ProtocolNotAvailable(format!("wl_shm: {e}")))?;
    let workspace_state = WorkspaceState::new(&registry_state, qh);
    let screencopy_state = ScreencopyState::new(globals, qh);

    let toplevel_info_state =
        cosmic_client_toolkit::toplevel_info::ToplevelInfoState::try_new(&registry_state, qh)
            .ok_or_else(|| {
                Error::ProtocolNotAvailable(
                    "ext-foreign-toplevel-list-v1 not available — is cosmic-comp running?"
                        .to_string(),
                )
            })?;

    let toplevel_manager_state = ToplevelManagerState::try_new(&registry_state, qh);

    Ok(AppData::new(
        registry_state,
        output_state,
        seat_state,
        shm_state,
        workspace_state,
        toplevel_info_state,
        toplevel_manager_state,
        screencopy_state,
    ))
}

/// Connect to the Wayland compositor, pump the event loop until the initial
/// workspace and toplevel snapshots have been received, and return both the
/// populated AppData and the EventQueue (so callers can flush after mutations).
pub fn collect_state() -> Result<(AppData, EventQueue<AppData>), Error> {
    let conn = Connection::connect_to_env().map_err(|e| Error::Connect(e.to_string()))?;
    let (globals, mut event_queue) =
        registry_queue_init(&conn).map_err(|e| Error::Connect(e.to_string()))?;
    let qh = event_queue.handle();

    let mut app_data = make_app_data(&globals, &qh)?;

    let max_rounds = 100;
    for _ in 0..max_rounds {
        event_queue
            .blocking_dispatch(&mut app_data)
            .map_err(|e| Error::Other(e.to_string()))?;
        if app_data.workspaces_done && app_data.toplevels_done {
            break;
        }
    }

    Ok((app_data, event_queue))
}

/// Subscribe to compositor events and print them forever (until Ctrl-C).
pub fn run_monitor(format: OutputFormat) -> Result<(), Error> {
    let conn = Connection::connect_to_env().map_err(|e| Error::Connect(e.to_string()))?;
    let (globals, mut event_queue) =
        registry_queue_init(&conn).map_err(|e| Error::Connect(e.to_string()))?;
    let qh = event_queue.handle();

    let mut app_data = make_app_data(&globals, &qh)?;

    let max_rounds = 100;
    for _ in 0..max_rounds {
        event_queue
            .blocking_dispatch(&mut app_data)
            .map_err(|e| Error::Other(e.to_string()))?;
        if app_data.workspaces_done && app_data.toplevels_done {
            break;
        }
    }

    let mut monitor = MonitorConfig::new(format);
    for ws in app_data.workspace_state.workspaces() {
        monitor.known_workspaces.insert(ws.handle.clone());
        monitor
            .workspace_snapshots
            .insert(ws.handle.clone(), (ws.name.clone(), ws.id.clone()));
    }
    app_data.monitor = Some(monitor);

    loop {
        event_queue
            .blocking_dispatch(&mut app_data)
            .map_err(|e| Error::Other(e.to_string()))?;
    }
}

/// Subscribe to compositor events, calling `handler` for each one.
/// Blocks until `handler` returns `Err`, or a Wayland connection error occurs.
#[allow(dead_code)] // used via lib crate's `cosmicmsg::subscribe`
pub fn run_monitor_with_handler(
    handler: impl FnMut(MonitorEvent) -> Result<(), Error> + 'static,
) -> Result<(), Error> {
    let conn = Connection::connect_to_env().map_err(|e| Error::Connect(e.to_string()))?;
    let (globals, mut event_queue) =
        registry_queue_init(&conn).map_err(|e| Error::Connect(e.to_string()))?;
    let qh = event_queue.handle();

    let mut app_data = make_app_data(&globals, &qh)?;

    // Phase 1: silent baseline sync.
    let max_rounds = 100;
    for _ in 0..max_rounds {
        event_queue
            .blocking_dispatch(&mut app_data)
            .map_err(|e| Error::Other(e.to_string()))?;
        if app_data.workspaces_done && app_data.toplevels_done {
            break;
        }
    }

    app_data.event_handler = Some(Box::new(handler));

    // Phase 2: live event loop.
    loop {
        event_queue
            .blocking_dispatch(&mut app_data)
            .map_err(|e| Error::Other(e.to_string()))?;
        if let Some(err) = app_data.handler_error.take() {
            return Err(err);
        }
    }
}
