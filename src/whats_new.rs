//! Release highlights and the once-per-upgrade decision shared by both apps.
//!
//! This module is deliberately free of GTK and platform code so the Linux
//! binary and the Windows settings binary can compile the same data, the same
//! version comparison, and the same state file.

use std::cmp::Ordering;
use std::path::PathBuf;

/// One line of a release's highlights.
#[derive(Debug, PartialEq, Eq)]
pub struct Highlight {
    /// The feature's name, as a gettext message id.
    pub title: &'static str,
    /// One sentence describing what the feature adds.
    pub body: &'static str,
    /// True when the highlight is a Studio feature; the window marks those.
    pub studio: bool,
    /// True when the highlight only matters to someone without Studio, such as
    /// the free trial or how to buy it; Studio owners never see it.
    pub without_studio_only: bool,
    /// The builds the highlight describes; the other build never shows it.
    pub platform: Platform,
}

/// Which build a highlight belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// Both the Linux and the Windows build.
    All,
    /// Only the Linux build.
    Linux,
    /// Only the Windows build.
    Windows,
}

impl Platform {
    /// The build this binary is.
    pub const CURRENT: Platform = if cfg!(target_os = "windows") {
        Platform::Windows
    } else {
        Platform::Linux
    };

    fn includes(self, build: Platform) -> bool {
        self == Platform::All || self == build
    }
}

/// The highlights of one release.
#[derive(Debug, PartialEq, Eq)]
pub struct ReleaseNotes {
    pub version: &'static str,
    /// The supporting line under the headline, when this release has one.
    pub subtitle: Option<&'static str>,
    pub highlights: &'static [Highlight],
}

/// Every release with highlights, newest first.
///
/// Free improvements for a release go first in this list before shipping, so
/// the window leads with what every copy gains.
pub const RELEASES: &[ReleaseNotes] = &[
    ReleaseNotes {
        version: "0.4.0",
        subtitle: None,
        highlights: &[
            Highlight {
                title: "Screenshots name themselves",
                body: "Each screenshot is named from its window and on-screen text right after you take it.",
                studio: false,
                without_studio_only: false,
                platform: Platform::All,
            },
            Highlight {
                title: "Search what was on screen",
                body: "Find a screenshot in History by any word it showed.",
                studio: false,
                without_studio_only: false,
                platform: Platform::All,
            },
            Highlight {
                title: "Share links in fewer steps",
                body: "Connecting Cloud now fills in your sign-in code for you.",
                studio: false,
                without_studio_only: false,
                platform: Platform::All,
            },
            Highlight {
                title: "Recording is ready from the first start",
                body: "Recording no longer goes missing after a slow first start.",
                studio: false,
                without_studio_only: false,
                platform: Platform::Windows,
            },
            Highlight {
                title: "Record the display you picked",
                body: "Multi-monitor recording on Hyprland, Sway, and Niri records your chosen display.",
                studio: false,
                without_studio_only: false,
                platform: Platform::Linux,
            },
            Highlight {
                title: "AI names (Beta)",
                body: "A small model on your computer writes clearer names. Nothing is uploaded.",
                studio: true,
                without_studio_only: false,
                platform: Platform::All,
            },
            Highlight {
                title: "Studio is 50% off",
                body: "Use code BLACKFRIDAY50 for $19.50 instead of $39, until November 30.",
                studio: true,
                without_studio_only: true,
                platform: Platform::All,
            },
        ],
    },
    ReleaseNotes {
    version: "0.3.2",
    subtitle: None,
    highlights: &[
        Highlight {
            title: "Recording works again",
            body: "Screen and webcam recordings start again. In 0.3.1 the bundled ffmpeg stopped every recording from starting.",
            studio: false,
            without_studio_only: false,
            platform: Platform::Windows,
        },
        Highlight {
            title: "Choose your mic and speaker",
            body: "In Video mode, right-click Mic or Speaker to pick a device. The toolbar shows the device you chose and a live level meter.",
            studio: false,
            without_studio_only: false,
            platform: Platform::Linux,
        },
        Highlight {
            title: "Record one app's sound",
            body: "The Speaker menu records every app, a single app, or no system sound at all.",
            studio: false,
            without_studio_only: false,
            platform: Platform::Linux,
        },
        Highlight {
            title: "Faster downloads and updates",
            body: "ApexShot now downloads from its own download servers, so installs and updates finish sooner.",
            studio: false,
            without_studio_only: false,
            platform: Platform::All,
        },
        Highlight {
            title: "Updates skip the setup wizard",
            body: "Updating goes straight to installing, without asking you to accept the licence again.",
            studio: false,
            without_studio_only: false,
            platform: Platform::Windows,
        },
        Highlight {
            title: "Quick Access in your recordings",
            body: "A Quick Access card that appears while you record now shows in the video, so you can record yourself capturing and editing.",
            studio: false,
            without_studio_only: false,
            platform: Platform::Windows,
        },
        Highlight {
            title: "Stop any recording",
            body: "A recording started without the controls bar now stops from the tray, with apexshot record stop, or with Ctrl+C.",
            studio: false,
            without_studio_only: false,
            platform: Platform::Linux,
        },
    ],
},
];

