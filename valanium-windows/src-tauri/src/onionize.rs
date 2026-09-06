//! Встроенный Tor: установка, запуск, остановка.
//!
//! # Что здесь происходит
//!
//! `valanium-onionize` — отдельная программа, поднимающая локальный SOCKS5
//! поверх Arti. Клиент и так умеет ходить через SOCKS5, поэтому включение Tor
//! сводится к трём шагам: скачать, запустить, сказать ядру адрес.
//!
//! # Почему подпись обязательна
//!
//! Мы скачиваем файл и **запускаем** его. Без проверки подписи это означало бы
//! вот что: тот, кто получит доступ к репозиторию, к GitHub или к любому
//! удостоверяющему центру, подменит файл и получит выполнение произвольного
//! кода на машинах всех, кто включил Onion. Удобство «не надо ставить Tor
//! отдельно» стоило бы дороже самого Tor.
//!
//! Поэтому проверок две, и обе обязательны. Подпись манифеста тем же
//! офлайновым ключом, что и у обновлений, — она говорит, какой файл правильный.
//! И хеш скачанного — он говорит, тот ли файл приехал. Ни одна из них не
//! заменяет другую.
//!
//! # Почему адрес не приходит снаружи
//!
//! Ссылку на скачивание берём из подписанного манифеста, но проверяем ещё и
//! её начало. Подпись защищает содержимое манифеста, а не наши намерения:
//! ключ, попавший не в те руки, иначе увёл бы загрузку на любой адрес.

use std::io::{BufRead, BufReader, Read};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::CREATE_NO_WINDOW;

/// Откуда разрешено скачивать. Совпадать обязано с тем, что кладёт в манифест
/// `deploy/sign-onionize.mjs`.
const ALLOWED_PREFIX: &str = "https://github.com/valanium-project/valanium-onionize/releases/download/";
const MANIFEST_URL: &str = "https://valanium.com/downloads/onionize.json";
static CIRCUIT: Mutex<Option<serde_json::Value>> = Mutex::new(None);

fn trusted_build(payload: &str) -> Result<serde_json::Value, String> {
    let envelope: serde_json::Value = serde_json::from_str(payload).map_err(|_| "неверный формат манифеста")?;
    let manifest = envelope["manifest"].as_str().ok_or("нет манифеста")?;
    let signature = envelope["signature"].as_str().ok_or("нет подписи")?;
    if !crate::verify_release(manifest.to_owned(), signature.to_owned()) {
        return Err("подпись манифеста не прошла проверку".into());
    }
    let manifest: serde_json::Value = serde_json::from_str(manifest).map_err(|_| "неверный манифест")?;
    if manifest["v"] != 1 || manifest["kind"] != "onionize" {
        return Err("манифест не предназначен для Onionize".into());
    }
    let build = manifest["windows"].clone();
    let url = build["url"].as_str().ok_or("нет адреса сборки")?;
    let bytes = build["bytes"].as_u64().ok_or("нет размера сборки")?;
    let hash = build["sha256"].as_str().ok_or("нет хеша сборки")?;
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("неверный хеш сборки".into());
    }
    accept(url, bytes)?;
    Ok(build)
}

/// Сколько ждём строку готовности.
///
/// Первый в жизни запуск строит цепь около минуты — замерено. Ставить меньше
/// значит объявлять поломкой нормальную работу; больше — заставлять человека
/// смотреть в никуда, когда Tor действительно не поднимется.
const READY_TIMEOUT: Duration = Duration::from_secs(180);

/// Запущенный процесс. Нужен, чтобы остановить его и не оставить сиротой.
fn running() -> &'static Mutex<Option<Child>> {
    static RUNNING: OnceLock<Mutex<Option<Child>>> = OnceLock::new();
    RUNNING.get_or_init(|| Mutex::new(None))
}

/// Куда кладём саму программу и её состояние.
///
/// Рядом с данными приложения, а не в профиль пользователя: состояние Arti
/// содержит `guards.json` — список входных узлов Tor этого человека, — и ему
/// место там же, где остальное, что мы обязаны уметь вычистить.
fn home() -> Result<PathBuf, String> {
    let base = std::env::var("LOCALAPPDATA").map_err(|_| "не найден LOCALAPPDATA".to_string())?;
    let dir = PathBuf::from(base).join("Valanium").join("tor");
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    Ok(dir)
}

fn binary() -> Result<PathBuf, String> {
    Ok(home()?.join("valanium-onionize.exe"))
}

