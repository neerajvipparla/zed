# Adjustable Git Graph — Design

Date: 2026-07-07
Crate: `git_ui` (with additions to `settings_content` / `settings`)

## Problem

The Git Graph view (`crates/git_ui/src/git_graph.rs`) renders the lane area with a
fixed, auto-computed width. `graph_canvas_content_width()` returns
`LANE_WIDTH(16px) × max(lanes, 6) + LEFT_PADDING × 2`, and the displayed width is then
hard-clamped between 28px and 140px (`git_graph.rs:2196`). Lane spacing (`LANE_WIDTH`,
16px) and row height (font line height + `ROW_VERTICAL_PADDING`) are compile-time
constants. The result is a rigid layout: the graph is crammed into a narrow left strip,
commit messages truncate, and the user has no control over the graph's horizontal space
or density.

## Goal

Make the graph adjustable along four dimensions the user can control:

1. A draggable divider between the graph lane area and the commits table (horizontal
   space).
2. No hard width cap — the graph may grow to fit all lanes, with horizontal scroll when
   it exceeds the available/chosen width.
3. Adjustable lane spacing (horizontal density).
4. Adjustable row height (vertical density).

Lane spacing and row height are driven by `settings.json` defaults plus a keyboard zoom
(`cmd-=` / `cmd--`) that scales both together. Runtime adjustments (dragged divider
position and zoom level) are **written back to `settings.json`** so they persist as the
new defaults.

## Non-goals

- Independent horizontal vs. vertical zoom axes (zoom scales both together).
- A toolbar/mouse zoom UI (keyboard + settings only).
- Persisting state in the `GitGraphsDb` SQLite store (settings.json is the chosen home).

## Design

### 1. New `git_graph` settings section

Add `GitGraphSettingsContent` to `crates/settings_content/src/settings_content.rs`,
alongside `GitPanelSettingsContent`, and a new `GitGraphSettings` in
`crates/git_ui/src/git_graph_settings.rs` registered with `RegisterSetting` /
`impl Settings`, mirroring `git_panel_settings.rs`.

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `git_graph.lane_width` | `f32` | `16.0` | Base horizontal spacing between lanes |
| `git_graph.row_height` | `f32` | `0.0` | Base extra row height; `0` preserves today's font-derived sizing |
| `git_graph.zoom` | `f32` | `1.0` | Combined multiplier for lane width and row height |
| `git_graph.graph_width` | `Option<f32>` | `None` | Divider position in px; `None` = auto content width |

Effective values used throughout rendering:

- `effective_lane_width = lane_width × zoom`
- `effective_row_height = base_row_height × zoom` (where `base_row_height` is today's
  font-derived value plus the `row_height` setting)

`zoom` is clamped to `[0.5, 3.0]`. `lane_width` is clamped to a sane floor (e.g. `>= 4.0`)
when read.

Register defaults in the default settings JSON (`assets/settings/default.json`) matching
the table above, so `from_settings` `.unwrap()` calls are safe.

### 2. Draggable graph/text divider and removed width cap

- Replace the clamp at `git_graph.rs:2196`
  (`self.graph_canvas_content_width().max(px(28.)).min(px(140.))`) with:
  - if `git_graph.graph_width` is `Some(w)`, use `w`;
  - else use `graph_canvas_content_width()` with no upper cap (keep a small minimum).
- Insert a resize handle between the graph area (`git_graph.rs:2206-2213`) and the commits
  table, reusing the existing drag pattern: `DraggedSplitHandle`,
  `render_commit_view_resize_handle` (`git_graph.rs:3993`), and a `SplitState`-style
  ratio/width holder. The handle sets an in-memory `graph_width` during drag for
  responsiveness.
- Double-clicking the handle resets to auto (clears `graph_width` back to `None`).
- The graph canvas container gets horizontal overflow scroll so lanes wider than the
  chosen area scroll rather than clip. (`graph_canvas_content_width()` continues to report
  true content width to drive the scroll region.)
- **On drag end** (mouse up / drag commit), call `settings::update_settings_file(fs, cx, …)`
  to persist the new `graph_width` into `settings.json`. Mid-drag updates stay in memory;
  only the final value is written.

### 3. Keyboard zoom, written back to settings

- Add `ZoomIn`, `ZoomOut`, `ResetZoom` to the `git_graph` `actions!` macro
  (`git_graph.rs:~588`), with doc comments.
- Bind `cmd-=` → `ZoomIn`, `cmd--` → `ZoomOut`, `cmd-0` → `ResetZoom` in the git graph
  keymap context (macOS; corresponding `ctrl` bindings on other platforms per existing
  keymap conventions).
- Handlers read the current `git_graph.zoom`, multiply/divide by a step (`1.1`), clamp to
  `[0.5, 3.0]` (`ResetZoom` → `1.0`), and write it back with `update_settings_file`.
- Because `zoom` is a registered setting, the existing settings-observer machinery
  triggers a re-render; there is no separate session zoom state to keep in sync.

### 4. Threading effective values through rendering

- `Self::row_height(window, cx)` (`git_graph.rs:1408`) already centralizes row height.
  Multiply its result by `zoom` and add the `row_height` setting there — single change
  point; all existing callers (`1418`, `1608`, `1867`, `2092`, `3541`, curve math) inherit it.
- `LANE_WIDTH` is a const referenced in a few spots (`lane_center_x` `1260`,
  `graph_canvas_content_width` `1432`, curve width `3668`). Introduce a helper
  `effective_lane_width(cx) -> Pixels` reading `lane_width × zoom` from settings, and
  thread it through those call sites (passing it as a parameter to the free functions that
  currently use the const, e.g. `lane_center_x`).

## Data flow

```
settings.json (git_graph.*)
    → GitGraphSettings::get_global(cx)
        → effective_lane_width(cx), row_height(window, cx), graph_width
            → render_graph_canvas / commits table geometry

user drag-end / cmd-+/-  → update_settings_file(fs, cx, …)  → settings.json → observer → re-render
```

## Error handling

- Settings reads use registered defaults, so no fallible unwraps at the call sites beyond
  the established `from_settings` pattern.
- `update_settings_file` is fire-and-forget by design (returns a task); follow the existing
  usage in `editable_setting_control.rs`. Do not silently discard a `Result` that should be
  handled — match the surrounding pattern for that API.
- Clamp all numeric settings on read so malformed user values can't produce degenerate
  layout (zero/negative widths).

## Testing

- Unit test: `effective_lane_width` and `row_height` scale correctly with `zoom` and base
  settings, and clamp at the bounds.
- Unit test: `graph_canvas_content_width` grows monotonically with `max_lanes` and is no
  longer capped at 140px.
- Unit test: zoom action handlers clamp to `[0.5, 3.0]` and `ResetZoom` returns `1.0`.
- Manual verification: divider drag + horizontal scroll, and settings.json write-back on
  drag-end and zoom (canvas geometry is impractical to assert in unit tests).

## Trade-offs

Writing to `settings.json` on every drag-end and zoom keystroke mutates user config
frequently (on discrete events only, not mid-drag). This makes state persistent and
visible at the cost of git-graph tweaks appearing as `settings.json` diffs. If undesirable,
the `GitGraphsDb` SQLite store (`git_graph.rs:4519`) is the alternative persistence home
without changing the rest of this design.

## Release Notes

- Added an adjustable Git Graph: draggable graph/message divider, removed width cap with
  horizontal scroll, and keyboard zoom (`cmd-=` / `cmd--` / `cmd-0`) with `git_graph`
  settings for lane width, row height, zoom, and graph width.
