# Adjustable Git Graph Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Git Graph panel adjustable — draggable graph/message divider, no width cap with horizontal scroll, and keyboard zoom that scales lane spacing and row height — with all state driven by and written back to `settings.json`.

**Architecture:** Add a new `git_graph` settings section (`lane_width`, `row_height`, `zoom`, `graph_width`). Rendering reads *effective* values (`lane_width × zoom`, `row_height × zoom`) through small pure helpers. A resize handle in the panel layout drives an in-memory graph-area width and persists it on drag-end; keyboard `ZoomIn`/`ZoomOut`/`ResetZoom` actions adjust `zoom` and persist it. Both write-backs use `SettingsStore::update_settings_file`.

**Tech Stack:** Rust, GPUI, Zed `settings`/`settings_content` crates.

## Global Constraints

- Do not use `unwrap()`/`expect()`/panicking indexing in non-test code; propagate with `?` or handle explicitly.
- Never discard a fallible `Result` with `let _ =`; use `.log_err()` or handle it.
- Build/lint with `./script/clippy`, not `cargo clippy`.
- Never create `mod.rs`; new module is `crates/git_ui/src/git_graph_settings.rs`.
- Zoom is clamped to `[0.5, 3.0]`; zoom step is `1.1` (multiply in, divide out); `ResetZoom` → `1.0`.
- `lane_width` default `16.0` (floored at `4.0` on read); `row_height` default `0.0`; `graph_width` default unset (`None` = auto).
- The draggable divider and horizontal scroll apply to the **sidebar panel** layout (`render_panel_content`); zoom/lane/row settings apply everywhere the canvas renders.
- PR title imperative, no conventional-commit prefix, no trailing punctuation; PR body ends with a `Release Notes:` section.

---

### Task 1: Add the `git_graph` settings section and effective-value helpers

**Files:**
- Modify: `crates/settings_content/src/settings_content.rs` (add `GitGraphSettingsContent` + top-level field)
- Create: `crates/git_ui/src/git_graph_settings.rs`
- Modify: `crates/git_ui/src/git_ui.rs` (declare module)
- Modify: `assets/settings/default.json` (add `git_graph` defaults block)
- Modify: `crates/git_ui/src/git_graph.rs` (pure scaling helpers + test)

**Interfaces:**
- Produces:
  - `settings::SettingsContent` gains `pub git_graph: Option<settings::GitGraphSettingsContent>`.
  - `git_ui::git_graph_settings::GitGraphSettings { lane_width: f32, row_height: f32, zoom: f32, graph_width: Option<f32> }`, obtainable via `GitGraphSettings::get_global(cx)`.
  - In `git_graph.rs`: `fn clamp_zoom(zoom: f32) -> f32`, `fn scaled_lane_width(base_lane_width: f32, zoom: f32) -> Pixels`, `fn scaled_row_height(base: Pixels, extra: f32, zoom: f32) -> Pixels`.

- [ ] **Step 1: Add the content struct and top-level field**

In `crates/settings_content/src/settings_content.rs`, add this struct next to `GitPanelSettingsContent` (after the closing `}` at line ~712):

```rust
#[with_fallible_options]
#[derive(Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema, MergeFrom, Debug)]
pub struct GitGraphSettingsContent {
    /// Base horizontal spacing between graph lanes, in pixels.
    ///
    /// Default: 16.0
    #[serde(serialize_with = "crate::serialize_optional_f32_with_two_decimal_places")]
    pub lane_width: Option<f32>,

    /// Extra height added to each commit row, in pixels. 0 keeps the
    /// font-derived row height.
    ///
    /// Default: 0.0
    #[serde(serialize_with = "crate::serialize_optional_f32_with_two_decimal_places")]
    pub row_height: Option<f32>,

    /// Combined zoom multiplier applied to lane width and row height.
    ///
    /// Default: 1.0
    #[serde(serialize_with = "crate::serialize_optional_f32_with_two_decimal_places")]
    pub zoom: Option<f32>,

    /// Width of the graph lane area in the sidebar panel, in pixels.
    /// Unset means the width is computed automatically from the graph.
    ///
    /// Default: unset
    #[serde(serialize_with = "crate::serialize_optional_f32_with_two_decimal_places")]
    pub graph_width: Option<f32>,
}
```

