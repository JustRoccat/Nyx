# Nyx configuration (KDL)

Nyx keeps Niri's KDL language and parser (`nyx-config`). This guide covers the
new canvas, zoom, tiling and effects sections plus the full default config.

- Default config: [`../resources/default-config.kdl`](../resources/default-config.kdl)
- Validate: `cargo run -q -- validate --config resources/default-config.kdl`
- Path: `$XDG_CONFIG_HOME/nyx/config.kdl` (`NYX_CONFIG`, compat `NIRI_CONFIG`),
  system `/etc/nyx/config.kdl`.

## `canvas { ... }`: infinite 2D canvas & camera

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

## `effects { ... }`: eye candy (Smithay GLES2, no wgpu)

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
| `borderangle loop` | `animations { border-angle { loop, angle-speed, allow-constant-repaint } }` | `loop false` (default) = static
