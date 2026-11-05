use super::{audio, Action, Media, Signal, TTL_MS};
use crate::{
    client::EventSink,
    command::{Command, Event},
    CoreError, Result,
};
use futures_util::{SinkExt, StreamExt};
use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    sync::mpsc,
    task::JoinHandle,
    time::{timeout, Instant},
};
use tokio_tungstenite::tungstenite::Message;
use zeroize::Zeroizing;
const RELAY: &str = "wss://valanium.com/ws";
const MAX_CALL: Duration = Duration::from_secs(7200);
tokio::task_local! { static STATE:RefCell<State>; static COMMANDS:mpsc::WeakUnboundedSender<Command>; static CANCEL:Arc<Cancellation>; }
#[derive(Default)]
pub(crate) struct Cancellation(std::sync::Mutex<Option<(String, tokio::task::AbortHandle)>>);
impl Cancellation {
    pub(crate) fn stop(&self, id: &str) {
        if let Ok(task) = self.0.lock() {
            if let Some((current, task)) = task.as_ref() {
                if current == id {
                    task.abort();
                }
            }
        }
    }
    fn register(&self, id: &str, task: &JoinHandle<()>) {
        if let Ok(mut current) = self.0.lock() {
            *current = Some((id.into(), task.abort_handle()));
        }
    }
}
#[derive(Default)]
struct State {
    active: Option<Active>,
    seen: HashMap<(String, String), i64>,
}
struct Frame {
    pcm: Zeroizing<Vec<i16>>,
    captured: Instant,
}
struct Active {
    peer: String,
    id: String,
    media: Media,
    caller: bool,
    phase: &'static str,
    expires: i64,
    alive: Arc<AtomicBool>,
    connected: Arc<AtomicBool>,
    muted: Arc<AtomicBool>,
    audio: Option<mpsc::Sender<Frame>>,
    task: JoinHandle<()>,
}
impl Drop for Active {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
        self.task.abort();
    }
}
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
fn bad() -> CoreError {
    CoreError::Rejected("call_state".into())
}
fn status(sink: &EventSink, peer: &str, id: &str, state: &str) {
    sink(Event::CallStatus {
        peer_device: peer.into(),
        call_id: id.into(),
        status: state.into(),
    });
}
fn signal(id: &str, action: Action, media: Option<Media>) -> Signal {
    Signal {
        v: 1,
        id: id.into(),
        action,
        expires_at: now() + TTL_MS,
        media,
    }
}
impl State {
    fn prune(&mut self) {
        self.seen.retain(|_, until| *until > now());
        if self
            .active
            .as_ref()
            .is_some_and(|a| !a.alive.load(Ordering::Acquire))
        {
            self.active = None;
        }
    }
    fn remember(&mut self, peer: &str, id: &str) {
        if self.seen.len() >= 256 {
            return;
        }
        self.seen
            .insert((peer.into(), id.into()), now() + TTL_MS + 5000);
    }
}
pub(crate) async fn scope(
    commands: mpsc::WeakUnboundedSender<Command>,
    cancel: Arc<Cancellation>,
    future: impl std::future::Future<Output = ()>,
) {
    COMMANDS
        .scope(
            commands,
            CANCEL.scope(cancel, STATE.scope(RefCell::new(State::default()), future)),
        )
        .await;
}
fn pending(
    peer: &str,
    id: &str,
    media: Media,
    caller: bool,
    sink: &EventSink,
    expires: i64,
) -> Active {
    let alive = Arc::new(AtomicBool::new(true));
    let timeout_alive = Arc::clone(&alive);
    let callback = Arc::clone(sink);
    let who = peer.to_owned();
    let call = id.to_owned();
    let task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis((expires - now()).max(1) as u64)).await;
        if timeout_alive.swap(false, Ordering::AcqRel) {
            status(&callback, &who, &call, "timeout");
        }
    });
    Active {
        peer: peer.into(),
        id: id.into(),
        media,
        caller,
        phase: if caller { "ringing" } else { "incoming" },
        expires,
        alive,
        connected: Arc::new(AtomicBool::new(false)),
        muted: Arc::new(AtomicBool::new(false)),
        audio: None,
        task,
    }
}
pub(crate) fn start(peer: &str, requested: Option<&str>, sink: &EventSink) -> Result<Signal> {
    STATE
        .try_with(|cell| {
            let mut state = cell.borrow_mut();
            state.prune();
            if state.active.is_some() {
                return Err(bad());
            }
            let id = requested
                .map(str::to_owned)
                .unwrap_or_else(|| hex::encode(crate::crypto::random_bytes(16)));
            if id.len() != 32
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            {
                return Err(bad());
            }
            let media = Media {
                room: hex::encode(crate::crypto::random_bytes(32)),
                secret: hex::encode(crate::crypto::random_bytes(32)),
            };
            let offer = signal(&id, Action::Offer, Some(media.clone()));
            state.active = Some(pending(peer, &id, media, true, sink, offer.expires_at));
            status(sink, peer, &id, "ringing");
            Ok(offer)
        })
        .map_err(|_| bad())?
}
fn connect(active: &mut Active, sink: &EventSink) {
    active.task.abort();
    active.phase = "connecting";
    let (tx, rx) = mpsc::channel(4);
    active.audio = Some(tx);
    let peer = active.peer.clone();
    let id = active.id.clone();
    let media = active.media.clone();
    let role = if active.caller { 0 } else { 1 };
    let callback = Arc::clone(sink);
    let alive = Arc::clone(&active.alive);
    let connected = Arc::clone(&active.connected);
    active.task = tokio::spawn(async move {
        let outcome = timeout(
            MAX_CALL,
            run_media(RELAY, &peer, &id, &media, role, rx, &callback, connected),
        )
        .await;
        if alive.swap(false, Ordering::AcqRel) {
            status(
                &callback,
                &peer,
                &id,
                if matches!(outcome, Ok(Ok(()))) {
                    "ended"
                } else {
                    "failed"
                },
            );
        }
    });
    CANCEL.with(|cancel| cancel.register(&active.id, &active.task));
    status(sink, &active.peer, &active.id, "connecting");
}
pub(crate) fn accept(id: &str, sink: &EventSink) -> Result<(String, Signal)> {
    STATE
        .try_with(|cell| {
            let mut state = cell.borrow_mut();
            state.prune();
            let active = state.active.as_mut().ok_or_else(bad)?;
            if active.id != id
                || active.caller
                || active.phase != "incoming"
                || active.expires <= now()
            {
                return Err(bad());
            }
            let peer = active.peer.clone();
            connect(active, sink);
            Ok((peer, signal(id, Action::Accept, None)))
        })
        .map_err(|_| bad())?
}
pub(crate) fn end(id: &str, reject: bool, sink: &EventSink) -> Result<(String, Signal)> {
    STATE
        .try_with(|cell| {
            let mut state = cell.borrow_mut();
            state.prune();
            if !state.active.as_ref().is_some_and(|a| a.id == id) {
                return Err(bad());
            }
            let active = state.active.take().unwrap();
            let peer = active.peer.clone();
            state.remember(&peer, id);
            status(sink, &peer, id, "ended");
            drop(active);
            Ok((
                peer,
                signal(id, if reject { Action::Reject } else { Action::End }, None),
            ))
        })
        .map_err(|_| bad())?
}
/// MLS has authenticated peer before this function. Privacy/approval are checked
/// by the caller. Responses never contain media keys; offers are single-use.
pub(crate) fn incoming(peer: &str, wire: &Signal, sink: &EventSink) -> Option<Signal> {
    STATE
        .try_with(|cell| {
            let mut state = cell.borrow_mut();
            state.prune();
            if !wire.fresh(now()) {
                return None;
            }
            if wire.action == Action::Offer {
                if state.seen.len() >= 256
                    || state.seen.contains_key(&(peer.into(), wire.id.clone()))
                {
                    return None;
                }
                state.remember(peer, &wire.id);
                if state.active.is_some() {
                    return Some(signal(&wire.id, Action::Busy, None));
                }
                state.active = Some(pending(
                    peer,
                    &wire.id,
                    wire.media.clone()?,
                    false,
                    sink,
                    wire.expires_at,
                ));
                status(sink, peer, &wire.id, "incoming");
                return None;
            }
            if matches!(wire.action, Action::End | Action::Reject | Action::Busy) {
                state.remember(peer, &wire.id);
            }
            let active = state.active.as_mut()?;
            if active.peer != peer || active.id != wire.id {
                return None;
            }
            match wire.action {
                Action::Accept if active.caller && active.phase == "ringing" => {
                    connect(active, sink)
                }
                Action::End | Action::Reject | Action::Busy => {
                    let label = match wire.action {
                        Action::Busy => "busy",
                        Action::Reject => "declined",
                        _ => "ended",
                    };
                    status(sink, peer, &wire.id, label);
                    state.active = None;
                }
                _ => {}
            }
            None
        })
        .ok()
        .flatten()
}
pub(crate) fn local(command: &Command, sink: &EventSink) -> bool {
    if let Command::CallEnd { call_id } | Command::CallReject { call_id } = command {
        if let Ok((peer, signal)) =
            end(call_id, matches!(command, Command::CallReject { .. }), sink)
        {
            let _ = COMMANDS.try_with(|commands| {
                if let Some(commands) = commands.upgrade() {
                    let _ = commands.send(Command::CallDispatch { peer, signal });
                }
            });
        }
        return true;
    }
    if !matches!(
        command,
        Command::CallAudio { .. } | Command::CallMute { .. } | Command::CallState
    ) {
        return false;
    }
    let _ = STATE.try_with(|cell| {
        let mut state = cell.borrow_mut();
        state.prune();
        match command {
            Command::CallState => {
                if let Some(a) = &state.active {
                    status(
                        sink,
                        &a.peer,
                        &a.id,
                        if a.connected.load(Ordering::Acquire) {
                            "active"
                        } else {
                            a.phase
                        },
                    );
                } else {
                    status(sink, "", "", "idle");
                }
            }
            Command::CallMute { call_id, muted } => {
                if let Some(a) = state.active.as_ref().filter(|a| &a.id == call_id) {
                    a.muted.store(*muted, Ordering::Release);
                }
            }
            Command::CallAudio {
                call_id,
                data,
                captured_at,
            } => {
                if let Some(a) = state
                    .active
                    .as_ref()
                    .filter(|a| &a.id == call_id && !a.muted.load(Ordering::Acquire))
                {
                    if captured_at.is_some_and(|at| {
                        now().saturating_sub(at) > 250 || at.saturating_sub(now()) > 5000
                    }) {
                        return;
                    }
                    if data.len() != audio::SAMPLES * 4 {
                        return;
                    }
                    if let Ok(bytes) = hex::decode(data) {
                        let pcm = Zeroizing::new(
                            bytes
                                .chunks_exact(2)
                                .map(|b| i16::from_le_bytes([b[0], b[1]]))
                                .collect(),
                        );
                        if let Some(tx) = &a.audio {
                            let _ = tx.try_send(Frame {
                                pcm,
                                captured: Instant::now(),
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    });
    true
}
async fn run_media(
    url: &str,
    peer: &str,
    id: &str,
    media: &Media,
    role: u8,
    mut input: mpsc::Receiver<Frame>,
    sink: &EventSink,
    connected: Arc<AtomicBool>,
) -> Result<()> {
    let mut socket = timeout(Duration::from_secs(15), crate::client::open_socket(url))
        .await
        .map_err(|_| bad())??;
    let hello = timeout(Duration::from_secs(10), socket.next())
        .await
        .map_err(|_| bad())?
        .ok_or_else(bad)?
        .map_err(|_| bad())?;
    let Message::Binary(hello) = hello else {
        return Err(bad());
    };
    if hello.first() != Some(&crate::proto::op::HELLO) {
        return Err(bad());
    }
    let hello: crate::proto::Hello = serde_json::from_slice(&hello[1..])?;
    if !hello.features.call_relay {
        return Err(CoreError::Rejected("call_server_unavailable".into()));
    }
    socket
        .send(Message::Binary(crate::proto::json_frame(
            crate::proto::op::CALL_JOIN,
            &serde_json::json!({"v":1,"room":media.room,"role":role}),
        )?))
        .await
        .map_err(|_| bad())?;
    let mut tx = audio::Cipher::new(&media.secret, id, role)?;
    let mut rx = audio::Cipher::new(&media.secret, id, 1 - role)?;
    let mut heartbeat = tokio::time::interval(Duration::from_secs(1));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last = Instant::now();
    let mut ready = false;
    loop {
        tokio::select! {
         packet=socket.next()=>{match packet{
          Some(Ok(Message::Binary(wire)))=>{if wire.first()==Some(&crate::proto::op::CALL_READY){ready=true;}else if wire.first()==Some(&crate::proto::op::CALL_MEDIA){
           let(kind,body)=rx.open(&wire[1..])?;last=Instant::now();if !connected.swap(true,Ordering::AcqRel){status(sink,peer,id,"active");}
           if kind==2{return Ok(());}if kind==1{let pcm=audio::decode(&body)?;let mut bytes=Zeroizing::new(Vec::with_capacity(pcm.len()*2));for sample in pcm.iter(){bytes.extend(sample.to_le_bytes());}sink(Event::CallAudio{call_id:id.into(),data:hex::encode(&*bytes)});}
          }else if wire.first()==Some(&crate::proto::op::ERROR){return Err(bad());}},
          Some(Ok(Message::Ping(value)))=>{socket.send(Message::Pong(value)).await.map_err(|_|bad())?;},
          Some(Ok(Message::Close(_)))|None=>return Ok(()),Some(Err(_))=>return Err(bad()),_=>{}
         }},
         _=heartbeat.tick()=>{if last.elapsed()>Duration::from_secs(if connected.load(Ordering::Acquire){30}else{90}){return Err(bad());}if ready{let packet=tx.seal(0,&[])?;timeout(Duration::from_secs(2),socket.send(Message::Binary(crate::proto::frame(crate::proto::op::CALL_MEDIA,&packet)))).await.map_err(|_|bad())?.map_err(|_|bad())?;}},
         pcm=input.recv()=>{let Some(pcm)=pcm else{return Ok(())};if ready && pcm.captured.elapsed()<Duration::from_millis(200){let encoded=audio::encode(&pcm.pcm)?;let packet=tx.seal(1,&encoded)?;timeout(Duration::from_secs(2),socket.send(Message::Binary(crate::proto::frame(crate::proto::op::CALL_MEDIA,&packet)))).await.map_err(|_|bad())?.map_err(|_|bad())?;}}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn signaling_binds_peer_rejects_replays_and_cancels_delayed_offers() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let cancel = Arc::new(Cancellation::default());
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed = events.clone();
        let sink: EventSink = Arc::new(move |e| observed.lock().unwrap().push(e));
        scope(tx.downgrade(), cancel, async {
            let offer = start("alice", None, &sink).unwrap();
            assert!(start("bob", None, &sink).is_err());
            let accepted = signal(&offer.id, Action::Accept, None);
            incoming("mallory", &accepted, &sink);
            STATE.with(|s| assert_eq!(s.borrow().active.as_ref().unwrap().phase, "ringing"));
            assert!(accept(&offer.id, &sink).is_err());
            end(&offer.id, false, &sink).unwrap();
            let delayed = signal(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                Action::Offer,
                offer.media.clone(),
            );
            incoming("alice", &signal(&delayed.id, Action::End, None), &sink);
            assert!(incoming("alice", &delayed, &sink).is_none());
            STATE.with(|s| assert!(s.borrow().active.is_none()));
            let fresh = signal(
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                Action::Offer,
                offer.media.clone(),
            );
            incoming("alice", &fresh, &sink);
            incoming("alice", &fresh, &sink);
            assert_eq!(
                events
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|e| matches!(e,Event::CallStatus{status,..}if status=="incoming"))
                    .count(),
                1
            );
            let other = signal(
                "cccccccccccccccccccccccccccccccc",
                Action::Offer,
                offer.media.clone(),
            );
            assert_eq!(incoming("bob", &other, &sink).unwrap().action, Action::Busy);
            assert!(end("wrong", false, &sink).is_err());
            end(&fresh.id, true, &sink).unwrap();
        })
        .await;
    }
    #[tokio::test]
    async fn cancellation_is_bound_to_the_current_call() {
        let cancel = Cancellation::default();
        let task = tokio::spawn(std::future::pending::<()>());
        cancel.register("current", &task);
        cancel.stop("old");
        assert!(!task.is_finished());
        cancel.stop("current");
        assert!(task.await.unwrap_err().is_cancelled());
    }
    #[tokio::test]
    async fn live_audio_crosses_two_real_websockets_encrypted_in_both_directions() {
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/ws", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            async fn join(
                listener: &TcpListener,
            ) -> tokio_tungstenite::WebSocketStream<tokio::net::TcpStream> {
                let (stream, _) = listener.accept().await.unwrap();
                let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
                let hello = serde_json::json!({"v":1,"nonce":"00".repeat(32),"heartbeatSec":30,"maxFrame":100000,"features":{"callRelay":true}});
                ws.send(Message::Binary(
                    crate::proto::json_frame(crate::proto::op::HELLO, &hello).unwrap(),
                ))
                .await
                .unwrap();
                let Message::Binary(join) = ws.next().await.unwrap().unwrap() else {
                    panic!()
                };
                assert_eq!(join[0], crate::proto::op::CALL_JOIN);
                let join: serde_json::Value = serde_json::from_slice(&join[1..]).unwrap();
                assert!(join.get("secret").is_none());
                ws
            }
            let mut a = join(&listener).await;
            let mut b = join(&listener).await;
            a.send(Message::Binary(vec![crate::proto::op::CALL_READY]))
                .await
                .unwrap();
            b.send(Message::Binary(vec![crate::proto::op::CALL_READY]))
                .await
                .unwrap();
            loop {
                tokio::select! {
                 frame=a.next()=>{let Some(Ok(frame))=frame else{break};if matches!(frame,Message::Close(_)){break;}if b.send(frame).await.is_err(){break;}},
                 frame=b.next()=>{let Some(Ok(frame))=frame else{break};if matches!(frame,Message::Close(_)){break;}if a.send(frame).await.is_err(){break;}},
                }
            }
        });
        let media = Media {
            room: "11".repeat(32),
            secret: "22".repeat(32),
        };
        let id = "cc".repeat(16);
        let (tx_a, rx_a) = mpsc::channel(4);
        let (tx_b, rx_b) = mpsc::channel(4);
        let (events, mut received) = mpsc::unbounded_channel();
        let ea = events.clone();
        let eb = events.clone();
        let sink_a: EventSink = Arc::new(move |e| {
            let _ = ea.send((0, e));
        });
        let sink_b: EventSink = Arc::new(move |e| {
            let _ = eb.send((1, e));
        });
        let ua = url.clone();
        let ma = media.clone();
        let ia = id.clone();
        let a = tokio::spawn(async move {
            run_media(
                &ua,
                "b",
                &ia,
                &ma,
                0,
                rx_a,
                &sink_a,
                Arc::new(AtomicBool::new(false)),
            )
            .await
        });
        let b = tokio::spawn(async move {
            run_media(
                &url,
                "a",
                &id,
                &media,
                1,
                rx_b,
                &sink_b,
                Arc::new(AtomicBool::new(false)),
            )
            .await
        });
        let mut connected = [false; 2];
        while !connected.iter().all(|x| *x) {
            let (i, e) = timeout(Duration::from_secs(5), received.recv())
                .await
                .unwrap()
                .unwrap();
            if matches!(e,Event::CallStatus{status,..}if status=="active") {
                connected[i] = true;
            }
        }
        let tone = |hz: f64| -> Vec<i16> {
            (0..audio::SAMPLES)
                .map(|i| ((i as f64 * std::f64::consts::TAU * hz / 16000.).sin() * 12000.) as i16)
                .collect()
        };
        let pa = tone(440.);
        let pb = tone(880.);
        tx_a.send(Frame {
            pcm: Zeroizing::new(pa.clone()),
            captured: Instant::now(),
        })
        .await
        .unwrap();
        tx_b.send(Frame {
            pcm: Zeroizing::new(pb.clone()),
            captured: Instant::now(),
        })
        .await
        .unwrap();
        let mut heard = [false; 2];
        while !heard.iter().all(|x| *x) {
            let (i, e) = timeout(Duration::from_secs(5), received.recv())
                .await
                .unwrap()
                .unwrap();
            if let Event::CallAudio { data, .. } = e {
                let bytes = hex::decode(data).unwrap();
                let pcm: Vec<i16> = bytes
                    .chunks_exact(2)
                    .map(|b| i16::from_le_bytes([b[0], b[1]]))
                    .collect();
                let expected = if i == 0 { &pb } else { &pa };
                let power: f64 = expected.iter().map(|x| (*x as f64).powi(2)).sum();
                let error: f64 = pcm
                    .iter()
                    .zip(expected.iter())
                    .map(|(x, y)| (*x as f64 - *y as f64).powi(2))
                    .sum();
                assert!(10. * (power / error).log10() > 20.);
                heard[i] = true;
            }
        }
        a.abort();
        b.abort();
        server.abort();
    }
}