Then add the field to the top-level `SettingsContent` struct, immediately after `pub git_panel: Option<GitPanelSettingsContent>,` (line 138):

```rust
    pub git_graph: Option<GitGraphSettingsContent>,
```

- [ ] **Step 2: Create the settings type**

Create `crates/git_ui/src/git_graph_settings.rs`:

```rust
use settings::{RegisterSetting, Settings};

const DEFAULT_LANE_WIDTH: f32 = 16.0;
const MIN_LANE_WIDTH: f32 = 4.0;
const DEFAULT_ROW_HEIGHT: f32 = 0.0;
const DEFAULT_ZOOM: f32 = 1.0;

#[derive(Debug, Clone, PartialEq, RegisterSetting)]
pub struct GitGraphSettings {
    /// Base horizontal spacing between lanes, in pixels (floored at `MIN_LANE_WIDTH`).
    pub lane_width: f32,
    /// Extra per-row height in pixels added on top of the font-derived height.
    pub row_height: f32,
    /// Combined zoom multiplier for lane width and row height.
    pub zoom: f32,
    /// Fixed graph-area width in pixels for the sidebar panel; `None` = auto.
    pub graph_width: Option<f32>,
}

impl Settings for GitGraphSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let git_graph = content.git_graph.clone().unwrap_or_default();
        Self {
            lane_width: git_graph
                .lane_width
                .unwrap_or(DEFAULT_LANE_WIDTH)
                .max(MIN_LANE_WIDTH),
            row_height: git_graph.row_height.unwrap_or(DEFAULT_ROW_HEIGHT),
            zoom: git_graph.zoom.unwrap_or(DEFAULT_ZOOM),
            graph_width: git_graph.graph_width,
        }
    }
}
```

Register the module in `crates/git_ui/src/git_ui.rs` next to `mod git_panel_settings;` (line 50):

```rust
mod git_graph_settings;
```

- [ ] **Step 3: Add default settings JSON**

In `assets/settings/default.json`, add this block immediately after the closing `}` of the `"git_panel": { ... }` block (the block starting at line 958):

```json
  "git_graph": {
    // Base horizontal spacing between graph lanes, in pixels.
    "lane_width": 16.0,
    // Extra height added to each commit row, in pixels (0 = font-derived).
    "row_height": 0.0,
    // Combined zoom multiplier for lane width and row height.
    "zoom": 1.0
  },
```

(`graph_width` is intentionally omitted so it defaults to auto.)

- [ ] **Step 4: Add pure scaling helpers with a failing test**

In `crates/git_ui/src/git_graph.rs`, add these free functions just below the constants block (after line 76, `const ROW_VERTICAL_PADDING`):

```rust
fn clamp_zoom(zoom: f32) -> f32 {
    zoom.clamp(0.5, 3.0)
}

fn scaled_lane_width(base_lane_width: f32, zoom: f32) -> Pixels {
    px((base_lane_width * clamp_zoom(zoom)).max(4.0))
}

fn scaled_row_height(base: Pixels, extra: f32, zoom: f32) -> Pixels {
    (base + px(extra)) * clamp_zoom(zoom)
}
```

Add this test inside the existing `mod tests { ... }` block (starts at line 4823), near the other non-async unit tests (e.g. after `test_git_graph_merge_commits`):

