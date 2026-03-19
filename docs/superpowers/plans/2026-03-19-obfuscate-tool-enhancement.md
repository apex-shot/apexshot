# Obfuscate Tool Enhancement Implementation Plan

> **For agentic workers:** Use subagent-driven-development or executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enhance obfuscate tool with method selector dropdown and per-method intensity controls

**Architecture:** Add a method selector button (icon only) before the slider when obfuscate tool is active. The dropdown shows 4 options: Pixelate, Blur (Secure), Blur (Smooth), Blackout. Each method has its own intensity value stored separately.

**Tech Stack:** Rust, GTK4, gtk4-layer-shell

---

## Chunk 1: Update ObfuscateMethod Enum

**Files:**
- Modify: `src/capture/editor/types.rs:39-44`
- Modify: `src/capture/editor/render.rs` (add Blackout rendering)
- Modify: `src/capture/editor/color.rs` (add clamp functions)

- [ ] **Step 1: Update ObfuscateMethod enum**

Modify `types.rs:39-44`:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObfuscateMethod {
    Pixelate,
    BlurSecure,
    BlurSmooth,
    Blackout,
}
```

- [ ] **Step 2: Add display_name method to ObfuscateMethod**

Add after the enum in `types.rs`:
```rust
impl ObfuscateMethod {
    pub fn display_name(&self) -> &'static str {
        match self {
            ObfuscateMethod::Pixelate => "Pixelate",
            ObfuscateMethod::BlurSecure => "Blur (Secure)",
            ObfuscateMethod::BlurSmooth => "Blur (Smooth)",
            ObfuscateMethod::Blackout => "Blackout",
        }
    }

    pub fn icon_name(&self) -> &'static str {
        match self {
            ObfuscateMethod::Pixelate => "obfuscate-pixelate",
            ObfuscateMethod::BlurSecure => "obfuscate-blur-secure",
            ObfuscateMethod::BlurSmooth => "obfuscate-blur-smooth",
            ObfuscateMethod::Blackout => "obfuscate-blackout",
        }
    }

    pub fn has_slider(&self) -> bool {
        !matches!(self, ObfuscateMethod::Blackout)
    }
}
```

- [ ] **Step 3: Add clamp functions for each method in color.rs**

Add after existing clamp functions in `color.rs:69-71`:
```rust
pub fn clamp_pixelate_amount(amount: f64) -> f64 {
    amount.clamp(MIN_OBFUSCATE_AMOUNT, MAX_OBFUSCATE_AMOUNT)
}

pub fn clamp_blur_secure_amount(amount: f64) -> f64 {
    amount.clamp(MIN_OBFUSCATE_AMOUNT, MAX_OBFUSCATE_AMOUNT)
}

pub fn clamp_blur_smooth_amount(amount: f64) -> f64 {
    amount.clamp(MIN_OBFUSCATE_AMOUNT, MAX_OBFUSCATE_AMOUNT)
}
```

- [ ] **Step 4: Add Blackout rendering function in render.rs**

Add function in `render.rs`:
```rust
pub fn apply_blackout_rect(image: &mut RgbaImage, rect: &Rect) {
    let x = rect.x.max(0) as u32;
    let y = rect.y.max(0) as u32;
    let width = rect.width as u32;
    let height = rect.height as u32;

    for dy in 0..height {
        for dx in 0..width {
            let px = x + dx;
            let py = y + dy;
            if px < image.width() && py < image.height() {
                image.put_pixel(px, py, Rgba([0, 0, 0, 255]));
            }
        }
    }
}
```

- [ ] **Step 5: Update render_obfuscate_action match in render.rs**

Find the `render_obfuscate_action` function and update to handle new methods:
```rust
match method {
    ObfuscateMethod::Pixelate => apply_censor_rect(&mut working, &rect, amount as i32),
    ObfuscateMethod::BlurSecure => apply_secure_pixelate(&mut working, &rect, amount as i32),
    ObfuscateMethod::BlurSmooth => apply_blur_rect(&mut working, &rect, amount as i32),
    ObfuscateMethod::Blackout => apply_blackout_rect(&mut working, &rect),
}
```

- [ ] **Step 6: Update apply_censor_rect usage for Pixelate**

The existing `apply_censor_rect` function already does pixelation. Keep it as-is.

- [ ] **Step 7: Run build to verify changes**

```bash
cd /home/codegoddy/Desktop/apexshot && cargo build 2>&1 | head -50
```

- [ ] **Step 8: Commit**

```bash
git add -A && git commit -m "feat(editor): update ObfuscateMethod enum with Blackout variant"
```

---

## Chunk 2: Update EditorState

**Files:**
- Modify: `src/capture/editor/state.rs:20-61`
- Modify: `src/capture/editor/state.rs` (add state methods)

- [ ] **Step 1: Add new state fields**

Add after `obfuscate_amount: f64` in `EditorState` struct:
```rust
pub obfuscate_method: ObfuscateMethod,
pub obfuscate_pixelate_amount: f64,
pub obfuscate_blur_secure_amount: f64,
pub obfuscate_blur_smooth_amount: f64,
```

- [ ] **Step 2: Update EditorState::new() defaults**

Find the `impl Default for EditorState` or initialization code and add:
```rust
obfuscate_method: ObfuscateMethod::Pixelate,
obfuscate_pixelate_amount: DEFAULT_OBFUSCATE_AMOUNT,
obfuscate_blur_secure_amount: DEFAULT_OBFUSCATE_AMOUNT,
obfuscate_blur_smooth_amount: DEFAULT_OBFUSCATE_AMOUNT,
```

- [ ] **Step 3: Add setter methods for obfuscate state**

Add methods to EditorState:
```rust
pub fn set_obfuscate_method(&mut self, method: ObfuscateMethod) {
    self.obfuscate_method = method;
}

