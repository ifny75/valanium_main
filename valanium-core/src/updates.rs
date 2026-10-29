//! Signed, bounded in-app downloads. Onion mode never falls back to HTTPS/DNS.
use std::{fs::{self, File}, io::{Read, Write}, path::{Path, PathBuf}, time::Duration};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const PUBLIC_KEY: &str = "a14e480c6926a1379f0d5bb4362f2c7bf214643b016edf6a7b008db0752388ec";
const HOSTS: [&str; 2] = [
    "5kghvwyxzmtzba4foenmg5pkhcoxv6iq2c6wf4pbg5uyjrviwkckvead.onion",
    "5amnu2di3yhtpqcpbcoaabfbzotw3giap2lvoe5bi5juflzhzdrsq4ad.onion",
];
const MAX_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Release { pub version: String, pub url: String, pub sha256: String, pub bytes: u64 }

fn validate(release: &Release, platform: &str) -> Result<(), String> {
    let extension = match platform { "windows" => "exe", "android" => "apk", _ => return Err("Unknown platform".into()) };
    let parts: Vec<_> = release.version.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|p| p.is_empty() || p.len() > 6 || !p.bytes().all(|b| b.is_ascii_digit()))
        || release.url != format!("https://valanium.com/downloads/Valanium-{}.{}", release.version, extension)
        || release.sha256.len() != 64 || !release.sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || release.bytes == 0 || release.bytes > MAX_BYTES {
        return Err("Invalid signed release".into());
    }
    Ok(())
}

pub fn trusted_release(payload: &str, platform: &str) -> Result<Release, String> {
    let envelope: serde_json::Value = serde_json::from_str(payload).map_err(|_| "Invalid release response")?;
    let manifest = envelope["manifest"].as_str().ok_or("Missing manifest")?;
    let signature = hex::decode(envelope["signature"].as_str().ok_or("Missing signature")?).map_err(|_| "Invalid signature")?;
    let public = hex::decode(PUBLIC_KEY).map_err(|_| "Invalid release key")?;
    if !crate::keys::verify(&signature, manifest.as_bytes(), &public) { return Err("Invalid release signature".into()); }
    let manifest: serde_json::Value = serde_json::from_str(manifest).map_err(|_| "Invalid manifest")?;
    if manifest["v"] != 1 { return Err("Unsupported manifest".into()); }
    let release: Release = serde_json::from_value(manifest[platform].clone()).map_err(|_| "Invalid platform release")?;
    validate(&release, platform)?;
    Ok(release)
}

fn request(path: &str, onion: bool) -> Result<ureq::Response, String> {
    let mut builder = ureq::AgentBuilder::new().redirects(0).try_proxy_from_env(false)
        .timeout_connect(Duration::from_secs(20)).timeout_read(Duration::from_secs(40)).timeout(Duration::from_secs(300));
    if onion {
        let address: std::net::SocketAddr = std::env::var("VALANIUM_TOR_SOCKS").map_err(|_| "Tor is not ready")?
            .parse().map_err(|_| "Invalid Tor proxy")?;
        if !address.ip().is_loopback() { return Err("Tor proxy must be local".into()); }
        builder = builder.proxy(ureq::Proxy::new(format!("socks5://{address}")).map_err(|_| "Invalid Tor proxy")?);
    }
    let agent = builder.build();
    let urls = if onion { HOSTS.iter().map(|host| format!("http://{host}{path}")).collect::<Vec<_>>() }
        else { vec![format!("https://valanium.com{path}")] };
    for url in urls {
        if let Ok(response) = agent.get(&url).set("Accept-Encoding", "identity").set("Cache-Control", "no-cache").call() {
            if response.status() == 200 { return Ok(response); }
        }
    }
    Err("Update server unavailable".into())
}

pub fn latest(onion: bool, platform: &str) -> Result<Release, String> {
    let mut payload = String::new();
    request("/v1/releases/latest", onion)?.into_reader().take(65537).read_to_string(&mut payload).map_err(|_| "Unreadable manifest")?;
    if payload.len() > 65536 { return Err("Manifest too large".into()); }
    trusted_release(&payload, platform)
}

pub fn newer(left: &str, right: &str) -> bool {
    fn parts(version: &str) -> Option<Vec<u64>> { version.split('.').map(|s| s.parse().ok()).collect() }
    match (parts(left), parts(right)) { (Some(a), Some(b)) => a > b, _ => false }
}