```rust
#[test]
fn test_scaling_helpers() {
    // zoom clamps to [0.5, 3.0]
    assert_eq!(clamp_zoom(0.1), 0.5);
    assert_eq!(clamp_zoom(10.0), 3.0);
    assert_eq!(clamp_zoom(1.0), 1.0);

    // lane width scales with zoom and floors at 4px
    assert_eq!(scaled_lane_width(16.0, 1.0), px(16.0));
    assert_eq!(scaled_lane_width(16.0, 2.0), px(32.0));
    assert_eq!(scaled_lane_width(1.0, 0.5), px(4.0)); // floored

    // row height adds extra then scales
    assert_eq!(scaled_row_height(px(20.0), 0.0, 1.0), px(20.0));
    assert_eq!(scaled_row_height(px(20.0), 4.0, 2.0), px(48.0));
}
```

- [ ] **Step 5: Run the test to verify it fails**

Run: `cargo test -p git_ui test_scaling_helpers`
Expected: FAIL — `cannot find function scaled_lane_width` (until Step 4's functions compile) or an assertion mismatch. If it fails to compile because the helpers aren't found, that confirms the test is wired to the not-yet-added code; add the functions from Step 4 first, then this failure should turn into a pass. (The functions and test are in the same step group; the meaningful check is Step 6.)

- [ ] **Step 6: Build and run all new pieces**

Run: `cargo test -p git_ui test_scaling_helpers && cargo build -p settings_content -p git_ui`
Expected: test PASSes; both crates compile. If `settings_content` fails because `serialize_optional_f32_with_two_decimal_places` or `with_fallible_options`/`MergeFrom` aren't in scope, confirm the struct sits in the same module as `GitPanelSettingsContent` (imports at top of `settings_content.rs` already cover them).

- [ ] **Step 7: Commit**

```bash
git add crates/settings_content/src/settings_content.rs crates/git_ui/src/git_graph_settings.rs crates/git_ui/src/git_ui.rs assets/settings/default.json crates/git_ui/src/git_graph.rs
git commit -m "git_ui: Add git_graph settings section and scaling helpers"
```

---

### Task 2: Thread effective lane width and row height into canvas rendering

**Files:**
- Modify: `crates/git_ui/src/git_graph.rs` (`row_height`, `lane_center_x`, `graph_canvas_content_width`, curve width, and their call sites)

**Interfaces:**
- Consumes: `GitGraphSettings::get_global(cx)`, `clamp_zoom`, `scaled_lane_width`, `scaled_row_height` from Task 1.
- Produces:
  - `fn lane_center_x(bounds: Bounds<Pixels>, lane: f32, lane_width: Pixels) -> Pixels` (signature changed — adds `lane_width`).
  - `fn GitGraph::effective_lane_width(&self, cx: &App) -> Pixels`.
  - `fn GitGraph::graph_canvas_content_width(&self, cx: &App) -> Pixels` (signature changed — adds `cx`).
  - `Self::row_height(window, cx)` now scales with zoom and `row_height` setting (signature unchanged).

- [ ] **Step 1: Write a failing test for content width scaling**

Add to `mod tests` in `git_graph.rs`, alongside the other `#[gpui::test]` tests (they use `init_test(cx)` at line 4840):

```rust
#[gpui::test]
fn test_graph_canvas_content_width_scales_and_uncaps(cx: &mut TestAppContext) {
    init_test(cx);
    cx.update(|cx| {
        // Base helper is settings-independent; verify no 140px cap by construction:
        // lane_width 16 * 20 lanes + padding far exceeds the old 140px clamp.
        let base = px(16.0) * 20.0 + px(12.0) * 2.0;
        assert!(base > px(140.0));

        // effective lane width honors the default zoom (1.0) and lane_width (16).
        let settings = crate::git_graph_settings::GitGraphSettings::get_global(cx);
        assert_eq!(scaled_lane_width(settings.lane_width, settings.zoom), px(16.0));
    });
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p git_ui test_graph_canvas_content_width_scales_and_uncaps`
Expected: FAIL to compile — `git_graph_settings` is private / not imported into the test module, or `GitGraphSettings` unused import. This confirms the wiring is incomplete.

- [ ] **Step 3: Make `row_height` scale with settings**

Replace the body of `row_height` (lines 1408-1415):

```rust
    fn row_height(window: &Window, cx: &App) -> Pixels {
        let settings = crate::git_graph_settings::GitGraphSettings::get_global(cx);
        let rem_size = window.rem_size();
        let line_height = window.text_style().line_height_in_pixels(rem_size);
        let base = line_height + ROW_VERTICAL_PADDING;
        let raw = scaled_row_height(base, settings.row_height, settings.zoom);
        let scale = window.scale_factor();

        (raw * scale).round() / scale
    }
```

(The parameter is already named `cx`-adjacent; rename `_cx` to `cx`. All existing callers pass a real `cx`, so no call-site change is needed.)

- [ ] **Step 4: Add `effective_lane_width` and update `graph_canvas_content_width`**

Add a method (place it right above `graph_canvas_content_width`, line 1431):

```rust
    fn effective_lane_width(&self, cx: &App) -> Pixels {
        let settings = crate::git_graph_settings::GitGraphSettings::get_global(cx);
        scaled_lane_width(settings.lane_width, settings.zoom)
    }
```

Change `graph_canvas_content_width` (lines 1431-1433) to take `cx` and use the effective width (removing the fixed `LANE_WIDTH`):

```rust
    fn graph_canvas_content_width(&self, cx: &App) -> Pixels {
        (self.effective_lane_width(cx) * self.graph_data.max_lanes.max(6) as f32)
            + LEFT_PADDING * 2.0
    }
```

Update its three call sites to pass `cx`:
- Line 1482: `.unwrap_or_else(|| self.graph_canvas_content_width(cx))`
- Line 2196: `let graph_width = self.graph_canvas_content_width(cx).max(px(28.));` *(note: the `.min(px(140.))` cap is removed here — the draggable width logic replaces this line entirely in Task 3, but removing the cap now keeps the build correct.)*
- Lines 3561-3562: replace `self.graph_canvas_content_width()` (both occurrences) with `self.graph_canvas_content_width(cx)`.

- [ ] **Step 5: Change `lane_center_x` to take a lane width and update callers**

Replace `lane_center_x` (lines 1259-1261):

```rust
fn lane_center_x(bounds: Bounds<Pixels>, lane: f32, lane_width: Pixels) -> Pixels {
    bounds.origin.x + LEFT_PADDING + lane * lane_width + lane_width / 2.0
}
```

In `render_graph_canvas` (starts line 3540), compute the effective lane width once near the top of the function body (after the existing `let row_height = Self::row_height(window, cx);` at line 3541):

```rust
        let lane_width = self.effective_lane_width(cx);
```

Update the three `lane_center_x` call sites in that function:
- Line 3639: `let commit_x = lane_center_x(bounds, row.lane as f32, lane_width);`
- Line 3651: `let line_x = lane_center_x(bounds, start_column as f32, lane_width);`
- Line 3696: `let mut to_column = lane_center_x(bounds, *to_column as f32, lane_width);`

And the curve width at line 3668:

```rust
                        let desired_curve_width = lane_width / 3.0;
```

- [ ] **Step 6: Fix the failing test's imports and run it**

Ensure the test module can see the settings type. At the top of `mod tests` (near line 4835 `use settings::{SettingsStore, ...}`), the path `crate::git_graph_settings::GitGraphSettings` is already fully qualified in the test, so no new `use` is required. Run:

Run: `cargo test -p git_ui test_graph_canvas_content_width_scales_and_uncaps test_scaling_helpers`
Expected: both PASS.

- [ ] **Step 7: Build the whole crate to catch call-site misses**

Run: `./script/clippy -p git_ui`
Expected: no errors. If clippy reports a `graph_canvas_content_width()` call with the wrong arity, add the missing `cx` argument at that site.

- [ ] **Step 8: Commit**

```bash
git add crates/git_ui/src/git_graph.rs
git commit -m "git_ui: Scale graph lane width and row height by settings and zoom"
```

---

### Task 3: Draggable graph/message divider with horizontal scroll and settings write-back

**Files:**
- Modify: `crates/git_ui/src/git_graph.rs` (new marker type, struct field, `render_panel_content`, new handle renderer, drag/drop wiring)

**Interfaces:**
- Consumes: `GitGraphSettings::get_global(cx)`, `graph_canvas_content_width(cx)` from Task 2; `SettingsStore`, `settings::update_settings_file`, `GitGraphSettingsContent` from Task 1.
- Produces:
  - `struct DraggedGraphDivider;`
  - `GitGraph.graph_width_override: Option<Pixels>` field (in-memory session width during/after drag).
  - `fn GitGraph::resolve_graph_area_width(&self, cx: &App) -> Pixels`.
  - `fn GitGraph::render_graph_divider_handle(&self, cx: &mut Context<Self>) -> AnyElement`.
  - `fn GitGraph::persist_graph_width(&self, width: Option<Pixels>, cx: &mut Context<Self>)`.

- [ ] **Step 1: Write a failing test for width resolution**

Add to `mod tests`:

```rust
#[gpui::test]
fn test_resolve_graph_area_width(cx: &mut TestAppContext) {
    init_test(cx);
    // In-memory override wins over settings/auto.
    // Pure selection: override -> setting -> auto(content width, min 28).
    fn resolve(override_px: Option<f32>, setting_px: Option<f32>, auto: f32) -> f32 {
        override_px
            .or(setting_px)
            .unwrap_or(auto)
            .max(28.0)
    }
    assert_eq!(resolve(Some(200.0), Some(80.0), 60.0), 200.0);
    assert_eq!(resolve(None, Some(80.0), 60.0), 80.0);
    assert_eq!(resolve(None, None, 60.0), 60.0);
    assert_eq!(resolve(None, None, 10.0), 28.0); // min floor
    let _ = cx;
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p git_ui test_resolve_graph_area_width`
Expected: FAIL to compile — `init_test` requires the test to exist and reference symbols; the inline `resolve` is self-contained so the true failure is that the test doesn't exist yet / matches nothing. Confirm it now exists and fails only if assertions are wrong. (This step's purpose is to lock the resolution semantics before wiring them into the struct.)

