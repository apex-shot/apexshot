//! Row toggles.
//!
//! The settings pages are shared with the Linux window and build check
//! buttons. The Windows design uses switches, so after the pages are built the
//! shell swaps every row-level check button for a switch bound to it: the check
//! button keeps the state the pages and the saver read, and the switch is what
//! the person sees and clicks.

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, CheckButton, Label, Switch};

/// Swap every row-level check button under `root` for a switch.
///
/// A check button qualifies when its box also holds the row's text — a label,
/// or a column of labels. Checks used as matrix cells, as a labelled
/// sub-option, or beside another control keep their checkbox look.
pub fn install(root: &impl IsA<gtk4::Widget>) {
    let mut checks = Vec::new();
    collect(root.as_ref(), &mut checks);
    for check in checks {
        upgrade(&check);
    }
}

fn collect(widget: &gtk4::Widget, out: &mut Vec<CheckButton>) {
    let mut child = widget.first_child();
    while let Some(node) = child {
        if let Some(check) = node.downcast_ref::<CheckButton>() {
            out.push(check.clone());
        }
        collect(&node, out);
        child = node.next_sibling();
    }
}

fn upgrade(check: &CheckButton) {
    if inside_grouped_control(check) {
        return;
    }
    let Some(parent) = check.parent().and_downcast::<GtkBox>() else {
        return;
    };
    let Some(text_column) = row_text_column(&parent, check) else {
        return;
    };

    let switch = Switch::new();
    switch.add_css_class("settings-switch");
    switch.set_halign(Align::End);
    switch.set_valign(Align::Center);
    switch.set_active(check.is_active());
    switch.set_sensitive(check.is_sensitive());
    if let Some(text) = check.tooltip_text() {
        switch.set_tooltip_text(Some(&text));
    }

    // The check button keeps the state; taking it out of the layout leaves the
    // switch as the only thing the row shows next to its text.
    check.set_visible(false);
    parent.insert_child_after(&switch, Some(check));

    // The switch belongs at the row's trailing edge, so the text takes the
    // space the hidden check left behind.
    if !text_column.hexpands() {
        text_column.set_hexpand(true);
    }

    let check_for_switch = check.clone();
    switch.connect_notify_local(Some("active"), move |switch, _| {
        if check_for_switch.is_active() != switch.is_active() {
            check_for_switch.set_active(switch.is_active());
        }
    });

    let switch_weak = switch.downgrade();
    check.connect_toggled(move |check| {
        if let Some(switch) = switch_weak.upgrade() {
            if switch.is_active() != check.is_active() {
                switch.set_active(check.is_active());
            }
        }
    });

    let switch_weak = switch.downgrade();
    check.connect_notify_local(Some("sensitive"), move |check, _| {
        if let Some(switch) = switch_weak.upgrade() {
            switch.set_sensitive(check.is_sensitive());
        }
    });
}

/// True when the check belongs to a grouped control that draws its own
/// options — the three-way theme picker, for one — and must keep its checkbox
/// shape.
fn inside_grouped_control(check: &CheckButton) -> bool {
    let mut ancestor = check.parent();
    while let Some(widget) = ancestor {
        if widget.has_css_class("settings-theme-picker") {
            return true;
        }
        ancestor = widget.parent();
    }
    false
}

/// The sibling that carries the row's text, if this check is a row toggle.
fn row_text_column(parent: &GtkBox, check: &CheckButton) -> Option<gtk4::Widget> {
    let check_ptr = check.as_ptr() as usize;
    let mut child = parent.first_child();
    while let Some(node) = child {
        if node.as_ptr() as usize != check_ptr && carries_text(&node) {
            return Some(node);
        }
        child = node.next_sibling();
    }
    None
}

/// True when `widget` is text, or a column of text, rather than another
/// control (a labelled sub-option's own check button contains a label too, and
/// must not be mistaken for the row's title).
fn carries_text(widget: &gtk4::Widget) -> bool {
    // GTK4 dresses a check button without a Button base, so it has to be
    // named: a labelled sub-option is a control, not the row's title.
    if widget.is::<CheckButton>()
        || widget.is::<gtk4::Button>()
        || widget.is::<Switch>()
        || widget.is::<gtk4::Entry>()
    {
        return false;
    }
    if widget.is::<Label>() {
        return true;
    }
    let mut child = widget.first_child();
    while let Some(node) = child {
        if carries_text(&node) {
            return true;
        }
        child = node.next_sibling();
    }
    false
}
