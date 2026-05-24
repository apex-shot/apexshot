use super::{Compositor, WindowInfo};
use serde::Deserialize;
use std::env;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

#[derive(Debug)]
pub struct Hyprland;

impl Default for Hyprland {
    fn default() -> Self {
        Self::new()
    }
}

impl Hyprland {
    pub fn new() -> Self {
        Self
    }

    pub fn is_supported() -> bool {
        env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
    }

    fn socket_path() -> Option<PathBuf> {
        let signature = env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
        let xdg_runtime_dir = env::var_os("XDG_RUNTIME_DIR")?;
        let mut path = PathBuf::from(xdg_runtime_dir);
        path.push("hypr");
        path.push(signature);
        path.push(".socket.sock");
        Some(path)
    }

    fn send_command(&self, cmd: &str) -> anyhow::Result<String> {
        let path =
            Self::socket_path().ok_or_else(|| anyhow::anyhow!("Hyprland socket not found"))?;
        let mut stream = UnixStream::connect(path)?;
        stream.write_all(cmd.as_bytes())?;
        let mut response = String::new();
        stream.read_to_string(&mut response)?;
        Ok(response)
    }
}

#[derive(Deserialize, Debug)]
struct HyprlandClient {
    address: String,
    at: [i32; 2],
    size: [i32; 2],
    workspace: HyprlandWorkspace,
    class: String,
    title: String,
    #[serde(default)]
    focus_history_id: i32,
}

#[derive(Deserialize, Debug)]
struct HyprlandWorkspace {
    name: String,
}

impl Compositor for Hyprland {
    fn name(&self) -> &str {
        "Hyprland"
    }

    fn get_windows(&self) -> anyhow::Result<Vec<WindowInfo>> {
        let response = self.send_command("j/clients")?;
        let clients: Vec<HyprlandClient> = serde_json::from_str(&response)?;

        Ok(clients
            .into_iter()
            .map(|c| WindowInfo {
                id: c.address,
                title: c.title,
                class: c.class,
                x: c.at[0],
                y: c.at[1],
                width: c.size[0],
                height: c.size[1],
                workspace: c.workspace.name,
                is_active: c.focus_history_id == 0,
            })
            .collect())
    }

    fn get_active_window(&self) -> anyhow::Result<Option<WindowInfo>> {
        let response = self.send_command("j/activewindow")?;
        if response.trim() == "{}" || response.trim().is_empty() {
            return Ok(None);
        }
        let client: HyprlandClient = serde_json::from_str(&response)?;
        Ok(Some(WindowInfo {
            id: client.address,
            title: client.title,
            class: client.class,
            x: client.at[0],
            y: client.at[1],
            width: client.size[0],
            height: client.size[1],
            workspace: client.workspace.name,
            is_active: true,
        }))
    }

    fn get_active_workspace(&self) -> anyhow::Result<Option<String>> {
        let response = self.send_command("j/activeworkspace")?;
        if response.trim().is_empty() {
            return Ok(None);
        }
        let ws: HyprlandWorkspace = serde_json::from_str(&response)?;
        Ok(Some(ws.name))
    }

    fn is_running(&self) -> bool {
        Self::is_supported() && Self::socket_path().map(|p| p.exists()).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hyprland_client() {
        let json = r#"{
            "address": "0x555555555555",
            "at": [100, 200],
            "size": [800, 600],
            "workspace": { "id": 1, "name": "1" },
            "class": "test-class",
            "title": "test-title",
            "focus_history_id": 0
        }"#;
        let client: HyprlandClient = serde_json::from_str(json).unwrap();
        assert_eq!(client.address, "0x555555555555");
        assert_eq!(client.at, [100, 200]);
        assert_eq!(client.size, [800, 600]);
        assert_eq!(client.workspace.name, "1");
    }
}
