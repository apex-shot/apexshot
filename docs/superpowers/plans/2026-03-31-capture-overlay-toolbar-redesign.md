# Capture Overlay Toolbar Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the centered capture overlay toolbar with a cinematic split-rail cockpit layout that keeps core actions fast while giving ApexShot a distinct silhouette.

**Architecture:** Keep the overlay in `src/overlay.rs`, but split the current single `ToolbarLayout` into a left tool rail and a right action rail with explicit card rectangles for hit-testing and drawing. Refactor hover state and pointer handling to target named cockpit regions, then update drawing to render the new chrome, active states, and edge-aware fallback behavior.

**Tech Stack:** Rust, GTK4, Cairo drawing in `src/overlay.rs`, in-file unit tests via `#[cfg(test)]`, `cargo test`

---

## File Structure

- Modify: `src/overlay.rs`
  - Replace the single-strip toolbar layout model with split cockpit rails
  - Update hover state and hit-testing to support tool, size, confirm, and cancel cards
  - Draw the cinematic rail chrome and card treatments
  - Add unit tests for layout placement and hit-testing fallbacks
- Verify: `Cargo.toml`
  - No dependency changes expected

---

### Task 1: Introduce Split-Rail Layout Data and Tests

**Files:**
- Modify: `src/overlay.rs`
- Test: `src/overlay.rs`

- [ ] **Step 1: Write the failing layout tests**

Add these tests to the existing `#[cfg(test)] mod tests` in `src/overlay.rs`:

```rust
    #[test]
    fn test_compute_toolbar_layout_prefers_split_side_rails() {
        let layout = compute_toolbar_layout(300.0, 220.0, 640.0, 420.0, 1600.0, 1000.0);

        assert!(layout.left_tools_panel.x + layout.left_tools_panel.width <= 300.0);
        assert!(layout.right_actions_panel.x >= 300.0 + 640.0);
        assert_eq!(layout.tool_cells.len(), TOOLBAR_ICONS.len());
        assert!(!layout.compact_mode);
    }

    #[test]
    fn test_compute_toolbar_layout_collapses_when_side_space_is_tight() {
        let layout = compute_toolbar_layout(12.0, 120.0, 260.0, 220.0, 420.0, 700.0);

        assert!(layout.compact_mode);
        assert!(layout.left_tools_panel.x >= FEATURE_PANEL_MARGIN);
        assert!(layout.right_actions_panel.x >= FEATURE_PANEL_MARGIN);
    }
```

- [ ] **Step 2: Run the new tests to verify they fail**

Run:

```bash
cargo test test_compute_toolbar_layout_prefers_split_side_rails test_compute_toolbar_layout_collapses_when_side_space_is_tight -- --nocapture
```

Expected: FAIL because `ToolbarLayout` does not yet expose `left_tools_panel`, `right_actions_panel`, `tool_cells`, or `compact_mode`.

- [ ] **Step 3: Replace the old layout model with split cockpit rails**

Update the toolbar data structures near the current `ToolbarLayout` and `ToolbarHit` definitions in `src/overlay.rs`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActionCard {
    Confirm,
    Cancel,
}

struct ToolbarLayout {
    left_tools_panel: RectF,
    right_actions_panel: RectF,
    tool_cells: [RectF; 8],
    size_card: RectF,
    confirm_card: RectF,
    cancel_card: RectF,
    compact_mode: bool,
}

