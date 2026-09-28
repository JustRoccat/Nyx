# Nyx

[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](LICENSE)
![Rust](https://img.shields.io/badge/rust-1.87%2B-orange.svg)
![Wayland](https://img.shields.io/badge/wayland-compositor-green.svg)

> Your windows live on one huge 2D canvas. You move a camera to see them. No columns, no fixed grid.

Nyx is a window manager for Linux, written in Rust. On Wayland, this kind of program is called a "compositor". It draws your windows and handles your mouse and keyboard.

Most window managers put windows in rows, columns, or tiles. Nyx works like a map. Each window has a fixed place on the canvas. You pan and zoom the camera to move around. You can zoom out to see all your work at once. You can zoom in on one window.

Nyx also has many visual effects. It has rounded corners, shadows, blur, and animated borders. The GPU draws all of them.

Nyx starts from [Niri](https://github.com/YaLTeR/niri), another Wayland compositor. Nyx keeps the config format, the backends, and the protocol code from Niri. The canvas, the camera, and the effects are new.

> [!NOTE]
> I am sorry for using this name. I did not know about the first one. Please also look at [nyxwm](https://github.com/nyangkosense/nyxwm).

> [!WARNING]
> Nyx is version 0.1.0. Expect rough edges.

## Features

- Infinite canvas with one camera per screen
- Pan, zoom, and a bird's-eye overview of all windows
- Smooth camera movement with spring physics
- Optional tiling mode: `grid`, `master-stack`, or `dwindle`
- Rounded corners, shadows, and background blur
- Solid or gradient borders, with an optional spinning gradient
- Animations for open, close, move, and resize
- Control a running Nyx with the `nyx msg` command
- Features from Niri: floating windows, fullscreen, window rules, named workspaces, X11 apps, screenshots, and screencast

## Installation

You need Rust 1.87 or newer. You also need these system libraries:

- cairo, dbus, libGL, libdisplay-info, libinput
- seatd (libseat), libxkbcommon, libgbm, pango, wayland
- pkg-config

Two libraries are optional. Use pipewire for screencast. Use systemd for session support.

```bash
git clone <repo-url>
cd nyx
cargo build --release
./target/release/nyx --help
```

> [!TIP]
> If you use Nix, `flake.nix` gives you a full dev shell. The community maintains this file.

> [!NOTE]
> There are no prebuilt packages yet.

## Usage

### Try it first

Run Nyx inside your current Wayland session. It opens as a normal window. This is the safest way to test it.

```bash
cargo run --
```

### Use it as your session

Pass `--session` only when Nyx is your login session. This flag copies the environment to the whole system.

### Check a config file

```bash
./target/release/nyx validate --config resources/default-config.kdl
```

### Default keys

| Key | Action |
|---|---|
| `Mod+H/J/K/L` | Pan the camera |
| `Mod + mouse wheel` | Zoom at the cursor |
| `Mod+Equal`, `Mod+Minus` | Zoom in, zoom out |
| `Mod+0` | Reset zoom to 1.0 |
| `Mod+Tab` | Overview of all windows. Click a window to go back. |
| `Mod+Home` | Go back to the origin at zoom 1.0 |
| `Mod+Y` | Turn tiling on or off |
| `Mod+Shift+Y` | Change the tiling layout |

> [!IMPORTANT]
> Tiling mode locks the camera. You cannot pan, zoom, or resize until you turn tiling off. Canvas navigation uses the keyboard and mouse only. There are no touch gestures.

## Configuration

Nyx reads its config file in KDL format. KDL is a simple text format for settings. Nyx looks for a file in this order:

1. The `--config` flag
2. The `NYX_CONFIG` variable (`NIRI_CONFIG` also works)
3. Your user config. Nyx creates it from the default on first run.
4. `/etc/nyx/config.kdl`

The full option list is in [`docs/CONFIG.md`](./docs/CONFIG.md). The file [`resources/default-config.kdl`](./resources/default-config.kdl) has comments for each option.

### Camera options

Put these in the `canvas { ... }` block:

- `min-scale`, `max-scale`: the zoom limits
- `default-scale`: the zoom at startup
- `zoom-speed`, `keyboard-zoom-step`: how fast zoom changes
- `pan-step`: how far one key press moves the camera
- `overview-scale`: the zoom level of the overview
- `smooth-camera`: turn spring movement on or off
- `stiffness`, `damping`, `mass`: settings for the spring

### Tiling options

Put these in `canvas { tiling { ... } }`:

- `gap`: space between windows
- `master-ratio`: size of the main window

### Effects

- `geometry-corner-radius`: corner size, set in a window rule
- `active-opacity`, `inactive-opacity`: see-through level of windows
- `dim-inactive`, `dim-strength`: darken windows that have no focus
- `border-angle`: spin the border gradient
- `workspace-slidefade`: from 0 (full slide) to 1 (fade only)

> [!TIP]
> The `border-angle` spin only moves when Nyx draws frames for other reasons. To force constant drawing, set `allow-constant-repaint`. This uses more power.

### Control a running Nyx

Use the `nyx msg` command. The socket path is in `NYX_SOCKET` (`NIRI_SOCKET` also works). You can call actions such as `set-zoom <scale>` this way.

## Contributing

Bug reports and pull requests are welcome. Read [`CONTRIBUTING.md`](./CONTRIBUTING.md) first. That file comes from Niri, so treat it as a general guide.

### Build environment

- Nyx pins Smithay to a fixed git revision in `Cargo.toml`.
- Nyx does not use wgpu. All drawing uses GLES2 and math on the CPU.
- Linux only. You need a GPU with GLES2 support.

### Tests

```bash
cargo test
./resources/eyecandy-test.sh --validate-only
./resources/camera-zoom-test.sh --validate-only
```

The two scripts open a nested Nyx window, so they need a Wayland session. They also need `kitty` or `alacritty`. The scripts use `swww` for the wallpaper. They still run without it.

- `eyecandy-test.sh` opens 13 windows. Each window shows one effect. Press `L` to close and reopen them.
- `camera-zoom-test.sh` places 6 labeled windows in a ring. Press `L` to cycle through fixed zoom levels.

Press `Ctrl+C` to stop a script. It cleans up after itself.

## Project layout

| Path | What it holds |
|---|---|
| `src/canvas/` | Camera math, springs, and tiling |
| `src/effects/` | Math for each effect. No GPU calls. |
| `src/render_helpers/` | Smithay render code and GLSL shaders |
| `src/layout/` | Canvas space, tiles, floating layer, focus rings, shadows |
| `nyx-config/` | KDL config parser |
| `nyx-ipc/` | Types and protocol for `nyx msg` |
| `nyx-visual-tests/` | A GTK app to check layouts by eye. It is not packaged. |
| `resources/` | Default config, test scripts, and session files |
| `docs/` | Config reference and wiki pages from Niri |

## License

GPL-3.0-or-later, the same as Niri.
