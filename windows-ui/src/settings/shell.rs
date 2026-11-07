//! The Windows Settings shell.
//!
//! A searchable sidebar with an account chip at its foot, and the inset card
//! the pages scroll inside. The pages themselves stay shared with the Linux
//! window; this module only owns the frame around them.

use std::cell::Cell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{
    gdk, Align, Box as GtkBox, Button, CssProvider, Image, Label, Orientation, PolicyType,
    Revealer, ScrolledWindow, Stack, Widget,
};

use super::usage::capitalize;
use crate::capture::editor::window::icon_names;
use crate::config::{is_cloud_logged_in, load_config, AppConfig};
use crate::i18n::t;

/// Sidebar width at 100% scale.
pub const SIDEBAR_WIDTH: f64 = 300.0;

/// Distance the content card keeps from the window edge at 100% scale.
pub(crate) const CARD_INSET: f64 = 8.0;

#[derive(Clone)]
struct NavItem {
    row: GtkBox,
    icon: Image,
    label: Label,
}

/// The pieces of the sidebar the window needs: the column to place in the
/// layout, and the draggable head that doubles as the window's title.
pub struct Sidebar {
    pub wrapper: GtkBox,
    pub head: GtkBox,
}

/// Build the sidebar for `entries` — a translated title and an icon name per
/// page — and wire its rows to `stack`.
pub fn build_sidebar(
    scale: f64,
    title_bar_height: f64,
    entries: &[(String, &str)],
    stack: &Stack,
) -> Sidebar {
    let inset = (CARD_INSET * scale).round() as i32;

    let wrapper = GtkBox::new(Orientation::Vertical, 0);
    wrapper.add_css_class("settings-sidebar-wrapper");
    wrapper.set_vexpand(true);
    wrapper.set_hexpand(false);
    // The column carries the gutter the content card is inset by, so the user
    // bar's divider runs all the way to the card's border; everything inside
    // it keeps the gutter out with its own end inset.
    wrapper.set_size_request((SIDEBAR_WIDTH * scale).round() as i32 + inset, -1);

    // The strip the window is dragged by. The sidebar carries no title: the
    // window chrome and the rows already say where you are.
    let head = GtkBox::new(Orientation::Horizontal, 0);
    head.add_css_class("settings-sidebar-head");
    head.set_size_request(-1, (title_bar_height * scale).round() as i32);

    let scroller = ScrolledWindow::new();
    scroller.set_policy(PolicyType::Never, PolicyType::Automatic);
    scroller.set_vexpand(true);

    let nav_strip = GtkBox::new(Orientation::Vertical, 0);
    nav_strip.add_css_class("settings-sidebar");
    nav_strip.set_halign(Align::Fill);
    nav_strip.set_valign(Align::Fill);
    nav_strip.set_hexpand(false);
    nav_strip.set_vexpand(true);
    nav_strip.set_margin_end(inset);
    scroller.set_child(Some(&nav_strip));

    let mut items = Vec::with_capacity(entries.len());

    for (index, (label_text, icon_name)) in entries.iter().enumerate() {
        let row = GtkBox::new(Orientation::Horizontal, 10);
        row.add_css_class("settings-nav-item");
        row.set_halign(Align::Fill);

        let icon = Image::from_icon_name(icon_name);
        icon.add_css_class("settings-nav-icon");
        icon.set_pixel_size((16.0 * scale).round() as i32);
        icon.set_halign(Align::Start);
        icon.set_valign(Align::Center);

        let label = Label::new(Some(label_text));
        label.add_css_class("settings-nav-label");
        label.set_halign(Align::Start);
        label.set_valign(Align::Center);
        label.set_yalign(0.5);
        label.set_hexpand(true);

        // Both children are centred in the row, so the glyph and the text share
        // one horizontal axis instead of the glyph riding the row's top edge.
        row.set_valign(Align::Center);

        row.append(&icon);
        row.append(&label);

        let motion = gtk4::EventControllerMotion::new();
        {
            let row = row.clone();
            motion.connect_enter(move |_, _, _| row.add_css_class("settings-nav-item-hover"));
        }
        {
            let row = row.clone();
            motion.connect_leave(move |_| row.remove_css_class("settings-nav-item-hover"));
        }
        row.add_controller(motion);

        let page = index.to_string();
        let stack_for_click = stack.clone();
        let click = gtk4::GestureClick::new();
        click.connect_released(move |_, _, _, _| stack_for_click.set_visible_child_name(&page));
        row.add_controller(click);

        nav_strip.append(&row);
        items.push(NavItem { row, icon, label });
    }

    let items = Rc::new(items);

    // Selection follows the stack, so a page set from anywhere — a nav click,
    // the account chip, the page-override env var — highlights the same row.
    {
        let items = Rc::clone(&items);
        stack.connect_visible_child_name_notify(move |stack| {
            let Some(name) = stack.visible_child_name() else {
                return;
            };
            let Ok(index) = name.parse::<usize>() else {
                return;
            };
            if let Some(child) = stack.visible_child() {
                if let Some(scroller) = child.downcast_ref::<ScrolledWindow>() {
                    let adjustment = scroller.vadjustment();
                    adjustment.set_value(adjustment.lower());
                }
            }
            for (i, item) in items.iter().enumerate() {
                set_selected(&item.row, &item.icon, &item.label, i == index);
            }
        });
    }

    wrapper.append(&head);
    wrapper.append(&scroller);

    Sidebar { wrapper, head }
}

