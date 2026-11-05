//! Foreground-only LAN account linking. Discovery carries no account identifiers.
//! QR pins a random 256-bit PSK; Noise NNpsk0 authenticates both ephemeral peers.
//! The source stays active; the destination generates a fresh device/MLS identity.
use crate::{
    client::EventSink,
    command::{Command, Event},
    error::{CoreError, Result},
    keys::{Credentials, SecretKey},
    store::Store,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    net::{Ipv4Addr, SocketAddrV4},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream, UdpSocket},
    sync::{mpsc, oneshot},
    task::JoinHandle,
    time::{timeout, Instant},
};
use zeroize::{Zeroize, Zeroizing};

const PREFIX: &str = "valanium-pair:v2:";
/// Shown by the device that already has the account; a new device scans it and receives.
const OFFER_PREFIX: &str = "valanium-pair:v2-offer:";
const DISCOVERY_PORT: u16 = 49173;
const MAGIC: &str = "valanium-pair-discovery-v1";
const LIFETIME: Duration = Duration::from_secs(180);
const IO_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_FRAME: usize = 4096;
const MAX_PAYLOAD: usize = 2 * 1024 * 1024;
const MAX_RECORD: usize = 16 * 1024 * 1024;
const TRANSFER: Duration = Duration::from_secs(3600);
const NOISE: &str = "Noise_NNpsk0_25519_ChaChaPoly_SHA256";

tokio::task_local! {
    static MANAGER: RefCell<Manager>;
    static COMMANDS: mpsc::WeakUnboundedSender<Command>;
    static CANCELLATION: Arc<Cancellation>;
}

/// Abort LAN sockets from the UI thread even while the command loop is waiting
/// for a slow Tor connection. Handles never contain account data or DB handles.
#[derive(Default)]
pub(crate) struct Cancellation {
    sessions: std::sync::Mutex<Vec<tokio::task::AbortHandle>>,
    epoch: std::sync::atomic::AtomicU64,
    discovery: std::sync::Mutex<Vec<tokio::task::AbortHandle>>,
}
impl Cancellation {
    fn register(&self, task: &JoinHandle<()>, discovery: bool) {
        let handles = if discovery {
            &self.discovery
        } else {
            &self.sessions
        };
        if let Ok(mut handles) = handles.lock() {
            handles.retain(|handle| !handle.is_finished());
            handles.push(task.abort_handle());
        }
    }
    pub(crate) fn stop(&self, discovery: bool) {
        if !discovery {
            self.epoch.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }
        let handles = if discovery {
            &self.discovery
        } else {
            &self.sessions
        };
        if let Ok(mut handles) = handles.lock() {
            for handle in handles.drain(..) {
                handle.abort();
            }
        }
    }
}

struct Export {
    payload: Zeroizing<Vec<u8>>,
    history: Option<mpsc::Receiver<Result<Zeroizing<Vec<u8>>>>>,
}
impl Export {
    fn receiver() -> Self {
        Self {
            payload: Zeroizing::new(Vec::new()),
            history: None,
        }
    }
}

