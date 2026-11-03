//! The warning a mode switch raises when work does not follow it across.
//!
//! Static annotations become the snapshot Motion plays, and Motion clips are
//! cleared on the way back to Static. Both switches ask before continuing, the
//! same question the Linux editor puts over its canvas, and each answer can be
//! remembered for that direction only.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, CheckButton, Label, Orientation};

use crate::config::{load_config, save_config, AppConfig};
use crate::i18n::t;

use super::ui_support::px;

/// Which direction of the mode switch is being asked about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ModeSwitch {
    ToMotion,
    ToStatic,
}

impl ModeSwitch {
    fn text(self) -> (&'static str, &'static str) {
        match self {
            Self::ToMotion => (
                "Annotations will be applied",
                "Motion will play a snapshot of your annotations. Returning to Static restores the live tools. The original image stays unchanged unless you enable overwrite in Settings → Annotate.",
            ),
            Self::ToStatic => (
                "Motion effects will be cleared",
                "Motion effect segments can't be edited in Static mode and will be cleared if you continue.",
            ),
        }
    }
}

/// Whether this switch has work to warn about and the user has not silenced
/// that direction's warning.
pub(super) fn needs_confirmation(
    switch: ModeSwitch,
    has_static_annotations: bool,
    has_motion_segments: bool,
    config: &AppConfig,
) -> bool {
    match switch {
        ModeSwitch::ToMotion => has_static_annotations && !config.skip_static_to_motion_confirm,
        ModeSwitch::ToStatic => has_motion_segments && !config.skip_motion_to_static_confirm,
    }
}

/// The action a confirmed switch runs.
type Continue = Rc<dyn Fn()>;
/// The action parked while the card is open; `None` when nothing is pending.
type PendingContinue = Rc<RefCell<Option<Continue>>>;

/// The scrim and card, built once and reused for both directions.
pub(super) struct ModeConfirm {
    scrim: GtkBox,
    title: Label,
    body: Label,
    skip: CheckButton,
    kind: Rc<Cell<ModeSwitch>>,
    on_continue: PendingContinue,
}

impl ModeConfirm {
    /// Ask the question for `switch`; `on_continue` runs only if the user
    /// accepts. A pending answer is replaced, never stacked.
    pub(super) fn ask(&self, switch: ModeSwitch, on_continue: Continue) {
        let (title, body) = switch.text();
        self.title.set_text(&t(title));
        self.body.set_text(&t(body));
        self.kind.set(switch);
        self.skip.set_active(false);
        *self.on_continue.borrow_mut() = Some(on_continue);
        self.scrim.set_visible(true);
    }

    /// Dismiss without continuing. Used by Cancel and Escape.
    pub(super) fn cancel(&self) {
        *self.on_continue.borrow_mut() = None;
        self.scrim.set_visible(false);
    }

    pub(super) fn visible(&self) -> bool {
        self.scrim.is_visible()
    }

    pub(super) fn overlay(&self) -> &GtkBox {
        &self.scrim
    }
}