/// The sidebar toggle and the Beta badge that sit in the window's top-left
/// corner.
pub struct ShellControls {
    pub row: GtkBox,
    pub toggle: Button,
}

/// Build the controls above the navigation rows: the button that hides the
/// column and the badge that marks the build.
pub fn build_shell_controls() -> ShellControls {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.add_css_class("settings-shell-controls");
    row.set_halign(Align::Start);
    row.set_valign(Align::Center);

    let toggle = Button::from_icon_name("sidebar-show-symbolic");
    toggle.add_css_class("settings-sidebar-toggle");
    toggle.set_valign(Align::Center);
    toggle.set_tooltip_text(Some(&t("Hide sidebar")));

    let badge = Label::new(Some(&t("Beta")));
    badge.add_css_class("settings-beta-badge");
    badge.set_valign(Align::Center);

    row.append(&toggle);
    row.append(&badge);
    ShellControls { row, toggle }
}

/// Hide or show the sidebar from its own toggle. The revealer slides the
/// column out and back; the controls ride in the sidebar's head while the
/// column is open and move into the title bar while it is closed, so the
/// button never leaves with the column it hides. The open sidebar supplies
/// the content card's left gutter; with it closed the card takes that gutter
/// itself, so no edge of the card reaches the window border.
pub fn wire_sidebar_toggle(
    controls: &ShellControls,
    sidebar: &Revealer,
    head: &GtkBox,
    title_bar: &GtkBox,
    card: &GtkBox,
    scale: f64,
) {
    let row = controls.row.clone();
    let sidebar = sidebar.clone();
    let head = head.clone();
    let title_bar = title_bar.clone();
    let card = card.clone();
    let inset = (CARD_INSET * scale).round() as i32;
    let hidden = Cell::new(false);
    controls.toggle.connect_clicked(move |button| {
        let hide = !hidden.get();
        hidden.set(hide);
        row.unparent();
        if hide {
            title_bar.prepend(&row);
            button.set_tooltip_text(Some(&t("Show sidebar")));
        } else {
            head.prepend(&row);
            button.set_tooltip_text(Some(&t("Hide sidebar")));
        }
        card.set_margin_start(if hide { inset } else { 0 });
        sidebar.set_reveal_child(!hide);
    });
}

fn set_selected(row: &GtkBox, icon: &Image, label: &Label, selected: bool) {
    if selected {
        row.add_css_class("settings-nav-item-selected");
        icon.add_css_class("settings-nav-icon-selected");
        label.add_css_class("settings-nav-label-selected");
    } else {
        row.remove_css_class("settings-nav-item-selected");
        icon.remove_css_class("settings-nav-icon-selected");
        label.remove_css_class("settings-nav-label-selected");
    }
}

/// The cloud account shown at the bottom of the sidebar.
pub struct UserChip {
    pub name: String,
    pub subtitle: String,
    pub badge: String,
    pub initials: String,
}

impl UserChip {
    pub fn from_config(config: &AppConfig) -> Option<Self> {
        if !is_cloud_logged_in(config) {
            return None;
        }

        let email = config.cloud_user_email.trim();
        let account_name = config.cloud_user_name.trim();
        let name = if !account_name.is_empty() {
            account_name.to_string()
        } else if !email.is_empty() {
            email.to_string()
        } else {
            t("ApexShot Cloud")
        };
        // The chip represents the Cloud account, so its badge reports that
        // account's plan tier. Studio is a separate, device-bound licence with
        // its own Settings page; showing its status here made paid Cloud
        // accounts read as "Free". An unknown tier earns no badge rather than
        // claiming a plan the app has not seen.
        let tier = config.cloud_plan_tier.trim();
        let badge = if tier.is_empty() {
            String::new()
        } else {
            capitalize(tier)
        };
        let subtitle = if !email.is_empty() && name != email {
            email.to_string()
        } else {
            t("Connected")
        };
        Some(Self {
            initials: initials_of(&name),
            name,
            subtitle,
            badge,
        })
    }
}

fn initials_of(name: &str) -> String {
    let mut initials = String::new();
    let name = name.split('@').next().unwrap_or(name);
    for word in name.split(|ch: char| ch.is_whitespace() || matches!(ch, '.' | '_' | '-')) {
        if let Some(first) = word.chars().next() {
            initials.extend(first.to_uppercase());
            if initials.chars().count() == 2 {
                break;
            }
        }
    }
    if initials.is_empty() {
        initials.push('A');
    }
    initials
}

#[derive(Clone)]
struct UserChipLabels {
    initials: Label,
    name: Label,
    subtitle: Label,
    badge: Label,
}

