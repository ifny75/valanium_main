//! Whether the physical adapter should be locked down to just the VPN
//! server's own address while connected, so a dropped tunnel fails closed
//! instead of silently leaking traffic onto the real network. Persisted the
//! same way as dns_settings.rs/mask_settings.rs — the tray's own connect
//! flow needs to see this choice too.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct KillSwitchSettings {
    // Defaults to on: unlike the DNS/mask settings, this is a safety
    // property a VPN is expected to have, not an opt-in extra — a fresh
    // install shouldn't silently leak just because the user never opened
    // the settings panel.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

impl Default for KillSwitchSettings {
    fn default() -> Self {
        Self { enabled: true }
    }
}

fn settings_path() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(|d| d.join("kill_switch_settings.json"))
}

impl KillSwitchSettings {
    pub fn load() -> Self {
        settings_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(path) = settings_path() {
            if let Ok(json) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(path, json);
            }
        }
    }
}
