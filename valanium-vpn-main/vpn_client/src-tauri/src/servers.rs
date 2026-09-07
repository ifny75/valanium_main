use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub id: String,
    /// City/node name, e.g. "Stockholm-01" — distinct from `country_name`
    /// so the UI can group multiple servers under one expandable country.
    pub name: String,
    pub country_name: String,
    pub country_code: String,
    pub load_percent: u8,
    pub ping_ms: u32,
    pub is_current: bool,
    /// Real geographic position (Mercator-projected capital coordinates,
    /// matching familiar web maps) as a percentage of assets/map-fill.svg's
    /// viewBox — see src/assets/build_map.py, REGION_VIEWBOX. Shared by
    /// every server in the same country (we don't have per-city
    /// coordinates).
    pub map_x: f32,
    pub map_y: f32,
}
