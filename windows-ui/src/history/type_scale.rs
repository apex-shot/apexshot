//! The History window's type scale.
//!
//! Settings, onboarding and the image editor each author their type at 100%
//! and let [`crate::platform::ui_scale`] carry it to the display. This is the
//! History window's sheet: the same DM Sans stack, the same 14px base, and the
//! same roles the Settings pages use — a 28px page heading, 15px row and nav
//! titles, 14px card titles and body text, 13px hints, and 11px badges.
//!
//! Only `font-size` values are scaled here. The shell and window sheets already
//! scale their spacing and control metrics, so running the whole sheet through
//! the length scaler would grow the layout twice.

use gtk4::gdk;
use gtk4::CssProvider;

const HISTORY_TYPE_CSS: &str = r#"
        /* Base */
        .history-root {
            font-family: 'DM Sans', 'Noto Sans', system-ui, sans-serif;
            font-size: 14px;
        }

        /* Sidebar and header */
        .history-root .settings-nav-label {
            font-size: 15px;
        }
        .history-root .settings-beta-badge {
            font-size: 11px;
        }
        .history-root .history-header-search {
            font-size: 14px;
        }

        /* Page headings and subtitles */
        .history-root .recent-captures-title {
            font-size: 28px;
        }
        .history-root .history-page-subtitle {
            font-size: 13px;
        }

        /* Cards, empty states and menus */
        .history-root .recent-captures-card-title {
            font-size: 14px;
        }
        .history-root .recent-captures-empty-title {
            font-size: 18px;
        }
        .history-root .recent-captures-empty-detail {
            font-size: 14px;
        }
        .history-root .history-load-more {
            font-size: 13px;
        }
        .history-action-popover .history-action-btn {
            font-size: 14px;
        }
        .history-edited-badge {
            font-size: 11px;
        }
        .settings-toast {
            font-size: 13px;
        }
        "#;

/// Install the History type sheet over the shell and window metrics.
pub(super) fn install() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let provider = CssProvider::new();
    provider.load_from_data(&crate::css_scale::scale_font_sizes(
        HISTORY_TYPE_CSS,
        crate::platform::ui_scale(),
    ));
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 6,
    );
}