enum ToolbarHit {
    Tool(usize),
    SizeCard,
    Action(ActionCard),
}
```

Refactor `compute_toolbar_layout()` so it:

```rust
fn compute_toolbar_layout(
    selection_x: f64,
    selection_y: f64,
    selection_width: f64,
    selection_height: f64,
    screen_width: f64,
    screen_height: f64,
) -> ToolbarLayout {
    let tool_card_w = 72.0;
    let tool_card_h = 62.0;
    let rail_gap = 18.0;
    let outer_gap = 18.0;
    let action_panel_w = 112.0;
    let action_card_h = 50.0;
    let tool_panel_h = tool_card_h * TOOLBAR_ICONS.len() as f64;
    let action_panel_h = action_card_h * 3.0 + 16.0;

    let center_y = selection_y + selection_height / 2.0;
    let left_x = selection_x - outer_gap - tool_card_w;
    let right_x = selection_x + selection_width + outer_gap;
    let split_fits = left_x >= FEATURE_PANEL_MARGIN
        && right_x + action_panel_w <= screen_width - FEATURE_PANEL_MARGIN;

    let compact_mode = !split_fits;

    let left_panel_x = if compact_mode {
        (selection_x + selection_width + rail_gap)
            .min(screen_width - tool_card_w - FEATURE_PANEL_MARGIN)
            .max(FEATURE_PANEL_MARGIN)
    } else {
        left_x
    };

    let right_panel_x = if compact_mode {
        left_panel_x
    } else {
        right_x
    };

    let left_panel_y = (center_y - tool_panel_h / 2.0).clamp(
        FEATURE_PANEL_MARGIN,
        screen_height - tool_panel_h - FEATURE_PANEL_MARGIN,
    );
    let right_panel_y = (center_y - action_panel_h / 2.0).clamp(
        FEATURE_PANEL_MARGIN,
        screen_height - action_panel_h - FEATURE_PANEL_MARGIN,
    );

    let left_tools_panel = RectF {
        x: left_panel_x,
        y: left_panel_y,
        width: tool_card_w,
        height: tool_panel_h,
    };
    let right_actions_panel = RectF {
        x: right_panel_x,
        y: if compact_mode {
            (left_panel_y + tool_panel_h + rail_gap)
                .min(screen_height - action_panel_h - FEATURE_PANEL_MARGIN)
        } else {
            right_panel_y
        },
        width: action_panel_w,
        height: action_panel_h,
    };

    let mut tool_cells = [RectF {
        x: 0.0,
        y: 0.0,
        width: tool_card_w,
        height: tool_card_h,
    }; 8];

    for (index, cell) in tool_cells.iter_mut().enumerate() {
        cell.x = left_tools_panel.x;
        cell.y = left_tools_panel.y + index as f64 * tool_card_h;
    }

    let size_card = RectF {
        x: right_actions_panel.x,
        y: right_actions_panel.y,
        width: action_panel_w,
        height: action_card_h + 8.0,
    };
    let confirm_card = RectF {
        x: right_actions_panel.x,
        y: size_card.y + size_card.height + 8.0,
        width: action_panel_w,
        height: action_card_h,
    };
    let cancel_card = RectF {
        x: right_actions_panel.x,
        y: confirm_card.y + confirm_card.height + 8.0,
        width: action_panel_w,
        height: action_card_h,
    };

    ToolbarLayout {
        left_tools_panel,
        right_actions_panel,
        tool_cells,
        size_card,
        confirm_card,
        cancel_card,
        compact_mode,
    }
}
```

- [ ] **Step 4: Run the layout tests to verify they pass**

Run:

```bash
cargo test test_compute_toolbar_layout_prefers_split_side_rails test_compute_toolbar_layout_collapses_when_side_space_is_tight -- --nocapture
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/overlay.rs
git commit -m "refactor: add split cockpit toolbar layout model"
```

---

### Task 2: Refactor Hover and Hit-Testing for Cockpit Regions

**Files:**
- Modify: `src/overlay.rs`
- Test: `src/overlay.rs`

- [ ] **Step 1: Write the failing hit-test coverage**

Add these tests in `src/overlay.rs`:

```rust
    #[test]
    fn test_toolbar_hit_at_reports_confirm_action() {
        let layout = compute_toolbar_layout(320.0, 220.0, 640.0, 420.0, 1600.0, 1000.0);
        let x = layout.confirm_card.x + layout.confirm_card.width / 2.0;
        let y = layout.confirm_card.y + layout.confirm_card.height / 2.0;

        let hit = toolbar_hit_at(320.0, 220.0, 640.0, 420.0, 1600.0, 1000.0, x, y);

        assert!(matches!(hit, Some(ToolbarHit::Action(ActionCard::Confirm))));
    }

    #[test]
    fn test_toolbar_hit_at_reports_tool_card() {
        let layout = compute_toolbar_layout(320.0, 220.0, 640.0, 420.0, 1600.0, 1000.0);
        let x = layout.tool_cells[0].x + layout.tool_cells[0].width / 2.0;
        let y = layout.tool_cells[0].y + layout.tool_cells[0].height / 2.0;

        let hit = toolbar_hit_at(320.0, 220.0, 640.0, 420.0, 1600.0, 1000.0, x, y);

        assert!(matches!(hit, Some(ToolbarHit::Tool(0))));
    }
```

- [ ] **Step 2: Run the targeted tests to verify they fail**

Run:

```bash
cargo test test_toolbar_hit_at_reports_confirm_action test_toolbar_hit_at_reports_tool_card -- --nocapture
```

Expected: FAIL because `toolbar_hit_at()` still only knows about `Tool` and `SizePanel`.

- [ ] **Step 3: Expand selector hover state and region matching**

Replace the old hover fields in `SelectorState`:

```rust
hover_tool_index: Option<usize>,
hover_size_panel: bool,
```

with:

```rust
hover_tool_index: Option<usize>,
hover_size_card: bool,
hover_action: Option<ActionCard>,
```

Update `Default for SelectorState` accordingly:

```rust
hover_tool_index: None,
hover_size_card: false,
hover_action: None,
```

Then update `toolbar_hit_at()` and `toolbar_item_at()`:

```rust
fn toolbar_hit_at(...) -> Option<ToolbarHit> {
    let layout = compute_toolbar_layout(...);

    for (index, cell) in layout.tool_cells.iter().enumerate() {
        if cell.contains(x, y) {
            return Some(ToolbarHit::Tool(index));
        }
    }

    if layout.size_card.contains(x, y) {
        return Some(ToolbarHit::SizeCard);
    }
    if layout.confirm_card.contains(x, y) {
        return Some(ToolbarHit::Action(ActionCard::Confirm));
    }
    if layout.cancel_card.contains(x, y) {
        return Some(ToolbarHit::Action(ActionCard::Cancel));
    }

    None
}