/// The highlights this person should see on this build: Studio owners skip
/// the ones that only matter before buying, and each build skips the other
/// build's features.
pub fn visible_highlights(notes: &ReleaseNotes, studio_active: bool) -> Vec<&Highlight> {
    highlights_for(notes, studio_active, Platform::CURRENT)
}

fn highlights_for(notes: &ReleaseNotes, studio_active: bool, build: Platform) -> Vec<&Highlight> {
    notes
        .highlights
        .iter()
        .filter(|highlight| highlight.platform.includes(build))
        .filter(|highlight| !(studio_active && highlight.without_studio_only))
        .collect()
}

/// The line under the highlight list, so the free tools stay visible.
pub const FOOTER: &str = "Capture, annotation, recording, and the editors' core tools stay free.";

/// What a fresh start should do with the highlights.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupDecision {
    /// The window should present these notes.
    Show(&'static ReleaseNotes),
    /// Record the running version as seen without opening a window.
    MarkSeen,
    /// Nothing to do.
    Nothing,
}

/// Parse the numeric parts of a dotted version, ignoring a `-suffix`.
fn version_number(version: &str) -> Vec<u64> {
    version
        .trim()
        .trim_start_matches('v')
        .split('-')
        .next()
        .unwrap_or("")
        .split('.')
        .map(|part| part.trim().parse::<u64>().unwrap_or(0))
        .collect()
}

/// Compare two dotted numeric versions, ignoring any `-suffix` and treating
/// missing trailing components as zero.
fn compare_versions(a: &str, b: &str) -> Ordering {
    let a = version_number(a);
    let b = version_number(b);
    for index in 0..a.len().max(b.len()) {
        let left = a.get(index).copied().unwrap_or(0);
        let right = b.get(index).copied().unwrap_or(0);
        match left.cmp(&right) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    Ordering::Equal
}

/// The newest entry that is newer than `last_seen` and no newer than
/// `current`.
///
/// `None` when there is no `last_seen` version (the caller decides what a
/// first run means), when the versions are equal, when the running build is
/// older than the recorded one, or when no entry qualifies.
pub fn notes_to_show(current: &str, last_seen: Option<&str>) -> Option<&'static ReleaseNotes> {
    let last_seen = last_seen?;
    RELEASES.iter().find(|release| {
        compare_versions(release.version, last_seen) == Ordering::Greater
            && compare_versions(release.version, current) != Ordering::Greater
    })
}

/// The newest entry no newer than `current`, for the manual command.
pub fn notes_for(current: &str) -> Option<&'static ReleaseNotes> {
    RELEASES
        .iter()
        .find(|release| compare_versions(release.version, current) != Ordering::Greater)
}

/// Decide what a daemon or shell start should do with the highlights.
///
/// An install that has not finished onboarding records the version instead of
/// showing anything, so a fresh install never meets the window. A finished
/// install with no recorded version is treated as an upgrade from before this
/// feature. A downgrade only refreshes the recorded version.
pub fn decide_on_startup(
    current: &str,
    last_seen: Option<&str>,
    onboarding_complete: bool,
) -> StartupDecision {
    if !onboarding_complete {
        return StartupDecision::MarkSeen;
    }

    let Some(last_seen) = last_seen else {
        return match notes_for(current) {
            Some(notes) => StartupDecision::Show(notes),
            None => StartupDecision::MarkSeen,
        };
    };

    if let Some(notes) = notes_to_show(current, Some(last_seen)) {
        return StartupDecision::Show(notes);
    }
    if last_seen != current {
        StartupDecision::MarkSeen
    } else {
        StartupDecision::Nothing
    }
}

