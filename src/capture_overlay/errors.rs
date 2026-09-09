pub fn is_launch_blocked_error(err: &SelectionError) -> bool {
    matches!(err, SelectionError::Blocked(_))
}

fn blocked_selection_error(reason: LaunchBlockedReason) -> SelectionError {
    match reason {
        LaunchBlockedReason::ApexOverlayAlreadyActive => {
            SelectionError::Blocked("ApexShot capture overlay is already active".into())
        }
        LaunchBlockedReason::BuiltinOverlayActive => {
            SelectionError::Blocked("GNOME screenshot UI is already active".into())
        }
    }
}

/// Pull the most useful line from apexshot-capture stderr for error messages.
fn extract_capture_error_detail(stderr: &str) -> Option<String> {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        return None;
    }

    // Prefer the last line that looks like a capture failure (C++ logs prefix these).
    let preferred = lines
        .iter()
        .rev()
        .find(|line| {
            line.contains("capture failed")
                || line.contains("Portal ")
                || line.contains("status=")
                || line.starts_with("apexshot-capture:")
        })
        .copied()
        .unwrap_or(*lines.last().unwrap());

    let detail = preferred
        .strip_prefix("apexshot-capture:")
        .map(str::trim)
        .unwrap_or(preferred);

    Some(detail.to_string())
}

/// Build a SelectionError for a non-success apexshot-capture exit code.
fn overlay_exit_error(mode: &str, code: i32, stderr: &str) -> SelectionError {
    let message = match extract_capture_error_detail(stderr) {
        Some(detail) => format!("apexshot-capture {mode} exited with code {code}: {detail}"),
        None => format!("apexshot-capture {mode} exited with code {code}"),
    };
    SelectionError::InitError(message)
}

/// Whether the error text points at a portal rejection / compositor refuse.
fn looks_like_portal_rejection(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    lower.contains("portal screenshot rejected")
        || lower.contains("status=2")
        || lower.contains("portal permission capture failed")
        || lower.contains("portal fullscreen capture failed")
        || lower.contains("the name is not activatable")
        || lower.contains("serviceunknown")
}

/// Strip `apexshot-capture <mode> exited with code <n>: ` when present.
fn strip_overlay_exit_prefix(detail: &str) -> &str {
    if !detail.starts_with("apexshot-capture ") {
        return detail;
    }
    let Some(exit_idx) = detail.find(" exited with code ") else {
        return detail;
    };
    let after_exit = &detail[exit_idx + " exited with code ".len()..];
    // Expect "<n>: <rest>" or just "<n>" with no detail.
    if let Some(colon_idx) = after_exit.find(": ") {
        return &after_exit[colon_idx + 2..];
    }
    detail
}

/// Build a concise, user-facing body for desktop notifications on capture failure.
///
/// Keeps technical detail when available and adds a KDE-specific hint when the
/// compositor's screenshot backend is missing or the portal returns status=2.
pub fn user_facing_capture_failure_message(err: &str) -> String {
    let detail = err.trim();
    let mut body = if detail.is_empty() {
        crate::i18n::t("An error occurred while taking a screenshot.")
    } else {
        let cleaned = strip_overlay_exit_prefix(detail);
        // Generic exit-code form with no extra detail → plain message.
        if cleaned.starts_with("apexshot-capture ") && cleaned.contains("exited with code") {
            crate::i18n::t("An error occurred while taking a screenshot.")
        } else {
            cleaned.to_string()
        }
    };

    let on_kde = crate::backend::kde_screenshot::is_kde_wayland_session();
    if on_kde {
        let kwin_missing = !crate::backend::kde_screenshot::is_kwin_screenshot_available();
        if kwin_missing || looks_like_portal_rejection(err) {
            body.push_str("\n\n");
            body.push_str(&crate::i18n::t(
                "On KDE Plasma, the compositor refused the screenshot. Enable KWin's screenshot plugin (System Settings → Desktop Effects, or set screenshotEnabled=true under [Plugins] in ~/.config/kwinrc) and try again.",
            ));
        }
    }

    body
}
