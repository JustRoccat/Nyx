# Nyx

Nyx is a Wayland compositor in Rust. Windows live on a 2D infinite canvas
that you pan and zoom with a camera, instead of sitting in columns.
Rendering is Smithay GLES2. The codebase started from Niri: config language,
backends, window management plumbing and protocol code are inherited from it,
while the layout (canvas + camera + effects) is Nyx specific.

## Infinite canvas

Each window owns an absolute rectangle `(x, y, width, height)` in canvas
units. Each output has a camera: center `(cam_x, cam_y)` plus `zoom`
(`1.0` is 1:1, below is bird's eye, above is zoomed in). Screen position is
derived per frame, windows are never moved by the camera:

```text
screen_x = (win_x - cam_x) * zoom + out_w / 2
screen_y = (win_y - cam_y) * zoom + out_h / 2
screen_w = win_w * zoom
screen_h = win_h * zoom
```

Layer-shell surfaces (bars, launchers, notifications, wallpaper) stay pinned
in screen space and ignore the camera.

Default controls (`resources/default-config.kdl`):

| Bind | Action |
|---|---|
| `Mod+H/J/K/L` | pan the camera |
| `Mod + wheel`, `Mod+Equal`, `Mod+Minus` | zoom around the cursor |
| `Mod+0` | reset zoom to 1.0 |
| `Mod+Tab` | bird's-eye overview, click a window to return |
| `Mod+Home` | back to origin at 1.0 |
| `Mod+Y`, `Mod+Shift+Y` | toggle dynamic tiling, cycle its layout |

Camera behavior is configured in `canvas { ... }`: `min-scale`, `max-scale`,
`zoom-speed`, `overview-scale`, `pan-step`, `default-scale`,
`keyboard-zoom-step`, `zoom-to-center`, `zoom-to-cursor`, `smooth-camera`,
`overview-restore-zoom`, and spring physics (`stiffness`, `damping`, `mass`).
There is also an exact `set-zoom <scale>` action (bindable, IPC callable).

## Eye candy: what actually works

All of this renders through Smithay GLES2 shaders, configured in KDL:

- Rounded corners: SDF clipping with antialiasing (`geometry-corner-radius`
  per window rule, `rounding-power` shaping).
- Borders and focus rings: solid colors or static linear gradients (angle,
  workspace-relative mode, oklab/oklch interpolation).
- Drop shadows: softness, spread, offset, active and inactive colors, plus
  render power and scale multipliers. They follow the corner radius.
- Background blur: dual-Kawase down/up chain (`passes`, `offset`, `noise`,
  `saturation`) behind translucent windows and blur regions. Fully opaque
  content skips the blur pass.
- Per-window opacity: global `active-opacity` / `inactive-opacity` multiplied
  with the window-rule `opacity`. Inactive windows can also be dimmed
  (`dim-inactive`, `dim-strength`); dimming is implemented as an alpha blend,
  not a per-pixel darkening pass.
- Animations: window open, close, movement and resize use configurable easing
  or spring curves, with optional custom shaders. Open/close take a `popin`
  start/end scale, a forced `slide` direction (30% of window size travel) and
  a `fade` toggle. Focus changes blend active/inactive opacity, dim factor
  and shadow color, timed by `fade-switch`, `fade-dim` and `fade-shadow`.
  Camera moves go through the canvas spring.
- Animated gradient borders: `border-angle { loop, angle-speed,
  allow-constant-repaint }` rotates the border gradient at `angle-speed`
  rad/s. Without `allow-constant-repaint` it advances on wall-clock time and
  visibly moves only while frames render for other reasons; with the opt-in
  the compositor repaints at monitor rate while any window is open.
- Workspace switches take `workspace-slidefade` 0..1: 0 is the plain full
  slide, higher values shorten the travel and dip opacity mid-switch
  (1 is an in-place crossfade).
- Background blur grading: `vibrancy` (extra colorfulness), `contrast` and
  `brightness` apply in the background postprocess shader on top of
  `saturation` and `noise`.

## Dynamic tiling

`Mod+Y` turns the visible canvas region into a real tiling layout: `grid`,
`master-stack` or Hyprland-style `dwindle` (`canvas { tiling { ... } }`,
`gap`, `master-ratio`). New windows take the master slot, closing reflows the
rest, `move-window-*` swaps slots. Turning it off flies every window back to
its free-canvas position. While tiling is on, camera navigation (pan, zoom,
overview, resize) is locked, because the layout is pinned to the region.

## Kept from Niri

KDL config with includes and validation, fullscreen and maximized windows,
floating layer, window rules, layer rules, keybindings, workspace overview,
XWayland via satellite, output management and hotplug, screen capture
block-out rules, `nyx msg` IPC (`NYX_SOCKET`, `NIRI_SOCKET` still accepted),
D-Bus, systemd and xdg-desktop-portal screencast features, screenshot UI,
recent-windows switcher. Input goes through libinput (keyboard, mouse,
touchpad, touch). Canvas zoom and pan are bound to keyboard and mouse only;
there are no touch gestures for canvas navigation.

## Requirements

- Rust 1.87 or newer.
- System libraries: cairo, dbus, libGL, libdisplay-info, libinput, seatd
  (libseat), libxkbcommon, libgbm, pango, wayland, pkg-config. Optional:
  pipewire for screencast, systemd for session integration. `flake.nix`
  provides the full dev shell.
- Smithay is pinned to a git revision in `Cargo.toml`. There is no wgpu
  dependency; all rendering is GLES2 plus CPU-side math.
- For the showcase test scripts: `kitty` (or `alacritty`). The test configs
  optionally call the `swww` binary for wallpaper; without it they still run.

## Build and run

```sh
cargo build --release
./target/release/nyx --help
./target/release/nyx validate --config resources/default-config.kdl
```

Inside an existing Wayland session Nyx opens as a nested window, which is the
normal way to hack on it (`cargo run --`). Pass `--session` only when running
as a login session (it imports the environment globally).

Config resolution order: `--config` flag, `NYX_CONFIG` (fallback
`NIRI_CONFIG`), user config (created from the default on first run), then
`/etc/nyx/config.kdl`. Full option reference: `docs/CONFIG.md` and the
commented `resources/default-config.kdl`.

## Tests

```sh
cargo test
./resources/eyecandy-test.sh --validate-only
./resources/camera-zoom-test.sh --validate-only
```

`resources/eyecandy-test.sh` opens nested Nyx and spawns 13 `kitty` windows,
each isolating one effect (rounding, opacity, dim, four blur variants,
shadow, static gradient, rotating borderangle, open/close animation,
movement, camera).
Press `L` inside to close and respawn them all.
`resources/camera-zoom-test.sh` scatters 6 labeled windows in a ring on the
free canvas; `L` cycles exact camera zooms (`1.0`, `0.7`, `0.4`, `0.25`,
`1.5`, `2.5`) through the `set-zoom` action. Both scripts need a Wayland
session for the nested window and clean up after themselves on `Ctrl+C`.
`nyx-visual-tests` is a separate GTK/libadwaita app with hardcoded layout
scenarios for visual inspection (`cargo run -p nyx-visual-tests`).

## Layout of the code

- `src/canvas/`: camera math (`viewport.rs`), springs (`spring.rs`),
  tiling (`tiling.rs`).
- `src/effects/`: pure per-effect math (corners, blur descriptor, borders,
  shadows, dim, opacity, camera helpers). No GPU calls here.
- `src/render_helpers/`: Smithay render elements and GLSL (`shaders/`),
  blur, shadows, borders, offscreen buffers.
- `src/layout/`: canvas space, tiles, floating layer, focus rings, shadows.
- `nyx-config/`: KDL parsing (`canvas.rs`, `appearance.rs`,
  `animations.rs`, `layout.rs`, `window_rule.rs`, `binds.rs`).
- `nyx-ipc/`: IPC types and the `nyx msg` protocol.
- `nyx-visual-tests/`: visual test app (not packaged).

## License

GPL-3.0-or-later, same as the Niri base.