/// The file that records the last version whose highlights were handled.
pub fn last_seen_path() -> Option<PathBuf> {
    let mut path = dirs::config_dir()?;
    path.push("apexshot");
    path.push("whats_new_seen");
    Some(path)
}

/// Read the version recorded by [`write_last_seen`].
pub fn read_last_seen() -> Option<String> {
    let path = last_seen_path()?;
    let text = std::fs::read_to_string(path).ok()?;
    let version = text.trim();
    (!version.is_empty()).then(|| version.to_string())
}

/// Record `version` as seen, replacing the file atomically.
///
/// Failing to record it only means the window may appear again; no caller has
/// anything useful to do with the error, so it is ignored.
pub fn write_last_seen(version: &str) {
    let Some(path) = last_seen_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let temporary = path.with_file_name(format!(".whats_new_seen.{}.tmp", std::process::id()));
    if std::fs::write(&temporary, version.trim()).is_ok() {
        let _ = std::fs::rename(&temporary, &path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_their_numeric_parts() {
        assert_eq!(compare_versions("0.3.0", "0.3.0"), Ordering::Equal);
        assert_eq!(compare_versions("0.3.1", "0.3.0"), Ordering::Greater);
        assert_eq!(compare_versions("0.3.0", "0.3.1"), Ordering::Less);
        assert_eq!(compare_versions("0.10.0", "0.9.9"), Ordering::Greater);
        assert_eq!(compare_versions("1.0", "1.0.0"), Ordering::Equal);
        // A suffix is metadata, not part of the ordering.
        assert_eq!(compare_versions("0.3.1-rc1", "0.3.1"), Ordering::Equal);
        assert_eq!(compare_versions("v0.3.1", "0.3.1"), Ordering::Equal);
    }

    #[test]
    fn notes_to_show_needs_a_previous_version_and_stays_within_the_range() {
        assert_eq!(notes_to_show("0.3.2", None), None);

        let notes = notes_to_show("0.3.2", Some("0.3.1")).expect("the new release qualifies");
        assert_eq!(notes.version, "0.3.2");

        // The recorded version already covers the release.
        assert_eq!(notes_to_show("0.3.2", Some("0.3.2")), None);
        // A build older than the entry shows nothing.
        assert_eq!(notes_to_show("0.3.1", Some("0.3.0")), None);
        // A recorded version newer than the running build is a downgrade.
        assert_eq!(notes_to_show("0.3.1", Some("0.4.0")), None);
    }

    #[test]
    fn the_newest_qualifying_entry_is_the_one_returned() {
        let notes = notes_to_show("0.4.0", Some("0.1.0")).expect("the release qualifies");
        assert_eq!(notes.version, RELEASES[0].version);
    }

    #[test]
    fn the_current_release_leads_with_free_improvements_and_has_no_subtitle() {
        let notes = notes_for("0.3.2").expect("the current release");
        assert_eq!(notes.subtitle, None);
        // Free improvements come before anything Studio-related.
        let first_studio = notes
            .highlights
            .iter()
            .position(|h| h.studio || h.without_studio_only)
            .unwrap_or(notes.highlights.len());
        assert!(first_studio > 0);
        assert!(notes.highlights[first_studio..]
            .iter()
            .all(|h| h.studio || h.without_studio_only));
    }

    #[test]
    fn notes_for_returns_the_newest_entry_covered_by_the_build() {
        assert_eq!(notes_for("0.3.2").map(|notes| notes.version), Some("0.3.2"));
        assert_eq!(notes_for("0.3.9").map(|notes| notes.version), Some("0.3.2"));
        assert_eq!(notes_for("0.4.0").map(|notes| notes.version), Some("0.4.0"));
        assert_eq!(notes_for("0.5.0").map(|notes| notes.version), Some("0.4.0"));
        assert_eq!(notes_for("0.3.1"), None);
    }

    #[test]
    fn a_fresh_install_marks_the_version_without_showing() {
        assert_eq!(
            decide_on_startup("0.3.2", None, false),
            StartupDecision::MarkSeen
        );
    }

    #[test]
    fn a_finished_install_with_no_record_shows_the_current_notes() {
        assert!(matches!(
            decide_on_startup("0.3.2", None, true),
            StartupDecision::Show(notes) if notes.version == "0.3.2"
        ));
    }

    #[test]
    fn a_finished_install_below_every_entry_marks_without_showing() {
        assert_eq!(
            decide_on_startup("0.2.0", None, true),
            StartupDecision::MarkSeen
        );
    }

    #[test]
    fn an_upgrade_shows_the_entries_after_the_recorded_version() {
        assert!(matches!(
            decide_on_startup("0.3.2", Some("0.2.0"), true),
            StartupDecision::Show(notes) if notes.version == "0.3.2"
        ));
    }

    #[test]
    fn the_same_version_does_nothing() {
        assert_eq!(
            decide_on_startup("0.3.2", Some("0.3.2"), true),
            StartupDecision::Nothing
        );
    }

    #[test]
    fn a_downgrade_records_the_running_version() {
        assert_eq!(
            decide_on_startup("0.3.1", Some("0.3.2"), true),
            StartupDecision::MarkSeen
        );
    }

    #[test]
    fn an_upgrade_past_the_newest_entry_records_the_running_version() {
        assert_eq!(
            decide_on_startup("0.3.3", Some("0.3.2"), true),
            StartupDecision::MarkSeen
        );
    }

    #[test]
    fn the_state_file_lives_beside_the_other_config_files() {
        let path = last_seen_path().expect("a config directory");
        assert!(path.ends_with("apexshot/whats_new_seen"));
    }

    #[test]
    fn studio_owners_skip_highlights_that_only_matter_before_buying() {
        const NOTES: ReleaseNotes = ReleaseNotes {
            version: "9.9.9",
            subtitle: None,
            highlights: &[
                Highlight {
                    title: "Free",
                    body: "b",
                    studio: false,
                    without_studio_only: false,
                    platform: Platform::All,
                },
                Highlight {
                    title: "Trial",
                    body: "b",
                    studio: true,
                    without_studio_only: true,
                    platform: Platform::All,
                },
                Highlight {
                    title: "New Studio tool",
                    body: "b",
                    studio: true,
                    without_studio_only: false,
                    platform: Platform::All,
                },
            ],
        };
        let titles = |active| -> Vec<&str> {
            visible_highlights(&NOTES, active)
                .iter()
                .map(|h| h.title)
                .collect()
        };
        assert_eq!(titles(false), ["Free", "Trial", "New Studio tool"]);
        assert_eq!(titles(true), ["Free", "New Studio tool"]);
    }

    #[test]
    fn each_build_skips_the_other_builds_highlights() {
        const NOTES: ReleaseNotes = ReleaseNotes {
            version: "9.9.9",
            subtitle: None,
            highlights: &[
                Highlight {
                    title: "Everywhere",
                    body: "b",
                    studio: false,
                    without_studio_only: false,
                    platform: Platform::All,
                },
                Highlight {
                    title: "Linux",
                    body: "b",
                    studio: false,
                    without_studio_only: false,
                    platform: Platform::Linux,
                },
                Highlight {
                    title: "Windows",
                    body: "b",
                    studio: false,
                    without_studio_only: false,
                    platform: Platform::Windows,
                },
            ],
        };
        let titles = |build| -> Vec<&str> {
            highlights_for(&NOTES, false, build)
                .iter()
                .map(|h| h.title)
                .collect()
        };
        assert_eq!(titles(Platform::Linux), ["Everywhere", "Linux"]);
        assert_eq!(titles(Platform::Windows), ["Everywhere", "Windows"]);
    }

    #[test]
    fn the_current_release_lists_each_build_and_hides_the_offer_from_owners() {
        let titles = |studio_active, build| -> Vec<&str> {
            highlights_for(&RELEASES[0], studio_active, build)
                .iter()
                .map(|h| h.title)
                .collect()
        };
        assert_eq!(
            titles(false, Platform::Linux),
            [
                "Screenshots name themselves",
                "Search what was on screen",
                "Share links in fewer steps",
                "Record the display you picked",
                "AI names (Beta)",
                "Studio is 50% off",
            ]
        );
        assert_eq!(
            titles(false, Platform::Windows),
            [
                "Screenshots name themselves",
                "Search what was on screen",
                "Share links in fewer steps",
                "Recording is ready from the first start",
                "AI names (Beta)",
                "Studio is 50% off",
            ]
        );
        // Owners keep AI names, their own Studio feature, but not the sale.
        for build in [Platform::Linux, Platform::Windows] {
            let owners = titles(true, build);
            assert!(owners.contains(&"AI names (Beta)"));
            assert!(!owners.contains(&"Studio is 50% off"));
        }
    }
}
