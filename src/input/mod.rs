use anyhow::{Context, Result};
use ashpd::desktop::{
    PersistMode, Session,
    remote_desktop::{ConnectToEISOptions, DeviceType, RemoteDesktop, SelectDevicesOptions},
};
use clap::Subcommand;
use enumflags2::BitFlags;
use futures_util::StreamExt;
use reis::{
    ei,
    event::{Device, DeviceCapability, EiEvent},
};
use std::{os::unix::net::UnixStream, path::PathBuf};
use tokio::time::{Duration, sleep};

// ── CLI types ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

impl MouseButton {
    fn code(&self) -> u32 {
        match self {
            MouseButton::Left => 0x110,   // BTN_LEFT
            MouseButton::Right => 0x111,  // BTN_RIGHT
            MouseButton::Middle => 0x112, // BTN_MIDDLE
        }
    }
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum KeyAction {
    /// Press then release
    Tap,
    /// Press only
    Press,
    /// Release only
    Release,
}

#[derive(Subcommand, Debug)]
pub enum InputCommand {
    // ── Pointer ───────────────────────────────────────────────────────────────
    /// Move the pointer by a relative offset
    MouseMove {
        /// Horizontal delta (positive = right)
        dx: f64,
        /// Vertical delta (positive = down)
        dy: f64,
    },

    /// Move the pointer to an absolute position
    MouseMoveAbs {
        /// X coordinate in screen pixels
        x: f64,
        /// Y coordinate in screen pixels
        y: f64,
    },

    // ── Buttons ───────────────────────────────────────────────────────────────
    /// Press, release, or tap a mouse button
    ///
    /// --button and --code are mutually exclusive. Use --code for non-standard
    /// buttons such as side (0x113) or extra (0x114).
    MouseClick {
        /// Named button shortcut (conflicts with --code)
        #[arg(long, short, default_value = "left", conflicts_with = "code")]
        button: MouseButton,

        /// Raw Linux evdev button code
        /// (0x110=left, 0x111=right, 0x112=middle, 0x113=side, 0x114=extra)
        #[arg(long, conflicts_with = "button")]
        code: Option<u32>,

        /// Whether to press, release, or tap (press+release)
        #[arg(long, short = 'a', default_value = "tap")]
        action: KeyAction,
    },

    // ── Scroll ────────────────────────────────────────────────────────────────
    /// Scroll the pointer
    ///
    /// Default: smooth pixel-precise scroll.
    /// With --discrete: wheel-click scroll (each unit = one detent).
    MouseScroll {
        /// Horizontal scroll amount
        #[arg(long, default_value_t = 0.0)]
        dx: f64,

        /// Vertical scroll amount (negative = up / away from user)
        #[arg(long, default_value_t = 0.0)]
        dy: f64,

        /// Discrete (wheel-click) scroll.
        /// dx/dy are detent counts: 1.0 = one wheel click (120 internal units).
        /// Fractions are valid (e.g. 0.5 = half a detent).
        #[arg(long, short = 'd')]
        discrete: bool,
    },

    // ── Keyboard ─────────────────────────────────────────────────────────────
    /// Send a keyboard key by Linux evdev keycode
    ///
    /// Common keycodes: 1=Esc, 14=Backspace, 28=Enter, 57=Space,
    /// 29=Ctrl, 42=Shift, 56=Alt, 125=Super, 59-68=F1-F10, 87=F11, 88=F12.
    /// Full list: /usr/include/linux/input-event-codes.h or `evtest`.
    Key {
        /// Linux evdev keycode
        keycode: i32,
        #[arg(long, short = 'a', default_value = "tap")]
        action: KeyAction,
    },

