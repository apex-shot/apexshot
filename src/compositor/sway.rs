use super::{Compositor, WindowInfo};
use serde::Deserialize;
use std::env;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

#[derive(Debug)]
pub struct Sway;

impl Default for Sway {
    fn default() -> Self {
        Self::new()
    }
}

impl Sway {
    pub fn new() -> Self {
        Self
    }

    pub fn is_supported() -> bool {
        env::var_os("SWAYSOCK").is_some() || env::var_os("I3SOCK").is_some()
    }

    fn socket_path() -> Option<PathBuf> {
        env::var_os("SWAYSOCK")
            .or_else(|| env::var_os("I3SOCK"))
            .map(PathBuf::from)
    }

    fn send_ipc(&self, msg_type: u32, payload: &str) -> anyhow::Result<String> {
        let path =
            Self::socket_path().ok_or_else(|| anyhow::anyhow!("Sway/i3 socket not found"))?;
        let mut stream = UnixStream::connect(path)?;

        let magic = b"i3-ipc";
        let len = payload.len() as u32;

        let mut header = Vec::new();
        header.extend_from_slice(magic);
        header.extend_from_slice(&len.to_ne_bytes());
        header.extend_from_slice(&msg_type.to_ne_bytes());

        stream.write_all(&header)?;
        stream.write_all(payload.as_bytes())?;

        let mut resp_header = [0u8; 14];
        stream.read_exact(&mut resp_header)?;

        let resp_len = u32::from_ne_bytes(resp_header[6..10].try_into().unwrap());
        let mut resp_payload = vec![0u8; resp_len as usize];
        stream.read_exact(&mut resp_payload)?;

        Ok(String::from_utf8(resp_payload)?)
    }
}

#[derive(Deserialize, Debug)]
struct SwayNode {
    id: u64,
    name: Option<String>,
    #[serde(rename = "type")]
    node_type: String,
    #[allow(dead_code)]
    window_rect: Option<SwayRect>,
    rect: SwayRect,
    focused: bool,
    nodes: Vec<SwayNode>,
    floating_nodes: Vec<SwayNode>,
    app_id: Option<String>,
    window_properties: Option<SwayWindowProps>,
}

#[derive(Deserialize, Debug)]
struct SwayRect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

#[derive(Deserialize, Debug)]
struct SwayWindowProps {
    class: Option<String>,
}

impl Sway {
    fn find_windows(node: &SwayNode, windows: &mut Vec<WindowInfo>, workspace: &str) {
        let current_workspace = if node.node_type == "workspace" {
            node.name.as_deref().unwrap_or(workspace)
        } else {
            workspace
        };

        if node.node_type == "con" || node.node_type == "floating_con" {
            if let Some(ref _props) = node.window_properties {
                // It's a window
                windows.push(WindowInfo {
                    id: node.id.to_string(),
                    title: node.name.clone().unwrap_or_default(),
                    class: node
                        .app_id
                        .clone()
                        .or_else(|| {
                            node.window_properties
                                .as_ref()
                                .and_then(|p| p.class.clone())
                        })
                        .unwrap_or_default(),
                    x: node.rect.x,
                    y: node.rect.y,
                    width: node.rect.width,
                    height: node.rect.height,
                    workspace: current_workspace.to_string(),
                    is_active: node.focused,
                });
            } else if node.app_id.is_some() {
                // Wayland window
                windows.push(WindowInfo {
                    id: node.id.to_string(),
                    title: node.name.clone().unwrap_or_default(),
                    class: node.app_id.clone().unwrap_or_default(),
                    x: node.rect.x,
                    y: node.rect.y,
                    width: node.rect.width,
                    height: node.rect.height,
                    workspace: current_workspace.to_string(),
                    is_active: node.focused,
                });
            }
        }

        for n in &node.nodes {
            Self::find_windows(n, windows, current_workspace);
        }
        for n in &node.floating_nodes {
            Self::find_windows(n, windows, current_workspace);
        }
    }
}

impl Compositor for Sway {
    fn name(&self) -> &str {
        "Sway/i3"
    }

    fn get_windows(&self) -> anyhow::Result<Vec<WindowInfo>> {
        let response = self.send_ipc(4, "")?; // GET_TREE
        let tree: SwayNode = serde_json::from_str(&response)?;
        let mut windows = Vec::new();
        Self::find_windows(&tree, &mut windows, "");
        Ok(windows)
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
    fn test_parse_sway_tree() {
        let json = r#"{
            "id": 1,
            "type": "root",
            "rect": {"x":0, "y":0, "width":1920, "height":1080},
            "focused": false,
            "nodes": [
                {
                    "id": 2,
                    "type": "output",
                    "name": "eDP-1",
                    "rect": {"x":0, "y":0, "width":1920, "height":1080},
                    "focused": false,
                    "nodes": [
                        {
                            "id": 3,
                            "type": "workspace",
                            "name": "1",
                            "rect": {"x":0, "y":0, "width":1920, "height":1080},
                            "focused": true,
                            "nodes": [
                                {
                                    "id": 4,
                                    "type": "con",
                                    "name": "test-window",
                                    "app_id": "test-app",
                                    "rect": {"x":10, "y":20, "width":800, "height":600},
                                    "focused": true,
                                    "nodes": [],
                                    "floating_nodes": []
                                }
                            ],
                            "floating_nodes": []
                        }
                    ],
                    "floating_nodes": []
                }
            ],
            "floating_nodes": []
        }"#;
        let tree: SwayNode = serde_json::from_str(json).unwrap();
        let mut windows = Vec::new();
        Sway::find_windows(&tree, &mut windows, "");
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].title, "test-window");
        assert_eq!(windows[0].x, 10);
        assert_eq!(windows[0].workspace, "1");
    }
}
