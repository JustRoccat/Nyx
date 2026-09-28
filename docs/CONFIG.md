# Nyx configuration (KDL)

Nyx keeps Niri's KDL language and parser (`nyx-config`). This guide covers the
new canvas, zoom, tiling and effects sections plus the full default config.

- Default config: [`../resources/default-config.kdl`](../resources/default-config.kdl)
- Validate: `cargo run -q -- validate --config resources/default-config.kdl`
- Path: `$XDG_CONFIG_HOME/nyx/config.kdl` (`NYX_CONFIG`, compat `NIRI_CONFIG`),
  system `/etc/nyx/config.kdl`.

## `canvas { ... }` — infinite 2D canvas & camera

```kdl
canvas {
    min-scale 0.1
    max-scale 3.0
    zoom-speed 0.12
    overview-scale 0.4
    pan-step 200
    default-scale 1.0
    keyboard-zoom-step 1.0
    zoom-to-center true
    zoom-to-cursor true
    smooth-camera true
    overview-restore-zoom true
    spring {
        stiffness 900
        damping 1.0
        mass 1.0
    }
    // tiling {
    //     kind "master-stack"  // "master-stack" | "grid" | "off"
    //     gap 16
    //     master-ratio 0.6
    // }
}
```

- `min-scale` / `max-scale`: zoom limits (`0.01..10`, clamped, `min <= max`).
- `zoom-speed`: exponential factor per wheel notch
  (`new = old * (1 + speed)^notches`).
- `set-zoom <scale>`: jumps to an exact zoom around the screen center
  (clamped to `min/max`), e.g. `set-zoom 0.5` or
  `nyx msg action set-zoom 0.5`. Goes through the camera spring, so the
  transition animates unless `smooth-camera false`. No-op while tiling or
  fullscreen lock the camera.
- `overview-scale`: bird's-eye scale for `toggle-canvas-overview` (clamped).
- `pan-step`: canvas units per `pan-*` keypress.
- `default-scale`: zoom at startup, clamped to `min/max`.
- `keyboard-zoom-step`: notches per `zoom-in`/`zoom-out` keypress
  (`0.1..10`, default `1.0`).
- `zoom-to-center` (default `true`): keyboard zoom anchors to screen center;
  `false` anchors to cursor.
- `zoom-to-cursor` (default `true`, Hyprland-like): wheel zoom keeps the
  canvas point under the cursor stable; `false` zooms to screen center.
- `smooth-camera` (default `true`): pan/zoom glide through the spring;
  `false` snaps instantly (low-motion / deterministic tests).
- `overview-restore-zoom` (default `true`): leaving overview restores the
  pre-overview zoom; `false` always returns to `1.0`.
- `spring`: camera physics; `stiffness` (snappiness), `damping` ratio
  (`1.0` critically damped, `< 1` bouncy), `mass` (heaviness).
- `tiling`: optional auto-placement around the viewport center.
  `off` (default) = free canvas; `master-stack` / `grid` place new windows in
  the visible area. `gap` in canvas units, `master-ratio` `0.2..0.8`.

Math (camera only, windows immutable):

```text
screen_x = (win_x - cam_x) * zoom + out_w / 2
screen_y = (win_y - cam_y) * zoom + out_h / 2
screen_w = win_w * zoom
screen_h = win_h * zoom
```

New windows spawn at the visible viewport center (zoom-aware).
Layer-shell stays in screen space.

## `effects { ... }` — eye candy (Smithay GLES2, no wgpu)

```kdl
effects {
    corner-radius 12
    border-width 2
    shadow-radius 12
    shadow-opacity 0.5
    blur-passes 2
    blur-radius 8
}
```

- `corner-radius`: SDF + smoothstep AA clipping, scaled by zoom at render.
- `border-width`: active/inactive gradient borders (linear/angular animated).
  Detailed colors/gradients stay in `layout { border/focus-ring }`.
- `shadow-radius` / `shadow-opacity`: soft SDF box-blur drop shadows following
  corner radius (screen-space radius = `radius * zoom`).
- `blur-passes` (`1..4`) / `blur-radius`: dual-Kawase down/up chain under
  translucent windows (`render_helpers/blur.rs`, `offscreen.rs`,
  `framebuffer_effect.rs`).

## Hyprland-style eye candy (all driven from KDL, Smithay GLES2 only)

Every effect below is driven from KDL and clamped; hot-path math allocates
nothing (pure `f32`/`f64` helpers in `src/effects/`), blur textures/FBOs are
reused across frames, and shadows/borders skip work when off-screen.