pub(super) fn build(scale: f64, dark: bool) -> ModeConfirm {
    let scrim = GtkBox::new(Orientation::Vertical, 0);
    scrim.add_css_class("image-editor-confirm-scrim");
    scrim.set_hexpand(true);
    scrim.set_vexpand(true);
    scrim.set_can_target(true);
    scrim.set_visible(false);

    let card = GtkBox::new(Orientation::Vertical, px(12.0, scale));
    card.add_css_class("image-editor-confirm-card");
    card.add_css_class(if dark {
        "editor-theme-dark"
    } else {
        "editor-theme-light"
    });
    card.set_halign(Align::Center);
    card.set_width_request(px(380.0, scale));
    card.set_can_target(true);

    // A vertical box gives the card exactly its natural height at the top, so
    // two equal expanding gaps are what actually place it in the middle.
    let top_gap = GtkBox::new(Orientation::Vertical, 0);
    top_gap.set_vexpand(true);
    let bottom_gap = GtkBox::new(Orientation::Vertical, 0);
    bottom_gap.set_vexpand(true);

    // The card keeps its natural width: the side gaps absorb the rest of the
    // window instead of the vertical box stretching the card edge to edge.
    let row = GtkBox::new(Orientation::Horizontal, 0);
    row.set_hexpand(true);
    let left_gap = GtkBox::new(Orientation::Horizontal, 0);
    left_gap.set_hexpand(true);
    let right_gap = GtkBox::new(Orientation::Horizontal, 0);
    right_gap.set_hexpand(true);
    row.append(&left_gap);
    row.append(&card);
    row.append(&right_gap);

    let title = Label::new(None);
    title.add_css_class("image-editor-confirm-title");
    title.set_xalign(0.0);
    title.set_wrap(true);
    title.set_max_width_chars(44);

    let body = Label::new(None);
    body.add_css_class("image-editor-confirm-body");
    body.set_xalign(0.0);
    body.set_wrap(true);
    body.set_max_width_chars(44);

    let skip = CheckButton::with_label(&t("Don't show this again"));
    skip.add_css_class("image-editor-confirm-skip");

    let buttons = GtkBox::new(Orientation::Horizontal, px(8.0, scale));
    buttons.set_halign(Align::Fill);
    let cancel = Button::with_label(&t("Cancel"));
    cancel.add_css_class("image-editor-confirm-cancel");
    cancel.set_has_frame(false);
    // Cancel stays alone on the left and the primary action hugs the far
    // edge, the shape the Linux confirm card uses.
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    let cont = Button::with_label(&t("Continue"));
    cont.add_css_class("image-editor-confirm-continue");
    cont.set_has_frame(false);
    buttons.append(&cancel);
    buttons.append(&spacer);
    buttons.append(&cont);

    card.append(&title);
    card.append(&body);
    card.append(&skip);
    card.append(&buttons);
    scrim.append(&top_gap);
    scrim.append(&row);
    scrim.append(&bottom_gap);

    let on_continue: PendingContinue = Rc::new(RefCell::new(None));
    let kind = Rc::new(Cell::new(ModeSwitch::ToMotion));

    cancel.connect_clicked({
        let scrim = scrim.clone();
        let on_continue = on_continue.clone();
        move |_| {
            *on_continue.borrow_mut() = None;
            scrim.set_visible(false);
        }
    });
    cont.connect_clicked({
        let scrim = scrim.clone();
        let skip = skip.clone();
        let kind = kind.clone();
        let on_continue = on_continue.clone();
        move |_| {
            if skip.is_active() {
                let mut config = load_config();
                match kind.get() {
                    ModeSwitch::ToMotion => config.skip_static_to_motion_confirm = true,
                    ModeSwitch::ToStatic => config.skip_motion_to_static_confirm = true,
                }
                let _ = save_config(&config);
            }
            scrim.set_visible(false);
            let callback = on_continue.borrow_mut().take();
            if let Some(callback) = callback {
                callback();
            }
        }
    });

    ModeConfirm {
        scrim,
        title,
        body,
        skip,
        kind,
        on_continue,
    }
}

#[cfg(test)]
mod tests {
    use super::{needs_confirmation, ModeSwitch};
    use crate::config::AppConfig;
    use gtk4::prelude::*;

    #[test]
    fn static_annotations_ask_before_motion() {
        let mut config = AppConfig::default();
        assert!(needs_confirmation(
            ModeSwitch::ToMotion,
            true,
            false,
            &config
        ));
        assert!(!needs_confirmation(
            ModeSwitch::ToMotion,
            false,
            false,
            &config
        ));
        config.skip_static_to_motion_confirm = true;
        assert!(!needs_confirmation(
            ModeSwitch::ToMotion,
            true,
            false,
            &config
        ));
    }

    #[test]
    fn motion_clips_ask_before_static() {
        let mut config = AppConfig::default();
        assert!(needs_confirmation(
            ModeSwitch::ToStatic,
            false,
            true,
            &config
        ));
        assert!(!needs_confirmation(
            ModeSwitch::ToStatic,
            false,
            false,
            &config
        ));
        config.skip_motion_to_static_confirm = true;
        assert!(!needs_confirmation(
            ModeSwitch::ToStatic,
            false,
            true,
            &config
        ));
    }

    #[test]
    fn the_card_keeps_a_narrow_natural_width() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let confirm = super::build(1.0, true);
            confirm.ask(ModeSwitch::ToMotion, std::rc::Rc::new(|| {}));
            let row = confirm
                .overlay()
                .first_child()
                .unwrap()
                .next_sibling()
                .unwrap();
            let card = row.first_child().unwrap().next_sibling().unwrap();
            let (_, natural, _, _) = card.measure(gtk4::Orientation::Horizontal, -1);
            assert!(natural < 600, "card natural width is {natural}px");
            confirm.overlay().allocate(1200, 800, -1, None);
            let allocated = card.width();
            assert!(
                allocated < 700,
                "card allocated {allocated}px inside a 1200px scrim"
            );
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }
}
