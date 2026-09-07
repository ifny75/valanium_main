// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod dns_settings;
mod identity;
mod mask_settings;
mod servers;
mod tls_mask;
mod vpn;
mod wg_keys;
mod wg_nt;

use api::{Account, ApiClient, DeviceEntry, RemoteServer};
use dns_settings::DnsSettings;
use identity::Identity;
use mask_settings::MaskSettings;
use servers::ServerInfo;
use std::sync::{Arc, Mutex};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent, Wry};
use vpn::{ConnectionState, Status, VpnManager};
use wg_nt::WgConnection;

/// Handles to the tray's dynamic menu items, so state changes (connect,
/// disconnect, coming from either the window UI or the tray menu itself)
/// can update the tray's status/toggle text in place.
struct TrayHandles {
    status_item: MenuItem<Wry>,
    toggle_item: MenuItem<Wry>,
}

struct AppState {
    vpn: Mutex<VpnManager>,
    wg: Mutex<WgConnection>,
    tray: Mutex<Option<TrayHandles>>,
    api: ApiClient,
    /// Ключ устройства. Заводится при первом запуске и живёт рядом с exe под
    /// DPAPI; None — если файл ключа не читается (чужой профиль Windows),
    /// и тогда единственный выход — привязаться заново, о чём говорит текст
    /// ошибки.
    identity: Mutex<Option<Arc<Identity>>>,
    /// Привязано ли устройство к аккаунту. Трею больше неоткуда это узнать:
    /// его пункт «Подключиться» работает мимо окна.
    session_active: Mutex<bool>,
    dns: Mutex<DnsSettings>,
    mask: Mutex<MaskSettings>,
    // Holds the currently running HTTPS-mask relay task (tls_mask.rs), if
    // any, so a subsequent connect/disconnect can abort the old one instead
    // of leaking a background task that keeps a TLS connection open forever.
    mask_relay: Mutex<Option<tokio::task::JoinHandle<()>>>,
    // Filled by list_servers, read by connect() so connecting doesn't pay
    // for a second round trip just to look up the target's display name.
    remote_servers_cache: Mutex<Vec<RemoteServer>>,
}

impl AppState {
    /// Ключ устройства для асинхронного вызова.
    ///
    /// Возвращается `Arc`, а не ссылка, намеренно: держать `MutexGuard`
    /// через `.await` нельзя — гвард не `Send`, и такой код просто не
    /// собирается, а если бы собирался, первый же сетевой запрос запирал бы
    /// приложение целиком на всё время ответа.
    fn identity(&self) -> Result<Arc<Identity>, String> {
        self.identity
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "Ключ этого устройства недоступен — войдите кодом доступа заново".into())
    }
}

// Ping only means anything once actually connected to a server, so it's
// folded into the one status line instead of its own permanently-visible
// "Ping: —" row that looked broken while disconnected.
fn tray_texts(conn: &ConnectionState, session_active: bool) -> (String, String, bool) {
    if !session_active {
        return ("Signed out".to_string(), "Connect".to_string(), false);
    }
    match conn.status {
        Status::Connected => {
            let text = conn
                .server
                .as_ref()
                .map(|s| format!("Connected · {}, {}", s.name, s.country_name))
                .unwrap_or_else(|| "Connected".into());
            (text, "Disconnect".to_string(), true)
        }
        Status::Connecting => ("Connecting…".to_string(), "Disconnect".to_string(), true),
        Status::Disconnected => ("Disconnected".to_string(), "Connect".to_string(), true),
    }
}

fn refresh_tray(handles: &TrayHandles, conn: &ConnectionState, session_active: bool) {
    let (status_text, toggle_text, toggle_enabled) = tray_texts(conn, session_active);
    let _ = handles.status_item.set_text(status_text);
    let _ = handles.toggle_item.set_text(toggle_text);
    let _ = handles.toggle_item.set_enabled(toggle_enabled);
}

/// After any state change (from a Tauri command or the tray menu), update
/// the tray's own labels and notify the window so both stay in sync.
fn broadcast_state(app: &AppHandle, state: &AppState, conn: &ConnectionState) {
    if let Some(handles) = state.tray.lock().unwrap().as_ref() {
        refresh_tray(handles, conn, *state.session_active.lock().unwrap());
    }
    let _ = app.emit("vpn-status-changed", conn.clone());
}