pub fn obfuscate_method(&self) -> ObfuscateMethod {
    self.obfuscate_method
}

pub fn current_obfuscate_amount(&self) -> f64 {
    match self.obfuscate_method {
        ObfuscateMethod::Pixelate => self.obfuscate_pixelate_amount,
        ObfuscateMethod::BlurSecure => self.obfuscate_blur_secure_amount,
        ObfuscateMethod::BlurSmooth => self.obfuscate_blur_smooth_amount,
        ObfuscateMethod::Blackout => 0.0,
    }
}

pub fn set_current_obfuscate_amount(&mut self, amount: f64) {
    match self.obfuscate_method {
        ObfuscateMethod::Pixelate => self.obfuscate_pixelate_amount = clamp_pixelate_amount(amount),
        ObfuscateMethod::BlurSecure => self.obfuscate_blur_secure_amount = clamp_blur_secure_amount(amount),
        ObfuscateMethod::BlurSmooth => self.obfuscate_blur_smooth_amount = clamp_blur_smooth_amount(amount),
        ObfuscateMethod::Blackout => {}
    }
}
```

- [ ] **Step 4: Update size_control_mode_for_tool to return current method**

Update the `size_control_mode_for_tool` function to return the correct amount based on current method:
```rust
SizeControlMode::Obfuscate => {
    Some(self.current_obfuscate_amount())
}
```

- [ ] **Step 5: Update draft_action for Obfuscate tool**

Find where `Tool::Obfuscate` creates an action and update:
```rust
Tool::Obfuscate => {
    Rect::from_points(start, end).map(|rect| AnnotationAction::Obfuscate {
        rect,
        method: self.obfuscate_method,
        amount: self.current_obfuscate_amount(),
    })
}
```

- [ ] **Step 6: Run build to verify**

```bash
cargo build 2>&1 | head -80
```

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "feat(editor): add per-method obfuscate state fields"
```

---

## Chunk 3: Add Method Selector to Toolbar

**Files:**
- Modify: `src/capture/editor/window/toolbar.rs`
- Modify: `src/capture/editor/window/mod.rs`

- [ ] **Step 1: Add method selector parts to ToolbarModeParts**

Add to `ToolbarModeParts` struct:
```rust
pub obfuscate_method_group: GtkBox,
pub obfuscate_method_button: gtk4::Button,
pub obfuscate_method_popover: Popover,
pub obfuscate_method_list: GtkBox,
```

- [ ] **Step 2: Add obfuscate method group builder function**

Add new function in `toolbar.rs`:
```rust
pub fn build_obfuscate_method_controls() -> (
    GtkBox,
    gtk4::Button,
    Popover,
    GtkBox,
) {
    let obfuscate_method_group = GtkBox::new(Orientation::Horizontal, 4);
    obfuscate_method_group.add_css_class("editor-obfuscate-method-group");
    obfuscate_method_group.set_visible(false);

    let obfuscate_method_button = gtk4::Button::new();
    obfuscate_method_button.set_has_frame(false);
    obfuscate_method_button.set_focusable(false);
    obfuscate_method_button.add_css_class("flat");
    obfuscate_method_button.set_tooltip_text(Some("Obfuscate method"));

    let obfuscate_method_icon = Image::from_icon_name("obfuscate-pixelate");
    obfuscate_method_button.set_child(Some(&obfuscate_method_icon));

    let obfuscate_method_popover = Popover::new();
    obfuscate_method_popover.set_has_arrow(false);
    obfuscate_method_popover.set_autohide(true);
    obfuscate_method_popover.add_css_class("editor-popover");
    obfuscate_method_popover.set_parent(&obfuscate_method_button);

    let obfuscate_method_list = GtkBox::new(Orientation::Vertical, 0);
    obfuscate_method_list.add_css_class("editor-popover-list");
    obfuscate_method_popover.set_child(Some(&obfuscate_method_list));

    let p_popover = obfuscate_method_popover.clone();
    obfuscate_method_button.connect_clicked(move |_| {
        p_popover.popup();
    });

    obfuscate_method_group.append(&obfuscate_method_button);

    (
        obfuscate_method_group,
        obfuscate_method_button,
        obfuscate_method_popover,
        obfuscate_method_list,
    )
}
```

- [ ] **Step 3: Update build_toolbar_mode_controls signature**

Add `obfuscate_btn: &Button` parameter and use it to build the method selector inside the function (similar to text_size_button pattern).

