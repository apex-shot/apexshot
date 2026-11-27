//! The `apexshot://` links the desktop entry hands the binary.
//!
//! The website opens one of these after a purchase or when someone chooses to
//! connect Cloud, so the entry point must tell them apart before deciding what
//! the process does. A link is only ever acted on when it names a surface this
//! build knows; anything else is treated as a plain launch.

/// What an `apexshot://` link asks the app to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppLink {
    /// Start Cloud sign-in, then show the Cloud page (or onboarding step).
    CloudConnect,
    /// Install the Studio licence carried by the link.
    Studio,
    /// Nothing this build recognises.
    Unknown,
}

/// The path of the Cloud connect link, after `apexshot://`.
const CLOUD_CONNECT: &str = "cloud/connect";

/// True when the argument is an `apexshot://` URL at all, whatever it names.
pub fn is_app_link(url: &str) -> bool {
    path_after_scheme(url).is_some()
}

pub fn classify_link(url: &str) -> AppLink {
    let Some(path) = path_after_scheme(url) else {
        return AppLink::Unknown;
    };

    // `cloud/connectfoo` is a different path, so the prefix only counts when
    // the link ends there or continues with a separator.
    if let Some(tail) = path.strip_prefix(CLOUD_CONNECT) {
        if matches!(tail.chars().next(), None | Some('?' | '#' | '/')) {
            return AppLink::CloudConnect;
        }
    }

    // The Studio page carries its licence on `studio/activate`, with the
    // token in the query string.
    if path.starts_with("studio/") {
        return AppLink::Studio;
    }

    AppLink::Unknown
}

fn path_after_scheme(url: &str) -> Option<&str> {
    let (scheme, path) = url.split_once("://")?;
    scheme.eq_ignore_ascii_case("apexshot").then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_connect_links_are_recognised() {
        for url in [
            "apexshot://cloud/connect",
            "apexshot://cloud/connect?source=web",
            "apexshot://cloud/connect/",
            "apexshot://cloud/connect#done",
            "APEXSHOT://cloud/connect",
        ] {
            assert_eq!(classify_link(url), AppLink::CloudConnect, "{url}");
        }
    }

    #[test]
    fn the_cloud_connect_prefix_needs_a_boundary() {
        for url in [
            "apexshot://cloud/connectx",
            "apexshot://cloud/connectfoo?source=web",
            "apexshot://cloud",
        ] {
            assert_eq!(classify_link(url), AppLink::Unknown, "{url}");
        }
    }

    #[test]
    fn studio_links_still_classify_as_studio() {
        for url in [
            "apexshot://studio/activate?token=e30",
            "apexshot://studio/activate/?token=e30",
            "apexshot://studio/purchase?token=e30",
        ] {
            assert_eq!(classify_link(url), AppLink::Studio, "{url}");
        }
    }

    #[test]
    fn only_the_apexshot_scheme_counts_as_an_app_link() {
        assert!(is_app_link("apexshot://cloud/connect"));
        assert!(is_app_link("APEXSHOT://studio/activate?token=e30"));
        assert!(!is_app_link("https://apexshot.org/cloud/connect"));
        assert!(!is_app_link("capture"));
    }

    #[test]
    fn other_links_are_unknown() {
        for url in [
            "apexshot://something/else",
            "apexshot://",
            "https://apexshot.org/cloud/connect",
            "cloud/connect",
        ] {
            assert_eq!(classify_link(url), AppLink::Unknown, "{url}");
        }
    }

    #[test]
    fn cloud_connect_link_never_reaches_studio_activation() {
        assert_eq!(
            classify_link("apexshot://cloud/connect?source=web"),
            AppLink::CloudConnect
        );
        assert_eq!(
            classify_link("apexshot://cloud/connect"),
            AppLink::CloudConnect
        );
        assert_eq!(
            classify_link("apexshot://cloud/connect/"),
            AppLink::CloudConnect
        );
    }
}