/// Starts the HTTPS-masking relay (tls_mask.rs) when the user has it turned
/// on, replacing any relay left over from a previous connection first.
/// Returns the local loopback address WireGuard should dial instead of the
/// real server, or None if masking is off (falls back to the real Endpoint
/// unmodified) or the relay failed to start (logged, not fatal — better to
/// connect unmasked than not connect at all).
async fn start_mask_relay_if_enabled(
    state: &State<'_, AppState>,
    wg_config: &str,
) -> Option<std::net::SocketAddr> {
    if !state.mask.lock().unwrap().enabled {
        return None;
    }
    let real_endpoint = wg_nt::parse_endpoint(wg_config)?;
    match tls_mask::start_relay(real_endpoint).await {
        Ok((local_addr, handle)) => {
            if let Some(old) = state.mask_relay.lock().unwrap().replace(handle) {
                old.abort();
            }
            Some(local_addr)
        }
        Err(e) => {
            eprintln!("[mask] failed to start HTTPS-mask relay, connecting unmasked: {e}");
            None
        }
    }
}

// ---------------------------------------------------------------------------
// команды
// ---------------------------------------------------------------------------

/// Привязать это устройство кодом доступа. Единственное место, где код
/// вообще появляется в клиенте.
#[tauri::command]
async fn enroll_code(code: String, state: State<'_, AppState>, app: AppHandle) -> Result<Account, String> {
    let identity = state.identity()?;
    let account = state.api.enroll(&identity, code.trim()).await?;
    *state.session_active.lock().unwrap() = true;
    let conn = state.vpn.lock().unwrap().status();
    broadcast_state(&app, &state, &conn);
    Ok(account)
}

/// Вход уже привязанного устройства — вызывается при старте окна.
#[tauri::command]
async fn sign_in(state: State<'_, AppState>, app: AppHandle) -> Result<Account, String> {
    let identity = state.identity()?;
    let account = state.api.sign_in(&identity).await?;
    *state.session_active.lock().unwrap() = true;
    let conn = state.vpn.lock().unwrap().status();
    broadcast_state(&app, &state, &conn);
    Ok(account)
}

#[tauri::command]
async fn get_account(state: State<'_, AppState>) -> Result<Account, String> {
    let identity = state.identity()?;
    state.api.account(&identity).await
}

/// Выйти: сессия, ключ устройства и ключи WireGuard стираются.
///
/// Устройство при этом **остаётся** привязанным на сервере — отозвать его
/// может только подпись, а ключа мы уже лишились. Поэтому выход отзывает
/// себя сам, до того как что-то удалять; если сеть недоступна, локальные
/// следы всё равно стираются, а строка в списке устройств останется, и её
/// придётся убрать с другого устройства.
#[tauri::command]
async fn sign_out(state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    if let Ok(identity) = state.identity() {
        let own = identity.device_pub_hex();
        if let Err(e) = state.api.revoke_device(&identity, &own).await {
            eprintln!("[auth] не удалось отозвать своё устройство при выходе: {e}");
        }
    }

    state.wg.lock().unwrap().disconnect();
    if let Some(relay) = state.mask_relay.lock().unwrap().take() {
        relay.abort();
    }
    let conn = state.vpn.lock().unwrap().disconnect().unwrap_or_else(|_| ConnectionState {
        status: Status::Disconnected,
        server: None,
    });

    state.api.forget_session();
    Identity::forget();
    wg_keys::forget_all();
    *state.identity.lock().unwrap() = Identity::load_or_create().ok().map(Arc::new);
    *state.session_active.lock().unwrap() = false;
    broadcast_state(&app, &state, &conn);
    Ok(())
}

#[tauri::command]
async fn list_devices(state: State<'_, AppState>) -> Result<Vec<DeviceEntry>, String> {
    let identity = state.identity()?;
    state.api.devices(&identity).await
}

#[tauri::command]
async fn revoke_device(device_pub: String, state: State<'_, AppState>) -> Result<Vec<DeviceEntry>, String> {
    let identity = state.identity()?;
    state.api.revoke_device(&identity, &device_pub).await?;
    state.api.devices(&identity).await
}