#[derive(Default)]
struct Manager {
    active: Option<Active>,
    discovery: Option<JoinHandle<()>>,
}
struct Active {
    id: String,
    sending: bool,
    epoch: u64,
    control: Option<oneshot::Sender<Option<Export>>>,
    task: JoinHandle<()>,
}
impl Drop for Active {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Drop for Manager {
    fn drop(&mut self) {
        if let Some(task) = self.discovery.take() {
            task.abort();
        }
    }
}

pub(crate) async fn scope(
    commands: mpsc::WeakUnboundedSender<Command>,
    cancellation: Arc<Cancellation>,
    future: impl std::future::Future<Output = ()>,
) {
    COMMANDS
        .scope(
            commands,
            CANCELLATION.scope(
                cancellation,
                MANAGER.scope(RefCell::new(Manager::default()), future),
            ),
        )
        .await;
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Invitation {
    id: String,
    addresses: Vec<Ipv4Addr>,
    port: u16,
    secret: String,
}
impl Drop for Invitation {
    fn drop(&mut self) {
        self.secret.zeroize();
    }
}

fn bad() -> CoreError {
    CoreError::Rejected("pairing_invalid".into())
}
fn io_error(_: impl std::fmt::Display) -> CoreError {
    CoreError::Transport("pairing_connection_lost".into())
}
fn is_lan(ip: Ipv4Addr) -> bool {
    ip.is_private() || ip.is_link_local() || (cfg!(test) && ip.is_loopback())
}
fn invitation(raw: &str, prefix: &str) -> Result<Invitation> {
    if raw.len() > 2048 {
        return Err(bad());
    }
    let value: Invitation =
        serde_json::from_str(raw.strip_prefix(prefix).ok_or_else(bad)?).map_err(|_| bad())?;
    if value.id.len() != 32
        || hex::decode(&value.id).map_err(|_| bad())?.len() != 16
        || hex::decode(&value.secret).map_err(|_| bad())?.len() != 32
        || value.port == 0
        || value.addresses.is_empty()
        || value.addresses.len() > 8
        || value.addresses.iter().any(|ip| !is_lan(*ip))
    {
        return Err(bad());
    }
    Ok(value)
}
fn interfaces() -> Vec<if_addrs::Ifv4Addr> {
    if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|interface| {
            if ["tun", "tap", "wg", "tailscale", "docker", "veth"]
                .iter()
                .any(|prefix| interface.name.starts_with(prefix))
            {
                return None;
            }
            match interface.addr {
                if_addrs::IfAddr::V4(addr) if is_lan(addr.ip) && !addr.ip.is_loopback() => {
                    Some(addr)
                }
                _ => None,
            }
        })
        .take(8)
        .collect()
}
fn local_peer(ip: Ipv4Addr) -> bool {
    is_lan(ip)
        && interfaces().iter().any(|addr| {
            u32::from(ip) & u32::from(addr.netmask) == u32::from(addr.ip) & u32::from(addr.netmask)
        })
}
fn prologue(id: &str) -> Vec<u8> {
    format!("valanium-account-link-v2:{id}").into_bytes()
}
fn handshake(id: &str, psk: &[u8], initiator: bool) -> Result<snow::HandshakeState> {
    let params = NOISE.parse().map_err(|_| bad())?;
    let context = prologue(id);
    let builder = snow::Builder::new(params).prologue(&context).psk(0, psk);
    if initiator {
        builder.build_initiator()
    } else {
        builder.build_responder()
    }
    .map_err(|_| bad())
}
async fn write_frame(stream: &mut TcpStream, data: &[u8]) -> Result<()> {
    if data.len() > MAX_FRAME {
        return Err(bad());
    }
    timeout(IO_TIMEOUT, async {
        let mut frame = Vec::with_capacity(data.len() + 4);
        frame.extend_from_slice(&(data.len() as u32).to_be_bytes());
        frame.extend_from_slice(data);
        stream.write_all(&frame).await
    })
    .await
    .map_err(io_error)?
    .map_err(io_error)
}
async fn read_frame(stream: &mut TcpStream) -> Result<Zeroizing<Vec<u8>>> {
    timeout(IO_TIMEOUT, async {
        let len = stream.read_u32().await.map_err(io_error)? as usize;
        if !(16..=MAX_FRAME).contains(&len) {
            return Err(bad());
        }
        let mut bytes = Zeroizing::new(vec![0; len]);
        stream.read_exact(&mut bytes).await.map_err(io_error)?;
        Ok(bytes)
    })
    .await
    .map_err(io_error)?
}
async fn send(
    stream: &mut TcpStream,
    noise: &mut snow::TransportState,
    plain: &[u8],
) -> Result<()> {
    let mut out = vec![0; plain.len() + 16];
    let len = noise.write_message(plain, &mut out).map_err(|_| bad())?;
    write_frame(stream, &out[..len]).await
}
async fn receive(
    stream: &mut TcpStream,
    noise: &mut snow::TransportState,
) -> Result<Zeroizing<Vec<u8>>> {
    let wire = read_frame(stream).await?;
    let mut plain = Zeroizing::new(vec![0; wire.len()]);
    let len = noise.read_message(&wire, &mut plain).map_err(|_| bad())?;
    plain.truncate(len);
    Ok(plain)
}
async fn exchange(
    stream: &mut TcpStream,
    invitation: &Invitation,
    initiator: bool,
) -> Result<(snow::TransportState, String)> {
    let psk = Zeroizing::new(hex::decode(&invitation.secret).map_err(|_| bad())?);
    let mut state = handshake(&invitation.id, &psk, initiator)?;
    let mut buffer = [0; MAX_FRAME];
    if initiator {
        let n = state.write_message(&[], &mut buffer).map_err(|_| bad())?;
        write_frame(stream, &buffer[..n]).await?;
        let wire = read_frame(stream).await?;
        state.read_message(&wire, &mut buffer).map_err(|_| bad())?;
    } else {
        let wire = read_frame(stream).await?;
        state.read_message(&wire, &mut buffer).map_err(|_| bad())?;
        let n = state.write_message(&[], &mut buffer).map_err(|_| bad())?;
        write_frame(stream, &buffer[..n]).await?;
    }
    let hash = Sha256::digest(state.get_handshake_hash());
    // 64-bit transcript comparison. The QR PSK is the main authenticator;
    // keep the manual comparison strong even if somebody photographs the QR.
    let code = hex::encode(&hash[..8])
        .to_uppercase()
        .as_bytes()
        .chunks(4)
        .map(|part| std::str::from_utf8(part).unwrap().to_owned())
        .collect::<Vec<_>>()
        .join(" ");
    Ok((state.into_transport_mode().map_err(|_| bad())?, code))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Link {
    identity: String,
    display_name: String,
    privacy: crate::privacy::Privacy,
    directory: crate::directory::Directory,
}
impl Drop for Link {
    fn drop(&mut self) {
        self.identity.zeroize();
    }
}
fn export_link(store: &Store) -> Result<Zeroizing<Vec<u8>>> {
    let credentials = store.load_credentials()?;
    let identity = Zeroizing::new(credentials.identity.to_bytes());
    let name = store
        .load_setting("own_display_name")?
        .and_then(|s| serde_json::from_slice::<String>(&s).ok())
        .unwrap_or_default();
    let privacy = match store.load_setting("privacy")? {
        Some(raw) => serde_json::from_slice(&raw).map_err(|_| bad())?,
        None => crate::privacy::Privacy::default(),
    };
    let directory = match store.load_setting("directory")? {
        Some(raw) => serde_json::from_slice(&raw).map_err(|_| bad())?,
        None => crate::directory::Directory::default(),
    };
    let payload = Zeroizing::new(serde_json::to_vec(&Link {
        identity: hex::encode(&*identity),
        display_name: name,
        privacy,
        directory,
    })?);
    if payload.len() > MAX_PAYLOAD {
        return Err(bad());
    }
    Ok(payload)
}
fn install_link(store: &Store, bytes: &[u8]) -> Result<()> {
    if store.has_credentials()? {
        return Err(CoreError::Rejected("identity_exists".into()));
    }
    let link: Link = serde_json::from_slice(bytes).map_err(|_| bad())?;
    let identity = Zeroizing::new(hex::decode(&link.identity).map_err(|_| bad())?);
    let name = crate::client::checked_display_name(&link.display_name)?;
    let credentials = Credentials {
        identity: SecretKey::from_bytes(&identity)?,
        device: SecretKey::generate(),
    };
    store.atomic(|| {
        store.save_credentials(&credentials)?;
        store.save_setting("own_display_name", &serde_json::to_vec(&name)?)?;
        store.save_setting("privacy", &serde_json::to_vec(&link.privacy)?)?;
        store.save_setting("directory", &serde_json::to_vec(&link.directory)?)
    })
}

async fn confirm(control: oneshot::Receiver<Option<Export>>) -> Result<Export> {
    timeout(LIFETIME, control)
        .await
        .map_err(io_error)?
        .map_err(io_error)?
        .ok_or_else(bad)
}
fn emit(sink: &EventSink, session: &str, state: &str, code: &str, qr: &str) {
    let qr_svg = if qr.is_empty() {
        String::new()
    } else {
        qrcode::QrCode::new(qr.as_bytes())
            .map(|q| {
                q.render::<qrcode::render::svg::Color>()
                    .min_dimensions(320, 320)
                    .build()
            })
            .unwrap_or_default()
    };
    sink(Event::Pairing {
        session: session.into(),
        state: state.into(),
        code: code.into(),
        qr: qr.into(),
        qr_svg,
    });
}

/// Waits for the scanning device on the listener. Only a device that is ready to
/// *receive* an account announces itself on the LAN; an offer stays silent.
async fn accept_peer(
    listener: TcpListener,
    invite: &Invitation,
    announce_ready: bool,
) -> Result<(TcpStream, snow::TransportState, String)> {
    let end = Instant::now() + LIFETIME;
    // The UDP task belongs to this listener and is aborted when it closes.
    let announcement = if announce_ready {
        announce(invite.id.clone(), invite.port).await
    } else {
        None
    };
    struct Abort(Option<JoinHandle<()>>);
    impl Drop for Abort {
        fn drop(&mut self) {
            if let Some(task) = self.0.take() {
                task.abort();
            }
        }
    }
    let _announcement = Abort(announcement);
    loop {
        let (mut stream, address) = tokio::time::timeout_at(end, listener.accept())
            .await
            .map_err(io_error)?
            .map_err(io_error)?;
        let _ = stream.set_nodelay(true);
        if !matches!(address.ip(), std::net::IpAddr::V4(ip) if is_lan(ip)) {
            continue;
        }
        // Failed PSK attempts never consume the valid QR. Each attempt is bounded.
        match exchange(&mut stream, invite, false).await {
            Ok((noise, code)) => return Ok((stream, noise, code)),
            Err(_) if Instant::now() < end => continue,
            Err(error) => return Err(error),
        }
    }
}
async fn connect_peer(invite: &Invitation) -> Result<(TcpStream, snow::TransportState, String)> {
    let mut connected = None;
    for ip in &invite.addresses {
        if let Ok(Ok(stream)) = timeout(
            Duration::from_secs(2),
            TcpStream::connect(SocketAddrV4::new(*ip, invite.port)),
        )
        .await
        {
            connected = Some(stream);
            break;
        }
    }
    let mut stream = connected.ok_or_else(|| io_error("connect"))?;
    let _ = stream.set_nodelay(true);
    let (noise, code) = exchange(&mut stream, invite, true).await?;
    Ok((stream, noise, code))
}
/// The destination side: generates its own device key and installs the account atomically.
async fn receive_account(
    mut stream: TcpStream,
    mut noise: snow::TransportState,
    code: String,
    session: String,
    control: oneshot::Receiver<Option<Export>>,
    commands: mpsc::WeakUnboundedSender<Command>,
    sink: EventSink,
) -> Result<()> {
    emit(&sink, &session, "verify", &code, "");
    confirm(control).await?;
    send(&mut stream, &mut noise, b"confirmed-v1").await?;
    receive_confirmation(&mut stream, &mut noise).await?;
    let payload = receive_payload(&mut stream, &mut noise).await?;
    if payload.is_empty() || payload.len() > MAX_PAYLOAD {
        return Err(bad());
    }
    emit(&sink, &session, "transferring", "0", "");
    let mut seq = 0u64;
    loop {
        let record = receive_payload(&mut stream, &mut noise).await?;
        if record.is_empty() {
            break;
        }
        let (done, completed) = oneshot::channel();
        commands
            .upgrade()
            .ok_or_else(bad)?
            .send(Command::PairingStage {
                session: session.clone(),
                seq,
                payload: record,
                done: std::sync::Mutex::new(Some(done)),
            })
            .map_err(|_| bad())?;
        if !timeout(Duration::from_secs(60), completed)
            .await
            .map_err(io_error)?
            .map_err(io_error)?
        {
            return Err(bad());
        }
        seq += 1;
        if seq % 32 == 0 {
            emit(&sink, &session, "transferring", &seq.to_string(), "");
        }
    }
    let (done, completed) = oneshot::channel();
    commands
        .upgrade()
        .ok_or_else(bad)?
        .send(Command::PairingInstall {
            session: session.clone(),
            payload,
            done: std::sync::Mutex::new(Some(done)),
        })
        .map_err(|_| bad())?;
    if !timeout(Duration::from_secs(600), completed)
        .await
        .map_err(io_error)?
        .map_err(io_error)?
    {
        return Err(bad());
    }
    // Local installation is durable before acknowledgement. A lost ACK does not
    // cause device-key reuse: this is a new device, never a cloned MLS snapshot.
    let _ = send(&mut stream, &mut noise, b"installed-v1").await;
    emit(&sink, &session, "complete", "", "");
    Ok(())
}
/// The source side: streams the confirmed export and waits for durable installation.
async fn send_account(
    mut stream: TcpStream,
    mut noise: snow::TransportState,
    code: String,
    session: String,
    control: oneshot::Receiver<Option<Export>>,
    sink: EventSink,
) -> Result<()> {
    emit(&sink, &session, "verify", &code, "");
    let mut exported = confirm(control).await?;
    send(&mut stream, &mut noise, b"confirmed-v1").await?;
    receive_confirmation(&mut stream, &mut noise).await?;
    send_payload(&mut stream, &mut noise, &exported.payload).await?;
    let mut history = exported.history.take().ok_or_else(bad)?;
    let mut count = 0u64;
    emit(&sink, &session, "transferring", "0", "");
    while let Some(record) = history.recv().await {
        let record = record?;
        send_payload(&mut stream, &mut noise, &record).await?;
        count += 1;
        if count % 32 == 0 {
            emit(&sink, &session, "transferring", &count.to_string(), "");
        }
    }
    send(&mut stream, &mut noise, &0u32.to_be_bytes()).await?;
    let wire = timeout(
        Duration::from_secs(600),
        read_frame_without_timeout(&mut stream),
    )
    .await
    .map_err(io_error)??;
    let mut confirmation = Zeroizing::new(vec![0; wire.len()]);
    let len = noise
        .read_message(&wire, &mut confirmation)
        .map_err(|_| bad())?;
    if &confirmation[..len] != b"installed-v1" {
        return Err(bad());
    }
    emit(&sink, &session, "complete", "", "");
    Ok(())
}
/// Clears staged history if the receiving task ends for any reason.
struct StageCleanup {
    session: String,
    commands: mpsc::WeakUnboundedSender<Command>,
}
impl Drop for StageCleanup {
    fn drop(&mut self) {
        if let Some(commands) = self.commands.upgrade() {
            let _ = commands.send(Command::PairingCleanup {
                session: self.session.clone(),
            });
        }
    }
}
/// New device shows the QR, account holder scans: listen, then receive.
async fn receiver(
    listener: TcpListener,
    invite: Invitation,
    control: oneshot::Receiver<Option<Export>>,
    commands: mpsc::WeakUnboundedSender<Command>,
    sink: EventSink,
) -> Result<()> {
    let _cleanup = StageCleanup { session: invite.id.clone(), commands: commands.clone() };
    let (stream, noise, code) = accept_peer(listener, &invite, true).await?;
    receive_account(stream, noise, code, invite.id.clone(), control, commands, sink).await
}
/// Account holder scans the new device's QR: connect, then send.
async fn sender(
    invite: Invitation,
    control: oneshot::Receiver<Option<Export>>,
    sink: EventSink,
) -> Result<()> {
    let (stream, noise, code) = connect_peer(&invite).await?;
    send_account(stream, noise, code, invite.id.clone(), control, sink).await
}
/// Account holder shows the QR (e.g. a PC), new device scans it with its camera: listen, then send.
async fn offerer(
    listener: TcpListener,
    invite: Invitation,
    control: oneshot::Receiver<Option<Export>>,
    sink: EventSink,
) -> Result<()> {
    let (stream, noise, code) = accept_peer(listener, &invite, false).await?;
    send_account(stream, noise, code, invite.id.clone(), control, sink).await
}
/// New device scans an offer QR: connect, then receive.
async fn joiner(
    invite: Invitation,
    control: oneshot::Receiver<Option<Export>>,
    commands: mpsc::WeakUnboundedSender<Command>,
    sink: EventSink,
) -> Result<()> {
    let _cleanup = StageCleanup { session: invite.id.clone(), commands: commands.clone() };
    let (stream, noise, code) = connect_peer(&invite).await?;
    receive_account(stream, noise, code, invite.id.clone(), control, commands, sink).await
}
async fn send_payload(
    stream: &mut TcpStream,
    noise: &mut snow::TransportState,
    payload: &[u8],
) -> Result<()> {
    if payload.is_empty() || payload.len() > MAX_RECORD {
        return Err(bad());
    }
    send(stream, noise, &(payload.len() as u32).to_be_bytes()).await?;
    for chunk in payload.chunks(MAX_FRAME - 16) {
        send(stream, noise, chunk).await?;
    }
    Ok(())
}
async fn receive_payload(
    stream: &mut TcpStream,
    noise: &mut snow::TransportState,
) -> Result<Zeroizing<Vec<u8>>> {
    let header = receive(stream, noise).await?;
    let bytes: [u8; 4] = header.as_slice().try_into().map_err(|_| bad())?;
    let len = u32::from_be_bytes(bytes) as usize;
    if len > MAX_RECORD {
        return Err(bad());
    }
    let mut payload = Zeroizing::new(Vec::with_capacity(len));
    while payload.len() < len {
        let chunk = receive(stream, noise).await?;
        if chunk.is_empty() || chunk.len() > len - payload.len() {
            return Err(bad());
        }
        payload.extend_from_slice(&chunk);
    }
    Ok(payload)
}

async fn receive_confirmation(
    stream: &mut TcpStream,
    noise: &mut snow::TransportState,
) -> Result<()> {
    let wire = timeout(LIFETIME, read_frame_without_timeout(stream))
        .await
        .map_err(io_error)??;
    let mut plain = [0; MAX_FRAME];
    let n = noise.read_message(&wire, &mut plain).map_err(|_| bad())?;
    if &plain[..n] != b"confirmed-v1" {
        return Err(bad());
    }
    Ok(())
}
async fn read_frame_without_timeout(stream: &mut TcpStream) -> Result<Vec<u8>> {
    let len = stream.read_u32().await.map_err(io_error)? as usize;
    if !(16..=MAX_FRAME).contains(&len) {
        return Err(bad());
    }
    let mut out = vec![0; len];
    stream.read_exact(&mut out).await.map_err(io_error)?;
    Ok(out)
}

#[derive(Serialize, Deserialize)]
struct Beacon {
    magic: String,
    nonce: String,
    session: String,
    port: u16,
}
async fn announce(session: String, port: u16) -> Option<JoinHandle<()>> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, DISCOVERY_PORT))
        .await
        .ok()?;
    Some(tokio::spawn(async move {
        let mut buffer = [0; 512];
        while let Ok((n, peer)) = socket.recv_from(&mut buffer).await {
            if !matches!(peer.ip(), std::net::IpAddr::V4(ip) if local_peer(ip)) {
                continue;
            }
            if let Ok(query) = serde_json::from_slice::<Beacon>(&buffer[..n]) {
                if query.magic != MAGIC || query.nonce.len() != 32 || !query.session.is_empty() {
                    continue;
                }
                let response = Beacon {
                    magic: MAGIC.into(),
                    nonce: query.nonce,
                    session: session.clone(),
                    port,
                };
                if let Ok(bytes) = serde_json::to_vec(&response) {
                    let _ = socket.send_to(&bytes, peer).await;
                }
            }
        }
    }))
}
async fn discover(sink: EventSink) {
    let Ok(socket) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await else {
        return;
    };
    if socket.set_broadcast(true).is_err() {
        return;
    }
    let mut seen = std::collections::HashSet::new();
    loop {
        let nonce = hex::encode(crate::crypto::random_bytes(16));
        let query = Beacon {
            magic: MAGIC.into(),
            nonce: nonce.clone(),
            session: String::new(),
            port: 0,
        };
        let bytes = serde_json::to_vec(&query).unwrap();
        for addr in interfaces() {
            if let Some(broadcast) = addr.broadcast {
                let _ = socket.send_to(&bytes, (broadcast, DISCOVERY_PORT)).await;
            }
        }
        let end = Instant::now() + Duration::from_secs(3);
        let mut buffer = [0; 512];
        while let Ok(Ok((n, peer))) =
            tokio::time::timeout_at(end, socket.recv_from(&mut buffer)).await
        {
            let Ok(beacon) = serde_json::from_slice::<Beacon>(&buffer[..n]) else {
                continue;
            };
            if beacon.magic != MAGIC
                || beacon.nonce != nonce
                || beacon.port == 0
                || beacon.session.len() != 32
                || !matches!(peer.ip(), std::net::IpAddr::V4(ip) if local_peer(ip))
                || seen.len() >= 32
            {
                continue;
            }
            if seen.insert(beacon.session.clone()) {
                sink(Event::PairingFound {
                    session: beacon.session,
                    address: peer.ip().to_string(),
                });
            }
        }
        tokio::time::sleep(Duration::from_secs(4)).await;
    }
}

