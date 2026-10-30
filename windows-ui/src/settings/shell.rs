//! The Windows Settings shell.
//!
//! A searchable sidebar with an account chip at its foot, and the inset card
//! the pages scroll inside. The pages themselves stay shared with the Linux
//! window; this module only owns the frame around them.

use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{
    gdk, Align, Box as GtkBox, CssProvider, Image, Label, Orientation, PolicyType, ScrolledWindow,
    Stack, Widget,
};

use crate::capture::editor::window::icon_names;
use crate::config::AppConfig;
use crate::i18n::t;
use crate::studio_entitlement::{current_status, StudioStatus};

/// Sidebar width at 100% scale.
pub const SIDEBAR_WIDTH: f64 = 300.0;

/// Distance the content card keeps from the window edge at 100% scale.
const CARD_INSET: f64 = 8.0;

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
        row.set_valign(Align::Center);

        let icon = Image::from_icon_name(icon_name);
        icon.add_css_class("settings-nav-icon");
        icon.set_pixel_size((16.0 * scale).round() as i32);
        icon.set_halign(Align::Start);

        let label = Label::new(Some(label_text));
        label.add_css_class("settings-nav-label");
        label.set_halign(Align::Start);
        label.set_hexpand(true);

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

/// The account chip that sits where a Save button would: the local account,
/// the licence badge, and the account the build is signed in as.
pub struct UserChip {
    pub name: String,
    pub subtitle: String,
    pub badge: String,
    pub initials: String,
}

impl UserChip {
    pub fn from_config(config: &AppConfig) -> Self {
        let name = crate::platform::account_name().unwrap_or_else(|| t("ApexShot"));
        // Only a verified licence earns the badge; a build that cannot verify
        // one shows none rather than claiming the free tier.
        let badge = match current_status() {
            StudioStatus::Active { .. } | StudioStatus::UpdateAccessEnded { .. } => t("Studio"),
            StudioStatus::NotActivated => t("Free"),
            StudioStatus::ConfigurationUnavailable
            | StudioStatus::InvalidEntitlement
            | StudioStatus::DeviceMismatch => String::new(),
        };
        let email = config.cloud_user_email.trim();
        let subtitle = if email.is_empty() {
            t("This device")
        } else {
            email.to_string()
        };
        Self {
            initials: initials_of(&name),
            name,
            subtitle,
            badge,
        }
    }
}

fn initials_of(name: &str) -> String {
    let mut initials = String::new();
    for word in name.split_whitespace() {
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

/// Build the chip as a clickable row; `on_click` opens the licence page.
pub fn build_user_chip(chip: &UserChip, scale: f64, on_click: impl Fn() + 'static) -> GtkBox {
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
    let initials = Label::new(Some(&chip.initials));
    initials.add_css_class("settings-user-initials");
    initials.set_halign(Align::Center);
    initials.set_valign(Align::Center);
    avatar.append(&initials);

    let text = GtkBox::new(Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.set_valign(Align::Center);

    let top = GtkBox::new(Orientation::Horizontal, 6);
    top.set_valign(Align::Center);
    let name = Label::new(Some(&chip.name));
    name.add_css_class("settings-user-name");
    name.set_xalign(0.0);
    name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    top.append(&name);
    if !chip.badge.is_empty() {
        let badge = Label::new(Some(&chip.badge));
        badge.add_css_class("settings-user-badge");
        badge.set_valign(Align::Center);
        top.append(&badge);
    }

    let subtitle = Label::new(Some(&chip.subtitle));
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

    let css = format!(
        "\
         .editor-root {{ background-color:#141414; }}          .editor-root.editor-theme-light {{ background-color:#f6f7fb; }}          .settings-window-controls {{ background-color:transparent; border-bottom:none; }} \n         .editor-root.editor-theme-light .settings-window-controls {{ background-color:transparent; }} \n         .settings-sidebar-wrapper {{ background-color:transparent; border-right:none; min-width:0; }} \
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
         .settings-user-avatar {{ border-radius:999px; background-image:none; background-color:#ff670d; }} \
         .settings-user-initials {{ font-size:{}px; font-weight:700; color:#ffffff; }} \
         .settings-user-name {{ font-size:{}px; font-weight:600; }} \
         .settings-user-badge {{ font-size:{}px; font-weight:700; padding:1px {}px; border-radius:999px; \
             background-color:rgba(255,255,255,0.10); color:rgba(255,255,255,0.72); }} \
         .settings-user-sub {{ font-size:{}px; opacity:0.46; }} \
         .settings-user-chevron {{ opacity:0.32; }} \
         .settings-content-card {{ border-radius:{}px; border:1px solid rgba(255,255,255,0.06); background-color:rgba(255,255,255,0.028); }} \
         .settings-group-title {{ font-size:{}px; font-weight:600; letter-spacing:0; opacity:0.92; margin-top:28px; margin-bottom:10px; }} \
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
         .settings-page-title {{ font-size:{}px; font-weight:600; letter-spacing:-0.2px; opacity:0.96; }} 
         .settings-select-popover contents {{ background-color:#252526; border:1px solid rgba(255,255,255,0.10);              border-radius:10px; padding:0; box-shadow:none; }} 
         .settings-select-menu {{ padding:4px; }} 
         .settings-select-popover contents .history-action-btn {{ min-height:0; padding:0 8px; border-radius:6px;              font-size:14px; font-weight:500; color:rgba(255,255,255,0.90); background-color:transparent;              border:none; box-shadow:none; }} 
         .settings-select-popover contents .history-action-btn:hover {{ background-color:rgba(255,255,255,0.07); color:#ffffff; }} 
         .settings-select-popover contents .history-action-btn:active {{ background-color:rgba(255,255,255,0.10); color:#ffffff; }} 
         .settings-select-popover contents .history-action-btn:focus,          .settings-select-popover contents .history-action-btn:focus-visible,          .settings-select-popover contents .history-action-btn:focus-within {{ background-color:transparent;              color:rgba(255,255,255,0.90); outline:none; box-shadow:none; }} 
         .settings-select-popover contents .history-action-btn:hover:focus {{ background-color:rgba(255,255,255,0.07);              color:#ffffff; }} 
         .settings-select-popover contents .settings-select-option-active {{ background-color:transparent; color:#ffffff;              box-shadow:none; border:none; }} 
         .settings-select-popover contents .settings-select-check {{ font-size:13px; color:rgba(255,255,255,0.85); }} 
         .editor-root.editor-theme-light .settings-select-popover contents {{ background-color:#f2f3f4;              border:1px solid rgba(17,24,39,0.14); }} 
         .editor-root.editor-theme-light .settings-select-popover contents .history-action-btn {{ color:rgba(29,33,41,0.90); }} 
         .editor-root.editor-theme-light .settings-select-popover contents .history-action-btn:hover {{ background-color:rgba(17,24,39,0.05); color:#111827; }} 
         .editor-root.editor-theme-light .settings-select-popover contents .history-action-btn:focus,          .editor-root.editor-theme-light .settings-select-popover contents .history-action-btn:focus-visible {{ background-color:transparent; color:rgba(29,33,41,0.90); outline:none; }} 
         .editor-root.editor-theme-light .settings-select-popover contents .settings-select-option-active {{ color:#111827; }} 
         .editor-root.editor-theme-light .settings-select-popover contents .settings-select-check {{ color:#111827; }}",
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

    let provider = CssProvider::new();
    provider.load_from_data(&css);
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 4,
    );
}
