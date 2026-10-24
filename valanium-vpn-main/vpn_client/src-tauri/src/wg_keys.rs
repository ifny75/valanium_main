//! Ключи WireGuard: рождаются здесь и отсюда не уезжают.
//!
//! Раньше пару генерировал сервер и присылал готовый конфиг вместе с
//! приватным ключом. Это означало, что приватные ключи всех пользователей
//! лежат в одной базе: изъяли диск — расшифровали записанный трафик. Теперь
//! наружу уходит только публичная половина (`api.rs::register_peer`).
//!
//! **На каждый узел — своя пара.** Один ключ на все узлы означал бы, что
//! операторы двух разных узлов видят одно и то же публичное значение и
//! сшивают по нему две сессии одного человека. Пара на узел стоит нам
//! нескольких строк, а им — этой возможности.

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::api::PeerInfo;

const FILE_NAME: &str = "wg_keys.json";

#[derive(Default, Serialize, Deserialize)]
struct KeyStore {
    /// id узла (строкой, потому что ключи JSON-объекта строковые) → приватный
    /// ключ в base64.
    keys: HashMap<String, String>,
}

fn store_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(FILE_NAME)))
        .unwrap_or_else(|| PathBuf::from(FILE_NAME))
}

fn load() -> KeyStore {
    std::fs::read_to_string(store_path())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save(store: &KeyStore) {
    if let Ok(text) = serde_json::to_string_pretty(store) {
        let _ = std::fs::write(store_path(), text);
    }
}

/// Приватный и публичный ключ для узла, заводя пару при первом подключении.
pub fn keypair_for_server(server_id: i64) -> Result<(String, String), String> {
    let mut store = load();
    let entry = store.keys.get(&server_id.to_string()).cloned();

    let secret_b64 = match entry {
        Some(existing) => existing,
        None => {
            let secret = x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng);
            let b64 = STANDARD.encode(secret.to_bytes());
            store.keys.insert(server_id.to_string(), b64.clone());
            save(&store);
            b64
        }
    };

    let raw = STANDARD
        .decode(secret_b64.trim())
        .map_err(|e| format!("испорчен файл ключей WireGuard: {e}"))?;
    let bytes: [u8; 32] = raw
        .try_into()
        .map_err(|_| "приватный ключ WireGuard должен быть 32 байта".to_string())?;
    let secret = x25519_dalek::StaticSecret::from(bytes);
    let public = x25519_dalek::PublicKey::from(&secret);

    Ok((STANDARD.encode(secret.to_bytes()), STANDARD.encode(public.to_bytes())))
}

/// Забыть все ключи узлов — при выходе из аккаунта.
pub fn forget_all() {
    let _ = std::fs::remove_file(store_path());
}

/// Собирает текст конфига WireGuard из ответа сервера и своего приватного
/// ключа. Формат тот же, что понимает `wg_nt::parse_config`, — приватный
/// ключ просто подставляется здесь, а не приезжает извне.
pub fn build_config(peer: &PeerInfo, private_key_b64: &str) -> String {
    format!(
        "[Interface]\n\
         PrivateKey = {private}\n\
         Address = {address}\n\
         DNS = {dns}\n\
         \n\
         [Peer]\n\
         PublicKey = {server_key}\n\
         Endpoint = {endpoint}\n\
         AllowedIPs = {allowed}\n\
         PersistentKeepalive = {keepalive}\n",
        private = private_key_b64,
        address = peer.address,
        dns = peer.dns,
        server_key = peer.server_public_key,
        endpoint = peer.endpoint,
        allowed = peer.allowed_ips,
        keepalive = peer.persistent_keepalive,
    )
}