/// Called synchronously on the core thread; no DB handle leaves that thread.
pub(crate) fn handle(command: &Command, store: &Store, sink: &EventSink) -> bool {
    let applicable = matches!(
        command,
        Command::PairingListen
            | Command::PairingOffer
            | Command::PairingConnect { .. }
            | Command::PairingConfirm { .. }
            | Command::PairingCancel
            | Command::PairingDiscover { .. }
    );
    if !applicable {
        return false;
    }
    let result = MANAGER
        .try_with(|cell| -> Result<()> {
            let mut manager = cell.borrow_mut();
            match command {
                Command::PairingDiscover { enabled } => {
                    if let Some(task) = manager.discovery.take() {
                        task.abort();
                    }
                    if *enabled {
                        let task = tokio::spawn(discover(Arc::clone(sink)));
                        CANCELLATION.with(|cancel| cancel.register(&task, true));
                        manager.discovery = Some(task);
                    }
                }
                Command::PairingCancel => {
                    if let Some(active) = manager.active.as_ref() {
                        store.clear_link_stage(Some(&active.id))?;
                    }
                    manager.active = None;
                    emit(sink, "", "cancelled", "", "");
                }
                Command::PairingConfirm { session, unlock } => {
                    let active = manager
                        .active
                        .as_mut()
                        .filter(|a| a.id == *session && !a.task.is_finished())
                        .ok_or_else(bad)?;
                    let payload = if active.sending {
                        let key = Zeroizing::new(hex::decode(unlock).map_err(|_| bad())?);
                        if !store.password_matches(&key)? {
                            return Err(CoreError::StoreLocked);
                        }
                        let payload = export_link(store)?;
                        let path = store.path();
                        let (tx, rx) = mpsc::channel(2);
                        tokio::task::spawn_blocking(move || {
                            let result = Store::open(&path, &key)
                                .and_then(|snapshot| snapshot.stream_link_snapshot(&tx));
                            if let Err(error) = result {
                                let _ = tx.blocking_send(Err(error));
                            }
                        });
                        Export {
                            payload,
                            history: Some(rx),
                        }
                    } else {
                        Export::receiver()
                    };
                    active
                        .control
                        .take()
                        .ok_or_else(bad)?
                        .send(Some(payload))
                        .map_err(|_| bad())?;
                }
                Command::PairingListen | Command::PairingOffer => {
                    // Listen: a new device waits for the account. Offer: the account holder waits.
                    let offering = matches!(command, Command::PairingOffer);
                    if store.has_credentials()? != offering {
                        return Err(if offering {
                            CoreError::NoCredentials
                        } else {
                            CoreError::Rejected("identity_exists".into())
                        });
                    }
                    manager.active = None;
                    if !offering {
                        store.clear_link_stage(None)?;
                    }
                    let addresses: Vec<_> = interfaces().into_iter().map(|a| a.ip).collect();
                    if addresses.is_empty() {
                        return Err(CoreError::Rejected("pairing_wifi_unavailable".into()));
                    }
                    // Tokio's bind does the OS bind immediately; convert a nonblocking
                    // std listener here so QR publication is synchronous and atomic.
                    let listener = std::net::TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0))
                        .map_err(io_error)?;
                    listener.set_nonblocking(true).map_err(io_error)?;
                    let port = listener.local_addr().map_err(io_error)?.port();
                    let listener = TcpListener::from_std(listener).map_err(io_error)?;
                    let invite = Invitation {
                        id: hex::encode(crate::crypto::random_bytes(16)),
                        addresses,
                        port,
                        secret: hex::encode(crate::crypto::random_bytes(32)),
                    };
                    let id = invite.id.clone();
                    let prefix = if offering { OFFER_PREFIX } else { PREFIX };
                    let qr = format!("{prefix}{}", serde_json::to_string(&invite)?);
                    let (control, confirmed) = oneshot::channel();
                    let callback = Arc::clone(sink);
                    let tx = COMMANDS.with(Clone::clone);
                    let session = id.clone();
                    let task = tokio::spawn(async move {
                        let flow = async {
                            if offering {
                                offerer(listener, invite, confirmed, Arc::clone(&callback)).await
                            } else {
                                receiver(listener, invite, confirmed, tx, Arc::clone(&callback)).await
                            }
                        };
                        if timeout(TRANSFER, flow).await.map_or(true, |r| r.is_err()) {
                            emit(&callback, &session, "failed", "", "");
                        }
                    });
                    CANCELLATION.with(|cancel| cancel.register(&task, false));
                    manager.active = Some(Active {
                        id: id.clone(),
                        sending: offering,
                        epoch: CANCELLATION
                            .with(|cancel| cancel.epoch.load(std::sync::atomic::Ordering::Acquire)),
                        control: Some(control),
                        task,
                    });
                    emit(sink, &id, "listening", "", &qr);
                }
                Command::PairingConnect { qr } => {
                    // The QR says which side shows it; the local account decides the rest.
                    // A mismatch (two sources or two empty devices) is refused before any I/O.
                    let sending = store.has_credentials()?;
                    let invite = if sending {
                        invitation(qr, PREFIX)?
                    } else {
                        store.clear_link_stage(None)?;
                        invitation(qr, OFFER_PREFIX)?
                    };
                    let id = invite.id.clone();
                    manager.active = None;
                    let (control, confirmed) = oneshot::channel();
                    let callback = Arc::clone(sink);
                    let tx = COMMANDS.with(Clone::clone);
                    let session = id.clone();
                    let task = tokio::spawn(async move {
                        let flow = async {
                            if sending {
                                sender(invite, confirmed, Arc::clone(&callback)).await
                            } else {
                                joiner(invite, confirmed, tx, Arc::clone(&callback)).await
                            }
                        };
                        if timeout(TRANSFER, flow).await.map_or(true, |r| r.is_err()) {
                            emit(&callback, &session, "failed", "", "");
                        }
                    });
                    CANCELLATION.with(|cancel| cancel.register(&task, false));
                    manager.active = Some(Active {
                        id: id.clone(),
                        sending,
                        epoch: CANCELLATION
                            .with(|cancel| cancel.epoch.load(std::sync::atomic::Ordering::Acquire)),
                        control: Some(control),
                        task,
                    });
                    emit(sink, &id, "connecting", "", "");
                }
                _ => unreachable!(),
            }
            Ok(())
        })
        .unwrap_or_else(|_| Err(bad()));
    if result.is_err() {
        sink(Event::Failed {
            code: "pairing".into(),
            message: "Не удалось выполнить сопряжение. Проверьте Wi-Fi, QR и повторите попытку."
                .into(),
        });
    }
    true
}

