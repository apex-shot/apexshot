//! Inline custom annotation color editor, using the shared HSV picker.

use gtk4::cairo::{Context, LinearGradient};
use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, DrawingArea, Label, Orientation, Overlay, Revealer, Scale,
};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use super::ui_support::px;
use crate::capture::editor::color::draw_color_to_rgba_u8;
use crate::capture::editor::types::DrawColor;
use crate::i18n::t;
use crate::video_editor::custom_fill::build_color_picker;

type RepaintSlot = Rc<RefCell<Option<Weak<dyn Fn()>>>>;

fn draw_opacity_track(cr: &Context, width: f64, height: f64, color: DrawColor, scale: f64) {
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    let radius = height.min(width) / 2.0;
    let _ = cr.save();
    cr.new_sub_path();
    cr.arc(
        width - radius,
        radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        std::f64::consts::FRAC_PI_2,
    );
    cr.arc(
        radius,
        radius,
        radius,
        std::f64::consts::FRAC_PI_2,
        3.0 * std::f64::consts::FRAC_PI_2,
    );
    cr.close_path();
    cr.clip();
    cr.set_source_rgb(0.80, 0.80, 0.80);
    let _ = cr.paint();
    let tile = (5.0 * scale).max(1.0);
    cr.set_source_rgb(0.58, 0.58, 0.58);
    for row in 0..(height / tile).ceil() as usize {
        for column in 0..(width / tile).ceil() as usize {
            if (row + column) % 2 == 0 {
                cr.rectangle(column as f64 * tile, row as f64 * tile, tile, tile);
            }
        }
    }
    let _ = cr.fill();
    let gradient = LinearGradient::new(0.0, 0.0, width, 0.0);
    gradient.add_color_stop_rgba(0.0, color.r, color.g, color.b, 0.0);
    gradient.add_color_stop_rgba(1.0, color.r, color.g, color.b, 1.0);
    let _ = cr.set_source(&gradient);
    let _ = cr.paint();
    let _ = cr.restore();
}

pub(super) struct CustomColor {
    pub panel: Revealer,
    draft: Rc<Cell<DrawColor>>,
    repaint: Rc<dyn Fn()>,
    opacity: Scale,
    opacity_track: DrawingArea,
    syncing: Rc<Cell<bool>>,
}

impl CustomColor {
    pub fn new(scale: f64, apply: Rc<dyn Fn(DrawColor)>) -> Self {
        let draft = Rc::new(Cell::new(DrawColor::new(0.0, 0.0, 0.0, 1.0)));
        let syncing = Rc::new(Cell::new(false));
        let panel = Revealer::new();
        panel.set_transition_type(gtk4::RevealerTransitionType::SlideDown);
        panel.set_transition_duration(180);
        panel.set_halign(Align::Center);

        let card = GtkBox::new(Orientation::Vertical, 0);
        card.add_css_class("image-editor-custom-color");
        card.set_size_request(px(264.0, scale), -1);
        let header = GtkBox::new(Orientation::Horizontal, 0);
        header.add_css_class("image-editor-custom-color-header");
        let title = Label::new(Some(&t("Custom color")));
        title.add_css_class("image-editor-custom-color-title");
        title.set_xalign(0.0);
        title.set_hexpand(true);
        header.append(&title);
        let close = Button::new();
        close.add_css_class("image-editor-custom-color-close");
        close.set_tooltip_text(Some(&t("Close custom color")));
        close.update_property(&[gtk4::accessible::Property::Label(&t("Close custom color"))]);
        let close_icon = gtk4::Image::from_icon_name("window-close-symbolic");
        close_icon.set_pixel_size(px(14.0, scale));
        close.set_child(Some(&close_icon));
        close.connect_clicked({
            let panel = panel.downgrade();
            move |_| {
                if let Some(panel) = panel.upgrade() {
                    panel.set_reveal_child(false);
                }
            }
        });
        header.append(&close);
        card.append(&header);

        let opacity_track = DrawingArea::new();
        opacity_track.set_hexpand(true);
        opacity_track.set_valign(Align::Center);
        opacity_track.set_content_width(px(80.0, scale));
        opacity_track.set_content_height(px(10.0, scale));
        opacity_track.set_margin_start(px(8.0, scale));
        opacity_track.set_margin_end(px(8.0, scale));
        opacity_track.set_can_target(false);
        opacity_track.set_draw_func({
            let draft = draft.clone();
            move |_, cr, width, height| {
                draw_opacity_track(cr, f64::from(width), f64::from(height), draft.get(), scale);
            }
        });

        let repaint_slot: RepaintSlot = Rc::new(RefCell::new(None));
        let picker = build_color_picker(
            {
                let draft = draft.clone();
                Rc::new(move || {
                    let (r, g, b, _) = draw_color_to_rgba_u8(draft.get());
                    (r, g, b)
                })
            },
            {
                let draft = draft.clone();
                Rc::new(move |(r, g, b)| {
                    let mut color = draft.get();
                    color.r = f64::from(r) / 255.0;
                    color.g = f64::from(g) / 255.0;
                    color.b = f64::from(b) / 255.0;
                    draft.set(color);
                })
            },
            {
                let repaint_slot = repaint_slot.clone();
                let draft = draft.clone();
                let apply = apply.clone();
                let opacity_track = opacity_track.clone();
                Rc::new(move || {
                    if let Some(repaint) = repaint_slot.borrow().as_ref().and_then(Weak::upgrade) {
                        repaint();
                    }
                    opacity_track.queue_draw();
                    apply(draft.get());
                })
            },
            scale,
        );
        *repaint_slot.borrow_mut() = Some(Rc::downgrade(&picker.repaint));
        card.append(&picker.widget);

        let opacity_row = GtkBox::new(Orientation::Horizontal, px(8.0, scale));
        opacity_row.add_css_class("image-editor-custom-opacity");
        let label = Label::new(Some(&t("Opacity")));
        label.add_css_class("image-editor-custom-opacity-label");
        let opacity = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
        opacity.add_css_class("image-editor-opacity-slider");
        opacity.set_hexpand(true);
        opacity.set_valign(Align::Center);
        opacity.set_draw_value(false);
        opacity.set_tooltip_text(Some(&t("Opacity")));
        opacity.update_property(&[gtk4::accessible::Property::Label(&t("Opacity"))]);
        let percent = Label::new(Some("100%"));
        percent.add_css_class("image-editor-custom-opacity-value");
        opacity.connect_value_changed({
            let draft = draft.clone();
            let syncing = syncing.clone();
            let percent = percent.clone();
            let apply = apply.clone();
            move |opacity| {
                let value = opacity.value().round().clamp(0.0, 100.0);
                percent.set_text(&format!("{value:.0}%"));
                if !syncing.get() {
                    let mut color = draft.get();
                    color.a = value / 100.0;
                    draft.set(color);
                    apply(color);
                }
            }
        });
        let opacity_overlay = Overlay::new();
        opacity_overlay.set_hexpand(true);
        opacity_overlay.set_valign(Align::Center);
        opacity_overlay.set_child(Some(&opacity_track));
        opacity_overlay.add_overlay(&opacity);
        opacity_overlay.set_measure_overlay(&opacity, true);
        opacity_row.append(&label);
        opacity_row.append(&opacity_overlay);
        opacity_row.append(&percent);
        card.append(&opacity_row);
        panel.set_child(Some(&card));

        Self {
            panel,
            draft,
            repaint: picker.repaint,
            opacity,
            opacity_track,
            syncing,
        }
    }

