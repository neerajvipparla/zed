# Collapsible Sidebar (Left/Right Docks) — Design

Date: 2026-07-07
Status: Approved design, ready for implementation planning

## Goal

Left/Right dock panels (Project Panel, Git Panel, Agent Panel, Outline Panel,
etc.) currently open at full width by default and permanently consume screen
space, similar to a pinned tool window. This wastes horizontal space for users
who only need to glance at the panel occasionally.

Change the default behavior to match JetBrains-style "auto-hide" tool
windows: panels collapse to icon-only by default, reveal on hover as a
floating overlay, and can be explicitly pinned open (reverting to today's
push-content docked behavior) via a pin control. The pinned/unpinned choice
persists across app restarts.

**Bottom dock (Terminal/Tasks) is explicitly out of scope** — it keeps its
current always-docked behavior. Auto-hiding a running terminal by default is
undesirable; users actively watch its output.

## Decisions (locked)

- **Default state: collapsed (icon-only)** for all panels in the Left and
  Right docks. This applies uniformly — no per-panel opt-out setting in v1.
- **Reveal interaction: hover-to-peek.** Hovering the panel's icon button (in
  the status bar — see "Current state" below) shows the panel as a floating
  overlay anchored to the dock's screen edge. Moving the mouse off both the
  icon and the overlay (after a short grace delay) hides it again.
- **Pin interaction: click a pin control to keep it open.** A pin/thumbtack
  `IconButton` in the overlay's header toggles the dock into today's normal
  docked mode (fixed/flexible width, pushes editor content, resizable via the
  existing drag handle). Clicking the pin again un-pins and returns to
  hover-only collapsed mode.
- **Persistence: global, not per-workspace.** Pin state survives app restart
  and applies across all workspaces/projects — it is a UI preference, not
  project state.
- **Scope: Left and Right docks only.** Bottom dock is unaffected.
- **No new vertical activity bar.** Zed does not have a VS Code-style
  vertical icon rail; panel toggle icons live in the bottom status bar via
  `PanelButtons` (see below). The hover trigger reuses these existing icons
  rather than introducing new UI chrome.

## Current state (codebase facts)

- `crates/workspace/src/dock.rs`:
  - `pub struct Dock { position, panel_entries, workspace, is_open,
    active_panel_index, focus_handle, focus_follows_mouse, serialized_dock,
    zoom_layer_open, modal_layer, _subscriptions }` (`dock.rs:269`).
  - `is_open: bool` tracks whether *some* panel is toggled active in this
    dock. There is currently no separate "pinned vs. collapsed" concept —
    once open, a panel always consumes layout width.
  - `PanelSizeState { size: Option<Pixels>, flex: Option<f32> }` — per-panel
    stored width, persisted via `db::kvp::KeyValueStore` under
    `PANEL_SIZE_STATE_KEY = "dock_panel_size"`, scoped by
    `{workspace_id}:{panel_key}` (`dock.rs:1078-1088`). This is the existing
    pattern for workspace-scoped persistence — our new pin state needs a
    *global* (non-workspace-scoped) variant of this.
  - `impl Render for Dock` (`dock.rs:1091`) renders the dock's single visible
    panel filling the container, plus a resize handle. It has no knowledge of
    a "collapsed" state today.
  - `struct PanelButtons { dock: Entity<Dock>, ... }` (`dock.rs:1200`) renders
    one icon button per panel in the dock (`impl Render for PanelButtons`,
    `dock.rs:1211`). Clicking toggles the panel's `toggle_action`.
- `crates/workspace/src/workspace.rs`:
  - `left_dock`, `bottom_dock`, `right_dock: Entity<Dock>` fields
    (`workspace.rs:1366-1368`).
  - `left_dock_buttons`, `bottom_dock_buttons`, `right_dock_buttons` are
    `PanelButtons` entities added to the **status bar** — `add_left_item` /
    `add_right_item` (`workspace.rs:1741-1743`). This confirms panel icons
    are bottom-status-bar buttons, not a left-edge vertical rail.
  - `fn render_dock(&self, position, dock, window, cx) -> Option<Div>`
    (`workspace.rs:7978`) is where dock width is actually applied to the
    layout: it reads `stored_panel_size_state`, falls back to
    `panel.default_size(...)`, and sets `container.w(size)` (horizontal docks)
    or `container.h(size)` (bottom dock). This is the function that must be
    changed to skip reserving width when a dock is unpinned.
  - `render_dock` already has an early return for zoomed panels
    (`self.zoomed_position == Some(position)`) — the unpinned/collapsed case
    is architecturally similar (dock present in the tree, but not
    contributing to flex layout the normal way).