fn toolbar_item_at(...) -> Option<ToolbarIcon> {
    match toolbar_hit_at(...) {
        Some(ToolbarHit::Tool(index)) => Some(TOOLBAR_ICONS[index]),
        _ => None,
    }
}
```

Update the motion handler in `setup_window()` so hover state becomes:

```rust
let (next_hover_tool_index, next_hover_size_card, next_hover_action, cursor_name) = match hit {
    Some(ToolbarHit::Tool(index)) => (Some(index), false, None, "pointer"),
    Some(ToolbarHit::SizeCard) => (None, true, None, "default"),
    Some(ToolbarHit::Action(action)) => (None, false, Some(action), "pointer"),
    None => (None, false, None, cursor_name),
};
```

Also update leave handling to clear `hover_action`.

- [ ] **Step 4: Run the targeted tests to verify they pass**

Run:

```bash
cargo test test_toolbar_hit_at_reports_confirm_action test_toolbar_hit_at_reports_tool_card -- --nocapture
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/overlay.rs
git commit -m "refactor: add cockpit toolbar hit testing"
```

---

### Task 3: Render the Cinematic Cockpit Rails

**Files:**
- Modify: `src/overlay.rs`
- Test: `src/overlay.rs`

- [ ] **Step 1: Write a failing visual-layout assertion**

Add this test to verify the right-rail cards remain vertically ordered:

```rust
    #[test]
    fn test_compute_toolbar_layout_stacks_right_rail_cards_in_order() {
        let layout = compute_toolbar_layout(300.0, 220.0, 640.0, 420.0, 1600.0, 1000.0);

        assert!(layout.size_card.y < layout.confirm_card.y);
        assert!(layout.confirm_card.y < layout.cancel_card.y);
        assert!(layout.confirm_card.width == layout.cancel_card.width);
    }
```

- [ ] **Step 2: Run the targeted test**

Run:

```bash
cargo test test_compute_toolbar_layout_stacks_right_rail_cards_in_order -- --nocapture
```

Expected: PASS or FAIL depending on Task 1 details. If it already passes, keep it as regression coverage and continue.

- [ ] **Step 3: Replace the old strip drawing with side-rail chrome**

Refactor `draw_feature_toolbar()` to use the new layout fields:

```rust
let layout = compute_toolbar_layout(
    selection_x,
    selection_y,
    selection_width,
    selection_height,
    screen_width,
    screen_height,
);

draw_frosted_panel(
    context,
    layout.left_tools_panel.x,
    layout.left_tools_panel.y,
    layout.left_tools_panel.width,
    layout.left_tools_panel.height,
    15.0,
    screen_width,
    screen_height,
    background,
);

draw_frosted_panel(
    context,
    layout.size_card.x,
    layout.size_card.y,
    layout.size_card.width,
    layout.size_card.height,
    14.0,
    screen_width,
    screen_height,
    background,
);

draw_frosted_panel(
    context,
    layout.confirm_card.x,
    layout.confirm_card.y,
    layout.confirm_card.width,
    layout.confirm_card.height,
    14.0,
    screen_width,
    screen_height,
    background,
);

draw_frosted_panel(
    context,
    layout.cancel_card.x,
    layout.cancel_card.y,
    layout.cancel_card.width,
    layout.cancel_card.height,
    14.0,
    screen_width,
    screen_height,
    background,
);
```

Change the tool-cell rendering loop to use `layout.tool_cells` and vertical centers, and replace the current glossy white hover fill with warmer cinematic accents:

```rust
let accent_rgba = if is_hovered {
    (1.0, 0.55, 0.24, 0.32)
} else {
    (1.0, 0.55, 0.24, 0.0)
};