/// Установлен ли Tor и работает ли он сейчас.
#[tauri::command]
pub fn onionize_status() -> serde_json::Value {
    let installed = binary().map(|path| path.exists()).unwrap_or(false);
    let running = running().lock().map(|mut guard| {
        if guard.as_mut().is_some_and(|child| matches!(child.try_wait(), Ok(Some(_)))) {
            *guard = None;
            std::env::remove_var("VALANIUM_TOR_SOCKS");
        }
        guard.is_some()
    }).unwrap_or(false);
    let port = std::env::var("VALANIUM_TOR_SOCKS").ok();
    let circuit = if running { CIRCUIT.lock().ok().and_then(|v| v.clone()) } else { None };
    serde_json::json!({ "installed": installed, "running": running, "socks": port, "circuit": circuit })
}

/// Можно ли вообще идти по этому адресу за таким объёмом.
///
/// Отдельной функцией ради проверяемости: это защита, и она обязана иметь
/// тест, который упадёт, если её однажды снимут.
fn accept(url: &str, bytes: u64) -> Result<(), String> {
    if !url.starts_with(ALLOWED_PREFIX) {
        return Err("недопустимый адрес загрузки".into());
    }
    // Потолок на размер: без него подменённый ответ увёл бы нас в бесконечное
    // чтение и съел бы память. Запас к объявленному, а не точное совпадение —
    // сверять точно будем вместе с хешем.
    if bytes == 0 || bytes > 64 * 1024 * 1024 {
        return Err("подозрительный размер в манифесте".into());
    }
    Ok(())
}

/// Тот ли файл приехал. Размер и хеш — обе проверки обязательны.
fn verify_body(body: &[u8], sha256: &str, bytes: u64) -> Result<(), String> {
    if body.len() as u64 != bytes {
        return Err(format!("размер не совпал: ждали {bytes}, получили {}", body.len()));
    }
    let got = hex::encode(Sha256::digest(body));
    if !got.eq_ignore_ascii_case(sha256.trim()) {
        return Err("хеш скачанного не совпал с подписанным".into());
    }
    Ok(())
}

/// The WebView never downloads executable metadata or supplies its hash.
/// Native verification avoids CORS and keeps the trust decision at the write boundary.
#[tauri::command]
pub async fn onionize_install() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(install_verified)
        .await.map_err(|err| err.to_string())?
}

fn install_verified() -> Result<(), String> {
    static INSTALL: Mutex<()> = Mutex::new(());
    let _install = INSTALL.try_lock().map_err(|_| "установка уже выполняется")?;
    if running().lock().map_err(|_| "состояние повреждено")?.is_some() {
        return Err("сначала остановите Tor".into());
    }
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(60))
        .timeout(Duration::from_secs(180)).build();
    let mut payload = String::new();
    agent.get(MANIFEST_URL).set("Cache-Control", "no-cache").call()
        .map_err(|err| format!("загрузка манифеста: {err}"))?
        .into_reader().take(64 * 1024 + 1).read_to_string(&mut payload)
        .map_err(|err| err.to_string())?;
    if payload.len() > 64 * 1024 { return Err("манифест слишком большой".into()); }
    let build = trusted_build(&payload)?;
    let url = build["url"].as_str().ok_or("нет адреса")?;
    let sha256 = build["sha256"].as_str().ok_or("нет хеша")?;
    let bytes = build["bytes"].as_u64().ok_or("нет размера")?;

    let response = agent.get(url).call().map_err(|err| format!("загрузка Onionize: {err}"))?;
    let mut body = Vec::with_capacity(bytes as usize);
    response
        .into_reader()
        .take(bytes + 1)
        .read_to_end(&mut body)
        .map_err(|err| err.to_string())?;

    verify_body(&body, sha256, bytes)?;

    // Пишем во временный файл и переименовываем: иначе прерванная загрузка
    // оставила бы наполовину записанный исполняемый файл, который мы потом
    // честно запустили бы.
    let target = binary()?;
    let temp = target.with_extension("part");
    std::fs::write(&temp, &body).map_err(|err| err.to_string())?;
    std::fs::rename(&temp, &target).map_err(|err| err.to_string())?;
    Ok(())
}

/// Запускает Tor и возвращает адрес его SOCKS5.
///
/// Порт выбирает сама программа (система даёт свободный) и называет его
/// строкой READY: занятые 9050 и 9150 — обычное дело на машине с Tor Browser.
#[tauri::command]
pub async fn onionize_start() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(start_blocking)
        .await.map_err(|err| err.to_string())?
}

