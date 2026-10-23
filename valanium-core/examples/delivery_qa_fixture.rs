//! Generate disposable offline JNI fixtures, never production credentials.
//! cargo run --example delivery_qa_fixture -- <new-directory>
use valanium_core::{keys::{Credentials, SecretKey}, mls::Mls, store::Store};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::path::PathBuf::from(std::env::args().nth(1).ok_or("new directory required")?);
    std::fs::create_dir(&path)?;
    let credentials = Credentials { identity: SecretKey::generate(), device: SecretKey::generate() };
    let peer_key = SecretKey::generate();
    let mut own = Mls::create(&credentials.device)?;
    let peer = Mls::create(&peer_key)?;
    let package = peer.key_packages(1)?.remove(0);
    let (group, _) = own.start_conversation(&package, &peer_key.public())?;
    let store = Store::open(path.join("offline-qa.db").to_str().ok_or("invalid path")?, b"qa-only")?;
    store.save_credentials(&credentials)?;
    store.set_conversation(&peer_key.public(), &group)?;
    store.set_thread(&group, &group)?;
    store.save_mls(&own.signer_public(), &own.snapshot())?;
    let manifest = serde_json::json!({ "peer": hex::encode(peer_key.public()), "conversation": hex::encode(group),
        "proof": valanium_core::keys::verification_payload(&credentials.device.public(), &peer_key.public()) });
    std::fs::write(path.join("offline-qa.json"), serde_json::to_vec_pretty(&manifest)?)?;
    println!("Disposable QA fixture generated");
    Ok(())
}
