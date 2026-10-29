use std::{path::PathBuf, process::Command, sync::Mutex};
use std::os::windows::process::CommandExt;
use tauri::Manager;
use valanium_core::updates::{self, Release};

static PENDING: Mutex<Option<(Release, PathBuf)>> = Mutex::new(None);
static PREPARING: Mutex<()> = Mutex::new(());

#[tauri::command]
pub async fn prepare_update(app: tauri::AppHandle, onion: bool) -> Result<serde_json::Value, String> {
    let directory = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("updates");
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = PREPARING.lock().map_err(|_| "Update state unavailable")?;
        match updates::download(onion,"windows",env!("CARGO_PKG_VERSION"),&directory)? {
            Some((release,path)) => {
                let version = release.version.clone();
                *PENDING.lock().map_err(|_| "Update state unavailable")? = Some((release,path));
                Ok(serde_json::json!({"ready":true,"version":version}))
            }
            None => { *PENDING.lock().map_err(|_| "Update state unavailable")?=None; Ok(serde_json::json!({"ready":false})) }
        }
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    let pending = PENDING.lock().map_err(|_| "Update state unavailable")?;
    let (release,path) = pending.as_ref().ok_or("No verified update is ready")?;
    updates::verify_file(path,release)?;
    let target = std::env::current_exe().map_err(|e| e.to_string())?;
    Command::new(path).arg("--valanium-apply-update").arg(&target)
        .arg(std::process::id().to_string()).arg(&release.version).arg(&release.sha256).arg(release.bytes.to_string())
        .creation_flags(crate::CREATE_NO_WINDOW).spawn().map_err(|e| e.to_string())?;
    app.exit(0);
    Ok(())
}

/// Runs before the GUI in the verified new executable. Retain the prior binary
/// until replacement and restart succeed; never use shell interpolation.
pub fn apply_pending() -> bool {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).and_then(|s|s.to_str()) != Some("--valanium-apply-update") { return false; }
    let result = (|| -> Result<(),String> {
        if args.len()!=7 { return Err("Invalid update arguments".into()); }
        let target = PathBuf::from(&args[2]).canonicalize().map_err(|e|e.to_string())?;
        let source = std::env::current_exe().map_err(|e|e.to_string())?;
        if target.extension().and_then(|s|s.to_str())!=Some("exe") || target==source { return Err("Invalid update target".into()); }
        let release=Release{version:args[4].to_string_lossy().into(),url:String::new(),sha256:args[5].to_string_lossy().into(),
            bytes:args[6].to_string_lossy().parse().map_err(|_|"Invalid size")?};
        updates::verify_file(&source,&release)?;
        let parent: u32=args[3].to_string_lossy().parse().map_err(|_|"Invalid parent")?;
        unsafe {
            use windows::Win32::{Foundation::{CloseHandle,WAIT_TIMEOUT},System::Threading::{OpenProcess,WaitForSingleObject,PROCESS_ACCESS_RIGHTS}};
            if let Ok(handle)=OpenProcess(PROCESS_ACCESS_RIGHTS(0x00100000),false,parent) {
                let outcome=WaitForSingleObject(handle,60000); let _=CloseHandle(handle);
                if outcome==WAIT_TIMEOUT {return Err("Client did not exit".into());}
            }
        }
        let staged=target.with_extension("update.part");
        let backup=target.with_extension("previous.exe");
        std::fs::copy(&source,&staged).map_err(|e|e.to_string())?;
        updates::verify_file(&staged,&release)?;
        if backup.exists(){std::fs::remove_file(&backup).map_err(|e|e.to_string())?;}
        std::fs::rename(&target,&backup).map_err(|e|e.to_string())?;
        if let Err(error)=std::fs::rename(&staged,&target) {
            let _=std::fs::rename(&backup,&target); return Err(error.to_string());
        }
        if let Err(error)=Command::new(&target).creation_flags(crate::CREATE_NO_WINDOW).spawn() {
            let _=std::fs::remove_file(&target); let _=std::fs::rename(&backup,&target);
            let _=Command::new(&target).creation_flags(crate::CREATE_NO_WINDOW).spawn();
            return Err(error.to_string());
        }
        Ok(())
    })();
    if let Err(error)=result {
        // No account data is touched. Preserve diagnostics next to the cache.
        if let Ok(exe)=std::env::current_exe(){let _=std::fs::write(exe.with_extension("update-error.txt"),error);}
    }
    true
}
