//! Cloud page facade for the Windows build.
//!
//! The Cloud page is shown so the configuration is visible and saved, but the
//! upload/login transports are not wired on Windows yet. The page reports the
//! reason instead of pretending a connection succeeded.

pub mod auth {
    #[derive(Debug)]
    pub struct LogoutError;

    impl std::fmt::Display for LogoutError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("Cloud sign-out is not available in the Windows build yet")
        }
    }

    impl std::error::Error for LogoutError {}

    pub fn logout() -> Result<(), LogoutError> {
        Err(LogoutError)
    }
}

pub mod listing {
    use std::path::PathBuf;

    pub fn cached_thumbnail(_url: &str) -> Result<PathBuf, String> {
        Err("Cloud thumbnails are not available in the Windows build yet".to_string())
    }
}

pub mod xbackbone {
    use crate::config::AppConfig;

    pub fn test_connection(_config: &AppConfig) -> Result<(), String> {
        Err("XBackBone uploads are not available in the Windows build yet".to_string())
    }
}
