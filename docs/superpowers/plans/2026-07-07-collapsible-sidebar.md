# Collapsible Sidebar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Left/Right dock panels (Project Panel, Git Panel, Agent Panel, etc.) default to a collapsed state that reserves zero layout width, reveal as a floating overlay on hovering their status-bar icon, and can be pinned open (reverting to today's push-content docked behavior) via a pin control — with the pinned choice persisted globally across restarts.

**Architecture:** Add a `pinned: bool` field to `Dock` (Left/Right only; Bottom is unaffected and always behaves as pinned) with global (non-workspace-scoped) KVP persistence. `render_dock` in `workspace.rs` skips width reservation when a Left/Right dock is unpinned. `Dock::render` gains a hover-triggered floating overlay (reusing the `deferred()`/`anchored()` popover pattern already used elsewhere in this codebase) and a pin/unpin `IconButton` that toggles `pinned`.

**Tech Stack:** Rust, GPUI (this repo's UI framework — see `CLAUDE.md` GPUI section), existing `db::kvp::KeyValueStore` for persistence.

## Global Constraints

- Follow `/Users/neeraj/Dev/zed/CLAUDE.md`: no `unwrap()`/panicking indexing, propagate errors with `?`, never silently discard errors with `let _ =`, no `mod.rs` files, full-word variable names, use `./script/clippy` not `cargo clippy`.
- **Bottom dock is out of scope** — `DockPosition::Bottom` must be unaffected by every change in this plan (see spec `docs/superpowers/specs/2026-07-07-collapsible-sidebar-design.md`, "Bottom dock (Terminal/Tasks) is explicitly out of scope").
- Pin state is **global**, not scoped by `workspace_id` — this is the one deliberate difference from the existing `PANEL_SIZE_STATE_KEY` pattern in `dock.rs`/`workspace.rs`.
- Run `cargo check -p workspace` after every step that touches `crates/workspace/src/dock.rs` or `crates/workspace/src/workspace.rs` — these files are large (`dock.rs` ~1550 lines, `workspace.rs` ~13700+ lines) and GPUI element-tree code is easy to get subtly wrong; catch it early rather than at the end of a task.

---

### Task 1: Add `pinned` state and global persistence to `Dock`

**Files:**
- Modify: `crates/workspace/src/dock.rs:269-281` (the `Dock` struct), `crates/workspace/src/dock.rs:391-470` (`Dock::new`), `crates/workspace/src/dock.rs:1072-1088` (persistence helpers area)
- Test: `crates/workspace/src/workspace.rs` (test module, alongside `test_toggle_docks_and_panels` at `workspace.rs:12881`)

**Interfaces:**
- Produces: `Dock::is_pinned(&self) -> bool`, `Dock::set_pinned(&mut self, pinned: bool, cx: &mut Context<Self>)`, `pub(crate) const DOCK_PINNED_KEY: &str = "dock_pinned"`, `pub(crate) fn load_pinned_state(position: DockPosition, cx: &App) -> bool`, `pub(crate) fn persist_pinned_state(position: DockPosition, pinned: bool, cx: &mut App)`.
- Consumes: `db::kvp::KeyValueStore` (already imported in `dock.rs` via `use db::kvp::KeyValueStore;` at `dock.rs:8`), the existing `DockPosition` enum (`dock.rs:290`).

- [ ] **Step 1: Add the `pinned` field and a `dock_position_key` helper**

In `crates/workspace/src/dock.rs`, add a helper right after `DockPosition::axis` (`dock.rs:335-340`):

```rust
impl DockPosition {
    fn label(&self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Bottom => "Bottom",
            Self::Right => "Right",
        }
    }

    pub fn axis(&self) -> Axis {
        match self {
            Self::Left | Self::Right => Axis::Horizontal,
            Self::Bottom => Axis::Vertical,
        }
    }

    /// Stable, lowercase key used for global (non-workspace-scoped)
    /// persistence, e.g. of pin state. Bottom is intentionally excluded
    /// from pin persistence (see collapsible sidebar spec) but the key is
    /// still defined here for completeness.
    fn persistence_key(&self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Bottom => "bottom",
            Self::Right => "right",
        }
    }
}
```

Then add the field to the `Dock` struct (`dock.rs:269-281`):

```rust
pub struct Dock {
    position: DockPosition,
    panel_entries: Vec<PanelEntry>,
    workspace: WeakEntity<Workspace>,
    is_open: bool,
    active_panel_index: Option<usize>,
    focus_handle: FocusHandle,
    focus_follows_mouse: FocusFollowsMouse,
    pub(crate) serialized_dock: Option<DockData>,
    zoom_layer_open: bool,
    modal_layer: Entity<ModalLayer>,
    pinned: bool,
    _subscriptions: [Subscription; 2],
}
```

- [ ] **Step 2: Add the global persistence constant and helpers**

Add near `PANEL_SIZE_STATE_KEY` (`dock.rs:361`):

```rust
pub(crate) const PANEL_SIZE_STATE_KEY: &str = "dock_panel_size";
pub(crate) const DOCK_PINNED_KEY: &str = "dock_pinned";

/// Bottom dock always behaves as pinned (unaffected by the collapsible
/// sidebar feature); only Left/Right read/write persisted pin state.
fn dock_position_supports_pinning(position: DockPosition) -> bool {
    matches!(position, DockPosition::Left | DockPosition::Right)
}

pub(crate) fn load_pinned_state(position: DockPosition, cx: &App) -> bool {
    if !dock_position_supports_pinning(position) {
        return true;
    }
    let kvp = KeyValueStore::global(cx);
    let scope = kvp.scoped(DOCK_PINNED_KEY);
    scope
        .read(position.persistence_key())
        .log_err()
        .flatten()
        .map(|value| value == "1")
        .unwrap_or(false)
}

pub(crate) fn persist_pinned_state(position: DockPosition, pinned: bool, cx: &mut App) {
    if !dock_position_supports_pinning(position) {
        return;
    }
    let kvp = KeyValueStore::global(cx);
    cx.background_spawn(async move {
        let scope = kvp.scoped(DOCK_PINNED_KEY);
        scope
            .write(
                position.persistence_key().to_string(),
                if pinned { "1" } else { "0" }.to_string(),
            )
            .await
    })
    .detach_and_log_err(cx);
}
```

`util::ResultExt` (for `.log_err()`) is already imported in `dock.rs:23`.

- [ ] **Step 3: Initialize `pinned` in `Dock::new` and add accessor/setter methods**

In `Dock::new` (`dock.rs:413-425`), load the persisted value:

```rust
Self {
    position,
    workspace: workspace.downgrade(),
    panel_entries: Default::default(),
    active_panel_index: None,
    is_open: false,
    focus_handle: focus_handle.clone(),
    focus_follows_mouse: WorkspaceSettings::get_global(cx).focus_follows_mouse,
    _subscriptions: [focus_subscription, zoom_subscription],
    serialized_dock: None,
    zoom_layer_open: false,
    modal_layer,
    pinned: load_pinned_state(position, cx),
}
```

Add accessor/setter methods near `is_open` (`dock.rs:476-478`):

```rust
pub fn is_open(&self) -> bool {
    self.is_open
}

pub fn is_pinned(&self) -> bool {
    self.pinned
}

pub fn set_pinned(&mut self, pinned: bool, cx: &mut Context<Self>) {
    // Bottom dock is always pinned (collapsible sidebar is Left/Right only,
    // see the collapsible sidebar spec) -- ignore any attempt to change it,
    // rather than relying solely on the UI never exposing a pin control for
    // it. This keeps the invariant enforced at the API boundary.
    if !dock_position_supports_pinning(self.position) {
        return;
    }
    if pinned == self.pinned {
        return;
    }
    self.pinned = pinned;
    persist_pinned_state(self.position, pinned, cx);
    cx.notify();
}
```

- [ ] **Step 4: Write the persistence round-trip test**

Add to the test module in `crates/workspace/src/workspace.rs`, immediately after `test_toggle_docks_and_panels` (ends around `workspace.rs:13031`, right before `test_close_panel_on_toggle` at `workspace.rs:13033`):

```rust
#[gpui::test]
async fn test_dock_pinned_state_defaults_and_persists(cx: &mut gpui::TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());

    let project = Project::test(fs, [], cx).await;
    let (workspace, cx) =
        cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));

    // Left/Right default to unpinned (collapsed); Bottom is always pinned.
    workspace.update_in(cx, |workspace, _window, cx| {
        assert!(!workspace.left_dock().read(cx).is_pinned());
        assert!(!workspace.right_dock().read(cx).is_pinned());
        assert!(workspace.bottom_dock().read(cx).is_pinned());
    });

    // Pinning the left dock persists and is readable back via the same
    // global (non-workspace-scoped) helper the next Dock::new call would use.
    workspace.update_in(cx, |workspace, _window, cx| {
        workspace.left_dock().update(cx, |dock, cx| {
            dock.set_pinned(true, cx);
        });
    });
    cx.executor().run_until_parked();

    workspace.read_with(cx, |workspace, cx| {
        assert!(workspace.left_dock().read(cx).is_pinned());
        assert!(dock::load_pinned_state(DockPosition::Left, cx));
        // Right dock is untouched.
        assert!(!dock::load_pinned_state(DockPosition::Right, cx));
    });

    // Unpinning persists back to false.
    workspace.update_in(cx, |workspace, _window, cx| {
        workspace.left_dock().update(cx, |dock, cx| {
            dock.set_pinned(false, cx);
        });
    });
    cx.executor().run_until_parked();

    workspace.read_with(cx, |_workspace, cx| {
        assert!(!dock::load_pinned_state(DockPosition::Left, cx));
    });

    // Bottom dock pin state is never persisted, even if set_pinned were
    // called on it directly -- this documents the trap noted in Dock::render
    // (Task 3): Bottom's Dock always reports is_pinned() == true regardless
    // of what's written here, because load_pinned_state short-circuits to
    // true for positions where dock_position_supports_pinning is false.
    workspace.update_in(cx, |workspace, _window, cx| {
        workspace.bottom_dock().update(cx, |dock, cx| {
            dock.set_pinned(false, cx);
        });
    });
    cx.executor().run_until_parked();

    workspace.read_with(cx, |workspace, cx| {
        assert!(workspace.bottom_dock().read(cx).is_pinned());
    });
}
```

This uses `cx.executor().run_until_parked()` (per this repo's convention — see `CLAUDE.md` "Timers in tests") to let the `cx.background_spawn` write complete before asserting the persisted value.

- [ ] **Step 5: Run the test to verify it fails, then passes**

Run: `cargo test -p workspace test_dock_pinned_state_defaults_and_persists`
Expected before Steps 1-3 exist: compile error (`is_pinned`, `set_pinned`, `load_pinned_state` not found).
After implementing Steps 1-3: `cargo test -p workspace test_dock_pinned_state_defaults_and_persists` → PASS.

- [ ] **Step 6: Compile-check the whole crate and commit**

Run: `cargo check -p workspace`
Expected: no errors (the `pinned` field is added but not yet read by `render_dock`/`Dock::render`, which is fine — it's just an unused-in-layout field until Task 2).

```bash
git add crates/workspace/src/dock.rs crates/workspace/src/workspace.rs
git commit -m "workspace: Add pinned state and global persistence to Dock"
```

---

### Task 2: Skip width reservation for unpinned Left/Right docks

**Files:**
- Modify: `crates/workspace/src/workspace.rs:7978-8048` (`render_dock`)
- Test: `crates/workspace/src/workspace.rs` (test module)

**Interfaces:**
- Consumes: `Dock::is_pinned(&self) -> bool` (Task 1), `Dock::visible_panel(&self) -> Option<&Arc<dyn PanelHandle>>` (already exists, `dock.rs:833`).
- Produces: no new public interface — `render_dock`'s behavior changes for unpinned Left/Right docks (zero-width container).

- [ ] **Step 1: Write a failing layout test**

`render_dock` isn't unit-testable in isolation easily (it needs a live `Window`), but we can assert the externally-observable effect: `Workspace::bounds_for_panel` — actually, the simplest observable contract is that `stored_panel_size_state`/`default_size` are irrelevant to the rendered dock width when unpinned. Since full pixel-layout assertions aren't practical here, cover this with a focused unit test on the new guard condition instead, added next to the persistence test from Task 1:

```rust
#[gpui::test]
async fn test_unpinned_left_right_docks_report_no_reserved_width(
    cx: &mut gpui::TestAppContext,
) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs, [], cx).await;
    let (workspace, cx) =
        cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));

    workspace.update_in(cx, |workspace, window, cx| {
        let left_panel = cx.new(|cx| TestPanel::new(DockPosition::Left, 100, cx));
        workspace.add_panel(left_panel.clone(), window, cx);
        workspace
            .left_dock()
            .update(cx, |dock, cx| dock.set_open(true, window, cx));
    });

    // Newly created docks default to unpinned (Task 1) -- render_dock must
    // report no reserved width for the left dock in this state.
    workspace.update_in(cx, |workspace, window, cx| {
        assert!(!workspace.left_dock().read(cx).is_pinned());
        assert!(workspace.left_dock().read(cx).is_open());
        assert_eq!(workspace.reserved_dock_width(DockPosition::Left, window, cx), None);
    });

    // Pinning restores the normal reserved-width behavior.
    workspace.update_in(cx, |workspace, window, cx| {
        workspace.left_dock().update(cx, |dock, cx| dock.set_pinned(true, cx));
        assert!(
            workspace
                .reserved_dock_width(DockPosition::Left, window, cx)
                .is_some()
        );
    });
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p workspace test_unpinned_left_right_docks_report_no_reserved_width`
Expected: FAIL — `reserved_dock_width` doesn't exist yet.

- [ ] **Step 3: Extract the width-reservation decision into a testable helper and use it from `render_dock`**

In `crates/workspace/src/workspace.rs`, add a new method right before `render_dock` (`workspace.rs:7978`):

```rust
/// Returns the width that should be reserved for `position`'s dock in the
/// horizontal flex layout, or `None` if the dock should reserve no space
/// (an unpinned Left/Right dock with an active panel — it renders as a
/// hover-triggered floating overlay instead; see `Dock::render`).
///
/// Bottom dock always reserves space when open (out of scope for the
/// collapsible sidebar feature).
fn reserved_dock_width(
    &self,
    position: DockPosition,
    window: &Window,
    cx: &App,
) -> Option<Pixels> {
    let dock = self.dock_at_position(position).read(cx);
    let panel = dock.visible_panel()?;
    if position.axis() == Axis::Horizontal && !dock.is_pinned() {
        return None;
    }
    let size_state = dock.stored_panel_size_state(panel.as_ref());
    Some(
        size_state
            .and_then(|state| state.size)
            .unwrap_or_else(|| panel.default_size(window, cx)),
    )
}
```

Check whether `dock_at_position` already exists as a method returning `&Entity<Dock>` for a given `DockPosition` — search `workspace.rs` for `fn dock_at_position`. If it doesn't exist, add it next to `left_dock()`/`right_dock()`/`bottom_dock()` (`workspace.rs:2165-2189`):

```rust
fn dock_at_position(&self, position: DockPosition) -> &Entity<Dock> {
    match position {
        DockPosition::Left => &self.left_dock,
        DockPosition::Bottom => &self.bottom_dock,
        DockPosition::Right => &self.right_dock,
    }
}
```

(If `dock_at_position` already exists with this exact shape, reuse it instead of adding a duplicate — check first with `grep -n "fn dock_at_position" crates/workspace/src/workspace.rs`.)

Now update `render_dock` (`workspace.rs:7978-8048`) to use this helper instead of its inline sizing logic for the fixed-width (non-flexible) branch:

```rust
fn render_dock(
    &self,
    position: DockPosition,
    dock: &Entity<Dock>,
    window: &mut Window,
    cx: &mut App,
) -> Option<Div> {
    if self.zoomed_position == Some(position) {
        return None;
    }

    let leader_border = dock.read(cx).active_panel().and_then(|panel| {
        let pane = panel.pane(cx)?;
        let follower_states = &self.follower_states;
        leader_border_for_pane(follower_states, &pane, window, cx)
    });

    let mut container = div()
        .flex()
        .overflow_hidden()
        .flex_none()
        .child(dock.clone())
        .children(leader_border);

    // Apply sizing only when the dock is open. When closed the dock is still
    // included in the element tree so its focus handle remains mounted --
    // without this, toggle_panel_focus cannot focus the panel when the dock
    // is closed.
    let dock_ref = dock.read(cx);
    if let Some(panel) = dock_ref.visible_panel() {
        let min_size = panel.min_size(window, cx);
        if position.axis() == Axis::Horizontal {
            let use_flexible = panel.has_flexible_size(window, cx);
            let flex_grow = if use_flexible && dock_ref.is_pinned() {
                dock_ref
                    .stored_panel_size_state(panel.as_ref())
                    .and_then(|state| state.flex)
                    .or_else(|| self.default_dock_flex(position))
            } else {
                None
            };
            if let Some(grow) = flex_grow {
                let grow = (grow / self.center_full_height_column_count()).max(0.001);
                let style = container.style();
                style.flex_grow = Some(grow);
                style.flex_shrink = Some(1.0);
                style.flex_basis = Some(relative(0.).into());
            } else if let Some(size) = self.reserved_dock_width(position, window, cx) {
                container = container.w(size);
                // Allow the fixed-width dock to shrink when there isn't
                // enough space (e.g. when the sidebar is open). The
                // stored size is preserved so the dock expands back
                // when space becomes available.
                let style = container.style();
                style.flex_shrink = Some(1.0);
            } else {
                // Unpinned: reserve no width. The panel renders as a
                // floating overlay from Dock::render instead (see Task 3).
                container = container.w(px(0.));
            }
            if let Some(min) = min_size
                && dock_ref.is_pinned()
            {
                container = container.min_w(min);
            }
        } else if let Some(size) = self.reserved_dock_width(position, window, cx) {
            container = container.h(size);
        }
    }

    Some(container)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p workspace test_unpinned_left_right_docks_report_no_reserved_width`
Expected: PASS

- [ ] **Step 5: Run the full existing dock/panel test suite to check for regressions**

Run: `cargo test -p workspace dock`
Run: `cargo test -p workspace panel`
Expected: all PASS, including pre-existing tests like `test_toggle_docks_and_panels`, `test_flexible_dock_sizing`, `test_toggle_all_docks_after_dock_move` — these exercise Right/Left docks and must still pass now that newly-created docks default to unpinned. If any pre-existing test fails because it now gets zero width unexpectedly, that test's dock needs an explicit `dock.set_pinned(true, cx)` added at its setup (these tests are asserting on today's always-docked behavior, which now requires pinning to reproduce).

- [ ] **Step 6: Compile-check and commit**

Run: `cargo check -p workspace`

```bash
git add crates/workspace/src/workspace.rs
git commit -m "workspace: Skip width reservation for unpinned Left/Right docks"
```

---

### Task 3: Hover-triggered floating overlay in `Dock::render`

**Files:**
- Modify: `crates/workspace/src/dock.rs:1091-1198` (`impl Render for Dock`)

**Interfaces:**
- Consumes: `Dock::is_pinned` (Task 1), `window.viewport_size() -> Size<Pixels>` (`gpui::Window`), `deferred()`/`anchored()` (already imported patterns — see `crates/ui/src/components/popover_menu.rs:376-386` for the reference usage this follows).
- Produces: `Dock` gains an ephemeral (non-persisted) `peeking: bool` field and `Dock::set_peeking(&mut self, peeking: bool, cx: &mut Context<Self>)`, consumed by Task 4.

- [ ] **Step 1: Add ephemeral `peeking` state**

Add a field to the `Dock` struct (`dock.rs:269-281`, alongside `pinned` from Task 1):

```rust
pub struct Dock {
    position: DockPosition,
    panel_entries: Vec<PanelEntry>,
    workspace: WeakEntity<Workspace>,
    is_open: bool,
    active_panel_index: Option<usize>,
    focus_handle: FocusHandle,
    focus_follows_mouse: FocusFollowsMouse,
    pub(crate) serialized_dock: Option<DockData>,
    zoom_layer_open: bool,
    modal_layer: Entity<ModalLayer>,
    pinned: bool,
    peeking: bool,
    _subscriptions: [Subscription; 2],
}
```

Initialize `peeking: false` in `Dock::new`'s struct literal (`dock.rs:413-426`, same spot edited in Task 1 Step 3).

Add the setter near `set_pinned` (Task 1 Step 3):

```rust
pub fn is_peeking(&self) -> bool {
    self.peeking
}

pub fn set_peeking(&mut self, peeking: bool, cx: &mut Context<Self>) {
    if peeking == self.peeking {
        return;
    }
    self.peeking = peeking;
    cx.notify();
}
```

- [ ] **Step 2: Run `cargo check -p workspace` to verify it compiles**

Run: `cargo check -p workspace`
Expected: no errors (new fields/methods are unused so far — fine at this point).

- [ ] **Step 3: Render the floating overlay when unpinned-but-peeking**

**Important:** `Dock::render` is shared by all three dock positions — `left_dock`, `bottom_dock`, and `right_dock` are all plain `Entity<Dock>` (`workspace.rs:1728-1730`), rendered through the same `impl Render for Dock`. The pin button and peek-overlay logic below **must not run for `DockPosition::Bottom`** — the spec requires Bottom to be completely unaffected (see `docs/superpowers/specs/2026-07-07-collapsible-sidebar-design.md`, "Bottom dock is explicitly out of scope"). `Dock::is_pinned()` already returns `true` for Bottom (Task 1's `load_pinned_state` short-circuits to `true` for positions where `dock_position_supports_pinning` is `false`), so gate the *entire* new branch on `dock_position_supports_pinning(position)`, not just on the `pinned` value — otherwise a pin/unpin button would incorrectly appear in the Terminal panel's UI, and clicking it would set the in-memory `pinned` field to `false` for the Bottom dock (even though persistence is correctly skipped), breaking the Terminal panel's layout for that session.

Rewrite `impl Render for Dock` (`dock.rs:1091-1198`). The existing rendering logic (`dock.rs:1155-1196`, unchanged) becomes a private helper `render_docked(...)` used unconditionally for Bottom and for pinned Left/Right docks; a new branch handles unpinned Left/Right docks (peek overlay or nothing):

```rust
impl Render for Dock {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dispatch_context = Self::dispatch_context();
        let Some(entry) = self.visible_entry() else {
            return div()
                .id("dock-panel")
                .key_context(dispatch_context)
                .track_focus(&self.focus_handle(cx));
        };

        let position = self.position;
        // Bottom dock is out of scope for collapsible/pin behavior -- always
        // render through the original always-docked path, with no pin
        // button and no peek overlay.
        let pinned = self.pinned || !dock_position_supports_pinning(position);
        let create_resize_handle = || {
            let handle = div()
                .id("resize-handle")
                .on_drag(DraggedDock(position), |dock, _, _, cx| {
                    cx.stop_propagation();
                    cx.new(|_| dock.clone())
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|_, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                    }),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|dock, e: &MouseUpEvent, window, cx| {
                        if e.click_count == 2 {
                            dock.resize_active_panel(None, None, window, cx);
                            dock.workspace
                                .update(cx, |workspace, cx| {
                                    workspace.serialize_workspace(window, cx);
                                })
                                .ok();
                            cx.stop_propagation();
                        }
                    }),
                )
                .occlude();
            match position {
                DockPosition::Left => deferred(
                    handle
                        .absolute()
                        .right(-RESIZE_HANDLE_SIZE / 2.)
                        .top(px(0.))
                        .h_full()
                        .w(RESIZE_HANDLE_SIZE)
                        .cursor_col_resize(),
                ),
                DockPosition::Bottom => deferred(
                    handle
                        .absolute()
                        .top(-RESIZE_HANDLE_SIZE / 2.)
                        .left(px(0.))
                        .w_full()
                        .h(RESIZE_HANDLE_SIZE)
                        .cursor_row_resize(),
                ),
                DockPosition::Right => deferred(
                    handle
                        .absolute()
                        .top(px(0.))
                        .left(-RESIZE_HANDLE_SIZE / 2.)
                        .h_full()
                        .w(RESIZE_HANDLE_SIZE)
                        .cursor_col_resize(),
                ),
            }
        };

        // Only Left/Right docks ever show pin UI. `Dock::set_pinned` (Task 1)
        // already no-ops for Bottom, so this is UX rather than a
        // correctness guard: Bottom's Terminal panel simply has no reason to
        // show a pin/unpin control that would do nothing when clicked.
        let pin_button = dock_position_supports_pinning(position)
            .then(|| self.render_pin_button(pinned, cx));

        let panel_content = || {
            div()
                .map(|this| match position.axis() {
                    Axis::Horizontal => this.w_full().h_full(),
                    Axis::Vertical => this.h_full().w_full(),
                })
                .child(
                    entry
                        .panel
                        .to_any()
                        .cached(StyleRefinement::default().v_flex().size_full()),
                )
        };

        if pinned {
            return div()
                .id("dock-panel")
                .key_context(dispatch_context)
                .track_focus(&self.focus_handle(cx))
                .focus_follows_mouse(self.focus_follows_mouse, cx)
                .flex()
                .bg(cx.theme().colors().panel_background)
                .border_color(cx.theme().colors().border)
                .overflow_hidden()
                .map(|this| match position.axis() {
                    Axis::Horizontal => this.w_full().h_full().flex_row(),
                    Axis::Vertical => this.h_full().w_full().flex_col(),
                })
                .map(|this| match position {
                    DockPosition::Left => this.border_r_1(),
                    DockPosition::Right => this.border_l_1(),
                    DockPosition::Bottom => this.border_t_1(),
                })
                .child(div().relative().size_full().child(panel_content()).children(pin_button))
                .when(self.resizable(cx), |this| this.child(create_resize_handle()));
        }

        // Unpinned: reserve no layout space here (Task 2 already made
        // render_dock skip width reservation). Only paint anything while
        // peeking; otherwise render an empty focus anchor so
        // toggle_panel_focus can still find a focus handle when collapsed.
        if !self.peeking {
            return div()
                .id("dock-panel")
                .key_context(dispatch_context)
                .track_focus(&self.focus_handle(cx));
        }

        let overlay_height = window.viewport_size().height;
        let overlay_width = entry.panel.default_size(window, cx);
        let anchor = match position {
            DockPosition::Left => Anchor::TopLeft,
            DockPosition::Right | DockPosition::Bottom => Anchor::TopRight,
        };

        div()
            .id("dock-panel")
            .key_context(dispatch_context)
            .track_focus(&self.focus_handle(cx))
            .child(deferred(
                anchored()
                    .anchor(anchor)
                    .child(
                        div()
                            .id("dock-peek-overlay")
                            .occlude()
                            .relative()
                            .w(overlay_width)
                            .h(overlay_height)
                            .flex()
                            .bg(cx.theme().colors().panel_background)
                            .border_color(cx.theme().colors().border)
                            .shadow_lg()
                            .overflow_hidden()
                            .map(|this| match position {
                                DockPosition::Left => this.border_r_1(),
                                DockPosition::Right => this.border_l_1(),
                                DockPosition::Bottom => this.border_t_1(),
                            })
                            .on_hover(cx.listener(move |dock, hovered, _window, cx| {
                                if !hovered {
                                    dock.set_peeking(false, cx);
                                }
                            }))
                            .child(panel_content())
                            .children(pin_button),
                    ),
            ))
    }
}
```

This introduces two new imports at the top of `dock.rs` (currently imports `gpui::{..., deferred, div, px}` at `dock.rs:10-15`): add `anchored`, `Anchor`. Update the import block:

```rust
use gpui::{
    Action, Anchor, AnyView, App, Axis, Context, Entity, EntityId, EventEmitter, FocusHandle,
    Focusable, IntoElement, KeyContext, MouseButton, MouseDownEvent, MouseUpEvent, ParentElement,
    Render, SharedString, StyleRefinement, Styled, Subscription, WeakEntity, Window, anchored,
    deferred, div, px,
};
```

(Check `crates/gpui/src/lib.rs` or `crates/gpui/src/elements/anchored.rs` re-exports `anchored` and `Anchor` at the crate root — the same names are already used this way in `crates/ui/src/components/popover_menu.rs:154,376`, which imports them from `gpui`.)

- [ ] **Step 4: Add the `render_pin_button` helper (stub for now, real behavior in Task 5)**

Add to `impl Dock` (near `resizable`, `dock.rs:480-482`), a minimal version for this task — Task 5 wires up the click handler:

```rust
fn render_pin_button(&self, pinned: bool, cx: &mut Context<Self>) -> AnyElement {
    let icon = if pinned { IconName::Unpin } else { IconName::Pin };
    let tooltip_text: SharedString = if pinned { "Unpin Panel" } else { "Pin Panel Open" }.into();
    ui::IconButton::new("dock-pin-toggle", icon)
        .icon_size(ui::IconSize::Small)
        .tooltip(move |window, cx| ui::Tooltip::simple(tooltip_text.clone(), cx))
        .on_click(cx.listener(|dock, _event, _window, cx| {
            let new_pinned = !dock.pinned;
            dock.set_pinned(new_pinned, cx);
        }))
        .into_any_element()
}
```

Add `AnyView` is already imported; add `AnyElement` and `IconName`/`IconSize`/`Tooltip` (from the `ui` re-exports already used elsewhere in `dock.rs`, e.g. `dock.rs:19-22` imports `ui::{ContextMenu, CountBadge, Divider, DividerColor, IconButton, Tooltip, prelude::*, right_click_menu}` — `IconButton` and `Tooltip` are already imported; add `IconName` and `IconSize` to that same `use ui::{...}` block, and add `AnyElement` to the `gpui::{...}` import block from Step 3).

- [ ] **Step 5: Compile-check**

Run: `cargo check -p workspace`
Expected: no errors. If `anchored`/`Anchor` aren't re-exported at the `gpui` crate root under those exact names, `cargo check` will report the real path — fix the `use` statement accordingly (this mirrors the working import in `crates/ui/src/components/popover_menu.rs`, so cross-reference that file's `use gpui::{...}` block if the names differ).

- [ ] **Step 6: Commit**

```bash
git add crates/workspace/src/dock.rs
git commit -m "workspace: Render collapsed dock panels as a hover peek overlay"
```

---

### Task 4: Wire hover-to-peek from the status bar panel icons

**Files:**
- Modify: `crates/workspace/src/dock.rs:1211` (`impl Render for PanelButtons`, specifically the per-panel button construction inside the `filter_map` starting at `dock.rs:1229`)

**Interfaces:**
- Consumes: `Dock::set_peeking` (Task 3), `Dock::activate_panel` (already exists, `dock.rs:818`), `Dock::set_open` (already exists, `dock.rs:536`).
- Produces: no new public interface — status bar icons gain hover behavior.

- [ ] **Step 1: Add an `on_hover` handler to each panel's status bar button**

In `crates/workspace/src/dock.rs`, inside `impl Render for PanelButtons` (`dock.rs:1211`), the per-panel icon is built via `right_click_menu(name)...` starting at `dock.rs:1262`. That builder chain ultimately produces a clickable element; add `.on_hover(...)` to it. First locate the end of the `right_click_menu(...)` chain for a single button (search past `dock.rs:1330` for where this per-panel `Some(...)` closure returns) and where the `action`/`tooltip` are used to actually build the clickable icon element — this is further down in the same function. Add the hover wiring at the point where the final icon element for this panel is constructed, using the already-captured `dock_entity`, `panel` (`Arc<dyn PanelHandle>`), and `i` (index) from the closure:

```rust
let dock_for_hover = dock_entity.clone();
let panel_for_hover = panel.clone();
let panel_index = i;
// ... existing icon-button construction chain continues here, adding:
.on_hover(move |hovered, window, cx| {
    let dock_for_hover = dock_for_hover.clone();
    let panel_for_hover = panel_for_hover.clone();
    if *hovered {
        dock_for_hover.update(cx, |dock, cx| {
            if dock.is_pinned() {
                return;
            }
            if Some(panel_index) != dock.active_panel_index() {
                dock.activate_panel(panel_index, window, cx);
            }
            dock.set_open(true, window, cx);
            dock.set_peeking(true, cx);
        });
    } else {
        dock_for_hover.update(cx, |dock, cx| {
            if !dock.is_pinned() {
                dock.set_peeking(false, cx);
            }
        });
        let _ = panel_for_hover;
    }
})
```

Note: `on_hover` on `div`/interactive elements in this codebase takes `impl Fn(&bool, &mut Window, &mut App)` (see `crates/gpui/src/elements/div.rs:602`) — adjust the closure signature to `move |hovered: &bool, window: &mut Window, cx: &mut App|` and dereference `*hovered` accordingly. Since `Dock::set_peeking`/`activate_panel`/`set_open` take `&mut Context<Dock>` not `&mut App`, call them via `dock_for_hover.update(cx, |dock, cx| ...)` as shown, which is exactly this pattern already used elsewhere in this same file (e.g. `dock.rs:1305-1317`).

Because the exact icon-button builder chain in this function spans many lines with a right-click context menu already attached (`dock.rs:1262-1330+`), the implementing engineer should read the full `impl Render for PanelButtons` function (`dock.rs:1211` to its closing brace) before inserting this, to attach `.on_hover` at the correct point in the chain (after the tooltip/click handlers, before `.into_any_element()` or equivalent).

- [ ] **Step 2: Compile-check**

Run: `cargo check -p workspace`
Expected: no errors. Fix closure signature/borrow issues as reported — `on_hover`'s exact closure signature must match `crates/gpui/src/elements/div.rs:602` precisely.

- [ ] **Step 3: Manual verification (GPUI hover interactions aren't practically unit-testable without a running window + real mouse events)**

Run the app: `cargo run -p zed`
Steps:
1. Confirm the Project Panel (or any Left-dock panel) is not visible on launch and its status bar icon shows no active/highlighted state consuming layout space.
2. Hover the Project Panel's status bar icon — confirm the panel appears as a floating overlay over the editor, not pushing it.
3. Move the mouse off the icon and the overlay — confirm the overlay disappears after leaving both.
4. Move the mouse from the icon directly onto the overlay without a gap — confirm the overlay stays visible (this is why the overlay itself also needs `on_hover` per Task 3 Step 3; without it, moving from icon to overlay would flicker-close).

- [ ] **Step 4: Commit**

```bash
git add crates/workspace/src/dock.rs
git commit -m "workspace: Trigger dock peek overlay from status bar icon hover"
```

---

### Task 5: Manual end-to-end verification of pin/unpin persistence

**Files:** none (verification only)

**Interfaces:** none

- [ ] **Step 1: Run the full workspace test suite**

Run: `cargo test -p workspace`
Expected: all PASS, including the new tests from Tasks 1-2 and all pre-existing dock/panel tests.

- [ ] **Step 2: Run clippy**

Run: `./script/clippy` (per `CLAUDE.md` — do not use `cargo clippy` directly)
Expected: no new warnings introduced by this feature's changes.

- [ ] **Step 3: Manual verification of the full pin/unpin/persist cycle**

Run the app: `cargo run -p zed`
Steps:
1. Launch with a fresh/default profile (or note current pin state first). Confirm Left and Right dock panels are collapsed (icon-only, no reserved width) by default.
2. Hover a panel icon, confirm the overlay appears; click the pin button in its top corner.
3. Confirm the panel now pushes editor content (today's normal docked behavior) and is resizable via the existing drag handle.
4. Quit and relaunch Zed.
5. Confirm the previously-pinned panel is still pinned/docked on relaunch, and any other Left/Right panel that was never pinned is still collapsed.
6. Click the pin button again on the now-docked panel to unpin it; confirm it collapses back to icon-only immediately.
7. Confirm the Bottom dock (Terminal) is entirely unaffected throughout — it should always behave exactly as it does today (always reserves space when open, no hover/peek/pin chrome).

- [ ] **Step 4: Report results**

No commit for this task — it's verification only. If any step in Step 3 fails, file it as a bug against the relevant Task (1-4) rather than patching ad hoc; note which task's code is implicated so it can be fixed with a proper test added retroactively.