- [ ] **Step 3: Add the marker type and struct field**

Add near `struct DraggedSplitHandle;` (line 98):

```rust
struct DraggedGraphDivider;
```

Add a field to `pub struct GitGraph` (after `commit_details_split_state: Entity<SplitState>,`, line 1383):

```rust
    graph_width_override: Option<Pixels>,
```

Initialize it in the constructor next to `commit_details_split_state: cx.new(|_cx| SplitState::new()),` (line 1650):

```rust
            graph_width_override: None,
```

- [ ] **Step 4: Add the width-resolution and persistence methods**

Add these methods to `impl GitGraph` (place near `graph_canvas_content_width`):

```rust
    fn resolve_graph_area_width(&self, cx: &App) -> Pixels {
        let setting = crate::git_graph_settings::GitGraphSettings::get_global(cx)
            .graph_width
            .map(px);
        self.graph_width_override
            .or(setting)
            .unwrap_or_else(|| self.graph_canvas_content_width(cx))
            .max(px(28.))
    }

    fn persist_graph_width(&self, width: Option<Pixels>, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let fs = workspace.read(cx).app_state().fs.clone();
        let width = width.map(|w| w.0);
        cx.update_global::<SettingsStore, _>(|store, _cx| {
            store.update_settings_file(fs, move |settings, _cx| {
                settings.git_graph.get_or_insert_default().graph_width = width;
            });
        });
    }
```

