# cosmicmsg

A CLI tool and Rust library for querying and controlling the [COSMIC desktop](https://github.com/pop-os/cosmic-epoch). Inspired by `swaymsg`, but built specifically for COSMIC.

## Overview

COSMIC exposes compositor functionality as Wayland protocol extensions rather than a Unix socket IPC. `cosmicmsg` speaks those protocols directly to query and control workspaces, windows, and outputs from the terminal — and exposes the same functionality as a library for other Rust programs to consume.

| Protocol / interface | Used for |
| --- | --- |
| `ext-workspace-v1` + `zcosmic-workspace-v2` | Workspace listing and control |
| `ext-foreign-toplevel-list-v1` + `zcosmic-toplevel-info-v1` | Window listing |
| `zcosmic-toplevel-management-v1` | Window actions (activate, close, maximize, …) |
| `ext-image-copy-capture-v1` + `ext-image-capture-source-v1` | Screen/window/workspace capture |
| `zcosmic-workspace-image-capture-source-v1` | Workspace capture source |
| `wl-output` / `xdg-output` | Output (monitor) information |
| XDG Remote Desktop portal v2 + EIS | Remote input injection (mouse, keyboard, touch) |

## Installation

### Using Nix (recommended)

If you are a Nix user, you can run `cosmicmsg` directly from the remote flake like this:

```sh
nix run github:varbhat/cosmicmsg -- -h
```

You can also clone the source code. A `flake.nix` is provided. Enter the dev shell with:

```sh
nix develop
cargo build --release
```

Or run directly from the flake:

```sh
nix run .#cosmicmsg -- get-workspaces
```


Format all source files:

```sh
nix fmt
```

### Manual

Dependencies: `wayland`, `libxkbcommon`, `pkg-config`, and a stable Rust toolchain.

```sh
cargo build --release
# binary at target/release/cosmicmsg
```

## CLI usage

```
cosmicmsg [OPTIONS] <COMMAND>

Options:
  -j, --json     Output compact JSON
  -p, --pretty   Output pretty-printed JSON
  -h, --help
  -V, --version
```

### Query commands

#### `get-workspaces`

List all workspaces across all outputs.

```sh
cosmicmsg get-workspaces
cosmicmsg --pretty get-workspaces
```

Example output (human):
```
1 * [tiling: enabled] on DP-1
2 on DP-1
3 [pinned] on DP-1
```

The `*` marks the active workspace.

#### `get-toplevels`

List all open windows.

```sh
cosmicmsg get-toplevels
cosmicmsg --json get-toplevels | jq '.[].app_id'
```

Example output (human):
```
"kitty" (kitty) [activated] on 1
"Firefox" (firefox) on 2
"zed" (dev.zed.Zed) [maximized] on 2
```

#### `get-outputs`

List all connected monitors.

```sh
cosmicmsg get-outputs
cosmicmsg --pretty get-outputs
```

Example output (human):
```
DP-1 "Dell U2722D" 2560x1440@59.951Hz at (0, 0)
HDMI-A-1 "LG TV" 1920x1080@60.000Hz at (2560, 0)
```

#### `get-tree`

Show a full tree of outputs → workspaces → windows.

```sh
cosmicmsg get-tree
cosmicmsg --pretty get-tree
```

Example output (human):
```
Output: DP-1 "Dell U2722D" 2560x1440 at (0, 0)
  Workspace: 1 * [tiling: enabled]
    - "kitty" (kitty)
    - "nvim" (kitty)
  Workspace: 2
    - "Firefox" (firefox)
    - "zed" (dev.zed.Zed)
  Workspace: 3 [pinned]
```

### Workspace commands

Window and workspace selectors match by name first, then fall back to case-insensitive substring. An error is returned if the selector matches more than one item.

```sh
# Switch to a workspace
cosmicmsg workspace activate 2
cosmicmsg workspace activate work   # substring match

# Rename
cosmicmsg workspace rename 1 dev
cosmicmsg workspace rename dev work

# Query tiling state (omit name for active workspace)
cosmicmsg workspace get-tiling
cosmicmsg workspace get-tiling 2

# Toggle tiling — use "active" to target the current workspace
cosmicmsg workspace set-tiling active enabled
cosmicmsg workspace set-tiling 1 disabled

# Set the tiling default for all new (empty) workspaces
cosmicmsg workspace set-tiling-default enabled

# Pin / unpin (workspace persists across output changes)
cosmicmsg workspace pin 3
cosmicmsg workspace unpin 3

# Reorder workspaces
cosmicmsg workspace move-before 3 1   # move "3" before "1"
cosmicmsg workspace move-after 1 3    # move "1" after "3"
```

### Window commands

Windows are matched by title, app-id, or identifier (exact match preferred, substring fallback).

```sh
# Focus a window
cosmicmsg window activate firefox
cosmicmsg window activate "kitty"

# Close
cosmicmsg window close zed

# Maximize / restore
cosmicmsg window maximize firefox
cosmicmsg window unmaximize firefox

# Minimize / restore
cosmicmsg window minimize firefox
cosmicmsg window unminimize firefox

# Fullscreen / restore
cosmicmsg window fullscreen firefox
cosmicmsg window unfullscreen firefox

# Sticky (show on all workspaces)
cosmicmsg window set-sticky kitty
cosmicmsg window unset-sticky kitty

# Move to a different workspace
cosmicmsg window move-to-workspace zed 2
cosmicmsg window move-to-workspace firefox work
```

### `input` — remote input injection

Injects mouse, keyboard, and touch events into the running desktop session via
the [XDG Remote Desktop portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html)
(version 2) and the [EIS](https://gitlab.freedesktop.org/libinput/libei) input
protocol. Requires `xdg-desktop-portal-cosmic`.

**First use:** a system permission dialog will appear asking you to grant
input access. After you approve, a restore token is saved to
`~/.config/cosmicmsg/rdp_restore_token` and subsequent invocations run
silently. To force a new dialog (e.g. to change granted devices) delete that
file.

#### Pointer

```sh
# Move the pointer by a relative offset (pixels)
cosmicmsg input mouse-move 50 0       # right 50px
cosmicmsg input mouse-move -- -20 -10 # left 20px, up 10px

# Move the pointer to an absolute position
cosmicmsg input mouse-move-abs 960 540
```

#### Buttons

```sh
# Click (press + release)
cosmicmsg input mouse-click                        # left click
cosmicmsg input mouse-click --button right         # right click
cosmicmsg input mouse-click --button middle        # middle click

# Arbitrary button by evdev code
cosmicmsg input mouse-click --code 0x113           # side button (BTN_SIDE)
cosmicmsg input mouse-click --code 0x114           # extra button (BTN_EXTRA)

# Hold / release separately
cosmicmsg input mouse-click --button left --action press
cosmicmsg input mouse-click --button left --action release
```

`--button` and `--code` are mutually exclusive. Common codes:
`0x110`=left, `0x111`=right, `0x112`=middle, `0x113`=side, `0x114`=extra.

#### Scroll

```sh
# Smooth (pixel-precise) scroll — negative dy = up
cosmicmsg input mouse-scroll --dy -3.0
cosmicmsg input mouse-scroll --dx 1.5 --dy -2.0

# Discrete (wheel-click) scroll — each unit = one detent
cosmicmsg input mouse-scroll --dy -3 --discrete    # 3 clicks up
cosmicmsg input mouse-scroll --dx 1 --discrete     # 1 click right
```

#### Keyboard — by evdev keycode

```sh
# Tap (press + release)
cosmicmsg input key 28          # Enter
cosmicmsg input key 1           # Escape
cosmicmsg input key 57          # Space
cosmicmsg input key 14          # Backspace

# Hold / release
cosmicmsg input key 29 --action press    # hold Left Ctrl
cosmicmsg input key 29 --action release  # release Left Ctrl
```

Common keycodes (Linux evdev, from `linux/input-event-codes.h`):

| Key | Code |
| --- | --- |
| Esc | 1 |
| Backspace | 14 |
| Tab | 15 |
| Enter | 28 |
| Space | 57 |
| Left Ctrl | 29 |
| Left Shift | 42 |
| Left Alt | 56 |
| Super | 125 |
| F1–F10 | 59–68 |
| F11, F12 | 87, 88 |
| a–z | 30–38, 44–50, 16–25 |

Use `evtest` or `wev` to find keycodes interactively.

#### Keyboard — by X11 keysym

Useful when you know the keysym but not the keycode, or when keycodes are
layout-dependent.

```sh
cosmicmsg input keysym 0xff0d           # Return
cosmicmsg input keysym 0xff1b           # Escape
cosmicmsg input keysym 0xff08           # BackSpace
cosmicmsg input keysym 0x41            # 'A'
cosmicmsg input keysym 0x61            # 'a'
cosmicmsg input keysym 0xffe3          # Left Ctrl (hold)
cosmicmsg input keysym 0xffe3 --action press
cosmicmsg input keysym 0xffe3 --action release
```

Use `xev` to look up keysyms interactively, or see
`/usr/include/X11/keysymdef.h`.

#### Type text

Injects a UTF-8 string directly via the `ei_text` interface — no keycode or
keyboard layout mapping involved. Full Unicode is supported.

```sh
cosmicmsg input type "hello world"
cosmicmsg input type "こんにちは"
cosmicmsg input type "$(date)"
```

#### Touch

```sh
# Finger down at (400, 300), slot 0
cosmicmsg input touch-down 0 400 300

# Move finger
cosmicmsg input touch-motion 0 410 310

# Lift finger
cosmicmsg input touch-up 0

# Abort a gesture (e.g. interrupted by a system event)
cosmicmsg input touch-cancel 0
```

`slot` is the finger index (0-based); use different slots for multi-touch.

#### Scripting examples

```sh
# Click at a specific position
cosmicmsg input mouse-move-abs 960 540
cosmicmsg input mouse-click

# Ctrl+C
cosmicmsg input key 29 --action press   # hold Ctrl
cosmicmsg input key 46                  # tap C
cosmicmsg input key 29 --action release # release Ctrl

# Select all and copy
cosmicmsg input keysym 0xffe3 --action press   # hold Ctrl
cosmicmsg input keysym 0x61                    # a (select all)
cosmicmsg input keysym 0x63                    # c (copy)
cosmicmsg input keysym 0xffe3 --action release # release Ctrl

# Type a string then press Enter
cosmicmsg input type "git status"
cosmicmsg input key 28
```

### `subscribe` — live event stream

Subscribe to compositor events and print them as they arrive. Runs indefinitely until interrupted with Ctrl-C.

```sh
cosmicmsg subscribe           # human-readable
cosmicmsg --json subscribe    # ndjson (one JSON object per line)
cosmicmsg --pretty subscribe  # pretty-printed JSON per event
```

Example output (human):
```
workspace-updated 1 * [tiling: enabled] on DP-1
window-opened     "Firefox" (firefox)
window-updated    "Firefox — GitHub" (firefox) [activated]
window-closed     "Firefox — GitHub" (firefox)
workspace-added   5 on DP-1
workspace-removed 5
```

Example output (`--json`), one object per line:
```json
{"type":"window-opened","window":{"title":"kitty","app_id":"kitty","identifier":"...","state":[],"outputs":["DP-1"],"workspaces":["1"]}}
{"type":"window-updated","window":{"title":"nvim","app_id":"kitty","identifier":"...","state":["activated"],"outputs":["DP-1"],"workspaces":["1"]}}
{"type":"workspace-updated","workspace":{"name":"1","id":null,"coordinates":[0],"active":true,"urgent":false,"hidden":false,"pinned":false,"tiling":"enabled","output":"DP-1","group_id":"..."}}
{"type":"window-closed","title":"kitty","app_id":"kitty","identifier":"..."}
```

#### Event types

| `type` field | When it fires |
|---|---|
| `workspace-added` | A new workspace appeared |
| `workspace-updated` | A workspace changed (active flag, name, tiling, pinned, …) |
| `workspace-removed` | A workspace was destroyed |
| `window-opened` | A new window was created |
| `window-updated` | A window changed (title, state flags, workspace, …) |
| `window-closed` | A window was destroyed |

#### Scripting with `subscribe`

```sh
# Print a desktop notification whenever a window opens
cosmicmsg --json subscribe | jq -r --unbuffered '
  select(.type == "window-opened") |
  "New window: \(.window.title) (\(.window.app_id))"
' | while read -r msg; do notify-send "cosmicmsg" "$msg"; done

# Watch for workspace switches
cosmicmsg --json subscribe | jq -r --unbuffered '
  select(.type == "workspace-updated" and .workspace.active == true) |
  "Switched to: \(.workspace.name)"
'

# Log all window titles that ever become active
cosmicmsg --json subscribe | jq -r --unbuffered '
  select(.type == "window-updated" and (.window.state[] == "activated")) |
  .window.title
'
```

## JSON output

Every command supports `--json` (compact) and `--pretty` (indented) flags for scripting.

```sh
# Get the name of the active workspace
cosmicmsg --json get-workspaces | jq -r '.[] | select(.active) | .name'

# List all window titles on workspace "2"
cosmicmsg --json get-toplevels | jq -r '.[] | select(.workspaces[] == "2") | .title'

# Check if any window is fullscreen
cosmicmsg --json get-toplevels | jq 'any(.[]; .state[] == "fullscreen")'

# Get the current output resolution
cosmicmsg --json get-outputs | jq '.[] | select(.name == "DP-1") | "\(.width)x\(.height)"'
```

### `capture` — screen/window/workspace capture

Capture any output, window, or workspace as a PNG. Output goes to a file or stdout (for piping into image viewers like kitty or chafa).

```sh
# Capture the first output to stdout and display in kitty
cosmicmsg capture output | kitty +kitten icat

# Capture output DP-1 to a file
cosmicmsg capture output --output DP-1 --file screenshot.png

# Capture a window by title (substring match)
cosmicmsg capture window firefox | kitty +kitten icat

# Downscale to 50% before encoding
cosmicmsg capture output --scale 0.5 | kitty +kitten icat
cosmicmsg capture output --scale 50% --file small.png

# Capture a workspace
cosmicmsg capture workspace 2 | chafa -

# Include the cursor in the capture
cosmicmsg capture output --cursor | kitty +kitten icat
```

Pixel format: PNG RGBA8888. Capture uses shared memory (`wl_shm`) so no GPU is required.

## MCP server

`cosmicmsg serve` starts a [Model Context Protocol](https://modelcontextprotocol.io/)
server (spec **2026-07-28**) that exposes every cosmicmsg capability as MCP tools
and resources.

```sh
cosmicmsg serve
```

For MCP clients that accept a `mcpServers` config block:

```json
{
  "mcpServers": {
    "cosmicmsg": {
      "command": "cosmicmsg",
      "args": ["serve"]
    }
  }
}
```

With Nix or NixOS (no local install needed):

```json
{
  "mcpServers": {
    "cosmicmsg": {
      "command": "nix",
      "args": ["run", "github:varbhat/cosmicmsg", "--", "serve"]
    }
  }
}
```

### Tools

37 tools organized in five groups:

| Group | Tools |
| --- | --- |
| **Query** | `get_workspaces`, `get_toplevels`, `get_outputs`, `get_tree` |
| **Workspace** | `workspace_activate`, `workspace_rename`, `workspace_set_tiling`, `workspace_set_tiling_default`, `workspace_pin`, `workspace_unpin`, `workspace_move_before`, `workspace_move_after` |
| **Window** | `window_activate`, `window_close`, `window_maximize`, `window_unmaximize`, `window_minimize`, `window_unminimize`, `window_fullscreen`, `window_unfullscreen`, `window_set_sticky`, `window_unset_sticky`, `window_move_to_workspace` |
| **Capture** | `capture_output`, `capture_window`, `capture_workspace` — returns `image/png` base64 |
| **Input** | `input_mouse_move`, `input_mouse_move_abs`, `input_mouse_click`, `input_mouse_click_code`, `input_mouse_scroll`, `input_key`, `input_keysym`, `input_type`, `input_touch_down`, `input_touch_motion`, `input_touch_up`, `input_touch_cancel` |

### Resources

Four read-only resources with 2-second cache hints:

| URI | Content |
| --- | --- |
| `cosmic://workspaces` | All workspaces as JSON |
| `cosmic://toplevels` | All open windows as JSON |
| `cosmic://outputs` | All monitors as JSON |
| `cosmic://tree` | Full output → workspace → window tree as JSON |

The `input_*` tools require a one-time XDG Remote Desktop portal permission
dialog. The restore token is cached at `~/.config/cosmicmsg/rdp_restore_token`.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 1 | Failed to connect to Wayland display |
| 2 | Required protocol not advertised by compositor |
| 3 | No workspace or window matched the selector |
| 4 | Selector matched multiple items (ambiguous) |
| 5 | Other error |

## Notes

- **Requires COSMIC desktop** (`cosmic-comp`). The protocols used are COSMIC-specific and won't work on other compositors.
- **Mutations are best-effort.** The compositor is free to ignore requests (e.g. `rename` if the compositor doesn't advertise `rename` capability). No error is returned in that case — this matches how the underlying Wayland protocols are specified.
- **`input` requires `xdg-desktop-portal-cosmic` version 2+.** The remote input feature uses the XDG Remote Desktop portal's `ConnectToEIS` interface, which was introduced in portal version 2. The restore token is stored at `~/.config/cosmicmsg/rdp_restore_token`; delete it to force a new permission dialog.

## License

[MIT](LICENSE)
