use super::{Compositor, WindowInfo};
use serde::Deserialize;
use std::env;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

#[derive(Debug)]
pub struct Niri;

impl Default for Niri {
    fn default() -> Self {
        Self::new()
    }
}

impl Niri {
    pub fn new() -> Self {
        Self
    }

    pub fn is_supported() -> bool {
        env::var_os("NIRI_SOCKET").is_some()
    }

    fn socket_path() -> Option<PathBuf> {
        env::var_os("NIRI_SOCKET").map(PathBuf::from)
    }

    fn send_request(&self, req: &str) -> anyhow::Result<String> {
        let path = Self::socket_path().ok_or_else(|| anyhow::anyhow!("Niri socket not found"))?;
        let mut stream = UnixStream::connect(path)?;
        stream.write_all(req.as_bytes())?;
        stream.write_all(b"\n")?;

        let mut response = String::new();
        stream.read_to_string(&mut response)?;
        Ok(response)
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
enum NiriReply {
    Handled(NiriResponse),
}

#[derive(Deserialize, Debug)]
enum NiriResponse {
    Windows(Vec<NiriWindow>),
}

#[derive(Deserialize, Debug)]
struct NiriWindow {
    id: u64,
    title: Option<String>,
    app_id: Option<String>,
    workspace_id: Option<u64>,
    is_focused: bool,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

impl Compositor for Niri {
    fn name(&self) -> &str {
        "Niri"
    }

    fn get_windows(&self) -> anyhow::Result<Vec<WindowInfo>> {
        let response = self.send_request(r#"{"Windows":{}}"#)?;

        // Niri returns a "Reply" which is an enum
        let reply: NiriReply = serde_json::from_str(&response)?;
        let NiriReply::Handled(NiriResponse::Windows(windows)) = reply;

        Ok(windows
            .into_iter()
            .map(|w| WindowInfo {
                id: w.id.to_string(),
                title: w.title.unwrap_or_default(),
                class: w.app_id.unwrap_or_default(),
                x: w.x,
                y: w.y,
                width: w.width,
                height: w.height,
                workspace: w.workspace_id.map(|id| id.to_string()).unwrap_or_default(),
                is_active: w.is_focused,
            })
            .collect())
    }

    fn get_active_window(&self) -> anyhow::Result<Option<WindowInfo>> {
        let windows = self.get_windows()?;
        Ok(windows.into_iter().find(|w| w.is_active))
    }

    fn is_running(&self) -> bool {
        Self::is_supported() && Self::socket_path().map(|p| p.exists()).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_niri_reply() {
        let json = r#"{
            "Handled": {
                "Windows": [
                    {
                        "id": 1,
                        "title": "test-title",
                        "app_id": "test-app",
                        "workspace_id": 1,
                        "is_focused": true,
                        "x": 0,
                        "y": 0,
                        "width": 1920,
                        "height": 1080
                    }
                ]
            }
        }"#;
        let reply: NiriReply = serde_json::from_str(json).unwrap();
        let NiriReply::Handled(NiriResponse::Windows(windows)) = reply;
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].title.as_deref(), Some("test-title"));
    }
}