    /// Send a key by X11 keysym value (via the ei_text interface)
    ///
    /// Common keysyms: 0xff0d=Return, 0xff1b=Escape, 0xff08=BackSpace,
    /// 0x20=space, 0x61-0x7a=a-z, 0x41-0x5a=A-Z, 0x30-0x39=0-9.
    /// Full list: /usr/include/X11/keysymdef.h or xev(1).
    Keysym {
        /// X11 keysym value (decimal or 0x-prefixed hex)
        keysym: u32,
        #[arg(long, short = 'a', default_value = "tap")]
        action: KeyAction,
    },

    /// Type a string of text using direct UTF-8 injection (ei_text interface)
    ///
    /// The compositor receives the string as-is; no keycode/layout mapping
    /// is performed. Supports full Unicode.
    #[command(name = "type")]
    TypeText { text: String },

    // ── Touch ─────────────────────────────────────────────────────────────────
    /// Touch-down event (first contact)
    TouchDown {
        /// Touch slot / finger index (0-based)
        slot: u32,
        /// X coordinate
        x: f64,
        /// Y coordinate
        y: f64,
    },

    /// Touch-motion event (finger moved while in contact)
    TouchMotion {
        /// Touch slot / finger index (0-based)
        slot: u32,
        /// New X coordinate
        x: f64,
        /// New Y coordinate
        y: f64,
    },

    /// Touch-up event (finger lifted)
    TouchUp {
        /// Touch slot / finger index (0-based)
        slot: u32,
    },

    /// Touch-cancel event (gesture aborted; clears the touch point)
    TouchCancel {
        /// Touch slot / finger index (0-based)
        slot: u32,
    },
}

// ── Restore-token persistence ─────────────────────────────────────────────────
//
// Portal shows a permission dialog on first use.  With ExplicitlyRevoked the
// portal returns a restore token; storing it lets subsequent runs skip the
// dialog silently.

fn token_path() -> PathBuf {
    // $XDG_CONFIG_HOME defaults to ~/.config per the XDG base-dir spec.
    // $XDG_RUNTIME_DIR (the previous location) is wiped on logout; a restore
    // token must survive reboots, so it belongs in the config directory.
    // COSMIC apps use ~/.config/cosmic/{app-id}/v1/{key}; as a CLI tool we
    // follow plain XDG convention: ~/.config/cosmicmsg/rdp_restore_token.
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| String::from("/tmp"));
            PathBuf::from(home).join(".config")
        });
    base.join("cosmicmsg").join("rdp_restore_token")
}

fn read_token() -> Option<String> {
    std::fs::read_to_string(token_path())
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

fn save_token(token: &str) {
    let path = token_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, token);
}

fn delete_token() {
    let _ = std::fs::remove_file(token_path());
}

/// `true` when the error is an explicit user cancellation (user clicked
/// "Deny" or closed the portal dialog).  We must NOT retry in that case —
/// the user has seen the dialog and intentionally refused.
fn is_user_cancelled(e: &anyhow::Error) -> bool {
    e.chain().any(|c| {
        let msg = c.to_string().to_lowercase();
        msg.contains("cancelled") || msg.contains("canceled")
    })
}

// ── Portal session ────────────────────────────────────────────────────────────

/// One attempt: create_session → select_devices (with optional restore token)
/// → start.  On success, stores any new token and returns the live session.
async fn attempt_session(
    proxy: &RemoteDesktop,
    restore_token: Option<&str>,
) -> Result<Session<RemoteDesktop>> {
    let session = proxy
        .create_session(Default::default())
        .await
        .context("CreateSession failed")?;

    proxy
        .select_devices(
            &session,
            SelectDevicesOptions::default()
                .set_devices(DeviceType::Keyboard | DeviceType::Pointer | DeviceType::Touchscreen)
                .set_persist_mode(PersistMode::ExplicitlyRevoked)
                .set_restore_token(restore_token),
        )
        .await
        .context("SelectDevices failed")?;

    let response = proxy
        .start(&session, None, Default::default())
        .await
        .context("Start failed")?
        .response()
        .context("portal request was denied or cancelled")?;

    if let Some(token) = response.restore_token() {
        save_token(token);
    }

    Ok(session)
}