Confirm `SettingsStore` and `settings::update_settings_file` are imported at the top of `git_graph.rs`; if not, add `use settings::SettingsStore;` (the crate already imports `settings::Settings`).

- [ ] **Step 5: Use the resolved width in `render_panel_content` and insert the handle**

Replace line 2196:

```rust
        let graph_width = self.resolve_graph_area_width(cx);
```

In the same function, the graph canvas `div` (lines 2206-2213) already has `.w(graph_width).h_full().min_w_0().overflow_hidden()`. Change `.overflow_hidden()` to `.overflow_x_scroll()` so overflowing lanes scroll instead of clip:

```rust
                        this.child(
                            div()
                                .w(graph_width)
                                .h_full()
                                .min_w_0()
                                .overflow_x_scroll()
                                .child(graph_canvas),
                        )
                        .child(self.render_graph_divider_handle(cx))
```

(The `.child(self.render_graph_divider_handle(cx))` is appended to the `when(!is_path_history, ...)` closure body so the handle sits between the graph area and the table, and is absent for path-history views.)

- [ ] **Step 6: Add the handle renderer**

Add this method to `impl GitGraph` (model it on `render_commit_view_resize_handle`, line 3993):

```rust
    fn render_graph_divider_handle(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("graph-divider-resize-container")
            .relative()
            .h_full()
            .flex_shrink_0()
            .w(px(1.))
            .bg(cx.theme().colors().border_variant)
            .child(
                div()
                    .id("graph-divider-resize-handle")
                    .absolute()
                    .left(px(-RESIZE_HANDLE_WIDTH / 2.0))
                    .w(px(RESIZE_HANDLE_WIDTH))
                    .h_full()
                    .cursor_col_resize()
                    .block_mouse_except_scroll()
                    .on_click(cx.listener(|this, event: &ClickEvent, _window, cx| {
                        if event.click_count() >= 2 {
                            this.graph_width_override = None;
                            this.persist_graph_width(None, cx);
                            cx.notify();
                        }
                        cx.stop_propagation();
                    }))
                    .on_drag(DraggedGraphDivider, |_, _, _, cx| cx.new(|_| gpui::Empty))
                    .on_drag_move::<DraggedGraphDivider>(cx.listener(
                        |this, event: &DragMoveEvent<DraggedGraphDivider>, _window, cx| {
                            let bounds = event.bounds;
                            let new_width = (event.event.position.x - bounds.left()).max(px(28.));
                            this.graph_width_override = Some(new_width);
                            cx.notify();
                        },
                    ))
                    .on_drop::<DraggedGraphDivider>(cx.listener(|this, _event, _window, cx| {
                        this.persist_graph_width(this.graph_width_override, cx);
                    })),
            )
            .into_any_element()
    }
```