pub(crate) fn valid_receiver(session: &str) -> bool {
    MANAGER
        .try_with(|cell| {
            cell.borrow().active.as_ref().is_some_and(|a| {
                a.id == session
                    && !a.sending
                    && a.control.is_none()
                    && !a.task.is_finished()
                    && CANCELLATION.with(|cancel| {
                        cancel.epoch.load(std::sync::atomic::Ordering::Acquire) == a.epoch
                    })
            })
        })
        .unwrap_or(false)
}

pub(crate) fn install(session: &str, payload: &[u8], store: &Store) -> bool {
    MANAGER
        .try_with(|cell| {
            let manager = cell.borrow();
            let valid = manager.active.as_ref().is_some_and(|a| {
                a.id == session && !a.sending && a.control.is_none() && !a.task.is_finished()
            });
            valid
                && store
                    .atomic(|| {
                        install_link(store, payload)?;
                        store.install_link_history(session)?;
                        store.clear_link_stage(Some(session))?;
                        if !valid_receiver(session) {
                            return Err(bad());
                        }
                        Ok(())
                    })
                    .is_ok()
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::Engine;
    use std::sync::mpsc as sync;

    struct Device {
        engine: Option<Engine>,
        events: sync::Receiver<Event>,
        path: String,
    }
    impl Device {
        fn new(credentials: Option<&Credentials>) -> Self {
            let path = std::env::temp_dir()
                .join(format!(
                    "valanium-pair-test-{}.db",
                    hex::encode(crate::crypto::random_bytes(8))
                ))
                .to_string_lossy()
                .into_owned();
            {
                let store = Store::open(&path, b"local-key").unwrap();
                if let Some(c) = credentials {
                    store.save_credentials(c).unwrap();
                }
            }
            let (tx, events) = sync::channel();
            let engine = Engine::start(
                path.clone(),
                b"local-key".to_vec(),
                Arc::new(move |event| {
                    let _ = tx.send(event);
                }),
            )
            .unwrap();
            Self {
                engine: Some(engine),
                events,
                path,
            }
        }
        fn send(&self, command: Command) {
            self.engine.as_ref().unwrap().submit(command).unwrap();
        }
        fn wait(&self, expected: &str) -> (String, String) {
            let end = std::time::Instant::now() + Duration::from_secs(60);
            loop {
                match self
                    .events
                    .recv_timeout(end.saturating_duration_since(std::time::Instant::now()))
                    .unwrap()
                {
                    Event::Pairing {
                        session,
                        state,
                        code,
                        qr,
                        ..
                    } if state == expected => {
                        return (session, if expected == "listening" { qr } else { code })
                    }
                    Event::Failed { code, .. } if expected == "error" => {
                        return (code, String::new())
                    }
                    _ => {}
                }
            }
        }
        fn store(&self) -> Store {
            Store::open(&self.path, b"local-key").unwrap()
        }
    }
    impl Drop for Device {
        fn drop(&mut self) {
            if let Some(engine) = self.engine.take() {
                engine.shutdown();
            }
            for suffix in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{suffix}", self.path));
            }
        }
    }

    #[test]
    fn qr_rejects_public_addresses_bad_keys_and_oversized_input() {
        let mut invite = Invitation {
            id: "11".repeat(16),
            addresses: vec![Ipv4Addr::new(192, 168, 1, 5)],
            port: 1234,
            secret: "22".repeat(32),
        };
        let encode = |i: &Invitation| format!("{PREFIX}{}", serde_json::to_string(i).unwrap());
        assert!(invitation(&encode(&invite), PREFIX).is_ok());
        invite.addresses = vec![Ipv4Addr::new(8, 8, 8, 8)];
        assert!(invitation(&encode(&invite), PREFIX).is_err());
        invite.addresses = vec![Ipv4Addr::new(10, 0, 0, 5)];
        invite.secret = "00".into();
        assert!(invitation(&encode(&invite), PREFIX).is_err());
        assert!(invitation(&"x".repeat(2050), PREFIX).is_err());
        assert!(serde_json::from_str::<Command>(
            r#"{"type":"pairing_install","session":"fake","payload":[]}"#
        )
        .is_err());
    }

    #[test]
    fn linking_requires_both_confirmations_and_correct_unlock_and_never_clones_device() {
        if interfaces().is_empty() {
            return;
        } // No LAN interface in some isolated CI runners.
        let credentials = Credentials::generate();
        let source = Device::new(Some(&credentials));
        let target = Device::new(None);
        source
            .store()
            .save_setting(
                "own_display_name",
                &serde_json::to_vec("Андрей Владимирович").unwrap(),
            )
            .unwrap();
        let mut privacy = crate::privacy::Privacy::default();
        privacy.maximize();
        privacy.direct_messages.deny = (0..80).map(|i| format!("{i:064x}")).collect();
        let privacy_bytes = serde_json::to_vec(&privacy).unwrap();
        assert!(privacy_bytes.len() > MAX_FRAME);
        source
            .store()
            .save_setting("privacy", &privacy_bytes)
            .unwrap();
        let mut directory = crate::directory::Directory::default();
        directory.set(&"aa".repeat(32), crate::directory::Standing::Blocked, 1);
        directory.set(&"bb".repeat(32), crate::directory::Standing::Contact, 1);
        let directory_bytes = serde_json::to_vec(&directory).unwrap();
        source
            .store()
            .save_setting("directory", &directory_bytes)
            .unwrap();
        let chat = b"restored-chat";
        let peer = [0xaa; 32];
        let long =
            serde_json::json!({"type":"text","text":"history-content ".repeat(40_000)}).to_string();
        {
            let db = source.store();
            db.set_conversation(&peer, chat).unwrap();
            for i in 0..3 {
                db.insert_message(
                    &[i + 1; 16],
                    chat,
                    i % 2 == 0,
                    100 + i as i64,
                    long.as_bytes(),
                )
                .unwrap();
            }
            db.insert_message(&[9;16],chat,false,200,b"\xe2\x81\xa3OBS1:{\"v\":1,\"type\":\"image\",\"id\":\"photo\",\"data\":\"iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aW2kAAAAASUVORK5CYII=\"}").unwrap();
            db.save_mls(&[7; 32], b"old-device-state-must-not-be-cloned")
                .unwrap();
        }
        target.send(Command::PairingListen);
        let (id, qr) = target.wait("listening");
        source.send(Command::PairingConnect { qr: qr.clone() });
        let (_, source_code) = source.wait("verify");
        let (_, target_code) = target.wait("verify");
        assert_eq!(source_code, target_code);
        assert!(!target.store().has_credentials().unwrap());
        source.send(Command::PairingConfirm {
            session: id.clone(),
            unlock: hex::encode(b"wrong-key"),
        });
        source.wait("error");
        assert!(!target.store().has_credentials().unwrap());
        source.send(Command::PairingConfirm {
            session: id.clone(),
            unlock: hex::encode(b"local-key"),
        });
        // Confirming only the source must never expose/install credentials.
        std::thread::sleep(Duration::from_millis(100));
        assert!(!target.store().has_credentials().unwrap());
        target.send(Command::PairingConfirm {
            session: id,
            unlock: String::new(),
        });
        target.wait("complete");
        source.wait("complete");
        let linked = target.store().load_credentials().unwrap();
        assert_eq!(linked.identity.public(), credentials.identity.public());
        assert_ne!(linked.device.public(), credentials.device.public());
        {
            let restored = target.store();
            let items = restored.list_messages(chat, 20, None).unwrap();
            assert_eq!(items.len(), 4);
            assert!(items.iter().any(|message| message.body == long.as_bytes()));
            assert!(items
                .iter()
                .any(|message| String::from_utf8_lossy(&message.body).contains("iVBORw0KGgo")));
            assert!(restored.load_mls().unwrap().is_none());
            assert_eq!(restored.conversation_with(&peer).unwrap().unwrap(), chat);
        }

        assert_eq!(
            target.store().load_setting("directory").unwrap().unwrap(),
            directory_bytes
        );
        assert_eq!(
            target.store().load_setting("privacy").unwrap().unwrap(),
            privacy_bytes
        );
        assert_eq!(
            serde_json::from_slice::<String>(
                &target
                    .store()
                    .load_setting("own_display_name")
                    .unwrap()
                    .unwrap()
            )
            .unwrap(),
            "Андрей Владимирович"
        );
        assert_eq!(
            source.store().load_credentials().unwrap().device.public(),
            credentials.device.public()
        );
        // Consumed QR is closed, and repeating it cannot replace the installed key.
        source.send(Command::PairingConnect { qr });
        source.wait("failed");
        assert_eq!(
            target.store().load_credentials().unwrap().device.public(),
            linked.device.public()
        );
    }

    #[test]
    fn invalid_psk_does_not_consume_qr_and_cancellation_does_not_install_identity() {
        if interfaces().is_empty() {
            return;
        }
        let source = Device::new(Some(&Credentials::generate()));
        let target = Device::new(None);
        target.send(Command::PairingListen);
        let (_, qr) = target.wait("listening");
        let mut wrong = invitation(&qr, PREFIX).unwrap();
        wrong.secret = "00".repeat(32);
        source.send(Command::PairingConnect {
            qr: format!("{PREFIX}{}", serde_json::to_string(&wrong).unwrap()),
        });
        source.wait("failed");
        assert!(!target.store().has_credentials().unwrap());
        source.send(Command::PairingConnect { qr });
        source.wait("verify");
        target.wait("verify");
        target.send(Command::PairingCancel);
        target.wait("cancelled");
        source.send(Command::PairingCancel);
        source.wait("cancelled");
        assert!(!target.store().has_credentials().unwrap());
    }

    #[test]
    fn account_holder_can_show_the_qr_and_a_new_device_scans_it() {
        if interfaces().is_empty() {
            return;
        }
        let credentials = Credentials::generate();
        let source = Device::new(Some(&credentials));
        let target = Device::new(None);
        let chat = b"offered-chat";
        let peer = [0xbb; 32];
        {
            let db = source.store();
            db.set_conversation(&peer, chat).unwrap();
            db.insert_message(&[1; 16], chat, true, 100, b"from the desktop").unwrap();
        }
        source.send(Command::PairingOffer);
        let (id, qr) = source.wait("listening");
        assert!(qr.starts_with(OFFER_PREFIX));
        target.send(Command::PairingConnect { qr: qr.clone() });
        let (_, target_code) = target.wait("verify");
        let (_, source_code) = source.wait("verify");
        assert_eq!(source_code, target_code);
        // The receiving side never needs (or gets) the source's local unlock key.
        target.send(Command::PairingConfirm { session: id.clone(), unlock: String::new() });
        std::thread::sleep(Duration::from_millis(100));
        assert!(!target.store().has_credentials().unwrap());
        source.send(Command::PairingConfirm { session: id, unlock: hex::encode(b"local-key") });
        target.wait("complete");
        source.wait("complete");
        let linked = target.store().load_credentials().unwrap();
        assert_eq!(linked.identity.public(), credentials.identity.public());
        assert_ne!(linked.device.public(), credentials.device.public());
        let items = target.store().list_messages(chat, 10, None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].body, b"from the desktop");
    }

    #[test]
    fn mismatched_roles_are_refused_before_connecting() {
        if interfaces().is_empty() {
            return;
        }
        // Two account holders: an offer QR cannot be used to push our account onto theirs.
        let holder = Device::new(Some(&Credentials::generate()));
        let other = Device::new(Some(&Credentials::generate()));
        holder.send(Command::PairingOffer);
        let (_, offer) = holder.wait("listening");
        other.send(Command::PairingConnect { qr: offer });
        other.wait("error");
        // Two empty devices: a receive QR has no account behind it to join.
        let empty = Device::new(None);
        let fresh = Device::new(None);
        empty.send(Command::PairingListen);
        let (_, listen) = empty.wait("listening");
        fresh.send(Command::PairingConnect { qr: listen });
        fresh.wait("error");
        // An empty device cannot offer an account it does not have.
        fresh.send(Command::PairingOffer);
        fresh.wait("error");
        assert!(!fresh.store().has_credentials().unwrap());
    }

    #[test]
    fn occupied_store_and_invalid_payload_are_never_overwritten() {
        let device = Device::new(Some(&Credentials::generate()));
        let key = device.store().load_credentials().unwrap().device.public();
        assert!(install_link(&device.store(), br#"{"identity":"00","display_name":"x"}"#).is_err());
        assert_eq!(
            device.store().load_credentials().unwrap().device.public(),
            key
        );
        let empty = Device::new(None);
        assert!(install_link(
            &empty.store(),
            br#"{"identity":"invalid","display_name":"x"}"#
        )
        .is_err());
        assert!(!empty.store().has_credentials().unwrap());
    }

    #[tokio::test]
    async fn oversized_wire_frame_is_rejected_before_allocation() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            stream.write_u32(u32::MAX).await.unwrap();
        });
        let (mut stream, _) = listener.accept().await.unwrap();
        assert!(read_frame(&mut stream).await.is_err());
        task.await.unwrap();
    }
    #[test]
    fn noise_binds_session_and_rejects_modified_or_replayed_ciphertext() {
        let key = [7; 32];
        let mut a = handshake("session-a", &key, true).unwrap();
        let mut wrong = handshake("session-b", &key, false).unwrap();
        let mut frame = [0; MAX_FRAME];
        let mut out = [0; MAX_FRAME];
        let n = a.write_message(&[], &mut frame).unwrap();
        assert!(wrong.read_message(&frame[..n], &mut out).is_err());
        let mut a = handshake("same-session", &key, true).unwrap();
        let mut b = handshake("same-session", &key, false).unwrap();
        let n = a.write_message(&[], &mut frame).unwrap();
        b.read_message(&frame[..n], &mut out).unwrap();
        let n = b.write_message(&[], &mut frame).unwrap();
        a.read_message(&frame[..n], &mut out).unwrap();
        let mut a = a.into_transport_mode().unwrap();
        let mut b = b.into_transport_mode().unwrap();
        let n = a.write_message(b"credential-payload", &mut frame).unwrap();
        frame[n - 1] ^= 1;
        assert!(b.read_message(&frame[..n], &mut out).is_err());
        frame[n - 1] ^= 1;
        assert_eq!(b.read_message(&frame[..n], &mut out).unwrap(), 18);
        assert!(b.read_message(&frame[..n], &mut out).is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn unused_listener_expires_without_installing_anything() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let invite = Invitation {
            id: "11".repeat(16),
            addresses: vec![Ipv4Addr::LOCALHOST],
            port,
            secret: "22".repeat(32),
        };
        let (commands, mut events) = mpsc::unbounded_channel();
        let (_hold, confirmation) = oneshot::channel();
        let sink: EventSink = Arc::new(|_| {});
        let result = timeout(
            LIFETIME,
            receiver(listener, invite, confirmation, commands.downgrade(), sink),
        )
        .await;
        assert!(result.map_or(true, |result| result.is_err()));
        assert!(!matches!(
            events.try_recv(),
            Ok(Command::PairingInstall { .. })
        ));
        assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .is_err());
    }
    #[tokio::test]
    async fn cancellation_aborts_lan_tasks_without_waiting_for_command_processing() {
        let cancellation = Cancellation::default();
        let session = tokio::spawn(std::future::pending::<()>());
        let discovery = tokio::spawn(std::future::pending::<()>());
        cancellation.register(&session, false);
        cancellation.register(&discovery, true);
        cancellation.stop(false);
        assert!(session.await.unwrap_err().is_cancelled());
        assert!(!discovery.is_finished());
        cancellation.stop(true);
        assert!(discovery.await.unwrap_err().is_cancelled());
    }
}
