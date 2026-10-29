//! Signed download smoke test; no account or messenger database is accessed.
fn main() -> Result<(),Box<dyn std::error::Error>> {
    let args:Vec<String>=std::env::args().collect();
    let platform=args.get(1).ok_or("platform required")?;
    let onion=args.get(2).map(String::as_str)==Some("onion");
    let directory=args.get(3).ok_or("cache directory required")?;
    if let Some(address)=args.get(4){std::env::set_var("VALANIUM_TOR_SOCKS",address);}
    let (release,path)=valanium_core::updates::download(onion,platform,"0.0.0",std::path::Path::new(directory))?.ok_or("No release")?;
    valanium_core::updates::verify_file(&path,&release)?;
    println!("{} {}: signed manifest, internal download and file integrity verified ({} bytes)",platform,release.version,release.bytes);
    Ok(())
}
