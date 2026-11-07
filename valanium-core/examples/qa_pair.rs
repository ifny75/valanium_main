//! Disposable LAN pairing peer for QA; works on a fresh database in a new directory.
//! cargo run --example qa_pair -- <new-dir> join <qr-text>   (new device scans an offer)
//! cargo run --example qa_pair -- <new-dir> listen           (new device shows a QR)
//! cargo run --example qa_pair -- <linked-dir> offer          (an account from an earlier run shows a QR)
use serde_json::{json, Value};
use std::sync::{mpsc::channel, Arc};
use std::time::Duration;
use valanium_core::{client::{Engine, EventSink}, command::Command, store::Store};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let dir = std::path::PathBuf::from(args.get(1).ok_or("new directory required")?);
    let offering = args.get(2).is_some_and(|mode| mode == "offer");
    if !offering { std::fs::create_dir(&dir)?; }
    let path = dir.join("device.db").to_string_lossy().into_owned();
    let (tx, events) = channel::<Value>();
    let sink: EventSink = Arc::new(move |event| { let _ = tx.send(serde_json::to_value(event).unwrap()); });
    let engine = Engine::start(path.clone(), b"disposable-qa-only".to_vec(), sink)?;
    let submit = |v: Value| engine.submit(serde_json::from_value::<Command>(v).unwrap()).unwrap();
    match args.get(2).map(String::as_str) {
        Some("join") => submit(json!({"type":"pairing_connect","qr":args.get(3).ok_or("qr text required")?})),
        Some("listen") => submit(json!({"type":"pairing_listen"})),
        Some("offer") => submit(json!({"type":"pairing_offer"})),
        _ => return Err("mode must be join or listen".into()),
    }
    loop {
        let e = events.recv_timeout(Duration::from_secs(240))?;
        match e["type"].as_str().unwrap_or_default() {
            "pairing" => {
                let state = e["state"].as_str().unwrap_or_default();
                println!("PAIRING {state} {}", e["code"].as_str().unwrap_or_default());
                if state == "listening" { println!("QR {}", e["qr"].as_str().unwrap_or_default()); }
                if state == "verify" {
                    // The source proves it can open its own database; a receiver sends nothing.
                    let unlock = if offering { hex::encode(b"disposable-qa-only") } else { String::new() };
                    submit(json!({"type":"pairing_confirm","session":e["session"],"unlock":unlock}));
                }
                if state == "complete" || state == "failed" { break; }
            }
            "failed" => println!("FAILED {}", e["code"]),
            _ => {}
        }
    }
    engine.shutdown();
    let store = Store::open(&path, b"disposable-qa-only")?;
    println!("INSTALLED identity={} conversations={}", store.has_credentials()?, store.list_threads()?.len());
    Ok(())
}