#[tauri::command]
async fn list_servers(state: State<'_, AppState>) -> Result<Vec<ServerInfo>, String> {
    let identity = state.identity()?;
    let remote = state.api.servers(&identity).await?;
    *state.remote_servers_cache.lock().unwrap() = remote.clone();

    // Задержка и загрузка узла приходят пустыми: раньше клиент мерил
    // TCP-время до открытого порта панели, но панель больше не смотрит в
    // мир, а адреса узлов клиенту не отдаются намеренно. Настоящие числа
    // должен отдавать сам узел — ARCHITECTURE.md §10, фаза 6.
    Ok(remote.into_iter().map(|s| s.into_server_info(0, 0)).collect())
}

#[tauri::command]
fn get_status(state: State<AppState>) -> ConnectionState {
    state.vpn.lock().unwrap().status()
}

#[tauri::command]
async fn connect(server_id: String, state: State<'_, AppState>, app: AppHandle) -> Result<ConnectionState, String> {
    if !*state.session_active.lock().unwrap() {
        return Err("Войдите кодом доступа, прежде чем подключаться".to_string());
    }
    let remote_id: i64 = server_id
        .parse()
        .map_err(|_| format!("Неизвестный узел: {server_id}"))?;

    let cached = state.remote_servers_cache.lock().unwrap().clone();
    let remote_servers = if cached.is_empty() {
        let identity = state.identity()?;
        state.api.servers(&identity).await?
    } else {
        cached
    };
    let target = remote_servers
        .into_iter()
        .find(|s| s.id == remote_id)
        .ok_or_else(|| format!("Неизвестный узел: {server_id}"))?;

    let (private_key, public_key) = wg_keys::keypair_for_server(remote_id)?;
    let identity = state.identity()?;
    let peer = state.api.register_peer(&identity, remote_id, &public_key).await?;
    let wg_config = wg_keys::build_config(&peer, &private_key);

    let dns_override = state.dns.lock().unwrap().to_override();
    let relay_endpoint = start_mask_relay_if_enabled(&state, &wg_config).await;
    state
        .wg
        .lock()
        .unwrap()
        .connect(&wg_config, dns_override.as_deref(), relay_endpoint)
        .map_err(|e| format!("Не удалось поднять тоннель WireGuard: {e}"))?;

    let new_state = state
        .vpn
        .lock()
        .unwrap()
        .connect(target.into_server_info(0, 0))
        .map_err(|e| e.to_string())?;
    broadcast_state(&app, &state, &new_state);
    Ok(new_state)
}

#[tauri::command]
fn disconnect(state: State<AppState>, app: AppHandle) -> Result<ConnectionState, String> {
    state.wg.lock().unwrap().disconnect();
    if let Some(relay) = state.mask_relay.lock().unwrap().take() {
        relay.abort();
    }
    let new_state = state.vpn.lock().unwrap().disconnect().map_err(|e| e.to_string())?;
    broadcast_state(&app, &state, &new_state);
    Ok(new_state)
}

#[tauri::command]
fn get_dns_settings(state: State<AppState>) -> DnsSettings {
    state.dns.lock().unwrap().clone()
}

#[tauri::command]
fn set_dns_settings(settings: DnsSettings, state: State<AppState>) {
    settings.save();
    *state.dns.lock().unwrap() = settings;
}

#[tauri::command]
fn get_mask_settings(state: State<AppState>) -> MaskSettings {
    *state.mask.lock().unwrap()
}