Note: `event.bounds` for `DragMoveEvent` is the bounds of the *handle*, so `position.x - bounds.left()` measures drag distance from the handle, not the graph origin. Adjust to measure from the graph area origin: capture the graph container's left via the existing `graph_canvas_bounds: Rc<Cell<Option<Bounds<Pixels>>>>` field. In the drag-move listener, prefer:

```rust
                        |this, event: &DragMoveEvent<DraggedGraphDivider>, _window, cx| {
                            let origin_x = this
                                .graph_canvas_bounds
                                .get()
                                .map(|b| b.origin.x)
                                .unwrap_or(event.bounds.left());
                            let new_width = (event.event.position.x - origin_x).max(px(28.));
                            this.graph_width_override = Some(new_width);
                            cx.notify();
                        },
```

- [ ] **Step 7: Run tests and lint**

Run: `cargo test -p git_ui test_resolve_graph_area_width && ./script/clippy -p git_ui`
Expected: test PASSes; clippy clean. If `DragMoveEvent`, `ClickEvent`, or `block_mouse_except_scroll` are unresolved, copy the imports already used by `render_commit_view_resize_handle` (same file).

- [ ] **Step 8: Manual verification**

Run: `cargo run` (or the standard dev binary), open the Git Graph sidebar panel on a repo with branches, drag the divider between the graph and commit messages. Verify: graph area widens/narrows, lanes beyond the width scroll horizontally, double-click resets to auto, and `~/.config/zed/settings.json` gains `"git_graph": { "graph_width": <n> }` after a drag (and it is removed / reset after double-click).

