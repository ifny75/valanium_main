//! Ключ устройства: то, чем клиент доказывает, что он — это он.
//!
//! Код доступа тратится один раз, на привязку (`api.rs::enroll`). Дальше вся
//! аутентификация — подпись этим ключом по свежему nonce, и код больше никуда
//! не едет. Значит, ключ и есть учётная запись: потерян — устройство надо
//! привязывать заново кодом; украден — его отзывают из списка устройств.
//!
//! Лежит рядом с exe, завёрнутый в DPAPI (`CryptProtectData`): расшифровать
//! может только эта учётная запись Windows на этой машине. Это не сейф от
//! администратора и не защита от вредоноса, запущенного тем же пользователем,
//! — от них не спасёт ничего в пользовательском процессе. Это защита от
//! скопированного профиля и от «унесли диск»: файл, вытащенный на другую
//! машину, не открывается.

use ed25519_dalek::{Signature, Signer, SigningKey};
use std::path::PathBuf;

const FILE_NAME: &str = "device.key";

pub struct Identity {
    signing: SigningKey,
}

impl Identity {
    /// Загружает ключ устройства или заводит новый при первом запуске.
    pub fn load_or_create() -> Result<Self, String> {
        let path = key_path();
        if let Ok(sealed) = std::fs::read(&path) {
            match unseal(&sealed) {
                Ok(seed) if seed.len() == 32 => {
                    let mut bytes = [0u8; 32];
                    bytes.copy_from_slice(&seed);
                    return Ok(Self { signing: SigningKey::from_bytes(&bytes) });
                }
                // Ключ есть, но не читается: другая машина, другой профиль
                // Windows или битый файл. Молча завести новый нельзя —
                // человек окажется «разлогинен» без объяснений и потратит
                // ещё одно устройство из лимита. Пусть скажет прямо.
                _ => {
                    return Err(
                        "Ключ этого устройства не читается — он привязан к другой учётной записи \
                         Windows или повреждён. Удалите device.key рядом с программой и войдите \
                         кодом доступа заново."
                            .to_string(),
                    )
                }
            }
        }

        let signing = SigningKey::generate(&mut rand::rngs::OsRng);
        let sealed = seal(signing.to_bytes().as_slice())?;
        std::fs::write(&path, sealed).map_err(|e| format!("не удалось сохранить ключ устройства: {e}"))?;
        Ok(Self { signing })
    }

    /// Публичный ключ в hex — то, как устройство называет себя серверу.
    pub fn device_pub_hex(&self) -> String {
        hex_encode(self.signing.verifying_key().as_bytes())
    }

    pub fn sign_hex(&self, message: &[u8]) -> String {
        let sig: Signature = self.signing.sign(message);
        hex_encode(&sig.to_bytes())
    }

    /// Забыть это устройство: файл ключа удаляется, следующий запуск заведёт
    /// новый. Вызывается при выходе из аккаунта.
    pub fn forget() {
        let _ = std::fs::remove_file(key_path());
    }
}

fn key_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(FILE_NAME)))
        .unwrap_or_else(|| PathBuf::from(FILE_NAME))
}

pub fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

pub fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    if s.len() % 2 != 0 {
        return Err("odd-length hex".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

// ---------------------------------------------------------------------------
// DPAPI
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn seal(plain: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB};

    let mut input = CRYPT_INTEGER_BLOB {
        cbData: plain.len() as u32,
        pbData: plain.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };

    let ok = unsafe {
        CryptProtectData(
            &mut input,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            &mut output,
        )
    };
    if ok == 0 {
        return Err("CryptProtectData failed".into());
    }
    Ok(take_blob(output))
}

#[cfg(windows)]
fn unseal(sealed: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};

    let mut input = CRYPT_INTEGER_BLOB {
        cbData: sealed.len() as u32,
        pbData: sealed.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };

    let ok = unsafe {
        CryptUnprotectData(
            &mut input,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            &mut output,
        )
    };
    if ok == 0 {
        return Err("CryptUnprotectData failed".into());
    }
    Ok(take_blob(output))
}

/// Копирует блоб в Vec и освобождает буфер, который выделил DPAPI.
#[cfg(windows)]
fn take_blob(blob: windows_sys::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB) -> Vec<u8> {
    use windows_sys::Win32::Foundation::LocalFree;

    let out = unsafe { std::slice::from_raw_parts(blob.pbData, blob.cbData as usize) }.to_vec();
    unsafe { LocalFree(blob.pbData as *mut std::ffi::c_void) };
    out
}

// Не-Windows сборки существуют только ради `cargo check` на CI и разработки
// интерфейса: настоящего тоннеля здесь всё равно нет (`wg_nt.rs` — WireGuardNT).
// Класть ключ в открытом виде на боевой платформе было бы обманом, поэтому
// путь оставлен честно голым и заметным.
#[cfg(not(windows))]
fn seal(plain: &[u8]) -> Result<Vec<u8>, String> {
    Ok(plain.to_vec())
}

#[cfg(not(windows))]
fn unseal(sealed: &[u8]) -> Result<Vec<u8>, String> {
    Ok(sealed.to_vec())
}