fn start_blocking() -> Result<String, String> {
    static START: Mutex<()> = Mutex::new(());
    let _start = START.lock().map_err(|_| "состояние запуска Tor повреждено")?;
    let _ = onionize_status();
    {
        let guard = running().lock().map_err(|_| "состояние повреждено".to_string())?;
        if guard.is_some() {
            if let Ok(socks) = std::env::var("VALANIUM_TOR_SOCKS") {
                return Ok(socks);
            }
        }
    }

    let path = binary()?;
    if !path.exists() {
        return Err("Tor не установлен".into());
    }
    if let Ok(mut value) = CIRCUIT.lock() { *value = None; }

    let mut child = Command::new(&path)
        .arg("--data")
        .arg(home()?)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|err| err.to_string())?;

    let stdout = child.stdout.take().ok_or("нет stdout у процесса")?;

    // Читаем в отдельном потоке: ждать строку готовности прямо здесь значило бы
    // подвесить интерфейс на всё время построения цепи.
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(addr) = line.strip_prefix("READY ") {
                let _ = sender.send(addr.trim().to_owned());
            } else if let Some(json) = line.strip_prefix("CIRCUIT ") {
                if json.len() <= 16384 {
                    if let Ok(value) = serde_json::from_str(json) {
                        if let Ok(mut last) = CIRCUIT.lock() { *last = Some(value); }
                    }
                }
            }
        }
    });

    match receiver.recv_timeout(READY_TIMEOUT) {
        Ok(socks) => {
            // Ядро читает эту переменную в момент подключения, поэтому её
            // достаточно выставить здесь — переподключение подхватит само.
            std::env::set_var("VALANIUM_TOR_SOCKS", &socks);
            *running().lock().map_err(|_| "состояние повреждено".to_string())? = Some(child);
            Ok(socks)
        }
        Err(_) => {
            let _ = child.kill();
            Err("Tor не поднялся за отведённое время".into())
        }
    }
}

/// Останавливает Tor. Вызывается при выходе и при выключении режима.
#[tauri::command]
pub fn onionize_stop() -> Result<(), String> {
    let mut guard = running().lock().map_err(|_| "состояние повреждено".to_string())?;
    if let Some(mut child) = guard.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    // Снимаем переменную, иначе ядро продолжит ходить на мёртвый порт и будет
    // сообщать «Tor недоступен» вместо того, чтобы взять обычный маршрут.
    std::env::remove_var("VALANIUM_TOR_SOCKS");
    if let Ok(mut value) = CIRCUIT.lock() { *value = None; }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_manifest_is_verified_in_native_installer() {
        let payload = include_str!("../../../deploy/onionize.json");
        assert!(trusted_build(payload).is_ok());
        let mut changed: serde_json::Value = serde_json::from_str(payload).unwrap();
        changed["manifest"] = serde_json::Value::String(
            changed["manifest"].as_str().unwrap().replace("6235136", "6235137"));
        assert!(trusted_build(&changed.to_string()).is_err());
        assert!(trusted_build(r#"{"manifest":"{}","signature":"00"}"#).is_err());
        assert!(trusted_build("{}").is_err());
    }

    #[test]
    #[ignore = "Downloads the signed public release and installs it in the local app directory"]
    fn live_download_install_and_start() {
        install_verified().expect("signed release installs");
        assert!(binary().unwrap().exists());
        let started = start_blocking();
        if let Ok(ref address) = started {
            assert!(address.parse::<std::net::SocketAddr>().unwrap().ip().is_loopback());
            onionize_stop().expect("stop test instance");
        }
        started.expect("installed Tor bootstraps");
    }

    #[test]
    fn only_our_release_url_is_accepted() {
        // Подпись защищает содержимое манифеста, но не наши намерения: ключ,
        // попавший не в те руки, иначе увёл бы загрузку куда угодно.
        assert!(accept(&format!("{ALLOWED_PREFIX}v0.1.0/x.exe"), 100).is_ok());
        for wrong in [
            "https://example.com/x.exe",
            "http://github.com/valanium-project/valanium-onionize/releases/download/v1/x.exe",
            "https://github.com/someone-else/valanium-onionize/releases/download/v1/x.exe",
            "https://github.com.evil.example/valanium-project/valanium-onionize/releases/download/v1/x.exe",
        ] {
            assert!(accept(wrong, 100).is_err(), "принят чужой адрес: {wrong}");
        }
    }

    #[test]
    fn absurd_sizes_are_refused() {
        assert!(accept(&format!("{ALLOWED_PREFIX}v1/x.exe"), 0).is_err());
        assert!(accept(&format!("{ALLOWED_PREFIX}v1/x.exe"), 1 << 30).is_err());
    }

    #[test]
    fn a_file_that_does_not_match_is_refused() {
        // Ради этого проверка и существует: подпись говорит, какой файл
        // правильный, но не проверяет, тот ли приехал.
        let body = b"valanium-onionize";
        let good = hex::encode(Sha256::digest(body));
        assert!(verify_body(body, &good, body.len() as u64).is_ok());
        assert!(verify_body(body, &good.to_uppercase(), body.len() as u64).is_ok());

        let tampered = b"valanium-onionizE";
        assert!(
            verify_body(tampered, &good, tampered.len() as u64).is_err(),
            "подменённый файл принят",
        );
        assert!(verify_body(body, &good, 999).is_err(), "размер не проверен");
        assert!(verify_body(body, "не хеш", body.len() as u64).is_err());
    }
}
