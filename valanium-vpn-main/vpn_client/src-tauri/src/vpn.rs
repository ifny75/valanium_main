use crate::servers::ServerInfo;
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Disconnected,
    Connecting,
    Connected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionState {
    pub status: Status,
    pub server: Option<ServerInfo>,
}

pub struct VpnManager {
    state: ConnectionState,
    // Handle to the spawned `naive` (NaiveProxy) client process, once real
    // process management is wired in. Kept as an Option<Child> placeholder
    // so `disconnect` has something concrete to kill.
    child: Option<std::process::Child>,
}

impl VpnManager {
    pub fn new() -> Self {
        Self {
            state: ConnectionState {
                status: Status::Disconnected,
                server: None,
            },
            child: None,
        }
    }

    pub fn status(&self) -> ConnectionState {
        self.state.clone()
    }

    /// Connects to `server`. Today this only flips local state — wiring it
    /// to a real NaiveProxy client means spawning something like:
    ///   naive --listen=socks://127.0.0.1:1080 \
    ///         --proxy=https://<naive_username>:<naive_password>@<host>:<port>
    /// using credentials fetched from a future main_server
    /// `GET /api/me/subscription` endpoint (not yet implemented server-side).
    pub fn connect(&mut self, server: ServerInfo) -> Result<ConnectionState> {
        if let Some(child) = self.child.take() {
            drop(child);
        }

        self.state.status = Status::Connected;
        self.state.server = Some(server);
        Ok(self.state.clone())
    }

    pub fn disconnect(&mut self) -> Result<ConnectionState> {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
        }
        self.state.status = Status::Disconnected;
        self.state.server = None;
        Ok(self.state.clone())
    }
}