impl UserChipLabels {
    fn update(&self, bar: &GtkBox, config: &AppConfig) {
        let Some(chip) = UserChip::from_config(config) else {
            bar.set_visible(false);
            return;
        };
        self.initials.set_text(&chip.initials);
        self.name.set_text(&chip.name);
        self.subtitle.set_text(&chip.subtitle);
        self.badge.set_text(&chip.badge);
        self.badge.set_visible(!chip.badge.is_empty());
        bar.set_visible(true);
    }
}

/// Show the account chip only for a real cloud session, updating after login
/// or logout without requiring the Settings window to be reopened.
pub fn build_user_chip(config: &AppConfig, scale: f64, on_click: impl Fn() + 'static) -> GtkBox {
    let bar = GtkBox::new(Orientation::Vertical, 0);
    bar.add_css_class("settings-user-bar");

    let row = GtkBox::new(Orientation::Horizontal, 9);
    row.add_css_class("settings-user-chip");
    row.set_valign(Align::Center);
    row.set_margin_end((CARD_INSET * scale).round() as i32);

    let avatar = GtkBox::new(Orientation::Horizontal, 0);
    avatar.add_css_class("settings-user-avatar");
    avatar.set_valign(Align::Center);
    avatar.set_halign(Align::Center);
    avatar.set_size_request((30.0 * scale).round() as i32, (30.0 * scale).round() as i32);
    let initials = Label::new(None);
    initials.add_css_class("settings-user-initials");
    initials.set_hexpand(true);
    initials.set_halign(Align::Fill);
    initials.set_xalign(0.5);
    initials.set_valign(Align::Center);
    avatar.append(&initials);

    let text = GtkBox::new(Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.set_valign(Align::Center);

    let top = GtkBox::new(Orientation::Horizontal, 6);
    top.set_valign(Align::Center);
    let name = Label::new(None);
    name.add_css_class("settings-user-name");
    name.set_xalign(0.0);
    name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    top.append(&name);
    let badge = Label::new(None);
    badge.add_css_class("settings-user-badge");
    badge.set_valign(Align::Center);
    top.append(&badge);

    let subtitle = Label::new(None);
    subtitle.add_css_class("settings-user-sub");
    subtitle.set_xalign(0.0);
    subtitle.set_ellipsize(gtk4::pango::EllipsizeMode::End);

    text.append(&top);
    text.append(&subtitle);

    let chevron = Image::from_icon_name(icon_names::CHEVRON_RIGHT_REGULAR);
    chevron.add_css_class("settings-user-chevron");
    chevron.set_pixel_size((12.0 * scale).round() as i32);
    chevron.set_valign(Align::Center);

    row.append(&avatar);
    row.append(&text);
    row.append(&chevron);

    let click = gtk4::GestureClick::new();
    click.connect_released(move |_, _, _, _| on_click());
    row.add_controller(click);

    bar.append(&row);
    let labels = UserChipLabels {
        initials,
        name,
        subtitle,
        badge,
    };
    labels.update(&bar, config);
    let weak_bar = bar.downgrade();
    gtk4::glib::timeout_add_seconds_local(2, move || {
        let Some(bar) = weak_bar.upgrade() else {
            return gtk4::glib::ControlFlow::Break;
        };
        labels.update(&bar, &load_config());
        gtk4::glib::ControlFlow::Continue
    });
    bar
}

/// Wrap the page heading and stack in the rounded card the pages scroll
/// inside.
pub fn build_content_card(stack: &Stack, scale: f64, heading: &impl IsA<Widget>) -> GtkBox {
    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("settings-content-card");
    card.set_vexpand(true);
    card.set_hexpand(true);
    let inset = (CARD_INSET * scale).round() as i32;
    card.set_margin_end(inset);
    card.set_margin_top(inset);
    card.set_margin_bottom(inset);
    card.append(heading);
    card.append(stack);
    card
}

/// DPI-scaled shell metrics, layered over the shared stylesheet.
pub fn install_css() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let scale = crate::platform::ui_scale().min(1.0);
    let px = |value: f64| (value * scale).round() as i32;
    // The shortcuts page groups its rows under an icon + title row. This
    // provider outranks the shared stylesheet, so the block spacing those
    // headings need has to live here: the row carries it and the title keeps
    // no margins of its own, which is what puts the title's centre on the
    // icon's centre.
    let heading_gap = px(28.0);
    let heading_gap_bottom = px(10.0);

    let mut css = format!(
        "\
         .editor-root {{ background-color:#0f0f0f; font-family:'DM Sans','Noto Sans',system-ui,sans-serif; }}          .editor-root.editor-theme-light {{ background-color:#ffffff; }}          .settings-window-controls {{ background-color:transparent; border-bottom:none; }} \n         .editor-root.editor-theme-light .settings-window-controls {{ background-color:transparent; }} \n         .settings-sidebar-wrapper {{ background-color:transparent; border-right:none; min-width:0; }} \
         .settings-sidebar-head {{ padding-left:{}px; }} \
         .settings-search-row {{ padding:0 {}px {}px {}px; }} \
         .editor-root entry.settings-search {{ min-height:{}px; border-radius:{}px; background-color:rgba(255,255,255,0.055); padding:0 {}px; font-size:{}px; }} \
         .editor-root entry.settings-search:focus {{ background-color:rgba(255,255,255,0.075); outline:none; }} \
         .settings-sidebar {{ padding:{}px {}px {}px {}px; }} \
         .settings-nav-item {{ min-height:{}px; padding:{}px 10px; border-radius:{}px; margin:0 0 {}px 0; background-image:none; }} \
         .settings-nav-item-hover {{ background-color:rgba(255,255,255,0.05); }} \
         .settings-nav-item-selected {{ background-color:rgba(255,255,255,0.075); }} \
         .settings-nav-item-hover.settings-nav-item-selected {{ background-color:rgba(255,255,255,0.08); }} \
         .settings-nav-icon {{ -gtk-icon-size:{}px; opacity:0.9; }} \
         .settings-nav-icon-selected {{ opacity:1; color:#ffffff; }} \
         .settings-nav-label {{ font-size:{}px; font-weight:500; opacity:0.88; }} \
         .settings-nav-label-selected {{ opacity:1; color:#ffffff; }} \
         .settings-nav-item-hover .settings-nav-label {{ opacity:0.96; }} \
         .settings-search-empty {{ font-size:{}px; opacity:0.45; padding:{}px 10px; }} \
         .settings-user-bar {{ border-top:1px solid rgba(255,255,255,0.07); padding:{}px; }} \
         .settings-user-chip {{ padding:{}px {}px; border-radius:{}px; }} \
         .settings-user-chip:hover {{ background-color:rgba(255,255,255,0.06); }} \
         .settings-user-avatar {{ border-radius:999px; background-image:linear-gradient(145deg, #965032, #532d28); border:1px solid rgba(255,166,112,0.26); }} \
         .settings-user-initials {{ font-size:{}px; font-weight:700; color:#fff3eb; }} \
         .settings-user-name {{ font-size:{}px; font-weight:600; }} \
         .settings-user-badge {{ font-size:{}px; font-weight:700; padding:1px {}px; border-radius:999px; \
             background-color:rgba(255,255,255,0.10); color:rgba(255,255,255,0.72); }} \
         .settings-user-sub {{ font-size:{}px; opacity:0.46; }} \
         .settings-user-chevron {{ opacity:0.32; }} \
         .settings-content-card {{ border-radius:{}px; border:1px solid rgba(255,255,255,0.06); background-color:rgba(255,255,255,0.028); }} \
         .settings-group-title {{ font-size:{}px; font-weight:600; letter-spacing:0; opacity:0.92; margin-top:28px; margin-bottom:10px; }} \
         .shortcut-heading-windows {{ margin-top:{heading_gap}px; margin-bottom:{heading_gap_bottom}px; }} \
         .shortcut-heading-windows .settings-group-title {{ margin-top:0; margin-bottom:0; }} \
         .settings-table-frame {{ border-radius:{}px; background-color:rgba(255,255,255,0.045); }} \
         .settings-table-frame > .settings-table-row {{ border-bottom:1px solid rgba(255,255,255,0.055); }} \
         .settings-table-frame > .settings-table-row:last-child {{ border-bottom:none; }} \
         .settings-table-row {{ padding:{}px {}px; }} \
         .settings-table-row label {{ font-weight:600; font-size:{}px; }} \
         .settings-table-row label.dim-label {{ font-weight:400; font-size:{}px; opacity:0.5; }} \
         .settings-table-row label.settings-sub-option {{ font-weight:400; }} \
         .settings-table-row label.settings-sub-option-hint {{ font-weight:400; opacity:0.5; }} \
         .settings-table-row button label {{ font-weight:500; font-size:{}px; }} \
         switch {{ min-width:30px; min-height:16px; padding:2px; border-radius:999px; border:none; \
             box-shadow:none; background-image:none; background-color:rgba(255,255,255,0.16); }} \
         switch:checked {{ background-color:#ff670d; }} \
         switch > slider {{ min-width:16px; min-height:16px; margin:0; border-radius:999px; border:none; \
             background-image:none; background-color:#ffffff; box-shadow:none; }} \
         switch > image {{ opacity:0; }} \
         switch:disabled {{ background-color:rgba(255,255,255,0.07); }} \
         switch:disabled > slider {{ background-color:rgba(255,255,255,0.45); }} \
         .editor-root button.settings-select {{ min-width:{}px; min-height:{}px; border-radius:{}px; font-size:{}px;              border:1px solid rgba(255,255,255,0.10); background-color:rgba(255,255,255,0.03); padding:0 {}px; }}
         .editor-root button.settings-select:focus, .editor-root button.settings-select:focus-visible {{ outline:none; box-shadow:none; }}          .editor-root button.settings-select:hover {{ background-color:rgba(255,255,255,0.055); border-color:rgba(255,255,255,0.16); }}
         .editor-root button.settings-select:active, .editor-root button.settings-select:checked {{ background-color:rgba(255,255,255,0.07); border-color:rgba(255,255,255,0.18); }}
         .editor-root button.settings-select:disabled {{ background-color:rgba(255,255,255,0.015); border-color:rgba(255,255,255,0.06); color:rgba(255,255,255,0.38); }}
         .editor-root button.settings-select .settings-select-arrow {{ opacity:0.5; }}
         .editor-root.editor-theme-light button.settings-select {{ border-color:rgba(17,24,39,0.14); background-color:rgba(17,24,39,0.02); color:#1d2129; }}
         .editor-root.editor-theme-light button.settings-select:hover {{ background-color:rgba(17,24,39,0.045); border-color:rgba(17,24,39,0.2); }} \
         .editor-root.editor-theme-light entry.settings-search {{ background-color:rgba(17,24,39,0.05); color:#1d2129; }} \
         .editor-root.editor-theme-light entry.settings-search:focus {{ background-color:rgba(17,24,39,0.08); outline:none; }} \
         .editor-root.editor-theme-light .settings-nav-item-hover {{ background-color:rgba(17,24,39,0.05); }} \
         .editor-root.editor-theme-light .settings-nav-item-selected {{ background-color:rgba(17,24,39,0.07); }} \
         .editor-root.editor-theme-light .settings-nav-item-hover.settings-nav-item-selected {{ background-color:rgba(17,24,39,0.09); }} \
         .editor-root.editor-theme-light .settings-nav-label {{ color:#1d2129; }} \
         .editor-root.editor-theme-light .settings-nav-label-selected {{ color:#1d2129; opacity:1; }} \
         .editor-root.editor-theme-light .settings-nav-icon-selected {{ color:#1d2129; }} \
         .editor-root.editor-theme-light .settings-user-bar {{ border-top-color:rgba(17,24,39,0.07); }} \
         .editor-root.editor-theme-light .settings-user-chip:hover {{ background-color:rgba(17,24,39,0.05); }} \
         .editor-root.editor-theme-light .settings-user-badge {{ background-color:rgba(17,24,39,0.07); color:rgba(17,24,39,0.7); }} \
         .editor-root.editor-theme-light .settings-content-card {{ background-color:#ffffff; border:1px solid rgba(17,24,39,0.07); }} \
         .editor-root.editor-theme-light .settings-table-frame {{ background-color:rgba(17,24,39,0.035); }} \
         .editor-root.editor-theme-light .settings-table-frame > .settings-table-row {{ border-bottom-color:rgba(17,24,39,0.06); }} \
         .editor-root.editor-theme-light switch {{ background-color:rgba(17,24,39,0.18); }} \
         .editor-root.editor-theme-light switch:checked {{ background-color:#ff670d; }} \
                  .settings-scale-readout {{ font-size:12px; font-weight:500; opacity:0.55; }} \
         .editor-root scale trough {{ min-height:4px; background-color:rgba(255,255,255,0.10);
             border-radius:999px; border:none; }} \
         .editor-root scale highlight {{ min-height:4px; background-color:#ff670d; border-radius:999px; }} \
         .editor-root scale slider {{ min-width:14px; min-height:14px; margin:0; background-image:none;
             background-color:#ffffff; border-radius:999px; border:none; box-shadow:none; }} \
         .editor-root scale:hover slider {{ background-color:#f4f4f5; }} \
         .editor-root.editor-theme-light scale trough {{ background-color:rgba(17,24,39,0.10); }} \
         .editor-root.editor-theme-light scale highlight {{ background-color:#ff670d; }} \
         .editor-root.editor-theme-light scale slider {{ background-color:#ffffff;
             border:1px solid rgba(17,24,39,0.16); }} \
         .settings-page-title {{ font-size:{}px; font-weight:600; letter-spacing:-0.2px; opacity:0.96; }}",
        px(14.0),
        px(14.0),
        px(8.0),
        px(14.0),
        px(31.0),
        px(8.0),
        px(10.0),
        px(13.0),
        px(CARD_INSET),
        px(14.0),
        px(8.0),
        px(14.0),
        px(28.0),
        px(4.0),
        px(8.0),
        px(6.0),
        px(16.0),
        px(14.0),
        px(13.0),
        px(10.0),
        px(8.0),
        px(6.0),
        px(8.0),
        px(8.0),
        px(11.0),
        px(15.0),
        px(10.0),
        px(6.0),
        px(13.0),
        px(12.0),
        px(15.0),
        px(10.0),
        px(18.0),
        px(18.0),
        px(15.0),
        px(15.0),
        px(14.0),
        px(170.0),
        px(34.0),
        px(8.0),
        px(14.0),
        px(12.0),
        px(24.0),
    );

    // The sidebar's head controls: the toggle that hides the column and the
    // build badge beside it. In the title bar the row keeps the same left
    // inset the sidebar head gives it, so the icon does not jump when the
    // sidebar closes.
    css.push_str(&format!(
        "\
         .settings-sidebar-toggle {{ min-width:{button}px; min-height:{button}px; padding:0; \
             border:1px solid transparent; border-radius:{radius}px; background-color:transparent; \
             background-image:none; box-shadow:none; color:rgba(255,255,255,0.82); }} \
         .settings-sidebar-toggle:hover {{ background-color:rgba(255,255,255,0.07); color:#ffffff; }} \
         .settings-sidebar-toggle:active {{ background-color:rgba(255,255,255,0.10); }} \
         .settings-sidebar-toggle image {{ -gtk-icon-size:{icon}px; }} \
         .settings-beta-badge {{ font-size:{badge_font}px; font-weight:700; padding:4px {badge_pad}px; \
             border-radius:999px; background-color:rgba(255,103,13,0.18); color:#ff9250; }} \
         .settings-window-controls > .settings-shell-controls {{ margin-left:{gap}px; }} \
         .editor-root.editor-theme-light .settings-sidebar-toggle {{ color:rgba(29,33,41,0.78); }} \
         .editor-root.editor-theme-light .settings-sidebar-toggle:hover {{ background-color:rgba(17,24,39,0.06); color:#1d2129; }} \
         .editor-root.editor-theme-light .settings-sidebar-toggle:active {{ background-color:rgba(17,24,39,0.09); }} \
         .editor-root.editor-theme-light .settings-beta-badge {{ background-color:rgba(214,84,6,0.13); color:#c2500a; }}",
        button = px(28.0),
        radius = px(6.0),
        icon = px(16.0),
        badge_font = px(12.0),
        badge_pad = px(10.0),
        gap = px(14.0),
    ));

    let provider = CssProvider::new();
    provider.load_from_data(&css);
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 4,
    );
    install_select_menu_css();
}

pub(crate) fn install_select_menu_css() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let provider = CssProvider::new();
    provider.load_from_data(include_str!("select_menu.css"));
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 5,
    );
}

/// The Settings window's type scale.
///
/// The shared sheets were tuned for Inter at the sizes the Linux build uses.
/// DM Sans runs a little lighter at the same size, so this provider restates
/// every role once — page heading, section heading, row title, row hint and
/// control text — and the pages stop carrying per-page weights of their own.
/// It is installed by the Settings window only, after the shell and page
/// sheets, so the History and editor windows keep their own typography.
///
/// The sheet is authored at 100% and its font sizes are scaled by
/// [`crate::platform::ui_scale`], the way the onboarding sheet is. Only type is
/// scaled here: the shell sheet already scales its own metrics, and running the
/// whole sheet through the length scaler would grow its spacing twice.
pub(crate) fn install_type_css() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let provider = CssProvider::new();
    provider.load_from_data(&crate::css_scale::scale_font_sizes(
        r#"
        /* Page chrome */
        .editor-root {
            font-size: 14px;
        }
        .settings-page-title {
            font-size: 28px;
            font-weight: 600;
            letter-spacing: -0.3px;
            opacity: 0.97;
        }
        .settings-group-title {
            font-size: 14px;
            font-weight: 600;
            letter-spacing: 0.2px;
            opacity: 0.72;
            margin-top: 24px;
            margin-bottom: 8px;
        }
        /* The Shortcuts page headings carry the title beside an icon and keep
           their spacing on the row instead. GTK resolves providers before
           specificity, so the zeroing has to be repeated in this sheet — the
           lower-priority rule cannot reach past this provider's margins. */
        .shortcut-heading-windows .settings-group-title {
            margin-top: 0;
            margin-bottom: 0;
        }
        .settings-table-header {
            font-size: 12px;
            font-weight: 600;
            letter-spacing: 0.4px;
            opacity: 0.55;
        }

        /* Rows: one title weight for the label, one body weight for the hint */
        .settings-table-row label {
            font-size: 15px;
            font-weight: 500;
        }
        .settings-table-row label.dim-label {
            font-size: 13px;
            font-weight: 400;
            opacity: 0.6;
        }
        .settings-table-row label.settings-sub-option {
            font-size: 13px;
            font-weight: 400;
            opacity: 0.7;
        }
        .settings-table-row label.settings-sub-option-hint {
            font-size: 13px;
            font-weight: 400;
            opacity: 0.5;
        }
        .settings-table-row label.settings-mono-value {
            font-size: 13px;
            font-weight: 500;
        }
        .settings-table-row button label {
            font-size: 13px;
            font-weight: 500;
        }
        .settings-sub-option {
            font-size: 13px;
            font-weight: 400;
            opacity: 0.66;
        }
        .settings-sub-option-hint {
            font-size: 13px;
            font-weight: 400;
        }

        /* Controls */
        entry.settings-search {
            font-size: 14px;
            font-weight: 400;
        }
        .settings-search-empty {
            font-size: 13px;
            font-weight: 400;
        }
        button.settings-select,
        button.settings-select label {
            font-size: 14px;
            font-weight: 500;
        }
        popover.settings-select-popover > contents button.history-action-btn {
            font-size: 14px;
            font-weight: 400;
        }
        .settings-primary-btn,
        .settings-save-btn {
            font-size: 14px;
            font-weight: 600;
        }
        .secondary-settings-button {
            font-size: 13px;
            font-weight: 500;
        }
        .settings-toast {
            font-size: 13px;
            font-weight: 500;
        }
        .settings-status-pill {
            font-size: 12px;
            font-weight: 600;
        }
        .settings-scale-caption {
            font-size: 15px;
            font-weight: 500;
            opacity: 0.9;
        }
        .settings-scale-readout {
            font-size: 13px;
            font-weight: 500;
            opacity: 0.55;
        }

        /* Sidebar */
        .settings-beta-badge {
            font-size: 11px;
            font-weight: 600;
        }
        .settings-nav-label {
            font-size: 15px;
            font-weight: 500;
        }
        .settings-user-name {
            font-size: 14px;
            font-weight: 500;
        }
        .settings-user-sub {
            font-size: 12px;
        }
        .settings-user-initials {
            font-size: 12px;
            font-weight: 600;
        }
        .settings-user-badge {
            font-size: 10px;
            font-weight: 600;
        }

        /* Shortcuts page and the capture dialog */
        .shortcuts-header-title {
            font-size: 15px;
            font-weight: 600;
        }
        .shortcuts-label {
            font-size: 15px;
            font-weight: 400;
        }
        .shortcuts-tip {
            font-size: 13px;
            font-weight: 400;
        }
        .shortcut-notice-title {
            font-size: 15px;
            font-weight: 600;
        }
        .shortcut-notice-detail {
            font-size: 13px;
            font-weight: 400;
        }
        .shortcut-notice-action {
            font-size: 13px;
            font-weight: 500;
        }
        .shortcut-row-status {
            font-size: 13px;
            font-weight: 500;
        }
        .shortcut-capture-title {
            font-size: 17px;
            font-weight: 600;
        }
        .shortcut-capture-subtitle {
            font-size: 15px;
            font-weight: 400;
        }
        .shortcut-capture-hint {
            font-size: 12px;
            font-weight: 400;
        }
        .shortcut-capture-keycap {
            font-size: 14px;
            font-weight: 600;
        }
        .shortcut-capture-plus {
            font-size: 17px;
            font-weight: 500;
        }
        .shortcut-capture-cleared-label {
            font-size: 15px;
            font-weight: 400;
        }
        .shortcut-capture-secondary-btn,
        .shortcut-capture-primary-btn {
            font-size: 13px;
            font-weight: 600;
        }

        /* Recording tabs */
        .recording-tab-button {
            font-size: 15px;
            font-weight: 500;
        }
        .settings-action-button {
            font-size: 13px;
            font-weight: 500;
        }

        /* About */
        .about-app-name {
            font-size: 24px;
            font-weight: 600;
            letter-spacing: -0.2px;
        }
        .about-version-label {
            font-size: 13px;
            font-weight: 400;
            opacity: 0.55;
        }
        .about-link-button {
            font-size: 15px;
            font-weight: 400;
        }
        .about-support-title {
            font-size: 13px;
            font-weight: 600;
        }
        .about-support-copy {
            font-size: 13px;
            font-weight: 400;
        }

        /* Cloud */
        .cloud-user-name {
            font-size: 16px;
            font-weight: 600;
        }
        .cloud-user-email {
            font-size: 13px;
            font-weight: 400;
        }
        .cloud-avatar {
            font-size: 15px;
            font-weight: 600;
        }
        .cloud-connect-title {
            font-size: 28px;
            font-weight: 600;
        }
        .cloud-connect-eyebrow {
            font-size: 11px;
            font-weight: 600;
            letter-spacing: 0.9px;
        }
        .cloud-connect-cell {
            font-size: 19px;
            font-weight: 600;
        }
        .cloud-connect-dash {
            font-size: 19px;
            font-weight: 500;
        }
        .cloud-connect-link {
            font-size: 15px;
            font-weight: 400;
        }
        .cloud-connect-action,
        .cloud-connect-secondary {
            font-size: 14px;
            font-weight: 600;
        }

        /* Usage */
        .usage-eyebrow {
            font-size: 11px;
            font-weight: 600;
            letter-spacing: 1.1px;
        }
        .usage-account {
            font-size: 16px;
            font-weight: 600;
        }
        .usage-plan {
            font-size: 12px;
            font-weight: 600;
        }
        .usage-status,
        .usage-banner-detail,
        .usage-banner-title {
            font-size: 13px;
        }
        .usage-banner-title {
            font-weight: 600;
        }
        .usage-label {
            font-size: 11px;
            font-weight: 600;
            letter-spacing: 0.6px;
        }
        .usage-value-hero {
            font-size: 34px;
            font-weight: 600;
        }
        .usage-value {
            font-size: 22px;
            font-weight: 600;
        }
        .usage-caption,
        .usage-note,
        .usage-muted {
            font-size: 13px;
            font-weight: 400;
        }
        .usage-scale {
            font-size: 12px;
            font-weight: 400;
        }
        .usage-fact-value {
            font-size: 15px;
            font-weight: 500;
        }
        .usage-empty-title {
            font-size: 18px;
            font-weight: 600;
        }
        "#,
        crate::platform::ui_scale(),
    ));
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 6,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn shared_select_menu_css_is_valid() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let provider = CssProvider::new();
            let errors = Rc::new(Cell::new(0));
            provider.connect_parsing_error({
                let errors = errors.clone();
                move |_, _, _| errors.set(errors.get() + 1)
            });
            provider.load_from_data(include_str!("select_menu.css"));
            assert_eq!(errors.get(), 0);
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn account_chip_requires_a_cloud_session() {
        let mut config = AppConfig {
            cloud_user_email: "old@example.com".to_string(),
            cloud_user_name: "Old account".to_string(),
            ..AppConfig::default()
        };
        assert!(UserChip::from_config(&config).is_none());

        config.cloud_api_token = "token".to_string();
        let chip = UserChip::from_config(&config).expect("active session");
        assert_eq!(chip.name, "Old account");
        assert_eq!(chip.subtitle, "old@example.com");
        assert_eq!(chip.initials, "OA");

        config.cloud_user_name.clear();
        let chip = UserChip::from_config(&config).expect("email-only account");
        assert_eq!(chip.name, "old@example.com");
        assert_eq!(initials_of("jane.doe@example.com"), "JD");
        assert_eq!(initials_of("codegoddy@example.com"), "C");

        config.cloud_api_token.clear();
        assert!(UserChip::from_config(&config).is_none());
    }

    #[test]
    fn account_chip_badge_reports_the_cloud_plan() {
        let mut config = AppConfig {
            cloud_api_token: "token".to_string(),
            cloud_user_email: "user@example.com".to_string(),
            ..AppConfig::default()
        };

        config.cloud_plan_tier = "pro".to_string();
        assert_eq!(UserChip::from_config(&config).unwrap().badge, "Pro");

        config.cloud_plan_tier = "free".to_string();
        assert_eq!(UserChip::from_config(&config).unwrap().badge, "Free");

        config.cloud_plan_tier = "  Team ".to_string();
        assert_eq!(UserChip::from_config(&config).unwrap().badge, "Team");

        // No cached tier is not evidence of a free plan.
        config.cloud_plan_tier.clear();
        assert_eq!(UserChip::from_config(&config).unwrap().badge, "");
    }

    #[test]
    fn account_chip_updates_visibility_after_session_changes() {
        let Some((initial, signed_in, signed_out)) = crate::test_support::with_gtk(|| {
            let bar = GtkBox::new(Orientation::Vertical, 0);
            let labels = UserChipLabels {
                initials: Label::new(None),
                name: Label::new(None),
                subtitle: Label::new(None),
                badge: Label::new(None),
            };
            let mut config = AppConfig::default();
            labels.update(&bar, &config);
            let initial = bar.is_visible();
            config.cloud_api_token = "token".to_string();
            config.cloud_user_email = "user@example.com".to_string();
            labels.update(&bar, &config);
            let signed_in = bar.is_visible();
            config.cloud_api_token.clear();
            labels.update(&bar, &config);
            (initial, signed_in, bar.is_visible())
        }) else {
            eprintln!("skipping: no display available");
            return;
        };
        assert_eq!((initial, signed_in, signed_out), (false, true, false));
    }

    #[test]
    fn sidebar_toggle_carries_the_controls_into_the_title_bar() {
        let Some((badge, open, closed, restored)) = crate::test_support::with_gtk(|| {
            let controls = build_shell_controls();
            let sidebar = Revealer::new();
            sidebar.set_child(Some(&GtkBox::new(Orientation::Vertical, 0)));
            sidebar.set_reveal_child(true);
            let head = GtkBox::new(Orientation::Horizontal, 0);
            let title_bar = GtkBox::new(Orientation::Horizontal, 0);
            let card = GtkBox::new(Orientation::Vertical, 0);
            head.append(&controls.row);
            wire_sidebar_toggle(&controls, &sidebar, &head, &title_bar, &card, 1.0);

            let badge = controls
                .row
                .last_child()
                .and_then(|widget| widget.downcast::<Label>().ok())
                .map(|label| label.text().to_string());
            let open = (
                sidebar.reveals_child(),
                head.first_child().is_some(),
                title_bar.first_child().is_none(),
                controls.toggle.tooltip_text().map(|text| text.to_string()),
                card.margin_start(),
            );

            controls.toggle.emit_clicked();
            let closed = (
                sidebar.reveals_child(),
                head.first_child().is_none(),
                title_bar
                    .first_child()
                    .is_some_and(|widget| widget.has_css_class("settings-shell-controls")),
                controls.toggle.tooltip_text().map(|text| text.to_string()),
                card.margin_start(),
            );

            controls.toggle.emit_clicked();
            let restored = (
                sidebar.reveals_child(),
                head.first_child().is_some(),
                title_bar.first_child().is_none(),
                card.margin_start(),
            );
            (badge, open, closed, restored)
        }) else {
            eprintln!("skipping: no display available");
            return;
        };

        assert_eq!(badge.as_deref(), Some(t("Beta").as_str()));
        assert_eq!(open, (true, true, true, Some(t("Hide sidebar")), 0));
        assert_eq!(closed, (false, true, true, Some(t("Show sidebar")), 8));
        assert_eq!(restored, (true, true, true, 0));
    }
}
