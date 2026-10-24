//! Клиент публичного API (`/v1`).
//!
//! Заменяет main_server_client.rs, который логинился в админскую панель под
//! `admin` с паролем, вкомпилированным в бинарник. Здесь учётных данных
//! сервиса нет вовсе: наружу уходит подпись ключом устройства по свежему
//! nonce, и всё.
//!
//! Жетон сессии живёт час и хранится только в памяти. На диск он не кладётся
//! намеренно: восстановить сессию всё равно дешевле одной подписью, а жетон
//! на диске — это то, что можно украсть, не имея ключа.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;

use crate::identity::{hex_decode, Identity};

/// Адрес плоскости управления. Переопределяется `VALANIUM_VPN_API` —
/// это нужно для своего сервера и для отладки против локального.
const DEFAULT_BASE_URL: &str = "http://2.27.205.9:62740";

const DOMAIN_ENROLL: &[u8] = b"valanium-vpn-enroll-v1";
const DOMAIN_AUTH: &[u8] = b"valanium-vpn-auth-v1";
const DOMAIN_REVOKE: &[u8] = b"valanium-vpn-device-revoke-v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub expires_at: String,
    pub is_active: bool,
    pub plan_name: String,
    pub traffic_limit_gb: f64,
    pub traffic_used_gb: f64,
    pub max_devices: i64,
    pub devices_used: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct TokenResponse {
    token: String,
    #[allow(dead_code)]
    expires_in: i64,
    account: Account,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceEntry {
    pub device_pub: String,
    pub created_at: String,
    pub is_current: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteServer {
    pub id: i64,
    pub name: String,
    pub country_name: Option<String>,
    pub country_code: Option<String>,
    pub map_x: Option<f64>,
    pub map_y: Option<f64>,
}

impl RemoteServer {
    pub fn into_server_info(self, ping_ms: u32, load_percent: u8) -> crate::servers::ServerInfo {
        crate::servers::ServerInfo {
            id: self.id.to_string(),
            name: self.name,
            country_name: self.country_name.unwrap_or_else(|| "Unknown".to_string()),
            country_code: self.country_code.unwrap_or_else(|| "un".to_string()),
            load_percent,
            ping_ms,
            is_current: false,
            map_x: self.map_x.unwrap_or(50.0) as f32,
            map_y: self.map_y.unwrap_or(50.0) as f32,
        }
    }
}

/// Что узел ответил про наш пир. Приватного ключа здесь нет и не бывает —
/// он не покидал устройство.
#[derive(Debug, Clone, Deserialize)]
pub struct PeerInfo {
    pub address: String,
    pub server_public_key: String,
    pub endpoint: String,
    pub dns: String,
    pub allowed_ips: String,
    pub persistent_keepalive: u16,
}

pub struct ApiClient {
    http: reqwest::Client,
    base_url: String,
    token: Mutex<Option<String>>,
}

impl ApiClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(20))
                .build()
                .expect("failed to build HTTP client"),
            base_url: std::env::var("VALANIUM_VPN_API")
                .unwrap_or_else(|_| DEFAULT_BASE_URL.to_string()),
            token: Mutex::new(None),
        }
    }

    pub fn has_session(&self) -> bool {
        self.token.lock().unwrap().is_some()
    }

    pub fn forget_session(&self) {
        *self.token.lock().unwrap() = None;
    }

    fn token(&self) -> Result<String, String> {
        self.token
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "Нет сессии — войдите кодом доступа".to_string())
    }

    async fn nonce(&self) -> Result<Vec<u8>, String> {
        #[derive(Deserialize)]
        struct Hello {
            nonce: String,
        }
        let resp = self
            .http
            .post(format!("{}/v1/hello", self.base_url))
            .send()
            .await
            .map_err(|e| format!("сервер недоступен: {e}"))?;
        let hello: Hello = parse(resp).await?;
        hex_decode(&hello.nonce)
    }

    /// Потратить код доступа и привязать это устройство.
    pub async fn enroll(&self, identity: &Identity, code: &str) -> Result<Account, String> {
        let nonce = self.nonce().await?;
        let device_pub = identity.device_pub_hex();
        let sig = identity.sign_hex(&message(DOMAIN_ENROLL, &nonce, &device_pub)?);

        let resp = self
            .http
            .post(format!("{}/v1/devices", self.base_url))
            .json(&serde_json::json!({
                "code": code,
                "nonce": crate::identity::hex_encode(&nonce),
                "device_pub": device_pub,
                "sig": sig,
            }))
            .send()
            .await
            .map_err(|e| format!("сервер недоступен: {e}"))?;

        let body: TokenResponse = parse(resp).await?;
        *self.token.lock().unwrap() = Some(body.token);
        Ok(body.account)
    }

    /// Вход уже привязанного устройства. Кода здесь нет.
    pub async fn sign_in(&self, identity: &Identity) -> Result<Account, String> {
        let nonce = self.nonce().await?;
        let device_pub = identity.device_pub_hex();
        let sig = identity.sign_hex(&message(DOMAIN_AUTH, &nonce, &device_pub)?);

        let resp = self
            .http
            .post(format!("{}/v1/auth", self.base_url))
            .json(&serde_json::json!({
                "nonce": crate::identity::hex_encode(&nonce),
                "device_pub": device_pub,
                "sig": sig,
            }))
            .send()
            .await
            .map_err(|e| format!("сервер недоступен: {e}"))?;

        let body: TokenResponse = parse(resp).await?;
        *self.token.lock().unwrap() = Some(body.token);
        Ok(body.account)
    }

    /// Повторяет запрос один раз, если жетон протух: он живёт час, а окно
    /// приложения открыто сутками, и просить человека «войти заново» ради
    /// того, что чинится одной подписью, незачем.
    async fn with_session<T, F, Fut>(&self, identity: &Identity, call: F) -> Result<T, String>
    where
        F: Fn(String) -> Fut,
        Fut: std::future::Future<Output = Result<T, String>>,
    {
        if !self.has_session() {
            self.sign_in(identity).await?;
        }
        match call(self.token()?).await {
            Err(e) if e.starts_with("401") => {
                self.sign_in(identity).await?;
                call(self.token()?).await
            }
            other => other,
        }
    }

    pub async fn account(&self, identity: &Identity) -> Result<Account, String> {
        self.with_session(identity, |token| async move {
            let resp = self
                .http
                .get(format!("{}/v1/account", self.base_url))
                .bearer_auth(token)
                .send()
                .await
                .map_err(|e| format!("сервер недоступен: {e}"))?;
            parse(resp).await
        })
        .await
    }

    pub async fn devices(&self, identity: &Identity) -> Result<Vec<DeviceEntry>, String> {
        self.with_session(identity, |token| async move {
            let resp = self
                .http
                .get(format!("{}/v1/devices", self.base_url))
                .bearer_auth(token)
                .send()
                .await
                .map_err(|e| format!("сервер недоступен: {e}"))?;
            parse(resp).await
        })
        .await
    }

    /// Отзыв требует свежей подписи, а не только жетона: жетон живёт час и
    /// мог утечь, а выкидывает отзыв навсегда.
    pub async fn revoke_device(&self, identity: &Identity, target_pub: &str) -> Result<(), String> {
        let nonce = self.nonce().await?;
        let sig = identity.sign_hex(&message(DOMAIN_REVOKE, &nonce, target_pub)?);
        let payload = serde_json::json!({
            "nonce": crate::identity::hex_encode(&nonce),
            "device_pub": target_pub,
            "sig": sig,
        });

        self.with_session(identity, |token| {
            let payload = payload.clone();
            async move {
                let resp = self
                    .http
                    .post(format!("{}/v1/devices/revoke", self.base_url))
                    .bearer_auth(token)
                    .json(&payload)
                    .send()
                    .await
                    .map_err(|e| format!("сервер недоступен: {e}"))?;
                let _: serde_json::Value = parse(resp).await?;
                Ok(())
            }
        })
        .await
    }

    pub async fn servers(&self, identity: &Identity) -> Result<Vec<RemoteServer>, String> {
        self.with_session(identity, |token| async move {
            let resp = self
                .http
                .get(format!("{}/v1/servers", self.base_url))
                .bearer_auth(token)
                .send()
                .await
                .map_err(|e| format!("сервер недоступен: {e}"))?;
            parse(resp).await
        })
        .await
    }

    /// Зарегистрировать публичный ключ WireGuard на узле.
    pub async fn register_peer(
        &self,
        identity: &Identity,
        server_id: i64,
        wg_public_key: &str,
    ) -> Result<PeerInfo, String> {
        let payload = serde_json::json!({
            "server_id": server_id,
            "wg_public_key": wg_public_key,
        });
        self.with_session(identity, |token| {
            let payload = payload.clone();
            async move {
                let resp = self
                    .http
                    .post(format!("{}/v1/peer", self.base_url))
                    .bearer_auth(token)
                    .json(&payload)
                    .send()
                    .await
                    .map_err(|e| format!("сервер недоступен: {e}"))?;
                parse(resp).await
            }
        })
        .await
    }

}