async fn open_session() -> Result<(RemoteDesktop, Session<RemoteDesktop>)> {
    let proxy = RemoteDesktop::new()
        .await
        .context("failed to connect to RemoteDesktop portal — is xdg-desktop-portal running?")?;

    let saved_token = read_token();

    match attempt_session(&proxy, saved_token.as_deref()).await {
        Ok(session) => Ok((proxy, session)),

        // The portal rejected or errored on the stored token.  Wipe it so
        // we don't keep re-presenting a bad token, then retry once with a
        // fresh dialog.  Skip the retry if the user explicitly cancelled —
        // that is an intentional denial, not a token problem.
        Err(e) if saved_token.is_some() && !is_user_cancelled(&e) => {
            delete_token();
            let session = attempt_session(&proxy, None)
                .await
                .context("session failed even after clearing stale token")?;
            Ok((proxy, session))
        }

        Err(e) => Err(e),
    }
}

// ── EIS helpers ───────────────────────────────────────────────────────────────

/// Which EIS device capability does this command require?
fn needed_cap(cmd: &InputCommand) -> BitFlags<DeviceCapability> {
    use DeviceCapability::*;
    match cmd {
        InputCommand::MouseMove { .. } => Pointer.into(),
        InputCommand::MouseMoveAbs { .. } => PointerAbsolute.into(),
        InputCommand::MouseClick { .. } => Button.into(),
        InputCommand::MouseScroll { .. } => Scroll.into(),
        InputCommand::Key { .. } => Keyboard.into(),
        InputCommand::Keysym { .. } | InputCommand::TypeText { .. } => Text.into(),
        InputCommand::TouchDown { .. }
        | InputCommand::TouchMotion { .. }
        | InputCommand::TouchUp { .. }
        | InputCommand::TouchCancel { .. } => Touch.into(),
    }
}

/// Wait on the EIS event stream for the first device that has `cap`, then
/// return it along with the last serial from its DeviceResumed event.
async fn wait_for_device(
    events: &mut (impl futures_util::Stream<Item = Result<EiEvent, reis::Error>> + Unpin),
    connection: &reis::event::Connection,
    cap: BitFlags<DeviceCapability>,
) -> Result<(Device, u32)> {
    let mut target: Option<Device> = None;
    let mut last_serial: u32 = 0;

    while let Some(result) = events.next().await {
        let event = result.context("EIS event stream error")?;
        match event {
            EiEvent::SeatAdded(e) => {
                e.seat.bind_capabilities(cap);
                connection
                    .flush()
                    .context("EIS flush after bind_capabilities")?;
            }
            EiEvent::DeviceAdded(e) => {
                if cap.iter().all(|c| e.device.has_capability(c)) {
                    target = Some(e.device);
                }
            }
            EiEvent::DeviceResumed(e) => {
                if target.as_ref().map_or(false, |d| d == &e.device) {
                    last_serial = e.serial;
                    break;
                }
            }
            _ => {}
        }
    }

    let device =
        target.context("portal did not advertise a device with the required capability")?;
    Ok((device, last_serial))
}

// ── EIS input injection ───────────────────────────────────────────────────────

// Sentinel: EIS device did not advertise itself within the timeout window.
// Only used to distinguish retryable-not-ready from a real protocol error.
#[derive(Debug)]
struct EisDeviceTimeout;
impl std::fmt::Display for EisDeviceTimeout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "EIS device not ready within timeout")
    }
}
impl std::error::Error for EisDeviceTimeout {}