pub fn verify_file(path: &Path, release: &Release) -> Result<(), String> {
    let mut file = File::open(path).map_err(|_| "Update file missing")?;
    if file.metadata().map_err(|_| "Unreadable update")?.len() != release.bytes { return Err("Update size mismatch".into()); }
    let mut hasher = Sha256::new();
    let mut buffer = [0; 65536];
    loop { let n = file.read(&mut buffer).map_err(|_| "Unreadable update")?; if n == 0 { break; } hasher.update(&buffer[..n]); }
    if hex::encode(hasher.finalize()) != release.sha256 { return Err("Update hash mismatch".into()); }
    Ok(())
}

pub fn download(onion: bool, platform: &str, current: &str, directory: &Path) -> Result<Option<(Release, PathBuf)>, String> {
    // The native layer fetches/verifies metadata itself; UI cannot supply a URL/hash.
    let release = latest(onion, platform)?;
    if !newer(&release.version, current) { return Ok(None); }
    fs::create_dir_all(directory).map_err(|_| "Cannot create update cache")?;
    let extension = if platform == "windows" { "exe" } else { "apk" };
    let target = directory.join(format!("Valanium-{}.{}", release.version, extension));
    if verify_file(&target, &release).is_ok() { return Ok(Some((release, target))); }
    let partial = target.with_extension("part");
    let result = (|| {
        let response = request(release.url.strip_prefix("https://valanium.com").ok_or("Invalid download")?, onion)?;
        let mut reader = response.into_reader().take(release.bytes + 1);
        let mut output = File::create(&partial).map_err(|_| "Cannot write update")?;
        let mut count = 0;
        let mut hasher = Sha256::new();
        let mut buffer = [0; 65536];
        loop {
            let n = reader.read(&mut buffer).map_err(|_| "Download interrupted")?;
            if n == 0 { break; }
            count += n as u64;
            if count > release.bytes { return Err("Update exceeds signed size".into()); }
            hasher.update(&buffer[..n]);
            output.write_all(&buffer[..n]).map_err(|_| "Cannot write update")?;
        }
        if count != release.bytes || hex::encode(hasher.finalize()) != release.sha256 { return Err("Update integrity check failed".into()); }
        output.sync_all().map_err(|_| "Cannot save update")?;
        drop(output);
        if target.exists() { fs::remove_file(&target).map_err(|_| "Cannot replace update cache")?; }
        fs::rename(&partial, &target).map_err(|_| "Cannot finalize update")?;
        Ok(Some((release, target)))
    })();
    if result.is_err() { let _ = fs::remove_file(partial); }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn production_manifest_and_tampering() {
        let payload = include_str!("../../deploy/releases.json");
        assert!(trusted_release(payload, "windows").is_ok());
        assert!(trusted_release(payload, "android").is_ok());
        let tampered = payload.replacen("Valanium-", "Malware--", 1);
        assert!(trusted_release(&tampered, "windows").is_err());
    }
    #[test]
    fn release_contract_rejects_paths_hosts_and_unbounded_downloads() {
        let good = Release { version:"1.2.3".into(), url:"https://valanium.com/downloads/Valanium-1.2.3.exe".into(), sha256:"a".repeat(64), bytes:10 };
        assert!(validate(&good,"windows").is_ok());
        for url in ["https://valanium.com.evil/downloads/a.exe", "https://valanium.com/downloads/../a.exe", "http://valanium.com/downloads/a.exe"] {
            assert!(validate(&Release{url:url.into(),..good.clone()},"windows").is_err());
        }
        assert!(validate(&Release{version:"../../bad".into(),..good.clone()},"windows").is_err());
        assert!(validate(&Release{bytes:MAX_BYTES+1,..good},"windows").is_err());
        assert!(newer("0.13.14","0.13.9")); assert!(!newer("0.13.14","0.13.14")); assert!(!newer("bad","0.13.14"));
    }
    #[test]
    fn corrupted_cached_file_is_never_accepted() {
        let path=std::env::temp_dir().join(format!("valanium-update-test-{}",std::process::id()));
        fs::write(&path,b"good").unwrap();
        let release=Release{version:"1.0.0".into(),url:String::new(),sha256:hex::encode(Sha256::digest(b"good")),bytes:4};
        assert!(verify_file(&path,&release).is_ok());
        fs::write(&path,b"evil").unwrap(); assert!(verify_file(&path,&release).is_err());
        fs::write(&path,b"goo").unwrap(); assert!(verify_file(&path,&release).is_err());
        fs::remove_file(path).unwrap();
    }
}