fn message(domain: &[u8], nonce: &[u8], pub_hex: &str) -> Result<Vec<u8>, String> {
    let mut msg = Vec::with_capacity(domain.len() + nonce.len() + 32);
    msg.extend_from_slice(domain);
    msg.extend_from_slice(nonce);
    msg.extend_from_slice(&hex_decode(pub_hex)?);
    Ok(msg)
}

/// Разбирает ответ, превращая ошибку сервера в человеческую строку.
///
/// Код состояния идёт в начало сообщения не для красоты: по нему
/// `with_session` отличает протухший жетон от настоящего отказа и решает,
/// повторять ли запрос.
async fn parse<T: serde::de::DeserializeOwned>(resp: reqwest::Response) -> Result<T, String> {
    let status = resp.status();
    if status.is_success() {
        return resp
            .json()
            .await
            .map_err(|e| format!("не разобрать ответ сервера: {e}"));
    }
    let body = resp.text().await.unwrap_or_default();
    let detail = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("detail").and_then(|d| d.as_str()).map(str::to_string))
        .unwrap_or(body);
    Err(format!("{}: {}", status.as_u16(), human(status.as_u16(), &detail)))
}

fn human(status: u16, detail: &str) -> String {
    match (status, detail) {
        (400, "malformed code") => "Код введён с ошибкой — проверьте последнюю группу".into(),
        (403, "code not valid") => "Такой код не выпускался или уже недействителен".into(),
        (403, "account inactive") => "Доступ приостановлен или срок истёк".into(),
        (409, "device limit reached") => "Достигнут лимит устройств — отвяжите одно в настройках".into(),
        (429, _) => "Слишком много попыток. Подождите немного".into(),
        (502, _) => "Узел не принял подключение — попробуйте другой".into(),
        _ => detail.to_string(),
    }
}
