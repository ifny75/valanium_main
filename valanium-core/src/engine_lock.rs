//! One MLS writer per database, including across processes. SQLite's OS locks
//! are released on a crash; a stale marker file never locks out its owner.
use crate::{CoreError, Result};
pub(crate) fn acquire(database: &str) -> Result<Option<rusqlite::Connection>> {
    if database == ":memory:" { return Ok(None); }
    let path = std::path::Path::new(database);
    let path = path.canonicalize().or_else(|_| {
        let parent=path.parent().filter(|p|!p.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
        parent.canonicalize().map(|parent|parent.join(path.file_name().unwrap_or_default()))
    }).map_err(|error|CoreError::Transport(format!("database location unavailable: {error}")))?;
    let mut name=path.into_os_string();name.push(".engine-lock");
    let lock=rusqlite::Connection::open(std::path::PathBuf::from(name))?;
    lock.busy_timeout(std::time::Duration::ZERO)?;
    match lock.execute_batch("BEGIN EXCLUSIVE") {
        Ok(())=>Ok(Some(lock)),
        Err(rusqlite::Error::SqliteFailure(error,_)) if matches!(error.code,rusqlite::ErrorCode::DatabaseBusy|rusqlite::ErrorCode::DatabaseLocked)=>Err(CoreError::StoreInUse),
        Err(error)=>Err(error.into()),
    }
}