#[tauri::command]
fn set_mask_settings(settings: MaskSettings, state: State<AppState>) {
    settings.save();
    *state.mask.lock().unwrap() = settings;
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Привязка устройства из командной строки: `valanium-vpn.exe --enroll <код>`.
///
/// Нужна там, где окна нет: проверка сборки, разворачивание на чужой машине,
/// диагностика перед `--test-wg` (тот кода не спрашивает намеренно, чтобы код
/// не оседал в истории команд — здесь он вводится ровно один раз).
///
/// Пишет в файл рядом с exe по той же причине, что и `run_test_wg`: у
/// релизной сборки нет консоли, и `println!` уходит в никуда.
fn run_enroll(code: &str) {
    let log_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("enroll.log")))
        .unwrap_or_else(|| std::path::PathBuf::from("enroll.log"));
    let mut out = String::new();

    let rt = tokio::runtime::Runtime::new().expect("failed to start tokio runtime");
    rt.block_on(async {
        let identity = match Identity::load_or_create() {
            Ok(id) => id,
            Err(e) => {
                out.push_str(&format!("[enroll] нет ключа устройства: {e}\n"));
                return;
            }
        };
        out.push_str(&format!("[enroll] ключ устройства: {}\n", identity.device_pub_hex()));

        let api = ApiClient::new();
        match api.enroll(&identity, code.trim()).await {
            Ok(account) => out.push_str(&format!(
                "[enroll] готово: тариф {}, до {}, устройств {}/{}\n",
                account.plan_name, account.expires_at, account.devices_used, account.max_devices
            )),
            Err(e) => out.push_str(&format!("[enroll] отказ: {e}\n")),
        }
    });

    let _ = std::fs::write(&log_path, &out);
    println!("{out}");
}

/// `valanium-vpn.exe --status` — вход подписью и состояние аккаунта, без
/// единой правки в сети машины.
///
/// Отделено от `--test-wg` намеренно: тот поднимает настоящий тоннель, то
/// есть заворачивает в него весь трафик машины и требует прав администратора.
/// Проверить, что вход и каталог узлов работают, должно быть можно, ничего
/// при этом не задев.
fn run_status() {
    let log_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("status.log")))
        .unwrap_or_else(|| std::path::PathBuf::from("status.log"));
    let mut out = String::new();

    let rt = tokio::runtime::Runtime::new().expect("failed to start tokio runtime");
    rt.block_on(async {
        let identity = match Identity::load_or_create() {
            Ok(id) => id,
            Err(e) => {
                out.push_str(&format!("[status] нет ключа устройства: {e}\n"));
                return;
            }
        };
        out.push_str(&format!("[status] устройство: {}\n", identity.device_pub_hex()));

        let api = ApiClient::new();
        match api.sign_in(&identity).await {
            Ok(a) => out.push_str(&format!(
                "[status] вход подписью: тариф {}, до {}, трафик {} из {} ГБ, устройств {}/{}\n",
                a.plan_name, a.expires_at, a.traffic_used_gb, a.traffic_limit_gb,
                a.devices_used, a.max_devices
            )),
            Err(e) => {
                out.push_str(&format!("[status] вход не прошёл: {e}\n"));
                let _ = std::fs::write(&log_path, &out);
                println!("{out}");
                return;
            }
        }

        match api.servers(&identity).await {
            Ok(list) => {
                out.push_str(&format!("[status] узлов доступно: {}\n", list.len()));
                for s in list {
                    out.push_str(&format!(
                        "          {} — {} ({})\n",
                        s.id,
                        s.name,
                        s.country_name.unwrap_or_else(|| "?".into())
                    ));
                }
            }
            Err(e) => out.push_str(&format!("[status] каталог узлов недоступен: {e}\n")),
        }

        match api.devices(&identity).await {
            Ok(list) => {
                out.push_str(&format!("[status] устройств привязано: {}\n", list.len()));
                for d in list {
                    out.push_str(&format!(
                        "          {} {}\n",
                        &d.device_pub[..8],
                        if d.is_current { "← это" } else { "" }
                    ));
                }
            }
            Err(e) => out.push_str(&format!("[status] список устройств недоступен: {e}\n")),
        }
    });

    let _ = std::fs::write(&log_path, &out);
    println!("{out}");
}

