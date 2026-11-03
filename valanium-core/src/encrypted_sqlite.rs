//! SQLCipher page encryption. The key is installed before any schema access.
//! Legacy plaintext databases are refused without modifying their contents.
use crate::error::{CoreError, Result};
use rusqlite::Connection;
use zeroize::Zeroizing;

pub(crate) fn open(path: &str, password: &[u8]) -> Result<Connection> {
    if password.is_empty() { return Err(CoreError::StoreLocked); }
    if path != ":memory:" {
        match std::fs::File::open(path) {
            Ok(mut file) => {
                use std::io::Read;
                let mut header = [0; 16];
                let count = file.read(&mut header).map_err(|_| CoreError::StoreLocked)?;
                if count == 16 && &header == b"SQLite format 3\0" {
                    return Err(CoreError::Rejected("database_migration_required".into()));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
            Err(_) => return Err(CoreError::StoreLocked),
        }
    }
    let connection = Connection::open(path)?;
    let version: String = connection.query_row("PRAGMA cipher_version", [], |r| r.get(0))?;
    if version.is_empty() { return Err(CoreError::StoreLocked); }
    // Encode arbitrary password bytes as a passphrase; SQLCipher performs its
    // own salted PBKDF2. This is not SQLCipher's raw-key x'...' syntax.
    let passphrase = Zeroizing::new(hex::encode(password));
    connection.pragma_update(None, "key", passphrase.as_str())?;
    connection.pragma_update(None, "cipher_memory_security", "ON")?;
    connection.pragma_update(None, "temp_store", "MEMORY")?;
    connection.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0))
        .map_err(|_| CoreError::StoreLocked)?;
    Ok(connection)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("valanium-cipher-{}.db", hex::encode(crate::crypto::random_bytes(16))))
    }
    #[test]
    fn file_and_wal_exclude_plaintext_and_wrong_key_does_not_modify_database() {
        let path = path(); let name = path.to_str().unwrap();
        let connection = open(name, b"correct").unwrap();
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE secret(v TEXT);").unwrap();
        connection.execute("INSERT INTO secret VALUES (?1)", ["private sentinel 739284918"]).unwrap();
        for file in [path.clone(), std::path::PathBuf::from(format!("{name}-wal"))] {
            let bytes = std::fs::read(file).unwrap();
            let sentinel = b"private sentinel 739284918";
            assert!(!bytes.windows(sentinel.len()).any(|w| w == sentinel));
            assert!(!bytes.starts_with(b"SQLite format 3\0"));
        }
        drop(connection);
        let before = std::fs::read(&path).unwrap();
        assert!(open(name, b"wrong").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let connection = open(name, b"correct").unwrap();
        assert_eq!(connection.query_row("SELECT v FROM secret", [], |r| r.get::<_, String>(0)).unwrap(), "private sentinel 739284918");
        drop(connection); std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn legacy_database_is_left_intact() {
        let path = path(); let name = path.to_str().unwrap();
        let c = Connection::open(name).unwrap(); c.execute_batch("CREATE TABLE legacy(v TEXT);").unwrap(); drop(c);
        let before = std::fs::read(&path).unwrap();
        assert!(matches!(open(name,b"password"),Err(CoreError::Rejected(code)) if code == "database_migration_required"));
        assert_eq!(std::fs::read(&path).unwrap(),before);
        std::fs::remove_file(path).unwrap();
    }
}