    pub fn open(&self, color: DrawColor) {
        self.draft.set(color);
        (self.repaint)();
        self.opacity_track.queue_draw();
        self.syncing.set(true);
        self.opacity.set_value(color.a.clamp(0.0, 1.0) * 100.0);
        self.syncing.set(false);
        self.panel.set_reveal_child(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk4::Widget;

    fn find_class(widget: &Widget, class: &str) -> Option<Widget> {
        if widget.has_css_class(class) {
            return Some(widget.clone());
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            if let Some(found) = find_class(&current, class) {
                return Some(found);
            }
            child = current.next_sibling();
        }
        None
    }

    #[test]
    fn custom_color_changes_are_live_and_preserve_opacity() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let applied = Rc::new(RefCell::new(Vec::new()));
            let editor = CustomColor::new(1.0, {
                let applied = applied.clone();
                Rc::new(move |color| applied.borrow_mut().push(color))
            });
            let initial = DrawColor::new(1.0, 0.0, 0.0, 0.6);
            editor.open(initial);
            assert!(applied.borrow().is_empty());
            let entry = find_class(editor.panel.upcast_ref(), "recording-editor-custom-hex")
                .unwrap()
                .downcast::<gtk4::Entry>()
                .unwrap();
            entry.set_text("#12AB34");
            entry.emit_activate();
            assert_eq!(
                draw_color_to_rgba_u8(applied.borrow()[0]),
                (18, 171, 52, 153)
            );
            editor.opacity.set_value(35.0);
            assert_eq!(draw_color_to_rgba_u8(editor.draft.get()), (18, 171, 52, 89));
            assert_eq!(
                draw_color_to_rgba_u8(*applied.borrow().last().unwrap()),
                (18, 171, 52, 89)
            );

            let field =
                find_class(editor.panel.upcast_ref(), "recording-editor-custom-field").unwrap();
            let controllers = field.observe_controllers();
            let gesture = (0..controllers.n_items())
                .find_map(|index| {
                    controllers
                        .item(index)?
                        .downcast::<gtk4::GestureDrag>()
                        .ok()
                })
                .unwrap();
            let before_drag = applied.borrow().len();
            gesture.emit_by_name::<()>("drag-begin", &[&0.0f64, &0.0f64]);
            assert!(applied.borrow().len() > before_drag);
            assert_eq!(applied.borrow().last().unwrap().a, 0.35);

            let close = find_class(editor.panel.upcast_ref(), "image-editor-custom-color-close")
                .unwrap()
                .downcast::<Button>()
                .unwrap();
            let count = applied.borrow().len();
            close.emit_clicked();
            assert_eq!(applied.borrow().len(), count);
            assert!(!editor.panel.reveals_child());
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn opacity_track_fades_from_checkerboard_to_selected_color() {
        let mut surface =
            gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 100, 12).unwrap();
        let context = Context::new(&surface).unwrap();
        draw_opacity_track(
            &context,
            100.0,
            12.0,
            DrawColor::new(1.0, 0.0, 0.0, 1.0),
            1.0,
        );
        drop(context);
        let stride = surface.stride() as usize;
        let pixels = surface.data().unwrap();
        let pixel = |x: usize| {
            let i = 6 * stride + x * 4;
            (pixels[i + 2], pixels[i + 1], pixels[i])
        };
        let (left_r, left_g, left_b) = pixel(4);
        let (right_r, right_g, right_b) = pixel(95);
        assert!((i16::from(left_r) - i16::from(left_g)).abs() < 20);
        assert_eq!(left_g, left_b);
        assert!(right_r > right_g * 2);
        assert_eq!(right_g, right_b);
    }
}