/// Hidden diagnostic entry point: exercises the real /v1 -> WireGuard-tunnel
/// path end-to-end without the GUI, so it can be verified from a script.
/// `valanium-vpn.exe --test-wg <server_id> <seconds> [mask]`.
///
/// Требует уже привязанного устройства: кода он не спрашивает, чтобы код не
/// оказался в истории команд.
///
/// Writes to a log FILE next to the exe rather than stdout: release builds
/// carry `windows_subsystem = "windows"` (no console window ever attaches),
/// so `println!` output has nowhere to go and silently vanishes even when
/// run from cmd/PowerShell — a file is the only reliable way to see it.
fn run_test_wg(server_id: i64, hold_seconds: u64, mask: bool) {
    let log_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("test-wg.log")))
        .unwrap_or_else(|| std::path::PathBuf::from("test-wg.log"));
    let file = std::sync::Mutex::new(
        std::fs::File::create(&log_path).expect("failed to create test-wg.log"),
    );
    macro_rules! logln {
        ($($arg:tt)*) => {{
            use std::io::Write;
            let line = format!($($arg)*);
            let _ = writeln!(file.lock().unwrap(), "{line}");
        }};
    }

    let rt = tokio::runtime::Runtime::new().expect("failed to start tokio runtime");
    rt.block_on(async move {
        let identity = match Identity::load_or_create() {
            Ok(id) => id,
            Err(e) => {
                logln!("[test-wg] нет ключа устройства: {e}");
                return;
            }
        };
        let api = ApiClient::new();
        logln!("[test-wg] signing in as {}...", identity.device_pub_hex());
        if let Err(e) = api.sign_in(&identity).await {
            logln!("[test-wg] FAILED to sign in: {e} (устройство привязано кодом?)");
            return;
        }

        let (private_key, public_key) = match wg_keys::keypair_for_server(server_id) {
            Ok(pair) => pair,
            Err(e) => {
                logln!("[test-wg] FAILED to prepare WireGuard keys: {e}");
                return;
            }
        };
        let peer = match api.register_peer(&identity, server_id, &public_key).await {
            Ok(p) => p,
            Err(e) => {
                logln!("[test-wg] FAILED to register peer: {e}");
                return;
            }
        };
        let config = wg_keys::build_config(&peer, &private_key);
        logln!("[test-wg] peer registered: {} via {}", peer.address, peer.endpoint);

        let mut relay_handle = None;
        let relay_endpoint = if mask {
            match wg_nt::parse_endpoint(&config) {
                Some(real_endpoint) => match tls_mask::start_relay(real_endpoint).await {
                    Ok((local_addr, handle)) => {
                        logln!("[test-wg] mask relay up: {real_endpoint} -> TLS(SNI www.microsoft.com) -> WireGuard dials {local_addr}");
                        relay_handle = Some(handle);
                        Some(local_addr)
                    }
                    Err(e) => {
                        logln!("[test-wg] mask relay FAILED to start: {e}");
                        None
                    }
                },
                None => {
                    logln!("[test-wg] couldn't parse endpoint out of config for masking");
                    None
                }
            }
        } else {
            None
        };

        let mut conn = WgConnection::new();
        logln!("[test-wg] bringing up tunnel...");
        if let Err(e) = conn.connect(&config, None, relay_endpoint) {
            logln!("[test-wg] FAILED to connect: {e}");
            if let Some(h) = relay_handle {
                h.abort();
            }
            return;
        }
        logln!("[test-wg] tunnel is up. Holding for {hold_seconds}s — check `ipconfig`/ping in another window now.");
        tokio::time::sleep(std::time::Duration::from_secs(hold_seconds)).await;

        logln!("[test-wg] tearing down tunnel...");
        conn.disconnect();
        if let Some(h) = relay_handle {
            h.abort();
        }
        logln!("[test-wg] done.");
    });
}