| Hyprland option | Nyx KDL | Notes / perf |
|---|---|---|
| `rounding` + `rounding_power` | `layout { rounding-power 1..4 }`, `window-rule { rounding-power, geometry-corner-radius }` | `2.0` = neutral (radius unchanged); `1.0` shrinks it slightly, `4.0` grows it ~15%. CPU-side radius scaling only, no extra overdraw. |
| `active_opacity` / `inactive_opacity` | `layout { active-opacity, inactive-opacity }`, `window-rule { opacity, inactive-opacity }` | Multiplied with rule opacity; fades to 1.0 in fullscreen. |
| `dim_inactive` + `dim_strength` | `layout { dim-inactive, dim-strength }`, `window-rule { dim-inactive, dim-strength }` | Darkening approximated via alpha blend (zero VRAM); full per-pixel dim is a shader follow-up. Animated via `animations { fade-dim }`. |
| `blur { size, passes, vibrancy, noise, contrast, brightness, saturation, new_optimizations }` | `blur { offset, passes, vibrancy, noise, saturation, contrast, brightness, new-optimizations }`, `window-rule/layer-rule { background-effect { blur, noise, saturation, vibrancy, contrast, brightness } }` | Dual-Kawase down/up chain reuses textures (`Blur::prepare_textures`); `new-optimizations true` bypasses blur for opaque content. High `offset` needs more `passes` (Hyprland guidance). `vibrancy` boosts colorfulness on top of `saturation`, then `contrast` (`(c - 0.5) * k + 0.5`) and `brightness` (`c + k`) apply in the background postprocess shader, for both xray and plain background effects. |
| `shadow { range, render_power, color, offset, scale }` | `layout { shadow { softness, render-power 1..4, scale 0.1..4, offset, color, inactive-color } }`, `window-rule { shadow { ... } }` | `sigma = softness/2`, `render_power` adds ~25%/step softness, `scale` grows the shadow. Inactive falls back to `color * 0.75`. Animated via `fade-shadow`. |
| `col.active/inactive_border` gradient | `layout { border/focus-ring { active/inactive-gradient ... angle relative-to } }` | Linear/angular gradients, `relative-to="workspace-view"` supported. Zoom-aware width. |
| `borderangle loop` | `animations { border-angle { loop, angle-speed, allow-constant-repaint } }` | `loop false` (default) = static angle. `loop true` rotates the gradient at `angle-speed` rad/s; it advances on wall-clock time, so without repaint it moves only while other frames render. `allow-constant-repaint true` forces monitor-Hz redraws while any window is open (CPU/GPU + battery cost, like Hyprland). The `border-angle` animation entry being `off` disables the loop entirely. |
| `windowsIn/Out popin/slide/fade` | `animations { window-open { popin 0..1, slide "left/right/top/bottom", fade }, window-close { ... } }` | Defaults `popin 0.87 + fade`, matching popular Hyprland configs. `popin` is the start (open) or end (close) scale. `slide` shifts the window along its travel by 30% of its size (into place on open, out of place on close). `fade false` keeps full opacity and animates scale only. Applies to the built-in path; custom shaders govern their own look (slide offset still applies). |
| `windowsMove` spring | `animations { window-movement, window-resize }` | Spring-damper drag/resize on canvas, zoom-compensated. |
| `workspaces slidefade` + `fadeSwitch/fadeShadow/fadeDim` | `animations { workspace-switch, workspace-slidefade 0..1, fade-switch, fade-shadow, fade-dim, overview-open-close }` | `workspace-slidefade 0` (default) = full slide, unchanged behavior. Higher values shorten the travel (`1 - fraction`) and dip workspace opacity mid-switch (`fraction * sin`). `fade-switch` times the active/inactive opacity blend on focus change, `fade-dim` the dim blend, `fade-shadow` the shadow color blend. Each honors its own `off` (instant switch). |

Window-rule overrides for per-window eyecandy showcase / testing:

```kdl
window-rule {
    match title="^nyx-fx-.*"
    opacity 1.0
    inactive-opacity 0.85
    dim-inactive true
    dim-strength 0.3
    rounding-power 2.5
}
```

## Keybindings (`binds { ... }`)

Nyx actions (kebab-case in KDL → PascalCase in Rust):