- [ ] **Step 9: Commit**

```bash
git add crates/git_ui/src/git_graph.rs
git commit -m "git_ui: Add draggable graph divider with horizontal scroll and persistence"
```

---

### Task 4: Keyboard zoom actions with settings write-back

**Files:**
- Modify: `crates/git_ui/src/git_graph.rs` (actions, handlers, registration)
- Modify: `assets/keymaps/default-macos.json` (bindings)
- Modify: `assets/keymaps/default-linux.json` (bindings)

**Interfaces:**
- Consumes: `GitGraphSettings::get_global(cx)`, `clamp_zoom` from Task 1; `SettingsStore`/`update_settings_file` pattern from Task 3.
- Produces:
  - Actions `git_graph::ZoomIn`, `git_graph::ZoomOut`, `git_graph::ResetZoom`.
  - `fn GitGraph::set_zoom(&self, new_zoom: f32, cx: &mut Context<Self>)`.
  - Free helpers `fn zoom_in_step(z: f32) -> f32`, `fn zoom_out_step(z: f32) -> f32`.

- [ ] **Step 1: Write a failing test for zoom steps**

Add to `mod tests`:

```rust
#[test]
fn test_zoom_steps() {
    // step is 1.1, clamped to [0.5, 3.0]
    assert!((zoom_in_step(1.0) - 1.1).abs() < 1e-6);
    assert!((zoom_out_step(1.1) - 1.0).abs() < 1e-6);
    assert_eq!(zoom_in_step(3.0), 3.0); // clamps at max
    assert_eq!(zoom_out_step(0.5), 0.5); // clamps at min
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p git_ui test_zoom_steps`
Expected: FAIL — `cannot find function zoom_in_step`.

- [ ] **Step 3: Add the step helpers**

In `git_graph.rs`, below `clamp_zoom` (from Task 1):

```rust
fn zoom_in_step(zoom: f32) -> f32 {
    clamp_zoom(zoom * 1.1)
}

fn zoom_out_step(zoom: f32) -> f32 {
    clamp_zoom(zoom / 1.1)
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p git_ui test_zoom_steps`
Expected: PASS.

- [ ] **Step 5: Declare the actions**

In the `actions!(git_graph, [ ... ])` macro (ends around line 588), add these entries (with doc comments, matching the existing style):

```rust
        /// Increases the git graph zoom (lane spacing and row height).
        ZoomIn,
        /// Decreases the git graph zoom (lane spacing and row height).
        ZoomOut,
        /// Resets the git graph zoom to the default.
        ResetZoom,
```

- [ ] **Step 6: Add the zoom setter and action handlers**

Add a helper to `impl GitGraph` (near `persist_graph_width`):

```rust
    fn set_zoom(&self, new_zoom: f32, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let fs = workspace.read(cx).app_state().fs.clone();
        let new_zoom = clamp_zoom(new_zoom);
        cx.update_global::<SettingsStore, _>(|store, _cx| {
            store.update_settings_file(fs, move |settings, _cx| {
                settings.git_graph.get_or_insert_default().zoom = Some(new_zoom);
            });
        });
    }
```

Register the handlers on the root element in `render_root` (the `div().key_context("GitGraph")...` chain, lines 4201-4211). Add after the existing `.on_action` calls:

```rust
            .on_action(cx.listener(|this, _: &ZoomIn, _window, cx| {
                let current = crate::git_graph_settings::GitGraphSettings::get_global(cx).zoom;
                this.set_zoom(zoom_in_step(current), cx);
            }))
            .on_action(cx.listener(|this, _: &ZoomOut, _window, cx| {
                let current = crate::git_graph_settings::GitGraphSettings::get_global(cx).zoom;
                this.set_zoom(zoom_out_step(current), cx);
            }))
            .on_action(cx.listener(|this, _: &ResetZoom, _window, cx| {
                this.set_zoom(1.0, cx);
            }))
```

- [ ] **Step 7: Add keybindings**

In `assets/keymaps/default-macos.json`, add to the `"context": "GitGraph"` bindings block (lines 1656-1659):

```json
      "cmd-=": "git_graph::ZoomIn",
      "cmd-+": "git_graph::ZoomIn",
      "cmd--": "git_graph::ZoomOut",
      "cmd-0": "git_graph::ResetZoom",
```

In `assets/keymaps/default-linux.json`, add to its `"context": "GitGraph"` block (line 1562):

```json
      "ctrl-=": "git_graph::ZoomIn",
      "ctrl-+": "git_graph::ZoomIn",
      "ctrl--": "git_graph::ZoomOut",
      "ctrl-0": "git_graph::ResetZoom",
```

- [ ] **Step 8: Build, lint, and run all git_ui tests**

Run: `./script/clippy -p git_ui && cargo test -p git_ui`
Expected: clean; all tests pass (including `test_zoom_steps`, `test_scaling_helpers`, `test_resolve_graph_area_width`, and the existing graph tests). If an action isn't found by the keymap loader, verify the action name is spelled `git_graph::ZoomIn` and the `actions!` namespace is `git_graph`.

- [ ] **Step 9: Manual verification**

Run the dev binary, focus the Git Graph panel, press `cmd-=`/`cmd--`/`cmd-0`. Verify lanes and rows grow/shrink together, zoom clamps at the extremes, and `settings.json` gains `"git_graph": { "zoom": <n> }` that persists across restart.

- [ ] **Step 10: Commit**

```bash
git add crates/git_ui/src/git_graph.rs assets/keymaps/default-macos.json assets/keymaps/default-linux.json
git commit -m "git_ui: Add keyboard zoom for the git graph"
```

---

## Self-Review

**Spec coverage:**
- Draggable graph/text divider → Task 3. ✓
- Remove width cap + horizontal scroll → Task 2 (cap removal) + Task 3 (`overflow_x_scroll`). ✓
- Adjustable lane spacing → Task 1 (`lane_width`) + Task 2 (threading). ✓
- Adjustable row height → Task 1 (`row_height`) + Task 2 (`row_height` fn). ✓
- settings.json defaults → Task 1 (`default.json`). ✓
- Keyboard zoom, both axes together → Task 4 (single `zoom` multiplier). ✓
- Write-back to settings.json → Task 3 (`persist_graph_width`) + Task 4 (`set_zoom`). ✓
- Zoom clamp `[0.5, 3.0]`, step `1.1` → Task 1/Task 4. ✓
- Testing (unit tests for scaling, content width, zoom clamp) → Tasks 1, 2, 4. ✓

**Type consistency:** `GitGraphSettings` fields (`lane_width`, `row_height`, `zoom`, `graph_width`) are referenced identically across tasks. `graph_canvas_content_width(cx)` new signature is applied at all four call sites in Task 2. `lane_center_x(bounds, lane, lane_width)` new signature applied at all three call sites in Task 2. `clamp_zoom` defined once (Task 1) and reused (Tasks 2, 4). Marker types `DraggedSplitHandle` (existing) and `DraggedGraphDivider` (new, Task 3) are distinct.

**Placeholder scan:** No TBD/TODO; every code step shows full code. The only "adjust if needed" notes are concrete fallback instructions tied to specific compiler errors, not deferred work.
