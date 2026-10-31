//! Disposable end-to-end delivery test. Creates two test identities on the
//! selected server; never opens user data.
//! Usage: live_delivery <new-dir> <socks> [--resume] [--maximum-privacy].
//! The optional maximum-privacy phase also checks capability bootstrap.
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::{
        mpsc::{channel, Receiver},
        Arc,
    },
    time::{Duration, Instant},
};
use valanium_core::{
    client::{Engine, EventSink},
    command::Command,
};

struct Client {
    engine: Option<Engine>,
    events: Receiver<Value>,
    device: String,
}
impl Drop for Client {
    fn drop(&mut self) {
        if let Some(engine) = self.engine.take() {
            let _ = engine.submit(Command::Disconnect);
        }
    }
}
impl Client {
    fn command(&self, value: Value) {
        self.engine
            .as_ref()
            .unwrap()
            .submit(serde_json::from_value::<Command>(value).unwrap())
            .unwrap();
    }
    fn wait(
        &self,
        predicate: impl Fn(&Value) -> bool,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let deadline = Instant::now() + Duration::from_secs(180);
        while Instant::now() < deadline {
            let event = self
                .events
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))?;
            if event["type"] == "failed" {
                eprintln!("Core failure: {}", event["code"]);
            }
            if predicate(&event) {
                return Ok(event);
            }
        }
        Err("event deadline exceeded".into())
    }
    fn authenticated(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.wait(|event| event["type"] == "authenticated")?;
        self.wait(|event| event["type"] == "registered")
            .map(|e| self.device = e["device"].as_str().unwrap().into())?;
        Ok(())
    }
    fn start(path: &Path, resume: bool) -> Result<Self, Box<dyn std::error::Error>> {
        let (tx, events) = channel();
        let label = path.file_stem().unwrap().to_string_lossy().into_owned();
        let sink: EventSink = Arc::new(move |event| {
            let value = serde_json::to_value(event).unwrap();
            if [
                "authenticated",
                "registered",
                "connected",
                "conversation_started",
                "delivery",
                "message",
                "failed",
                "disconnected",
            ]
            .contains(&value["type"].as_str().unwrap_or(""))
            {
                println!(
                    "{label}: {} {} {}",
                    value["type"],
                    value.get("state").unwrap_or(&Value::Null),
                    value.get("code").unwrap_or(&Value::Null)
                );
            }
            let _ = tx.send(value);
        });
        let engine = Engine::start(
            path.to_str().unwrap().into(),
            b"disposable-qa-only".to_vec(),
            sink,
        )?;
        let mut client = Self {
            engine: Some(engine),
            events,
            device: String::new(),
        };
        client.command(json!({"type":if resume { "connect" } else { "register" },"url":"valanium://onion","handle":null,"invite":null}));
        client.authenticated()?;
        Ok(client)
    }
}
fn exchange(from: &Client, to: &Client, id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let body = format!(
        "\u{2063}OBS1:{}",
        json!({"v":1,"type":"text","id":id,"text":format!("disposable test {id}")})
    );
    from.command(json!({"type":"send","recipient_device":to.device,"body":body}));
    let received =
        to.wait(|e| e["type"] == "message" && e["body"].as_str().is_some_and(|b| b == body))?;
    assert_eq!(received["sender_device"], from.device);
    from.wait(|e| {
        e["type"] == "delivery"
            && e["id"] == id
            && (e["state"] == "accepted" || e["state"] == "delivered")
    })?;
    println!("{id}: peer decrypted message, sender received server acknowledgement");
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let dir = Path::new(args.get(1).ok_or("new directory required")?);
    let resume = args.iter().any(|v| v == "--resume");
    if resume {
        if !dir.is_dir() {
            return Err("existing QA directory required".into());
        }
    } else {
        std::fs::create_dir(dir)?;
    }
    std::env::set_var(
        "VALANIUM_TOR_SOCKS",
        args.get(2).ok_or("local SOCKS required")?,
    );
    let mut alice = Client::start(&dir.join("alice.db"), resume)?;
    let mut bob = Client::start(&dir.join("bob.db"), resume)?;
    alice.command(json!({"type":"directory_set","device":bob.device,"standing":"approved"}));
    bob.command(json!({"type":"directory_set","device":alice.device,"standing":"approved"}));
    let baseline = if args.iter().any(|v|v=="--repair-missing") {
        let store=valanium_core::store::Store::open(dir.join("alice.db").to_str().unwrap(),b"disposable-qa-only")?;
        Some(store.load_mls()?.ok_or("Missing test snapshot")?)
    } else {None};
    let run = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis();
    exchange(&alice, &bob, &format!("first-contact-{run}"))?;
    exchange(&bob, &alice, &format!("reply-{run}"))?;
    if let Some((public,snapshot))=baseline {
        alice.engine.take().unwrap().shutdown();
        {
            let store=valanium_core::store::Store::open(dir.join("alice.db").to_str().unwrap(),b"disposable-qa-only")?;
            store.save_mls(&public,&snapshot)?;
        }
        alice=Client::start(&dir.join("alice.db"),true)?;
        bob.wait(|e|e["type"]=="conversation_started" && e["peer_device"]==alice.device)?;
        exchange(&bob,&alice,&format!("repaired-phone-to-pc-{run}"))?;
        exchange(&alice,&bob,&format!("repaired-pc-to-phone-{run}"))?;
        println!("Missing desktop channel repaired after restart; bidirectional delivery verified");
    }
    if args.iter().any(|v| v == "--maximum-privacy") {
        for client in [&alice, &bob] {
            client.command(json!({"type":"privacy_maximize"}));
            client.wait(|e| e["type"] == "privacy_maximized")?;
            client.command(json!({"type":"connect","url":"valanium://onion"}));
        }
        alice.authenticated()?;
        bob.authenticated()?;
        exchange(&alice, &bob, &format!("maximum-privacy-{run}"))?;
        println!("Delivery after maximum privacy verified");
    }
    println!("Live private delivery verified in both directions");
    Ok(())
}
