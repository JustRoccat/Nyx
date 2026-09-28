### Overview

> [!NOTE]
> Tabbed columns were a Niri feature and do not exist in Nyx.
> Windows live as independent nodes on a free 2D canvas instead; there are no
> columns to group into tabs, so `toggle-column-tabbed-display`,
> `set-column-display`, the `tab-indicator` layout section and window rule, and
> the `default-column-display` option were all removed.
> Configs containing them will fail to parse.

If you used tabs to stack several same-size windows in one place, put the
windows on top of each other on the canvas instead: open them (they all spawn
at the view center) and drag all but one elsewhere with Mod + left-drag, or
leave them stacked and cycle focus with the spatial `focus-window-*` binds.