| KDL | Action | Default bind |
|---|---|---|
| `zoom-in` | `ZoomIn` | `Mod+Equal`, `Mod+WheelScrollUp` |
| `zoom-out` | `ZoomOut` | `Mod+Minus`, `Mod+WheelScrollDown` |
| `set-zoom <scale>` | `SetZoom` | - |
| `reset-zoom` | `ResetZoom` | `Mod+0` |
| `toggle-canvas-overview` | `ToggleCanvasOverview` | `Mod+Tab` |
| `pan-left/right/up/down` | `Pan*` | `Mod+H/J/K/L` |
| `home-canvas` | `HomeCanvas` | `Mod+Home` |
| `toggle-dynamic-tiling` | `ToggleDynamicTiling` | `Mod+Y` |
| `cycle-tiling-layout` | `CycleTilingLayout` | `Mod+Shift+Y` |

Notes:

- `toggle-dynamic-tiling` turns the active workspace into a real tiler: the camera
  snaps to 1:1, the visible chunk of canvas becomes the tiling region, and windows
  are arranged master-stack / grid / dwindle inside it. Turning it off restores each
  window's previous canvas position and size.
- While tiling is on, `move-window-*` swaps tiling slots instead of nudging, and
  dragging a window onto another one swaps the two.
- While tiling is on, canvas navigation is locked: `zoom-in`, `zoom-out`,
  `reset-zoom`, `pan-*`, `home-canvas`, `toggle-canvas-overview`, Mod+wheel zoom,
  Mod+drag panning and interactive window resizing all do nothing until you turn
  tiling back off.
- `canvas { tiling { kind "master-stack"|"grid"|"dwindle"; gap 16; master-ratio 0.6 } }`
  configures the layout; `kind` also picks what `toggle-dynamic-tiling` turns on.
- `Mod + wheel` zooms around the cursor pivot (canvas point under cursor stays).
- `Mod + left-drag` moves the focused window anywhere on the canvas (grab
  offset preserved, zoom-compensated, like vxwm's `movemouse`).
- `Mod + right-drag` on a window edge resizes it; `Mod + middle-drag` (or
  right-drag on empty space in the overview) pans the camera.
- `Mod + wheel` is reserved for zoom; workspace switching on the wheel moved to
  `Alt + wheel`.
- Directional focus (`focus-window-left/right/up/down`) picks the nearest
  window center in that direction; `move-window-*` nudges the active tile by
  50 canvas units.
- New windows spawn at the center of the current camera view, focused and
  with the camera centered on them.
- All other binds (fullscreen, floating, window rules, workspaces, monitors,
  `nyx msg` IPC) keep their Niri meaning.

Example:

```kdl
binds {
    Mod+H { pan-left; }
    Mod+J { pan-down; }
    Mod+K { pan-up; }
    Mod+L { pan-right; }
    Mod+Equal { zoom-in; }
    Mod+Minus { zoom-out; }
    Mod+0 { reset-zoom; }
    Mod+Tab { toggle-canvas-overview; }
    Mod+Home { home-canvas; }
    Mod+Y { toggle-dynamic-tiling; }
    Mod+Shift+Y { cycle-tiling-layout; }
    Mod+WheelScrollUp { zoom-in; }
    Mod+WheelScrollDown { zoom-out; }
}
```

## Preserved sections

`input`, `outputs`, `layout` (gaps, focus-ring, border, shadow, struts),
`window-rule`, `layer-rule`, `workspace`, `animations`, `blur`, `overview`,
`binds`, `gestures`, `environment`, `hotkey-overlay`, `screenshot-path`, etc.
keep Niri semantics. The `preset-column-widths` / `default-column-width` /
`preset-window-heights` presets now size individual tiles instead of columns.

## Removed (columns are gone)

The column strip, tabbed display, consume/expel, column focus/move actions,
`center-focused-column`, `always-center-single-column`,
`default-column-display`, `tab-indicator` and `insert-hint` were removed
together with the strip. Configs mentioning them fail to parse with the
offending key named.

## Migration from Niri

1. Copy `resources/default-config.kdl` to `~/.config/nyx/config.kdl`.
2. Drop column binds (`focus-column-*`, `move-column-*`, `consume-*`,
   `expel-*`, `swap-window-*`, `toggle-column-tabbed-display`,
   `center-column`, `expand-column-to-available-width`, …) and use the spatial
   `focus-window-*` / `move-window-*` (nudge) actions instead.
3. Tune `canvas.spring` if camera feels too snappy/heavy.
4. `NYX_SOCKET` replaces `NIRI_SOCKET` (`NIRI_*` still accepted as fallback).
