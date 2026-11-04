//! Замер скорости доставки через onion на двух одноразовых тестовых аккаунтах.
//! Данные пользователя не открываются; содержимое и ключи в вывод не попадают.
//!
//! Usage: onion_speed <new-dir> <socks> [rounds]
//!
//! Каждый раунд имитирует человека: простой дольше прежнего срока жизни
//! готового сокета (60 с), сигнал «печатает», 3 с набора, отправка. Затем
//! очередь из трёх сообщений подряд. Время — от команды send до расшифровки
//! у получателя.
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::{mpsc::{channel, Receiver}, Arc},
    time::{Duration, Instant},
};
use valanium_core::{client::{Engine, EventSink}, command::Command};

struct Client { engine: Option<Engine>, events: Receiver<Value>, device: String }
impl Drop for Client {
    fn drop(&mut self) {
        if let Some(engine) = self.engine.take() { let _ = engine.submit(Command::Disconnect); }
    }
}
impl Client {
    fn command(&self, value: Value) {
        self.engine.as_ref().unwrap().submit(serde_json::from_value::<Command>(value).unwrap()).unwrap();
    }
    fn wait(&self, predicate: impl Fn(&Value) -> bool) -> Result<Value, Box<dyn std::error::Error>> {
        let deadline = Instant::now() + Duration::from_secs(180);
        while Instant::now() < deadline {
            let event = self.events.recv_timeout(deadline.saturating_duration_since(Instant::now()))?;
            if predicate(&event) { return Ok(event); }
        }
        Err("event deadline exceeded".into())
    }
    fn start(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let (tx, events) = channel();
        let sink: EventSink = Arc::new(move |event| {
            let _ = tx.send(serde_json::to_value(event).unwrap());
        });
        let engine = Engine::start(path.to_str().unwrap().into(), b"disposable-qa-only".to_vec(), sink)?;
        let mut client = Self { engine: Some(engine), events, device: String::new() };
        client.command(json!({"type":"register","url":"valanium://onion","handle":null,"invite":null}));
        client.wait(|e| e["type"] == "authenticated")?;
        let registered = client.wait(|e| e["type"] == "registered")?;
        client.device = registered["device"].as_str().unwrap().into();
        Ok(client)
    }
}

fn body(id: &str) -> String {
    format!("\u{2063}OBS1:{}", json!({"v":1,"type":"text","id":id,"text":format!("disposable speed test {id}")}))
}

/// Отправляет и ждёт расшифровки у получателя. Возвращает миллисекунды.
fn deliver(from: &Client, to: &Client, ids: &[String]) -> Result<u128, Box<dyn std::error::Error>> {
    let started = Instant::now();
    for id in ids { from.command(json!({"type":"send","recipient_device":to.device,"body":body(id)})); }
    let mut left: Vec<String> = ids.iter().map(|id| body(id)).collect();
    while !left.is_empty() {
        let event = to.wait(|e| e["type"] == "message")?;
        if let Some(text) = event["body"].as_str() { left.retain(|b| b != text); }
    }
    Ok(started.elapsed().as_millis())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let dir = Path::new(args.get(1).ok_or("new directory required")?);
    std::fs::create_dir(dir)?;
    std::env::set_var("VALANIUM_TOR_SOCKS", args.get(2).ok_or("local SOCKS required")?);
    let rounds: usize = args.get(3).and_then(|v| v.parse().ok()).unwrap_or(3);

    let alice = Client::start(&dir.join("alice.db"))?;
    let bob = Client::start(&dir.join("bob.db"))?;
    alice.command(json!({"type":"directory_set","device":bob.device,"standing":"approved"}));
    bob.command(json!({"type":"directory_set","device":alice.device,"standing":"approved"}));
    let run = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis();

    let first = deliver(&alice, &bob, &[format!("first-{run}")])?;
    println!("SPEED first_contact_ms={first}");
    let reply = deliver(&bob, &alice, &[format!("reply-{run}")])?;
    println!("SPEED reply_ms={reply}");

    let mut single = Vec::new();
    let mut burst = Vec::new();
    for round in 0..rounds {
        std::thread::sleep(Duration::from_secs(60));
        alice.command(json!({"type":"typing","recipient_device":bob.device,"active":true}));
        std::thread::sleep(Duration::from_secs(3));
        let ms = deliver(&alice, &bob, &[format!("after-idle-{run}-{round}")])?;
        alice.command(json!({"type":"typing","recipient_device":bob.device,"active":false}));
        println!("SPEED round={round} after_idle_ms={ms}");
        single.push(ms);
        std::thread::sleep(Duration::from_secs(5));
        let ids: Vec<_> = (0..3).map(|n| format!("burst-{run}-{round}-{n}")).collect();
        let ms = deliver(&alice, &bob, &ids)?;
        println!("SPEED round={round} burst3_ms={ms}");
        burst.push(ms);
    }
    let median = |v: &mut Vec<u128>| { v.sort_unstable(); v[v.len() / 2] };
    println!("SPEED summary after_idle_median_ms={} burst3_median_ms={}", median(&mut single), median(&mut burst));
    Ok(())
}