- `crates/workspace/src/notifications.rs`: `AutoHideState` /
  `AutoHideFade` (`notifications.rs:487-620`) is an existing hover/fade-timer
  pattern (used for toast auto-dismiss) that the peek-overlay's
  show/hide-with-grace-delay logic should follow for consistency, though it
  will need adaptation (hover-to-show vs. this existing hover-to-delay-hide).
- `crates/project_panel/src/project_panel.rs` /
  `project_panel_settings.rs`: reference `impl Panel` — `default_size` reads
  `ProjectPanelSettings::get_global(cx).default_width`. Same pattern is used
  by other panels (Git Panel, Agent Panel, Outline Panel). No changes needed
  to individual panel implementations — this feature is implemented entirely
  at the `Dock`/`workspace.rs` layer.

## Target architecture

### 1. New `pinned` state on `Dock`

Add `pinned: bool` to `Dock` (default `false`). This is orthogonal to
`is_open`: `is_open` + `active_panel_index` say *which* panel is active (or
none); `pinned` says *how* the active panel renders — collapsed/overlay vs.
normal docked.

Only Left and Right docks use this field meaningfully. Bottom dock keeps
`pinned` hardcoded `true` (or the field is simply not read for
`DockPosition::Bottom` in the layout/hover logic) so its behavior is
unchanged.

### 2. Persistence

New global KVP scope, e.g. `"dock_pinned"`, with keys `"left"` / `"right"`
(bottom is excluded). Unlike `PANEL_SIZE_STATE_KEY`, this is **not** scoped by
`workspace_id` — it's a single global preference, consistent with the
"persist across sessions" requirement (not per-workspace).

Read on `Dock::new` (or lazily on first render) to initialize `pinned`.
Written whenever the pin toggle fires.

### 3. Layout change in `render_dock` (`workspace.rs:7978`)

When `position` is `Left`/`Right`, dock has a visible panel, and
`!dock.pinned`: skip the `container.w(size)` / flex-grow sizing entirely —
the container contributes no width to the editor's flex layout (same
"present in tree but not laid out normally" pattern already used for the
zoomed-panel case, needed so the panel's `FocusHandle` stays mounted).

When `dock.pinned` is true: unchanged — today's existing sizing logic runs
exactly as now.

### 4. Floating overlay (hover-to-peek)

New overlay rendered via the existing `deferred()` anchored-popover
primitives (same family used by `right_click_menu`/`PopoverMenu` elsewhere in
this codebase). Anchored to the dock's screen edge (left edge for Left dock,
right edge for Right dock), sized to the panel's normal
`default_size`/stored width, full height, with a border/shadow to visually
separate it from the editor content it overlaps.

Trigger: hovering the panel's icon in `PanelButtons` (status bar) sets a
"peeking" ephemeral (non-persisted) state on the `Dock` and activates that
panel (`active_panel_index`) if not already active. Moving the mouse off
*both* the icon and the overlay hides it after a short grace delay — reuse
the timer/fade approach from `AutoHideState` (`notifications.rs`) adapted for
this show/hide direction.

### 5. Pin control

Small pin/thumbtack `IconButton` rendered as **dock-level chrome** — i.e. by
`Dock::render` itself (`dock.rs:1091`), the same way the existing resize
handle is added on top of the panel's own content, rather than by each
individual panel. This means no changes are needed to `GitPanel`,
`ProjectPanel`, etc. — the control works uniformly for every panel because it
lives in the `Dock` wrapper, not inside `entry.panel.to_any()`.

Positioned in the top corner of the container in both states (overlay when
unpinned, normal docked panel when pinned), so it's always in the same visual
spot regardless of pin state.

Clicking it flips `dock.pinned`, persists the new value, and triggers
`cx.notify()` so `render_dock` re-renders using the new layout path
immediately (pinned → docked width reserved & overlay dismissed; unpinned →
width freed & overlay shown while still hovered, or fully hidden if the
mouse has left).

## Testing

- `crates/workspace/src/dock.rs` — extend or add unit tests (alongside
  existing dock tests, e.g. around line 1433+ where a test panel type is
  already defined) covering: `pinned` defaults to `false`; toggling
  `pinned` persists and reloads correctly; Bottom dock is unaffected by
  pin/unpin calls.
- Manual verification (per project convention — GPUI rendering isn't easily
  unit-tested end-to-end): confirm hover-to-peek shows/hides correctly for
  Left and Right docks, confirm pin/unpin visually switches between overlay
  and push-content modes, confirm the pinned state survives an app restart,
  confirm Bottom dock (Terminal) is entirely unaffected.

## Out of scope (v1)

- Per-panel pin overrides (e.g. pin Project Panel but not Git Panel) — pin
  state is per-dock, not per-panel, matching the "apply to all panels
  uniformly" requirement.
- Bottom dock collapse/peek behavior.
- Configurable hover delay or overlay width via settings.