fn main() {
    // rustls 0.23's ClientConfig::builder() panics (silently, if that panic
    // happens inside a spawned tokio task like tls_mask's relay) unless a
    // process-wide default CryptoProvider is installed first. reqwest's own
    // rustls-tls usage may or may not trigger this before tls_mask needs
    // it, so install it explicitly, once, up front.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 3 && args[1] == "--enroll" {
        run_enroll(&args[2]);
        return;
    }
    if args.len() >= 2 && args[1] == "--status" {
        run_status();
        return;
    }
    if args.len() >= 2 && args[1] == "--test-wg" {
        let server_id: i64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
        let hold_seconds: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(20);
        let mask = args.get(4).map(|s| s == "mask").unwrap_or(false);
        run_test_wg(server_id, hold_seconds, mask);
        return;
    }

    tauri::Builder::default()
        .manage(AppState {
            vpn: Mutex::new(VpnManager::new()),
            wg: Mutex::new(WgConnection::new()),
            tray: Mutex::new(None),
            api: ApiClient::new(),
            identity: Mutex::new(Identity::load_or_create().ok().map(Arc::new)),
            session_active: Mutex::new(false),
            dns: Mutex::new(DnsSettings::load()),
            mask: Mutex::new(MaskSettings::load()),
            mask_relay: Mutex::new(None),
            remote_servers_cache: Mutex::new(Vec::new()),
        })
        .invoke_handler(tauri::generate_handler![
            enroll_code,
            sign_in,
            sign_out,
            get_account,
            list_devices,
            revoke_device,
            list_servers,
            get_status,
            connect,
            disconnect,
            get_dns_settings,
            set_dns_settings,
            get_mask_settings,
            set_mask_settings
        ])
        .setup(|app| {
            let initial = app.state::<AppState>().vpn.lock().unwrap().status();
            let (status_text, toggle_text, toggle_enabled) = tray_texts(&initial, false);

            let show_item = MenuItem::with_id(app, "show", "Open Valanium VPN", true, None::<&str>)?;
            let status_item =
                MenuItem::with_id(app, "status", status_text, false, None::<&str>)?;
            let toggle_item = MenuItem::with_id(app, "toggle", toggle_text, toggle_enabled, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let sep = || PredefinedMenuItem::separator(app);

            let tray_menu = Menu::with_items(
                app,
                &[
                    &show_item,
                    &sep()?,
                    &status_item,
                    &sep()?,
                    &toggle_item,
                    &sep()?,
                    &quit_item,
                ],
            )?;

            app.state::<AppState>().tray.lock().unwrap().replace(TrayHandles {
                status_item: status_item.clone(),
                toggle_item: toggle_item.clone(),
            });

            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Valanium VPN")
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => app.exit(0),
                    "show" => show_main_window(app),
                    "toggle" => {
                        let handle = app.clone();
                        // The tray menu callback is sync, but connecting for
                        // real means awaiting /v1 + the WireGuard adapter —
                        // run it on Tauri's async runtime instead of blocking
                        // the menu event thread.
                        tauri::async_runtime::spawn(async move {
                            let state = handle.state::<AppState>();
                            if !*state.session_active.lock().unwrap() {
                                show_main_window(&handle);
                                return;
                            }

                            let current = state.vpn.lock().unwrap().status();
                            let new_state = if current.status == Status::Disconnected {
                                let cached = state.remote_servers_cache.lock().unwrap().clone();
                                let Some(target) = cached.into_iter().next() else {
                                    // Каталог ещё не загружался — окно
                                    // покажет список и подключит по выбору.
                                    show_main_window(&handle);
                                    return;
                                };
                                let Ok((private_key, public_key)) = wg_keys::keypair_for_server(target.id) else {
                                    return;
                                };
                                let Ok(identity) = state.identity() else { return };
                                let Ok(peer) =
                                    state.api.register_peer(&identity, target.id, &public_key).await
                                else {
                                    return;
                                };
                                let wg_config = wg_keys::build_config(&peer, &private_key);
                                let dns_override = state.dns.lock().unwrap().to_override();
                                let relay_endpoint = start_mask_relay_if_enabled(&state, &wg_config).await;
                                if state
                                    .wg
                                    .lock()
                                    .unwrap()
                                    .connect(&wg_config, dns_override.as_deref(), relay_endpoint)
                                    .is_err()
                                {
                                    return;
                                }
                                state.vpn.lock().unwrap().connect(target.into_server_info(0, 0)).ok()
                            } else {
                                state.wg.lock().unwrap().disconnect();
                                if let Some(relay) = state.mask_relay.lock().unwrap().take() {
                                    relay.abort();
                                }
                                state.vpn.lock().unwrap().disconnect().ok()
                            };

                            if let Some(new_state) = new_state {
                                broadcast_state(&handle, &state, &new_state);
                            }
                        });
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        button_state: tauri::tray::MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                })
                .build(app)?;

            Ok(())
        })
        // Closing the window (the titlebar X) minimizes Valanium VPN to the
        // tray instead of exiting — a VPN client should keep tunneling in
        // the background. Quitting for real happens from the tray menu.
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                window.hide().ok();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Valanium VPN");
}