async fn inject_via_eis(fd: std::os::fd::OwnedFd, cmd: &InputCommand) -> Result<()> {
    let context = ei::Context::new(UnixStream::from(fd))
        .context("failed to create EI context from portal FD")?;

    let (connection, mut events) = context
        .handshake_tokio("cosmicmsg", ei::handshake::ContextType::Sender)
        .await
        .context("EI handshake failed")?;
    connection.flush().context("EI flush after handshake")?;

    let cap = needed_cap(cmd);
    // 500ms is the deadline for the compositor to advertise a ready device.
    // If it doesn't, we return EisDeviceTimeout so the caller can retry
    // rather than hanging forever.
    let (device, last_serial) = tokio::time::timeout(
        Duration::from_millis(500),
        wait_for_device(&mut events, &connection, cap),
    )
    .await
    .map_err(|_| anyhow::Error::new(EisDeviceTimeout))?
    .context("wait_for_device failed")?;

    let t = now_us();

    device.device().start_emulating(0, last_serial);

    match cmd {
        // ── Pointer ───────────────────────────────────────────────────────────
        InputCommand::MouseMove { dx, dy } => {
            let ptr = device
                .interface::<ei::Pointer>()
                .context("device missing Pointer interface")?;
            ptr.motion_relative(*dx as f32, *dy as f32);
            device.device().frame(last_serial, t);
        }

        InputCommand::MouseMoveAbs { x, y } => {
            let ptr = device
                .interface::<ei::PointerAbsolute>()
                .context("device missing PointerAbsolute interface")?;
            ptr.motion_absolute(*x as f32, *y as f32);
            device.device().frame(last_serial, t);
        }

        // ── Buttons ───────────────────────────────────────────────────────────
        InputCommand::MouseClick {
            button,
            code,
            action,
        } => {
            let btn_code = code.unwrap_or_else(|| button.code());
            let btn = device
                .interface::<ei::Button>()
                .context("device missing Button interface")?;
            match action {
                KeyAction::Press => {
                    btn.button(btn_code, ei::button::ButtonState::Press);
                    device.device().frame(last_serial, t);
                }
                KeyAction::Release => {
                    btn.button(btn_code, ei::button::ButtonState::Released);
                    device.device().frame(last_serial, t);
                }
                KeyAction::Tap => {
                    btn.button(btn_code, ei::button::ButtonState::Press);
                    device.device().frame(last_serial, t);
                    btn.button(btn_code, ei::button::ButtonState::Released);
                    device.device().frame(last_serial, t + 50_000);
                }
            }
        }

        // ── Scroll ────────────────────────────────────────────────────────────
        InputCommand::MouseScroll { dx, dy, discrete } => {
            let scr = device
                .interface::<ei::Scroll>()
                .context("device missing Scroll interface")?;

            if *discrete {
                // scroll_discrete takes values in units of 120 per detent
                let x = (*dx * 120.0) as i32;
                let y = (*dy * 120.0) as i32;
                scr.scroll_discrete(x, y);
                device.device().frame(last_serial, t);
                // No scroll_stop needed for discrete scroll (it's instantaneous)
            } else {
                scr.scroll(*dx as f32, *dy as f32);
                device.device().frame(last_serial, t);
                // scroll_stop must be in a *separate* frame from scroll
                let x_stop = u32::from(*dx != 0.0);
                let y_stop = u32::from(*dy != 0.0);
                scr.scroll_stop(x_stop, y_stop, 0);
                device.device().frame(last_serial, t + 16_000);
            }
        }

        // ── Keyboard ─────────────────────────────────────────────────────────
        InputCommand::Key { keycode, action } => {
            let kb = device
                .interface::<ei::Keyboard>()
                .context("device missing Keyboard interface")?;
            match action {
                KeyAction::Press => {
                    kb.key(*keycode as u32, ei::keyboard::KeyState::Press);
                    device.device().frame(last_serial, t);
                }
                KeyAction::Release => {
                    kb.key(*keycode as u32, ei::keyboard::KeyState::Released);
                    device.device().frame(last_serial, t);
                }
                KeyAction::Tap => {
                    kb.key(*keycode as u32, ei::keyboard::KeyState::Press);
                    device.device().frame(last_serial, t);
                    kb.key(*keycode as u32, ei::keyboard::KeyState::Released);
                    device.device().frame(last_serial, t + 50_000);
                }
            }
        }

        InputCommand::Keysym { keysym, action } => {
            let txt = device
                .interface::<ei::Text>()
                .context("device missing Text interface")?;
            match action {
                KeyAction::Press => {
                    txt.keysym(*keysym, ei::keyboard::KeyState::Press);
                    device.device().frame(last_serial, t);
                }
                KeyAction::Release => {
                    txt.keysym(*keysym, ei::keyboard::KeyState::Released);
                    device.device().frame(last_serial, t);
                }
                KeyAction::Tap => {
                    txt.keysym(*keysym, ei::keyboard::KeyState::Press);
                    device.device().frame(last_serial, t);
                    txt.keysym(*keysym, ei::keyboard::KeyState::Released);
                    device.device().frame(last_serial, t + 50_000);
                }
            }
        }

        InputCommand::TypeText { text } => {
            let txt = device
                .interface::<ei::Text>()
                .context("device missing Text interface")?;
            txt.utf8(text);
            device.device().frame(last_serial, t);
        }

        // ── Touch ─────────────────────────────────────────────────────────────
        InputCommand::TouchDown { slot, x, y } => {
            let ts = device
                .interface::<ei::Touchscreen>()
                .context("device missing Touchscreen interface")?;
            ts.down(*slot, *x as f32, *y as f32);
            device.device().frame(last_serial, t);
        }

        InputCommand::TouchMotion { slot, x, y } => {
            let ts = device
                .interface::<ei::Touchscreen>()
                .context("device missing Touchscreen interface")?;
            ts.motion(*slot, *x as f32, *y as f32);
            device.device().frame(last_serial, t);
        }

        InputCommand::TouchUp { slot } => {
            let ts = device
                .interface::<ei::Touchscreen>()
                .context("device missing Touchscreen interface")?;
            ts.up(*slot);
            device.device().frame(last_serial, t);
        }

        InputCommand::TouchCancel { slot } => {
            let ts = device
                .interface::<ei::Touchscreen>()
                .context("device missing Touchscreen interface")?;
            ts.cancel(*slot);
            device.device().frame(last_serial, t);
        }
    }

    device.device().stop_emulating(last_serial);
    connection.flush().context("EIS flush after input")?;

    // Give the compositor time to process the input frame before we exit.
    sleep(Duration::from_millis(50)).await;

    Ok(())
}

