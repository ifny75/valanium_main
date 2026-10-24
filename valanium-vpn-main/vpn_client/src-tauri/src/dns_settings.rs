//! User's DNS-server choice for the tunnel — persisted to a small JSON file
//! next to the exe so it survives restarts and, crucially, so the tray's
//! own connect flow (which never touches the webview/localStorage) sees the
//! same choice the settings panel saved.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// AdGuard DNS's default (non-family) resolver: no account/setup needed, and
// it blocks ads/trackers at the resolver level — the closest thing to a
// one-click "block ads & trackers" DNS option.
const ADGUARD_PRIMARY: &str = "94.140.14.14";
const ADGUARD_SECONDARY: &str = "94.140.15.15";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsSettings {
    pub mode: String, // "default" | "adblock" | "custom"
    #[serde(default)]
    pub custom_primary: String,
    #[serde(default)]
    pub custom_secondary: String,
}

impl Default for DnsSettings {
    fn default() -> Self {
        Self {
            mode: "default".to_string(),
            custom_primary: String::new(),
            custom_secondary: String::new(),
        }
    }
}

fn settings_path() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(|d| d.join("dns_settings.json"))
}

impl DnsSettings {
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

    /// `None` means "use whatever DNS the server's own wg_config specifies"
    /// — the tunnel's default. `Some` overrides it before the tunnel comes up.
    pub fn to_override(&self) -> Option<Vec<String>> {
        match self.mode.as_str() {
            "adblock" => Some(vec![ADGUARD_PRIMARY.to_string(), ADGUARD_SECONDARY.to_string()]),
            "custom" => {
                let mut dns = Vec::new();
                if !self.custom_primary.trim().is_empty() {
                    dns.push(self.custom_primary.trim().to_string());
                }
                if !self.custom_secondary.trim().is_empty() {
                    dns.push(self.custom_secondary.trim().to_string());
                }
                if dns.is_empty() {
                    None
                } else {
                    Some(dns)
                }
            }
            _ => None,
        }
    }
}
