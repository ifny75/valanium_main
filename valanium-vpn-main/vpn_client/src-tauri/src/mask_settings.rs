//! Whether to route the tunnel through the HTTPS-masking relay (see
//! tls_mask.rs) — persisted the same way as dns_settings.rs, and for the
//! same reason: the tray's own connect flow needs to see this choice too.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MaskSettings {
    #[serde(default)]
    pub enabled: bool,
}

impl Default for MaskSettings {
    fn default() -> Self {
        Self { enabled: false }
    }
}

fn settings_path() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(|d| d.join("mask_settings.json"))
}

impl MaskSettings {
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