// ── Command dispatch ──────────────────────────────────────────────────────────

pub async fn dispatch(cmd: InputCommand) -> Result<()> {
    let (proxy, session) = open_session().await?;

    // After portal dialog approval the compositor may need time to stand up
    // the EIS server.  We retry connect+handshake+device-wait (up to 4×)
    // instead of sleeping blindly: on normal runs (restore token, no dialog)
    // the device is ready immediately and no retry fires; after a dialog the
    // retry loop absorbs however long the compositor actually needs.
    let mut last_err = anyhow::anyhow!("EIS setup never succeeded");
    for attempt in 0..4u32 {
        if attempt > 0 {
            sleep(Duration::from_millis(150)).await;
        }
        let fd = proxy
            .connect_to_eis(&session, ConnectToEISOptions::default())
            .await
            .context(
                "ConnectToEIS failed — requires portal version ≥ 2; \
                 ensure xdg-desktop-portal-cosmic is up to date",
            )?;
        match inject_via_eis(fd, &cmd).await {
            Ok(()) => return Ok(()),
            Err(e) if e.is::<EisDeviceTimeout>() => last_err = e, // not ready yet; retry
            Err(e) => return Err(e),                              // real error
        }
    }
    Err(last_err.context("EIS device never became ready after 4 attempts"))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn now_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}