- [ ] **Step 4: Populate method list with options**

In `build_toolbar_mode_controls`, after creating the popover, add:
```rust
let icons = [
    ("obfuscate-pixelate", "Pixelate"),
    ("obfuscate-blur-secure", "Blur (Secure)"),
    ("obfuscate-blur-smooth", "Blur (Smooth)"),
    ("obfuscate-blackout", "Blackout"),
];

for (icon_name, label) in icons {
    let btn = GtkBox::new(Orientation::Horizontal, 8);
    btn.add_css_class("editor-popover-list-item");
    
    let icon = Image::from_icon_name(icon_name);
    let label_widget = Label::new(Some(label));
    btn.append(&icon);
    btn.append(&label_widget);
    
    obfuscate_method_list.append(&btn);
}
```

- [ ] **Step 5: Add method selector to toolbar center**

In `build_toolbar_mode_controls`, after `primary_tools_group`, add:
```rust
standard_mode_group.append(&obfuscate_method_group);
```

- [ ] **Step 6: Add icon names to window/mod.rs**

Update the icon names initialization:
```rust
obfuscate_pixelate: "view-grid-symbolic",
obfuscate_blur_secure: "blur-symbolic",
obfuscate_blur_smooth: "blur-on-symbolic",
obfuscate_blackout: "moon-filled-symbolic",
```

- [ ] **Step 7: Run build to verify**

```bash
cargo build 2>&1 | head -100
```

- [ ] **Step 8: Commit**

```bash
git add -A && git commit -m "feat(toolbar): add obfuscate method selector dropdown"
```

---

## Chunk 4: Wire Up Method Selection Handler

**Files:**
- Modify: `src/capture/editor/window/events.rs`
- Modify: `src/capture/editor/window/mod.rs`

- [ ] **Step 1: Add handler for method button click in events.rs**

Find the obfuscate button handler section and add similar handler for method button. The handler should:
1. Update the icon on the button
2. Call state.set_obfuscate_method()
3. Update slider visibility if Blackout selected
4. Rebuild effects if needed

```rust
let state_method = state.clone();
let method_button_clone = obfuscate_method_button.clone();
let slider_clone = size_slider.clone();
let rebuild_async_clone = rebuild_effects_async.clone();

for (index, item) in obfuscate_method_list.iter().enumerate() {
    let method = match index {
        0 => ObfuscateMethod::Pixelate,
        1 => ObfuscateMethod::BlurSecure,
        2 => ObfuscateMethod::BlurSmooth,
        3 => ObfuscateMethod::Blackout,
        _ => continue,
    };
    
    let item = item.clone();
    let state_m = state_method.clone();
    let btn = method_button_clone.clone();
    let slider = slider_clone.clone();
    let rebuild = rebuild_async_clone.clone();
    
    item.add_controller(GestureClick::new());
    item.connect_closure(
        "clicked",
        false,
        Closure::new(move |_| {
            state_m.borrow_mut().set_obfuscate_method(method);
            let icon_name = method.icon_name();
            if let Some(child) = btn.get_child() {
                if let Some(img) = child.downcast_ref::<Image>() {
                    img.set_from_icon_name(Some(icon_name));
                }
            }
            slider.set_visible(method.has_slider());
            rebuild();
        }),
    );
}
```

- [ ] **Step 2: Add visibility control for method selector**

Update toolbar tool updater function to show/hide method selector based on tool:
```rust
let obfuscate_method_visible = matches!(tool, Tool::Obfuscate);
obfuscate_method_group.set_visible(obfuscate_method_visible);
```

- [ ] **Step 3: Run build to verify**

```bash
cargo build 2>&1 | head -100
```

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat(editor): wire up obfuscate method selection handler"
```

---

## Chunk 5: Final Integration

**Files:**
- Modify: `src/capture/editor/window/mod.rs` (update icon_names struct)

- [ ] **Step 1: Update ToolbarBaseIconNames struct**

Add new icon name fields:
```rust
pub obfuscate_pixelate: &'a str,
pub obfuscate_blur_secure: &'a str,
pub obfuscate_blur_smooth: &'a str,
pub obfuscate_blackout: &'a str,
```

- [ ] **Step 2: Update icon_names initialization in EditorWindow**

Find where ToolbarBaseIconNames is created and add:
```rust
obfuscate_pixelate: icon_names.obfuscate_pixelate,
obfuscate_blur_secure: icon_names.obfuscate_blur_secure,
obfuscate_blur_smooth: icon_names.obfuscate_blur_smooth,
obfuscate_blackout: icon_names.obfuscate_blackout,
```

- [ ] **Step 3: Run full build**

```bash
cargo build 2>&1
```

- [ ] **Step 4: Test manually**

Run the application and verify:
1. Click obfuscate tool - method selector appears before slider
2. Click method selector - dropdown shows 4 options with icons
3. Select each method - icon updates, slider shows/hides for Blackout
4. Adjust slider - value changes for current method
5. Select different tool - method selector hides
6. Select obfuscate again - previous method is remembered

- [ ] **Step 5: Final commit**

```bash
git add -A && git commit -m "feat(editor): complete obfuscate method selector integration"
```