rounded_rect_path(context, cell.x + 4.0, cell.y + 4.0, cell.width - 8.0, cell.height - 8.0, 10.0);
context.set_source_rgba(accent_rgba.0, accent_rgba.1, accent_rgba.2, accent_rgba.3);
let _ = context.fill();
```

Add helper text for the right rail:

```rust
let size_text = format!("{}×{}", selection_width as i32, selection_height as i32);
draw_card_label(context, "SIZE", &layout.size_card, 0.74);
draw_card_value(context, &size_text, &layout.size_card);
draw_card_label(context, "CAPTURE", &layout.confirm_card, 0.92);
draw_card_label(context, "CANCEL", &layout.cancel_card, 0.78);
```

If `draw_feature_toolbar()` becomes hard to follow, split the new rendering into small helpers in the same file:

```rust
fn draw_toolbar_tool_card(...) { ... }
fn draw_toolbar_action_card(...) { ... }
fn draw_toolbar_size_card(...) { ... }
```

- [ ] **Step 4: Run overlay unit tests**

Run:

```bash
cargo test overlay::tests -- --nocapture
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/overlay.rs
git commit -m "feat: draw cinematic cockpit capture toolbar"
```

---

### Task 4: Wire Mouse Confirm/Cancel Actions and Final Verification

**Files:**
- Modify: `src/overlay.rs`
- Test: `src/overlay.rs`

- [ ] **Step 1: Add failing click-behavior tests for action hits**

Add focused tests for the action hit mapping:

```rust
    #[test]
    fn test_toolbar_hit_at_reports_cancel_action() {
        let layout = compute_toolbar_layout(320.0, 220.0, 640.0, 420.0, 1600.0, 1000.0);
        let x = layout.cancel_card.x + layout.cancel_card.width / 2.0;
        let y = layout.cancel_card.y + layout.cancel_card.height / 2.0;

        let hit = toolbar_hit_at(320.0, 220.0, 640.0, 420.0, 1600.0, 1000.0, x, y);

        assert!(matches!(hit, Some(ToolbarHit::Action(ActionCard::Cancel))));
    }
```

- [ ] **Step 2: Run the targeted test**

Run:

```bash
cargo test test_toolbar_hit_at_reports_cancel_action -- --nocapture
```

Expected: PASS if Task 2 is complete. If it fails, finish hit-testing first.

- [ ] **Step 3: Add explicit click handlers for confirm and cancel**

In the toolbar click handler inside `setup_window()`, expand the `match clicked` flow so it uses `toolbar_hit_at()` for non-tool actions:

```rust
let clicked_hit = toolbar_hit_at(
    rect.left,
    rect.top,
    rect.width(),
    rect.height(),
    screen_width as f64,
    screen_height as f64,
    x,
    y,
);

match clicked_hit {
    Some(ToolbarHit::Action(ActionCard::Cancel)) => {
        let mut st = state_click.lock().unwrap();
        st.cancelled = true;
        st.fullscreen_mode = false;
        drop(st);

        let _ = result_tx.send(Ok(None));
        if let Some(win) = window_weak_click.upgrade() {
            win.close();
        }
    }
    Some(ToolbarHit::Action(ActionCard::Confirm)) => {
        let st = state_click.lock().unwrap();
        let is_fullscreen = st.fullscreen_mode;
        let rect = current_selection_rect(&st);
        drop(st);

        let area = if is_fullscreen {
            SelectionArea {
                x: 0,
                y: 0,
                width: screen_width,
                height: screen_height,
            }
        } else {
            SelectionArea {
                x: rect.left.floor() as i32,
                y: rect.top.floor() as i32,
                width: rect.width().round() as i32,
                height: rect.height().round() as i32,
            }
        };

        let _ = result_tx.send(if area.is_valid() { Ok(Some(area)) } else { Ok(None) });
        if let Some(win) = window_weak_click.upgrade() {
            win.close();
        }
    }
    Some(ToolbarHit::Tool(_)) => { /* keep existing tool mode handling */ }
    _ => {}
}
```

Make `result_tx` and `window_weak_click` available to this closure the same way the key handler currently uses them.

- [ ] **Step 4: Run the full verification set**

Run:

```bash
cargo test overlay::tests -- --nocapture
cargo test -- --nocapture
```

Expected: PASS

Manual verification:

```bash
cargo run -- area
```

Check:
- left rail appears beside the selection when space exists
- right rail shows size, capture, and cancel cards
- hover states track individual cards
- confirm works by mouse click
- cancel works by mouse click
- tiny selections trigger the compact fallback without clipping off-screen

- [ ] **Step 5: Commit**

```bash
git add src/overlay.rs
git commit -m "feat: wire cockpit overlay actions"
```

---

## Self-Review

- Spec coverage: layout split, cinematic styling, side fallback behavior, hover isolation, size card, confirm/cancel actions, and manual edge-case verification are all covered by Tasks 1-4.
- Placeholder scan: no `TODO`, `TBD`, or deferred implementation markers remain.
- Type consistency: the plan consistently uses `ActionCard`, `ToolbarLayout`, `ToolbarHit`, `hover_size_card`, and `hover_action`.
