//! Движок: одно соединение, один цикл, всё состояние внутри задачи.
//!
//! Наружу — только команды и события. UI не видит ни сокета, ни ключей, ни
//! шифротекста: он оперирует открытым текстом, а граница доверия проходит
//! ровно здесь.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_socks::tcp::Socks5Stream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{client_async_tls_with_config, MaybeTlsStream, WebSocketStream};

use crate::command::{Command, ConversationItem, DeviceItem, Event, HistoryItem};
use crate::crypto::random_bytes;
use crate::error::{CoreError, Result};
use crate::keys::Credentials;
use crate::keys;
use crate::mls::{Incoming, Mls};
use crate::onion;
#[cfg(feature = "ton")]
use crate::proto::PayInfo;
use crate::proto::{self, op, AuthErr, AuthOk, AuthRequest, Hello, ServerError, ID_LEN, KEY_LEN};
use crate::store::Store;

pub type EventSink = Arc<dyn Fn(Event) + Send + Sync>;

trait NetworkStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> NetworkStream for T {}

type Socket = WebSocketStream<MaybeTlsStream<Box<dyn NetworkStream>>>;

/// Открывает WebSocket напрямую либо через локальный Tor SOCKS5.
///
/// Onion-имя никогда не резолвится системным DNS: строка назначения передаётся
/// SOCKS-прокси как имя. По умолчанию это Tor/Orbot на `127.0.0.1:9050`;
/// нестандартный порт можно задать `VALANIUM_TOR_SOCKS`.
async fn open_socket(url: &str) -> Result<Socket> {
    let request = crate::edge::ws_request(url)?;
    let uri = request.uri();
    let host = uri
        .host()
        .ok_or_else(|| CoreError::Transport("в адресе сервера нет имени хоста".into()))?
        .to_owned();
    let port = uri.port_u16().unwrap_or_else(|| if uri.scheme_str() == Some("wss") { 443 } else { 80 });

    // Если строка была задумана как onion-маршрут, но URL-парсер видит другой
    // host (userinfo/path/query), запрещаем прямой DNS/TCP вместо downgrade.
    if url.contains(".onion") && !valid_onion_host(&host) {
        return Err(CoreError::Transport("некорректный onion-адрес".into()));
    }
    let started = std::time::Instant::now();
    let stream: Box<dyn NetworkStream> = if valid_onion_host(&host) {
        let proxy = std::env::var("VALANIUM_TOR_SOCKS").unwrap_or_else(|_| "127.0.0.1:9050".into());
        Box::new(
            Socks5Stream::connect(proxy.as_str(), (host.as_str(), port))
                .await
                .map_err(|err| CoreError::Transport(format!("Tor SOCKS5 недоступен: {err}")))?,
        )
    } else {
        Box::new(
            TcpStream::connect((host.as_str(), port))
                .await
                .map_err(|err| CoreError::Transport(err.to_string()))?,
        )
    };

    delivery_timing(if valid_onion_host(&host) { "socks_connect" } else { "tcp_connect" }, started);
    client_async_tls_with_config(request, stream, None, None)
        .await
        .map(|(socket, _)| socket)
        .map_err(|err| CoreError::Transport(err.to_string()))
}

/// Cloudflare рвёт WS после ~100 с тишины, поэтому пинг обязателен. Если
/// сервер вдруг не назвал период — берём безопасное значение сами.
const FALLBACK_HEARTBEAT_SEC: u64 = 30;
const MAX_BACKOFF_SEC: u64 = 60;
/// Билетов на руках меньше — просим ещё, не дожидаясь, пока кончатся совсем:
/// доставка не должна упираться в пустой запас посреди набора сообщения.
const TICKET_LOW_WATERMARK: usize = 3;
/// Столько просим за раз. Сервер по умолчанию отдаёт до двадцати — берём с
/// запасом, но не всю пачку: несколько отправок между запросами это переживут.
const TICKET_BATCH_SIZE: u8 = 10;
/// Сколько ждать анонимную отправку целиком — открытие соединения (иногда
/// через Tor), HELLO, ответ. Дольше — не тише, а просто медленнее: сообщение
/// останется в запечатанной очереди до следующей анонимной попытки.
const ANON_SEND_TIMEOUT: Duration = Duration::from_secs(25);
const ANON_POOL_SIZE: usize = 2;
const ANON_READY_LIFETIME: Duration = Duration::from_secs(45);
const TICKET_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const TICKET_SEND_MARGIN_MS: i64 = 30_000;

struct PreparedAnonymous {
    route: String,
    started: tokio::time::Instant,
    socket: Socket,
    supports_passes: bool,
}

/// Внутренний адрес, который UI передаёт для автоматического выбора пути.
/// Он никогда не попадает в DNS или URL-парсер: `session` разворачивает его в
/// реальные маршруты перед каждой попыткой соединения.
const AUTO_ROUTE_URL: &str = "valanium://auto";
/// То же самое, но только через Tor: интерфейс просит режим, а не адрес.
///
/// Адресов у onion-входа больше одного, и знать их клиенту наперёд не нужно:
/// сервер называет свои в HELLO. Метка позволяет UI сказать «через Tor», не
/// вписывая в себя ни одного адреса.
const ONION_ROUTE_URL: &str = "valanium://onion";

/// Cloudflare распределяет этот маршрут между hop1 и hop3; оба ведут на main.
const DIRECT_ROUTES: [&str; 1] = ["wss://valanium.com/ws"];

/// Запасные onion-входы — на случай, когда HELLO ещё не получали ни разу.
///
/// Два независимых Tor-входа; сервер обновляет список подписанным HELLO.
const FALLBACK_ONION: [&str; 2] = [
    "ws://5kghvwyxzmtzba4foenmg5pkhcoxv6iq2c6wf4pbg5uyjrviwkckvead.onion/ws",
    "ws://5amnu2di3yhtpqcpbcoaabfbzotw3giap2lvoe5bi5juflzhzdrsq4ad.onion/ws",
];

/// Эти входы принадлежали выведенным из сети узлам. Они могли остаться в
/// подписанном локальном кеше старого клиента.
const RETIRED_ONION_HOSTS: [&str; 2] = [
    "ho2sji2l42eqclnmu6gtbbg5nvtrz5jvpr5nqkehbstshcmspsnfkiyd.onion",
    "anb5vtfi4ztizycwj6nnclo75kpjb4mhz4wmc6ax3zwy2xlz3slx26yd.onion",
];

/// Ключ настройки, где лежат onion-адреса, названные сервером.
const ONION_HOSTS_KEY: &str = "onion_hosts";

fn valid_onion_host(host: &str) -> bool {
    let Some(label) = host.strip_suffix(".onion") else { return false };
    label.len() == 56 && label.bytes().all(|b| b.is_ascii_lowercase() || (b'2'..=b'7').contains(&b))
}

/// Что перебирать в этом режиме.
///
/// Порядок для Auto — доступный Relay, затем два Tor-входа.
/// Для onion-режима — только Tor.
fn routes_for(url: &str, store: &Store) -> Vec<String> {
    let onion: Vec<String> = load_onion_hosts(store);
    match url {
        AUTO_ROUTE_URL => DIRECT_ROUTES
            .iter()
            .map(|route| (*route).to_owned())
            .chain(onion)
            .collect(),
        ONION_ROUTE_URL => onion,
        single => vec![single.to_owned()],
    }
}

/// Где помним время выпуска последнего принятого списка.
const ONION_ISSUED_AT_KEY: &str = "onion_hosts_issued_at";

/// Время выпуска последнего принятого списка. Ничего не принимали — `i64::MIN`,
/// то есть подойдёт любой.
fn accepted_issued_at(store: &Store) -> i64 {
    store
        .load_setting(ONION_ISSUED_AT_KEY)
        .ok()
        .flatten()
        .and_then(|raw| String::from_utf8(raw).ok())
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(i64::MIN)
}

/// Запоминает onion-входы, названные сервером, — если они подписаны.
///
/// Пустой список не стирает известное: сервер мог ответить старой сборкой, а
/// забыть адрес, по которому человек только и может подключиться, — худшее из
/// возможных решений. Стирается он только явной сменой на непустой список.
///
/// Ключ передаётся, а не берётся из константы, ровно ради проверяемости: тесту
/// нужен свой, иначе проверить приём подписанного списка было бы нечем.
fn remember_onion_hosts(
    store: &Store,
    hosts: &[String],
    signature: &str,
    issued_at: i64,
    public_key: &str,
    sink: &EventSink,
) {
    if hosts.is_empty() {
        return;
    }
    // Проверяем ровно то, что прислал сервер. Очистка до проверки дала бы
    // другое сообщение, и подпись перестала бы сходиться на ровном месте.
    if !onion::verify(signature, hosts, issued_at, public_key) {
        return;
    }
    // Откат. Подписанный список не перестаёт быть подписанным, когда
    // устаревает: без этой проверки сервер отдал бы старый список и увёл на
    // узел, который мы уже вывели из сети — возможно, потому что его изъяли.
    if issued_at < accepted_issued_at(store) {
        return;
    }
    let clean: Vec<&String> = hosts.iter().filter(|host| valid_onion_host(host)).collect();
    if clean.is_empty() {
        return;
    }
    if let Err(err) = store.save_setting(ONION_ISSUED_AT_KEY, issued_at.to_string().as_bytes()) {
        fail(sink, "storage", &err.to_string());
        return;
    }
    match serde_json::to_vec(&clean) {
        Ok(encoded) => {
            if let Err(err) = store.save_setting(ONION_HOSTS_KEY, &encoded) {
                fail(sink, "storage", &err.to_string());
            }
        }
        Err(err) => fail(sink, "encoding", &err.to_string()),
    }
}

/// Onion-входы: сначала названные сервером, потом запасные из сборки.
///
/// Запасные не выбрасываются даже когда список приехал: сервер мог назвать
/// узел, до которого именно у этого человека Tor не достучится.
fn load_onion_hosts(store: &Store) -> Vec<String> {
    let known: Vec<String> = store
        .load_setting(ONION_HOSTS_KEY)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default();

    let mut routes: Vec<String> = known
        .iter()
        .filter(|host| valid_onion_host(host) && !RETIRED_ONION_HOSTS.contains(&host.as_str()))
        .map(|host| format!("ws://{host}/ws"))
        .collect();
    for fallback in FALLBACK_ONION {
        if !routes.iter().any(|route| route == fallback) {
            routes.push(fallback.to_owned());
        }
    }
    routes
}
const DEFAULT_TTL_SEC: u32 = 14 * 24 * 3600;
/// Сколько KeyPackages держать на сервере.
///
/// Каждый расходуется одним новым собеседником и исчезает: переиспользовать
/// пакет нельзя, это ломает forward secrecy. Отсюда и способ навредить —
/// вычерпать чужой запас, чтобы с человеком нельзя было начать переписку.
/// Пяти для этого хватало любому, поэтому запас поднят: доливается он всё
/// равно только при подключении, а между подключениями брать больше уже не
/// дадут ограничители на стороне сервера.
const KEY_PACKAGES_PER_CONNECT: usize = 20;

/// Как представляться серверу при подключении.
enum Entry {
    /// Личность уже зарегистрирована.
    Existing,
    /// Первый вход: нужен пропуск.
    Register { handle: Option<String>, invite: Option<String>, payment_ref: Option<String> },
    /// Только выставить счёт и ждать оплаты.
    #[cfg(feature = "ton")]
    Invoice,
}

/// Отправка, ждущая KeyPackage собеседника либо восстановления соединения.
struct PendingSend {
    device: [u8; KEY_LEN],
    body: String,
    /// Своя копия уже лежит в базе. При повторе её не надо класть снова —
    /// иначе разрыв связи раздваивал бы сообщение в собственной переписке.
    stored: bool,
}

/// Не ушедшее из-за обрыва.
type Outbox = Vec<PendingSend>;

/// Зачем мы просили KeyPackage.
///
/// Раньше причина была одна — первое сообщение человеку, — и ответ сервера
/// однозначно означал «заводим беседу вдвоём». С группами тот же кадр может
/// прийти на приглашение, и перепутать эти два случая нельзя: во втором
/// заводить новую беседу не надо, надо добавить лист в существующую.
enum Claim {
    Start(PendingSend),
    Invite { group_id: Vec<u8>, device: [u8; KEY_LEN] },
}

/// Состояние, переживающее переподключение.
///
/// Живёт в `session`, а не в `pump`: отправной ящик и пул билетов имеют смысл
/// только между попытками соединения. Память о нечитаемых конвертах здесь
/// **не** живёт — это, в отличие от них, обязано пережить не переподключение,
/// а перезапуск всего приложения, и потому лежит на диске (`store`, см.
/// `on_envelope`).
#[derive(Default)]
struct Live {
    outbox: Outbox,
    /// Sealed sender (ARCHITECTURE.md §7a): непотраченные билеты. Живут в
    /// `Live`, а не в `pump`, по той же причине, что и `outbox`, — переживают
    /// переподключение. Свой короткий срок годности билеты и так несут сами;
    /// срок проверяется до отправки с запасом на установление Tor-соединения.
    tickets: VecDeque<[u8; proto::TICKET_LEN]>,
    ticket_request_until: Option<tokio::time::Instant>,
    anonymous_queued: bool,
    calls: crate::calls::Pending,
    anon_inflight: HashMap<[u8; ID_LEN], tokio::task::AbortHandle>,
    anon_retries: HashMap<[u8; ID_LEN], (tokio::time::Instant, u32)>,
    route_cooldowns: HashMap<String, tokio::time::Instant>,
    // Unauthenticated, single-use socket. Never shared between messages.
    prewarming: tokio::task::JoinSet<(String, Result<PreparedAnonymous>)>,
    prepared: VecDeque<PreparedAnonymous>,
    prewarm_retry_at: Option<tokio::time::Instant>,
    require_anonymous: bool,
}

impl Drop for Live {
    fn drop(&mut self) {
        self.prewarming.abort_all();
        for task in self.anon_inflight.values() { task.abort(); }
    }
}

fn invalidate_network(live: &mut Live, outcomes: &mut mpsc::UnboundedReceiver<AnonOutcome>) {
    live.prewarming.abort_all();
    live.prepared.clear();
    for (_,task) in live.anon_inflight.drain() { task.abort(); }
    while outcomes.try_recv().is_ok() {}
    live.anon_retries.clear();
    live.route_cooldowns.clear();
    live.prewarm_retry_at=None;
    live.ticket_request_until=None;
    // Ciphertexts and stable client references remain in the durable queue.
}

/// Анонимная отправка не удалась (билетов не было / Tor недоступен / сервер
/// отказал / соединение оборвалось) — сообщить об этом некому, кроме `pump`:
/// он обновляет запечатанную очередь и планирует следующую onion-попытку.
///
/// Канал, а не прямой вызов: отправка идёт своим, отдельным соединением и
/// своей задачей — она не может занимать `&mut Socket`/`&mut Mls` основного
/// соединения, они уже заняты циклом `pump`. Канал создаётся в `session()` и
/// переживает переподключение, поэтому исход, пришедший уже после разрыва
/// связи, не потеряется — дождётся следующего захода `pump`.
struct AnonOutcome {
    route: String,
    client_ref: [u8; ID_LEN],
    result: std::result::Result<[u8; ID_LEN], ()>,
}

/// Что нужно `encrypt_and_send`, чтобы попробовать отправить письмо в обход
/// уже открытого, аутентифицированного соединения.
///
/// Отдельная структура, а не россыпь параметров, ровно потому, что параметр
/// этот — не про одно сообщение, а про всю сессию: пул билетов и канал исхода
/// общие на всю связь и передаются по цепочке `on_send → deliver_to_person →
/// deliver → encrypt_and_send` реborrow'ом, без клонирования состояния.
struct SealedSenderCtx {
    required: bool,
}

/// Обрыв связи, а не отказ сервера. Такую ошибку лечит переподключение, и
/// продолжать писать в этот сокет бессмысленно: он уже закрыт.
///
/// Сбои MLS сюда не входят: связь при них цела, и переподключение их не чинит —
/// на новом соединении повторится то же самое.
fn is_transport(error: &CoreError) -> bool {
    matches!(error, CoreError::Transport(_))
}

pub struct Engine {
    commands: mpsc::UnboundedSender<Command>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Engine {
    /// Поднимает рантайм в отдельном потоке. Возвращается сразу.
    pub fn start(db_path: String, password: Vec<u8>, sink: EventSink) -> Result<Self> {
        // В итоговой Tauri-сборке зависимости могут включить одновременно
        // ring и aws-lc-rs. Rustls в таком случае намеренно не выбирает
        // провайдер сам и паникует при первом wss:// соединении. Клиент
        // использует ring явно на всех платформах.
        let _ = rustls::crypto::ring::default_provider().install_default();

        let (tx, rx) = mpsc::unbounded_channel();
        let ownership = crate::engine_lock::acquire(&db_path)?;
        let store = Store::open(&db_path, &password)?;

        let thread = std::thread::Builder::new()
            .name("valanium-core".into())
            .spawn(move || {
                let _ownership = ownership;
                let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                    Ok(runtime) => runtime,
                    Err(_) => {
                        sink(Event::Failed {
                            code: "runtime".into(),
                            message: "cannot start async runtime".into(),
                        });
                        return;
                    }
                };
                runtime.block_on(run(rx, store, sink));
            })
            .map_err(|_| CoreError::Transport("cannot spawn core thread".into()))?;

        Ok(Self { commands: tx, thread: Some(thread) })
    }

    pub fn submit(&self, command: Command) -> Result<()> {
        self.commands
            .send(command)
            .map_err(|_| CoreError::Transport("core thread is gone".into()))
    }

    /// Останавливает рантайм и ждёт освобождения SQLite-файлов.
    ///
    /// Обычный Disconnect разрывает только сеть: база остаётся открытой, что
    /// правильно для работы в трее, но не подходит для выхода из аккаунта.
    pub fn shutdown(mut self) {
        drop(self.commands);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Главный цикл. Живёт, пока UI не закроет канал команд.
/// Команды, которым не нужна сеть: они читают локальную базу и отвечают сразу.
/// `true` — команда обработана здесь, дальше её вести не надо.
///
/// Собрано в одно место намеренно. Раньше этот набор был продублирован в `run`
/// и в `pump`, и любая новая локальная команда молча получала «already
/// connected», если про второе место забывали, — на этом и попался
/// `recovery_code`, который нужен как раз тогда, когда клиент подключён.
fn handle_local(command: &Command, store: &Store, sink: &EventSink) -> bool {
    let restricting = match command {
        Command::PrivacyMaximize => true,
        Command::PrivacySet { privacy } => privacy.direct_messages.scope != crate::privacy::Scope::Everyone,
        _ => false,
    };
    if restricting {
        match restrictions_ready(store) {
            Ok(true) => {},
            _ => {
                fail(sink, "privacy_pass_exchange_pending",
                    "Сначала дождитесь обмена пропусками с контактами онлайн, затем повторите включение приватности. Настройки не изменены.");
                return true;
            }
        }
    }

    // Account entry and discovery are seed-only. Handle legacy commands locally,
    // before either the disconnected or connected network dispatcher sees them.
    if matches!(command, Command::UsernameSet { .. } | Command::UsernameLookup { .. }
        | Command::RecoverySetup { .. } | Command::RecoverPassword { .. } | Command::TotpSecret { .. }) {
        fail(sink, "seed_only", "Account recovery uses the 24-word seed phrase; usernames and password recovery are retired");
        return true;
    }
    match command {
        Command::VerificationConfirm { peer_device, proof } => {
            let result = (|| -> Result<bool> {
                let device = hex::decode(peer_device).map_err(|_| CoreError::BadFrame)?;
                if device.len() != KEY_LEN { return Err(CoreError::BadKeyLength); }
                ensure_pin_allows(store, peer_device)?;
                if store.conversation_with(&device)?.is_none() { return Err(CoreError::Rejected("no_conversation".into())); }
                let own = store.load_credentials()?.device.public();
                let token = keys::verification_payload(&own, &device);
                let digits = |s: &str| s.split_whitespace().collect::<String>();
                let matches = proof.trim() == token || digits(proof) == digits(&keys::safety_number(&own, &device));
                if matches { store.save_setting(&format!("verified:{peer_device}"), token.as_bytes())?; }
                Ok(matches)
            })();
            match result {
                Ok(verified) => sink(Event::VerificationChecked { peer_device: peer_device.clone(), verified }),
                Err(error) => fail(sink, "verification", &error.to_string()),
            }
        }
        Command::Status => {
            sink(status(store));
            sink(username_event(store));
        }

        Command::RecoveryCode => match store.load_credentials() {
            Ok(credentials) => sink(Event::RecoveryCode {
                code: crate::recovery::encode(&credentials.identity),
                words: crate::recovery::encode_words(&credentials.identity),
            }),
            Err(_) => fail(sink, "no_identity", "личности в этой базе ещё нет"),
        },

        Command::TotpSecret { login } => {
            // Сети не нужно: секрет заводится на устройстве и до подтверждения
            // кодом никуда не уходит.
            let secret = crate::totp::new_secret(&login);
            sink(Event::TotpSecret {
                readable: crate::totp::readable(&secret.base32),
                secret: secret.base32,
                url: secret.url,
            });
        }

        Command::AccountExport { password, unlock } => {
            // Сначала подтверждение, потом сбор: собирать архив, который затем
            // некому отдать, значило бы держать всю переписку в памяти зря.
            let confirmed = store.password_matches(unlock.as_bytes());
            match confirmed {
                Ok(false) => fail(sink, "account_export", "пароль устройства не подходит"),
                Err(err) => fail(sink, "account_export", &err.to_string()),
                Ok(true) => {
                    let built = store.export_archive().and_then(|archive| {
                        let count = archive.messages.len() as u64;
                        crate::migrate::seal(password, &archive).map(|file| (file, count))
                    });
                    match built {
                        Ok((file, messages)) => sink(Event::AccountExported {
                            data: hex::encode(file),
                            messages,
                        }),
                        Err(err) => fail(sink, "account_export", &err.to_string()),
                    }
                }
            }
        }
        Command::AccountImport { password, data } => {
            let restored = hex::decode(&data)
                .map_err(|_| CoreError::BadFrame)
                .and_then(|file| crate::migrate::open(&password, &file))
                .and_then(|archive| store.import_archive(&archive));
            match restored {
                Ok(messages) => sink(Event::AccountImported { messages: messages as u64 }),
                Err(err) => fail(sink, "account_import", &err.to_string()),
            }
        }
        Command::Storage => match store.counts() {
            Ok((conversations, messages)) => sink(Event::Storage {
                database_bytes: store.footprint(),
                conversations,
                messages,
            }),
            Err(err) => fail(sink, "storage", &err.to_string()),
        },
        Command::Groups => {
            // Список групп лежит на устройстве, и связь для него не нужна:
            // интерфейсу есть что показать сразу после запуска. Состав берётся
            // из снимка, который обновляется при каждом изменении группы.
            for (group_id, _kind, raw) in store.list_groups().unwrap_or_default() {
                if let Ok(meta) = serde_json::from_slice::<GroupMeta>(&raw) {
                    sink(Event::Group {
                        group: hex::encode(&group_id),
                        kind: meta.kind,
                        title: meta.title,
                        owner: meta.owner,
                        members: meta.members,
                    });
                }
            }
        }
        Command::PrivacyGet => sink(privacy_event(store)),

        Command::DirectoryList => sink(directory_event(store)),

        Command::AccessGet => sink(access_event(store)),

        Command::ClearConversation { conversation } => match hex::decode(conversation) {
            Ok(id) => match crate::anon_queue::cancel_thread(store, &id).and_then(|_| store.delete_conversation(&id)) {
                Ok(_) => sink(Event::ConversationCleared {
                    conversation: conversation.clone(),
                    forgotten: false,
                }),
                Err(err) => fail(sink, "storage", &err.to_string()),
            },
            Err(_) => fail(sink, "bad_conversation", "conversation must be hex"),
        },

        Command::DeleteConversation { conversation } => match hex::decode(conversation) {
            Ok(id) => match crate::anon_queue::forget_thread(store, &id).and_then(|_| store.forget_conversation(&id)) {
                Ok(()) => sink(Event::ConversationCleared {
                    conversation: conversation.clone(),
                    forgotten: true,
                }),
                Err(err) => fail(sink, "storage", &err.to_string()),
            },
            Err(_) => fail(sink, "bad_conversation", "conversation must be hex"),
        },

        Command::DirectorySet { device, standing } => {
            let mut directory = load_directory(store);
            directory.set(device, *standing, now_millis());
            match save_directory(store, &directory) {
                Ok(()) => sink(directory_event(store)),
                Err(err) => fail(sink, "storage", &err.to_string()),
            }
        }

        Command::PinAccept { name, device } => {
            match load_pins_checked(store) {
                Ok(mut pins) => {
                    if pins.accept(name, device, now_millis()) {
                        match save_pins(store, &pins) {
                            Ok(()) => sink(Event::PinAccepted {
                                name: name.clone(),
                                device: device.clone(),
                            }),
                            Err(err) => fail(sink, "storage", &err.to_string()),
                        }
                    } else {
                        fail(
                            sink,
                            "pin_not_pending",
                            "этот ключ не является ожидающим подтверждения результатом поиска",
                        );
                    }
                }
                Err(err) => fail(sink, "storage", &err.to_string()),
            }
        }

        Command::PinForget { name } => {
            match load_pins_checked(store) {
                Ok(mut pins) => {
                    pins.forget(name);
                    if let Err(err) = save_pins(store, &pins) {
                        fail(sink, "storage", &err.to_string());
                    }
                }
                Err(err) => fail(sink, "storage", &err.to_string()),
            }
        }

        Command::DirectoryForget { device } => {
            let mut directory = load_directory(store);
            directory.forget(device);
            match save_directory(store, &directory) {
                Ok(()) => sink(directory_event(store)),
                Err(err) => fail(sink, "storage", &err.to_string()),
            }
        }

        Command::PrivacyMaximize => {
            let result = (|| -> Result<crate::privacy::Privacy> {
                let mut privacy = load_privacy_checked(store)?;
                privacy.maximize();
                store.save_setting(PRIVACY_KEY, &serde_json::to_vec(&privacy)?)?;
                store.save_setting(DELIVERY_POLICY_KEY, b"onion")?;
                crate::anon_queue::drop_ephemeral(store)?;
                Ok(privacy)
            })();
            match result {
                Ok(privacy) => sink(Event::PrivacyMaximized { privacy }),
                Err(error) => fail(sink, "privacy_preset", &error.to_string()),
            }
        }

        Command::PrivacySet { privacy } => match serde_json::to_vec(privacy) {
            Ok(encoded) => match store.save_setting(PRIVACY_KEY, &encoded) {
                Ok(()) => sink(Event::Privacy { privacy: privacy.clone() }),
                Err(err) => fail(sink, "storage", &err.to_string()),
            },
            Err(err) => fail(sink, "encoding", &err.to_string()),
        },

        Command::Fingerprint { identity } => match hex::decode(identity) {
            Ok(bytes) => sink(Event::Fingerprint {
                fingerprint: crate::keys::fingerprint(&bytes),
                identity: identity.clone(),
            }),
            Err(_) => fail(sink, "bad_identity", "identity must be hex"),
        },

        // Список чатов — по нитям, а не по группам: у собеседника с телефоном
        // и ноутбуком групп две, а строка одна.
        /*
          Свои устройства.

          Отвечаем из того, что уже лежит: список приезжает при подключении и
          проверяется нашим же ключом личности. Отдельного похода в сеть нет —
          значит экран открывается и без связи, показывая то, что было в
          прошлый раз. Пустой список — не ошибка: список ещё не приезжал, либо
          сервер старый и такого не умеет.

          Порядок — по времени появления. Не по свежести: она меняется от
          каждого подключения, и строки прыгали бы при каждом открытии экрана.
        */
        Command::Devices => {
            let mine = store
                .load_credentials()
                .map(|creds| hex::encode(creds.device_pub()))
                .unwrap_or_default();
            let mut devices: Vec<DeviceItem> = own_devices(store)
                .into_iter()
                .map(|(device, (_cert, added_at))| DeviceItem {
                    current: device == mine,
                    device,
                    added_at,
                })
                .collect();
            devices.sort_by_key(|item| item.added_at);
            sink(Event::Devices { devices });
        }

        Command::Conversations => match store.list_threads() {
            Ok(items) => sink(Event::Conversations {
                items: items
                    .into_iter()
                    .map(|(peer_device, conversation)| {
                        let last = store
                            .list_messages(&conversation, 1, None)
                            .ok()
                            .and_then(|mut rows| rows.pop());
                        ConversationItem {
                            peer_device: hex::encode(peer_device),
                            conversation: hex::encode(conversation),
                            last_body: last.as_ref().map(|row| {
                                String::from_utf8_lossy(&row.body).into_owned()
                            }),
                            last_at: last.map(|row| row.created_at),
                        }
                    })
                    .collect(),
            }),
            Err(err) => fail(sink, "storage", &err.to_string()),
        },

        Command::History { conversation, limit, before } => match hex::decode(conversation) {
            Ok(id) => match store.list_messages(&id, *limit, parse_cursor(before)) {
                Ok(rows) => {
                    // Отдали ровно столько, сколько просили, — значит дальше,
                    // скорее всего, есть ещё. Лишний пустой запрос дешевле, чем
                    // лишний счётный проход по всей переписке.
                    let has_more = rows.len() as i64 >= *limit;
                    sink(Event::History {
                        conversation: conversation.clone(),
                        has_more,
                        messages: rows
                            .into_iter()
                            .map(|row| HistoryItem {
                                delivery: if row.outgoing {
                                    content_json(&String::from_utf8_lossy(&row.body))
                                        .and_then(|v| v.get("id")?.as_str().map(str::to_owned))
                                        .and_then(|message_id| crate::delivery::state(store, &id, &message_id).ok().flatten())
                                } else { None },
                                id: hex::encode(&row.id),
                                cursor: format!("{}:{}", row.created_at, row.seq),
                                outgoing: row.outgoing,
                                created_at: row.created_at,
                                body: String::from_utf8_lossy(&row.body).into_owned(),
                            })
                            .collect(),
                    })
                }
                Err(err) => fail(sink, "storage", &err.to_string()),
            },
            Err(_) => fail(sink, "bad_conversation", "conversation must be hex"),
        },

        _ => return false,
    }
    true
}

async fn run(mut commands: mpsc::UnboundedReceiver<Command>, store: Store, sink: EventSink) {
    while let Some(command) = commands.recv().await {
        if handle_local(&command, &store, &sink) {
            continue;
        }
        match command {
            Command::Recover { url, code } => match recover(&store, &code) {
                Ok(()) => session(&url, Entry::Existing, &store, &sink, &mut commands).await,
                Err(err) => fail(&sink, recovery_code_of(&err), &err.to_string()),
            },
            Command::RecoverPassword { url, login, password, code } => {
                match recover_with_password(&store, &url, &login, &password, code.as_deref()).await {
                    Ok(()) => session(&url, Entry::Existing, &store, &sink, &mut commands).await,
                    Err(err) => fail(&sink, password_code_of(&err), &err.to_string()),
                }
            }
            Command::Verify { peer_device } => {
                // Сверка не требует сети: всё нужное лежит в локальном снимке.
                match load_or_create(&store, &Entry::Existing).and_then(|c| load_or_create_mls(&store, &c)) {
                    Ok(mls) => sink(verification(&mls, &store, &peer_device)),
                    Err(err) => fail(&sink, "keys", &err.to_string()),
                }
            }
            #[cfg(feature = "ton")]
            Command::RequestInvoice { url } => {
                session(&url, Entry::Invoice, &store, &sink, &mut commands).await;
            }
            Command::Register { url, handle: _, invite, payment_ref } => {
                session(&url, Entry::Register { handle: None, invite, payment_ref }, &store, &sink, &mut commands)
                    .await;
            }
            Command::Connect { url } => {
                session(&url, Entry::Existing, &store, &sink, &mut commands).await;
            }
            Command::Disconnect => sink(Event::Disconnected { reason: "by request".into() }),
            Command::NetworkChanged | Command::ProbeConnection => {},
            Command::Send { recipient_device, body } => {
                queue_offline(&store, &sink, &recipient_device, &body);
            }
            Command::ProfileGet { .. }
            | Command::ProfileSet { .. }
            | Command::ProfileDecor { .. }
            | Command::AdminGet { .. }
            | Command::SupportGet { .. }
            | Command::SupportMark { .. }
            | Command::ChannelCreate { .. }
            | Command::ChannelPublish { .. }
            | Command::ChannelList
            | Command::ChannelFeed { .. }
            | Command::ChannelSubscribe { .. }
            | Command::ChannelFind { .. }
            | Command::ChannelDeletePost { .. }
            | Command::ChannelDelete { .. }
            | Command::ChannelUpdate { .. }
            | Command::ChannelAdmin { .. }
            | Command::RevokeDevice { .. }
            | Command::RevokeOtherDevices
            | Command::AdminAction { .. }
            | Command::RecoverySetup { .. }
            | Command::DeleteMessage { .. }
            | Command::EditMessage { .. }
            | Command::CallProbe { .. }
            | Command::Typing { .. }
            | Command::AccessSet { .. }
            | Command::PassInvite { .. }
            | Command::PassRevoke { .. }
            | Command::UsernameSet { .. }
            | Command::UsernameClear
            | Command::UsernameLookup { .. }
            | Command::RecoveryForget => {
                fail(&sink, "not_connected", "connect before using network features")
            }
            other => {
                // Досюда доходят только команды, которые уже забрал handle_local,
                // то есть не доходит ничего. Ветка явная, а не `_ => {}`: если
                // появится новая команда и её забудут развести, это будет видно
                // событием, а не тишиной — ровно так потерялся recovery_code.
                fail(&sink, "unhandled", &format!("команда не обработана: {other:?}"));
            }
        }
    }
}

/// Собирает то, что собеседники сравнивают между собой.
///
/// Два числа с разным назначением. `safety_number` считается от пары ключей
/// устройств и держится, пока ключи не сменились, — его сверяют один раз при
/// знакомстве. `epoch_code` выведен из секрета текущей эпохи MLS: он совпадает
/// у участников исправной беседы и меняется на каждом коммите, поэтому им
/// проверяют «мы прямо сейчас в одном состоянии».
fn verification(mls: &Mls, store: &Store, peer_device: &str) -> Event {
    let Ok(device) = hex::decode(peer_device) else {
        return failure("bad_device", "device must be hex");
    };
    let group = match store.conversation_with(&device) {
        Ok(Some(group)) => group,
        Ok(None) => return failure("no_conversation", "сверять нечего: беседа ещё не заведена"),
        Err(err) => return failure("storage", &err.to_string()),
    };
    let snapshot = match mls.inspect(&group) {
        Ok(snapshot) => snapshot,
        Err(err) => return failure("inspect", &err.to_string()),
    };

    Event::Verification {
        qr_payload: keys::verification_payload(&mls.device_pub(), &device),
        verified: store.load_setting(&format!("verified:{peer_device}")).ok().flatten()
            .is_some_and(|saved| saved == keys::verification_payload(&mls.device_pub(), &device).as_bytes()),
        safety_number: keys::safety_number(&mls.device_pub(), &device),
        epoch: snapshot.epoch,
        epoch_code: keys::fingerprint(&snapshot.epoch_authenticator),
        members: snapshot.members.iter().map(hex::encode).collect(),
        peer_device: peer_device.to_owned(),
    }
}

/// Состав беседы обязан оставаться прежним: мы и тот, с кем переписываемся.
/// Лишний лист в диалоге один на один — это ровно то, как выглядит атака с
/// участием сервера.
fn check_membership(mls: &Mls, store: &Store, group_id: &[u8], sink: &EventSink) {
    let snapshot = match mls.inspect(group_id) {
        Ok(snapshot) => snapshot,
        Err(err) => {
            sink(Event::Anomaly { kind: "inspect".into(), detail: err.to_string() });
            return;
        }
    };
    if snapshot.members.len() != 2 {
        sink(Event::Anomaly {
            kind: "member_set".into(),
            detail: format!("в беседе {} участников вместо двух", snapshot.members.len()),
        });
        return;
    }
    let Ok(Some(expected)) = store.peer_of_conversation(group_id) else { return };
    if !snapshot.members.iter().any(|member| member.as_slice() == expected.as_slice()) {
        sink(Event::Anomaly {
            kind: "peer_changed".into(),
            detail: "устройство собеседника в беседе не то, что было".into(),
        });
    }
}

/// Курсор страницы: `"<время>:<позиция>"`. Мусор трактуется как «с начала» —
/// пустой список выглядел бы для человека как потерянная переписка.
fn parse_cursor(raw: &Option<String>) -> Option<(i64, i64)> {
    let (time, seq) = raw.as_ref()?.split_once(':')?;
    Some((time.parse().ok()?, seq.parse().ok()?))
}

fn failure(code: &str, message: &str) -> Event {
    Event::Failed { code: code.to_owned(), message: message.to_owned() }
}

/// Кладёт восстановленную личность в пустую базу.
///
/// Ключ устройства создаётся новый: восстанавливается личность, а не старое
/// устройство. Занятую базу трогать нельзя — иначе восстановление затёрло бы
/// личность, которая там уже живёт, и человек потерял бы доступ вместо того,
/// чтобы его вернуть.
fn recover(store: &Store, code: &str) -> Result<()> {
    if store.has_credentials()? {
        return Err(CoreError::Rejected("identity_exists".into()));
    }
    let identity = crate::recovery::decode(code)?;
    store.save_credentials(&Credentials { identity, device: crate::keys::SecretKey::generate() })
}

/// Машиночитаемый повод отказа: интерфейсу нужно отличать опечатку в коде от
/// занятой базы.
fn recovery_code_of(error: &CoreError) -> &str {
    match error {
        CoreError::BadRecoveryCode(_) => "bad_recovery_code",
        // Сюда попадают и коды сервера: они уже машиночитаемые слаги.
        CoreError::Rejected(reason) => reason,
        _ => "recover",
    }
}

/// То же для входа по паролю. Отдельный код нужен интерфейсу: «не подошёл
/// пароль» и «код набран с ошибкой» ведут к разным экранам.
fn password_code_of(error: &CoreError) -> &str {
    match error {
        CoreError::BadRecoveryCode(_) => "bad_password",
        CoreError::Rejected(reason) => reason,
        _ => "recover",
    }
}

/// Убирает сообщение из локальной базы и сообщает об этом интерфейсу.
///
/// Возвращает устройство собеседника — оно понадобится, если удалить просят и
/// у него.
/// Заменяет тело у себя и сообщает, кому уходит просьба.
fn edit_locally(
    store: &Store,
    sink: &EventSink,
    conversation: &str,
    id: &str,
    body: &str,
) -> Result<Option<[u8; KEY_LEN]>> {
    let group = hex::decode(conversation).map_err(|_| CoreError::BadFrame)?;
    if crate::anon_queue::contains_message(store, &group, id)? {
        return Err(CoreError::Rejected("Дождитесь отправки или удалите сообщение из очереди перед изменением".into()));
    }
    if store.update_message_by_id(&group, id, body.as_bytes())? {
        sink(Event::Edited {
            conversation: conversation.to_owned(),
            id: id.to_owned(),
            body: body.to_owned(),
        });
    }
    let peer = store
        .peer_of_conversation(&group)?
        .and_then(|raw| raw.try_into().ok());
    Ok(peer)
}

fn delete_locally(
    store: &Store,
    sink: &EventSink,
    conversation: &str,
    id: &str,
) -> Result<Option<[u8; KEY_LEN]>> {
    let group = hex::decode(conversation).map_err(|_| CoreError::BadFrame)?;
    crate::anon_queue::cancel_message(store, &group, id)?;
    if store.delete_message_by_id(&group, id)? {
        sink(Event::Deleted {
            conversation: conversation.to_owned(),
            ids: vec![id.to_owned()],
        });
    }
    let peer = store
        .peer_of_conversation(&group)?
        .and_then(|raw| raw.try_into().ok());
    Ok(peer)
}

/// Объявляет присутствие тем, кому это разрешено.
///
/// Правило «сейчас в сети» проверяется здесь, у отправителя: скрыть себя может
/// только тот, о ком речь. Тем, кому не разрешено, сигнал просто не уходит — и
/// у них не появится ни строчки о нас.
async fn announce_presence(
    socket: &mut Socket,
    store: &Store,
    mls: &mut Mls,
    sink: &EventSink,
    outbox: &mut Outbox,
) -> Result<()> {
    let privacy: crate::privacy::Privacy = store
        .load_setting(PRIVACY_KEY)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default();
    let directory = load_directory(store);

    for (device, entry) in directory.entries.iter() {
        if !privacy.presence.permits(device, entry.standing.relation()) {
            continue;
        }
        let Ok(raw) = hex::decode(device) else { continue };
        let Ok(key): std::result::Result<[u8; KEY_LEN], _> = raw.clone().try_into() else { continue };
        // Без заведённой беседы канала нет, а заводить её ради статуса незачем.
        let Ok(Some(group_id)) = store.conversation_with(&raw) else { continue };
        if matches!(mls.inspect(&group_id),Err(CoreError::MissingGroup)){continue;}

        let waiting = PendingSend {
            device: key,
            body: crate::access::presence_signal(),
            stored: true,
        };
        encrypt_and_send(socket, store, mls, sink, &group_id, waiting, outbox, None).await?;
    }
    Ok(())
}

/// Отправляет «печатает» собеседнику.
///
/// Сигнал едет обычным шифрованным конвертом: сервер видит непрозрачные байты и
/// о наборе текста не узнаёт ничего. Плата за это — лишний конверт, поэтому
/// интерфейс обязан слать сигнал редко, а не на каждую букву.
#[allow(clippy::too_many_arguments)]
async fn send_typing(
    socket: &mut Socket,
    store: &Store,
    mls: &mut Mls,
    sink: &EventSink,
    recipient_device: &str,
    active: bool,
    outbox: &mut Outbox,
) -> Result<()> {
    let device = hex::decode(recipient_device).map_err(|_| CoreError::BadFrame)?;
    let device: [u8; KEY_LEN] = device.try_into().map_err(|_| CoreError::BadKeyLength)?;

    // Беседы ещё нет — и сообщать нечего: заводить её ради индикатора набора
    // текста было бы странно.
    let Some(group_id) = store.conversation_with(&device)? else { return Ok(()) };

    let waiting = PendingSend {
        device,
        body: crate::access::typing_signal(active),
        stored: true,
    };
    encrypt_and_send(socket, store, mls, sink, &group_id, waiting, outbox, None).await
}

/// Нить для только что заведённой беседы.
///
/// Если с этим человеком нить уже есть — новая группа встаёт в неё. Если нет,
/// нить и есть сама группа, и записывать ничего не нужно: отсутствие записи в
/// `threads` именно это и означает.
///
/// Личность мы знаем не всегда: до первого объявления собеседник для нас —
/// просто устройство. Тогда группа становится собственной нитью, а когда
/// объявление придёт, [`merge_threads`] сведёт их вместе.
fn adopt_thread(store: &Store, device: &[u8], group_id: &[u8]) -> Result<Vec<u8>> {
    let Some(identity) = store.identity_of_device(device)? else {
        return Ok(group_id.to_vec());
    };
    let Some(thread) = store.thread_of_identity(&identity)? else {
        return Ok(group_id.to_vec());
    };
    if thread == group_id {
        return Ok(thread);
    }
    store.set_thread(group_id, &thread)?;
    Ok(thread)
}

/// Changing a cryptographic group must keep the existing visible conversation,
/// including peers whose identity announcement has never arrived.
fn attach_channel(store: &Store, device: &[u8], group: &[u8]) -> Result<Vec<u8>> {
    let previous=store.conversation_with(device)?.map(|old|store.thread_of(&old)).transpose()?;
    store.set_conversation(device,group)?;
    let mut sent: std::collections::BTreeSet<String> = store.load_setting(PROFILE_KEY_SENT)?
        .and_then(|raw|serde_json::from_slice(&raw).ok()).unwrap_or_default();
    if sent.remove(&hex::encode(device)) {
        store.save_setting(PROFILE_KEY_SENT,&serde_json::to_vec(&sent)?)?;
    }
    if let Some(thread)=previous {
        if thread!=group {store.set_thread(group,&thread)?;}
        return Ok(thread);
    }
    adopt_thread(store,device,group)
}

const CHANNEL_READY: &str = "\u{2063}VALCTL1:{\"channel_ready\":true}";

/// Only a locally missing state triggers repair, never an unauthenticated
/// network packet. The new KeyPackage must prove the same pinned device key.
async fn repair_missing_channels(socket:&mut Socket,store:&Store,mls:&Mls,sink:&EventSink,
    pending:&mut HashMap<[u8;ID_LEN],Claim>) -> Result<()> {
    for (peer,group) in store.list_conversations()? {
        if !matches!(mls.inspect(&group),Err(CoreError::MissingGroup)) {continue;}
        let Ok(device)=<[u8;KEY_LEN]>::try_from(peer.as_slice()) else {continue};
        if blocked(store,&hex::encode(device))||!pin_allows_or_reports(store,sink,&device)? {continue;}
        if pending.values().any(|claim|matches!(claim,Claim::Start(waiting) if waiting.device==device)) {continue;}
        let reference:[u8;ID_LEN]=random_bytes(ID_LEN).try_into().unwrap();
        pending.insert(reference,Claim::Start(PendingSend{device,body:CHANNEL_READY.into(),stored:true}));
        if let Err(error)=send(socket,proto::keypkg_claim_frame(&reference,&device)).await {
            pending.remove(&reference);return Err(error);
        }
    }
    Ok(())
}

/// Сводит все беседы одной личности в одну нить.
///
/// Нужно потому, что личность узнаётся задним числом: беседа с человеком могла
/// начаться до того, как он прислал список устройств, а с его вторым
/// устройством — и вовсе отдельно. Пока мы не знали, что это один человек, в
/// списке чатов он двоился.
///
/// Нитью становится самая ранняя из групп: под ней уже лежит история, и
/// переносить сообщения не требуется.
fn merge_threads(store: &Store, identity: &[u8]) -> Result<()> {
    let Some(thread) = store.thread_of_identity(identity)? else { return Ok(()) };
    for (device, group_id) in store.list_conversations()? {
        if store.identity_of_device(&device)?.as_deref() != Some(identity) {
            continue;
        }
        if group_id != thread {
            store.set_thread(&group_id, &thread)?;
        }
    }
    Ok(())
}

/// Свой проверенный список устройств: hex ключа → (hex сертификата, когда завели).
const OWN_DEVICES: &str = "own_devices";

/// Что лежит в [`OWN_DEVICES`]. Пусто — список ещё не приезжал.
fn own_devices(store: &Store) -> std::collections::BTreeMap<String, (String, i64)> {
    store
        .load_setting(OWN_DEVICES)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}
/// Кому этот список уже разослан. Ключ — hex устройства собеседника.
const OWN_DEVICES_TOLD: &str = "own_devices_told";

/// Пришёл ответ на запрос своих устройств.
///
/// # Почему список всё равно проверяется
///
/// Он про нас самих, и спросили его мы сами — но пришёл он от сервера, а
/// подписи под ним ставил наш собственный ключ личности. Проверить их нам
/// ничего не стоит, а разница велика: непроверенный список мы бы разослали
/// собеседникам, и приписанное в него чужое устройство начало бы получать
/// копии нашей переписки. Сервер здесь — почтальон, а не свидетель.
///
/// Своё текущее устройство обязано найтись в ответе. Если его там нет, список
/// не про нас, и рассылать его нельзя: собеседники перестали бы писать нам
/// самим.
async fn own_devices_arrived(
    socket: &mut Socket,
    store: &Store,
    mls: &mut Mls,
    sink: &EventSink,
    live: &mut Live,
    own: proto::OwnDevices,
) -> Result<()> {
    let credentials = store.load_credentials()?;
    if own.identity != hex::encode(credentials.identity_pub()) {
        return Ok(());
    }
    let identity = credentials.identity_pub();
    let mine = hex::encode(credentials.device_pub());

    let verified: std::collections::BTreeMap<String, (String, i64)> = own
        .devices
        .into_iter()
        .filter(|entry| {
            let (Ok(device), Ok(cert)) = (hex::decode(&entry.device), hex::decode(&entry.cert))
            else {
                return false;
            };
            keys::verify(&cert, &keys::device_cert_message(&identity, &device), &identity)
        })
        .map(|entry| (entry.device, (entry.cert, entry.added_at)))
        .collect();

    if !verified.contains_key(&mine) {
        return Ok(());
    }

    let known: std::collections::BTreeMap<String, (String, i64)> = store
        .load_setting(OWN_DEVICES)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default();

    if known != verified {
        // Состав изменился — прежние рассылки устарели, и рассказать надо всем
        // заново. Иначе отозванное устройство осталось бы у собеседников
        // навсегда, а «выйти на других устройствах» ничего бы не значило.
        store.save_setting(OWN_DEVICES, &serde_json::to_vec(&verified)?)?;
        store.save_setting(OWN_DEVICES_TOLD, &serde_json::to_vec::<Vec<String>>(&Vec::new())?)?;
    }

    announce_devices(socket, store, mls, sink, live).await
}

/// Рассылает свой список устройств тем, с кем беседа уже заведена.
///
/// Тем, с кем канала ещё нет, рассылка откладывается — ровно как с пропусками:
/// передать список в открытую нельзя, а заводить ради него беседу незачем. Он
/// уедет сам, как только беседа появится и сверка пройдёт в следующий раз.
///
/// Каждому — по разу на состав: отметка о рассказанном лежит в настройках,
/// поэтому переподключение не превращается в рассылку.
async fn announce_devices(
    socket: &mut Socket,
    store: &Store,
    mls: &mut Mls,
    sink: &EventSink,
    live: &mut Live,
) -> Result<()> {
    let entries = own_devices(store);
    if entries.is_empty() {
        return Ok(());
    }

    let mut pairs: Vec<([u8; KEY_LEN], [u8; keys::SIG_LEN])> = Vec::new();
    for (device, (cert, _added_at)) in &entries {
        let (Ok(device), Ok(cert)) = (hex::decode(device), hex::decode(cert)) else { continue };
        let (Ok(device), Ok(cert)) = (device.try_into(), cert.try_into()) else { continue };
        pairs.push((device, cert));
    }

    let identity = store.load_credentials()?.identity_pub();
    let body = crate::access::devices_announce(&identity, &pairs);

    let mut told: std::collections::BTreeSet<String> = store
        .load_setting(OWN_DEVICES_TOLD)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default();

    let mut sent_any = false;
    for (peer, group_id) in store.list_conversations()? {
        if matches!(mls.inspect(&group_id),Err(CoreError::MissingGroup)){continue;}
        let device = hex::encode(&peer);
        if told.contains(&device) {
            continue;
        }
        let Ok(key): std::result::Result<[u8; KEY_LEN], _> = peer.clone().try_into() else {
            continue;
        };
        let waiting = PendingSend {
            device: key,
            // Служебное сообщение в переписку не кладётся.
            body: body.clone(),
            stored: true,
        };
        encrypt_and_send(socket, store, mls, sink, &group_id, waiting, &mut live.outbox, None).await?;
        told.insert(device);
        sent_any = true;
    }

    if sent_any {
        store.save_setting(OWN_DEVICES_TOLD, &serde_json::to_vec(&told)?)?;
    }
    Ok(())
}

/// Приводит выданные пропуска в соответствие с правилом.
///
/// В обе стороны: кому полагается и не выдан — выдаём, у кого есть и больше не
/// полагается — отзываем. Сверка идёт при каждом подключении, поэтому включение
/// политики никого не отрезает (знакомые получают пропуска тем же заходом), а
/// сужение круга или блокировка действительно закрывают дверь, а не оставляют
/// её приоткрытой старым пропуском.
///
/// Пропуск уезжает служебным сообщением внутри шифрованного канала — сервер
/// видит только очередной непрозрачный конверт. Тем, с кем беседа ещё не
/// заведена, выдача откладывается: без канала передать секрет некуда, а
/// отправлять его в открытую нельзя.
async fn grant_missing(
    socket: &mut Socket,
    store: &Store,
    mls: &mut Mls,
    sink: &EventSink,
    live: &mut Live,
) -> Result<()> {
    let mut access = load_access(store);
    let peers = deserving(store);
    let mut changed = false;

    // Сначала отбираем у тех, кому больше не полагается.
    let stale: Vec<String> = access
        .granted
        .keys()
        .filter(|device| !peers.contains(*device))
        .cloned()
        .collect();
    for device in stale {
        if let Some(hash) = access.take_grant(&device) {
            send(socket, proto::pass_revoke_frame(&hash)?).await?;
            changed = true;
        }
    }

    for device in access.missing_grants(peers.iter()) {
        let Ok(raw) = hex::decode(&device) else { continue };
        let Ok(key): std::result::Result<[u8; KEY_LEN], _> = raw.clone().try_into() else { continue };
        // Беседы нет — значит и канала нет. Выдадим, когда она появится.
        let Ok(Some(group_id)) = store.conversation_with(&raw) else { continue };
        if matches!(mls.inspect(&group_id),Err(CoreError::MissingGroup)){continue;}

        let (pass, hash) = crate::access::new_pass();
        send(socket, proto::pass_create_frame(&hash, false, 0)?).await?;

        let waiting = PendingSend {
            device: key,
            body: crate::access::pass_gift(&pass),
            // Служебное сообщение в переписку не кладётся.
            stored: true,
        };
        encrypt_and_send(socket, store, mls, sink, &group_id, waiting, &mut live.outbox, None).await?;

        access.remember_grant(&device, &hash);
        changed = true;
    }

    // Ключ от аватара — тем же, кому и пропуск, и тем же каналом. Один раз на
    // собеседника: отметка о выданном лежит в настройках, поэтому переподключение
    // не превращается в рассылку.
    if avatar_is_private(store) {
        let key = own_profile_key(store)?;
        let mut sent: std::collections::BTreeSet<String> = store
            .load_setting(PROFILE_KEY_SENT)
            .ok()
            .flatten()
            .and_then(|raw| serde_json::from_slice(&raw).ok())
            .unwrap_or_default();

        let mut shared = false;
        for device in &avatar_recipients(store) {
            if sent.contains(device) {
                continue;
            }
            let Ok(raw) = hex::decode(device) else { continue };
            let Ok(peer): std::result::Result<[u8; KEY_LEN], _> = raw.clone().try_into() else {
                continue;
            };
            let Ok(Some(group_id)) = store.conversation_with(&raw) else { continue };
        if matches!(mls.inspect(&group_id),Err(CoreError::MissingGroup)){continue;}

            let waiting = PendingSend {
                device: peer,
                body: crate::access::profile_key_gift(&hex::encode(key)),
                stored: true,
            };
            encrypt_and_send(socket, store, mls, sink, &group_id, waiting, &mut live.outbox, None)
                .await?;
            sent.insert(device.clone());
            shared = true;
        }
        if shared {
            store.save_setting(PROFILE_KEY_SENT, &serde_json::to_vec(&sent)?)?;
        }
    }

    if changed {
        save_access(store, &access)?;
        sink(access_event(store));
    }
    Ok(())
}

/// Готовит кадр с запечатанной личностью и канонический логин для события.
///
/// Пароль отсюда не уходит никуда, кроме Argon2id: на сервер отправляются
/// только хеш логина, хеш доказательства и шифротекст.
fn seal_recovery(
    store: &Store,
    login: &str,
    password: &str,
    totp: Option<&str>,
    code: Option<&str>,
) -> Result<(String, Vec<u8>)> {
    let credentials = store.load_credentials()?;
    let sealed = crate::passphrase::seal(login, password, &credentials.identity)?;
    let frame = proto::recovery_set_frame(
        &sealed.login_id,
        &crate::passphrase::verifier(&sealed.token),
        &sealed.sealed,
        totp,
        code,
    )?;
    Ok((crate::passphrase::normalize_login(login)?, frame))
}

/// Кладёт в пустую базу личность, распечатанную по логину и паролю.
async fn recover_with_password(
    store: &Store,
    url: &str,
    login: &str,
    password: &str,
    code: Option<&str>,
) -> Result<()> {
    if store.has_credentials()? {
        return Err(CoreError::Rejected("identity_exists".into()));
    }
    let identity = fetch_sealed_identity(url, login, password, code).await?;
    store.save_credentials(&Credentials { identity, device: keys::SecretKey::generate() })
}

/// Забирает запечатанную личность до всякой аутентификации.
///
/// Это единственный разговор с сервером без ключей, и иначе быть не может:
/// тому, кто потерял устройство, подписываться нечем. Поэтому обмен короткий и
/// одноразовый — соединение закрывается сразу после ответа, а дальше вход идёт
/// обычным путём, уже восстановленным ключом.
///
/// Argon2id считается здесь же и блокирует поток ядра примерно на секунду.
/// Это осознанно: параллельно всё равно ничего не происходит, а вынос в
/// отдельный поток стоил бы `block_in_place`, которого на однопоточном
/// рантайме нет.
async fn fetch_sealed_identity(
    url: &str,
    login: &str,
    password: &str,
    code: Option<&str>,
) -> Result<keys::SecretKey> {
    let (login_id, token, key) = crate::passphrase::request(login, password)?;

    let mut socket = open_socket(url).await?;
    send(&mut socket, proto::recovery_get_frame(&login_id, &token, code)?).await?;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let sealed = loop {
        let message = tokio::time::timeout_at(deadline, socket.next())
            .await
            .map_err(|_| CoreError::Transport("сервер не ответил".into()))?
            .ok_or_else(|| CoreError::Transport("соединение закрыто".into()))?
            .map_err(|err| CoreError::Transport(err.to_string()))?;

        let Message::Binary(data) = message else { continue };
        let (opcode, body) = proto::split(&data)?;
        match opcode {
            op::RECOVERY_BLOB => {
                let blob: proto::RecoveryBlob = proto::parse_json(body)?;
                break hex::decode(&blob.sealed).map_err(|_| CoreError::BadFrame)?;
            }
            op::ERROR => {
                let error: ServerError = proto::parse_json(body)?;
                return Err(CoreError::Rejected(error.code));
            }
            // HELLO и прочее по дороге — не наше дело.
            _ => {}
        }
    };
    let _ = socket.close(None).await;

    crate::passphrase::open(&key, &login_id, &sealed)
}

/// Ключ, под которым правила приватности лежат в запечатанной базе.
const PRIVACY_KEY: &str = "privacy";
/// Книга отношений: кто контакт, кто ждёт решения, кто заблокирован.
const DIRECTORY_KEY: &str = "directory";
/// Политика доступа, выданные и полученные пропуска.
const ACCESS_KEY: &str = "access";
/// Свой юзернейм. На сервере лежит только его хеш, читаемое имя — здесь.
const USERNAME_KEY: &str = "username";
/// Закреплённые ключи: под каким ключом мы видели каждое имя.
const PINS_KEY: &str = "pins";
/// Когда каждая беседа в последний раз меняла ключ. См. `rekey_stale`.
const REKEYED_KEY: &str = "rekeyed";

/// Как часто менять свой ключ в беседе.
///
/// Смысл не в частоте, а в том, чтобы это вообще происходило: пока эпоха не
/// сменилась, украденные ключи открывают и то, что будет написано дальше. В
/// диалоге вдвоём состав не меняется никогда, поэтому без этого таймера
/// post-compromise security — свойство, ради которого и выбран MLS, — не
/// срабатывает ни разу.
///
/// Сутки — размен между стоимостью (коммит это лишний конверт каждому
/// участнику) и тем, сколько времени украденный ключ остаётся полезным.
const REKEY_AFTER_SEC: i64 = 24 * 3600;

/// Свой ключ профиля: им запечатан наш аватар.
const PROFILE_KEY: &str = "profile_key";
/// Запоминает разошедшиеся по одному человеку устройства.
///
/// Список кладётся под каждое из них: с какого бы устройства собеседник ни
/// написал, мы найдём остальные. Прежние записи для этих же устройств
/// затираются — объявление всегда свежее того, что лежит, а устройство,
/// пропавшее из списка, из него именно что убрали.
fn remember_peer_devices(store: &Store, devices: &[[u8; KEY_LEN]]) {
    let list: Vec<String> = devices.iter().map(hex::encode).collect();
    let mut known: std::collections::BTreeMap<String, Vec<String>> = store
        .load_setting(PEER_DEVICES)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default();
    for device in &list {
        known.insert(device.clone(), list.clone());
    }
    if let Ok(encoded) = serde_json::to_vec(&known) {
        let _ = store.save_setting(PEER_DEVICES, &encoded);
    }
}

/// Ключи профилей собеседников: device в hex → ключ в hex.
const PEER_PROFILE_KEYS: &str = "peer_profile_keys";
/// Кому наш ключ профиля уже отправлен.
const PROFILE_KEY_SENT: &str = "profile_key_sent";
/// Устройства собеседников: устройство в hex → все устройства того же человека.
///
/// Ключ здесь — устройство, а не личность, потому что весь остальной код ядра
/// адресует собеседника устройством: и беседы, и справочник, и правила
/// приватности. Личность станет ключом на следующем шаге, вместе со схемой; до
/// тех пор список лежит рядом с каждым известным устройством того же человека.
const PEER_DEVICES: &str = "peer_devices";

/// Свой юзернейм из локальной базы.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct OwnUsername {
    #[serde(default)]
    name: Option<String>,
    #[serde(default = "yes")]
    discoverable: bool,
    /// Занято ли имя дорогим хешем. У баз, заведённых раньше, поля нет — и
    /// `false` здесь правильное значение: оно означает «ещё не занимали».
    #[serde(default)]
    strong_hash: bool,
}

fn yes() -> bool {
    true
}

fn load_username(store: &Store) -> OwnUsername {
    store
        .load_setting(USERNAME_KEY)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}

fn username_event(store: &Store) -> Event {
    let own = load_username(store);
    Event::Username { name: own.name, discoverable: own.discoverable }
}

/// Книга отношений. Испорченная запись не должна лишать доступа к настройкам:
/// в этом случае берётся пустая — все становятся незнакомцами, а это
/// безопасная сторона ошибки, а не разрешающая.
/// Закрепления. Испорченная запись означает «ничего не помним»: это заставит
/// закрепить ключи заново, но не пропустит подмену молча под видом знакомого.
/// Меняет наш ключ в беседах, где он не менялся дольше `REKEY_AFTER_SEC`.
///
/// Коммит уходит остальным участникам обычным конвертом. Пока он не дошёл,
/// собеседник остаётся в прежней эпохе и продолжает читать — MLS держит ключи
/// предыдущей эпохи ровно для таких опозданий.
async fn rekey_stale(
    socket: &mut Socket,
    store: &Store,
    mls: &mut Mls,
    sink: &EventSink,
) -> Result<()> {
    let now = now_millis() / 1000;
    let mut done: std::collections::BTreeMap<String, i64> = store
        .load_setting(REKEYED_KEY)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default();

    let mut changed = false;
    for (_peer, group_id) in store.list_conversations()? {
        if matches!(mls.inspect(&group_id),Err(CoreError::MissingGroup)){continue;}
        let key = hex::encode(&group_id);
        let last = done.get(&key).copied();

        // Беседу, которую видим впервые, не перешифровываем сразу: её ключи
        // и так только что созданы. Просто запоминаем, когда встретили.
        let Some(last) = last else {
            done.insert(key, now);
            changed = true;
            continue;
        };
        if now - last < REKEY_AFTER_SEC {
            continue;
        }

        match mls.rekey(&group_id) {
            Ok(commit) => {
                persist(store, mls, sink);
                fan_out(socket, store, mls, &group_id, &commit, None, None).await?;
                done.insert(key, now);
                changed = true;
            }
            Err(err) => {
                // Одна беседа не должна мешать остальным: причин может быть
                // много — от испорченного состояния до чужого коммита,
                // пришедшего раньше нашего.
                fail(sink, "rekey", &err.to_string());
            }
        }
    }

    if changed {
        store.save_setting(REKEYED_KEY, &serde_json::to_vec(&done)?)?;
    }
    Ok(())
}

/// Перезанимает своё имя, добавляя к нему дорогой хеш.
///
/// Ничего не делает, если имени нет или это уже сделано.

/// Свой ключ профиля, заводится при первом обращении.
fn own_profile_key(store: &Store) -> Result<[u8; 32]> {
    if let Some(raw) = store.load_setting(PROFILE_KEY)? {
        if let Ok(key) = <[u8; 32]>::try_from(raw.as_slice()) {
            return Ok(key);
        }
    }
    let key = crate::profile::new_key();
    store.save_setting(PROFILE_KEY, &key)?;
    Ok(key)
}

/// Ключи профилей собеседников. Испорченная запись означает «ключей нет»:
/// аватары просто не покажутся, и это безопасная сторона ошибки.
fn load_peer_profile_keys(store: &Store) -> std::collections::BTreeMap<String, String> {
    store
        .load_setting(PEER_PROFILE_KEYS)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}

fn profile_key_for_device(store: &Store, device: &str) -> Option<Vec<u8>> {
    if store.load_credentials().ok().is_some_and(|own| hex::encode(own.device_pub()) == device) {
        return store.load_setting(PROFILE_KEY).ok().flatten();
    }
    let keys = load_peer_profile_keys(store);
    // Read keys stored by older clients under an accidentally double-hexed ID.
    hex::decode(keys.get(device).or_else(|| keys.get(&hex::encode(device)))?).ok()
}

/// Открывает аватар, если он запечатан и ключ от него у нас есть.
///
/// Возвращает `None`, когда открыть нечем: аватара для нас просто нет, и
/// показать надо инициалы, а не сломанную картинку. Так же выглядит и случай,
/// когда человек убрал нас из контактов и сменил ключ.
fn unseal_avatar(
    store: &Store,
    device: Option<&str>,
    mime: Option<String>,
    data: Option<String>,
) -> (Option<String>, Option<String>) {
    if !crate::profile::is_sealed(mime.as_deref()) {
        return (mime, data);
    }
    let (Some(device), Some(data)) = (device, data) else {
        return (None, None);
    };
    let Some(key) = profile_key_for_device(store, device) else { return (None, None) };
    match crate::profile::open(&key, &data) {
        Ok((mime, data)) => (Some(mime), Some(data)),
        Err(_) => (None, None),
    }
}

/*
  Подтверждение авторства в каналах.

  Канал не шифруется — это принято осознанно. Но право писать до сих пор
  проверял только сервер, и захвативший его писал бы от имени любого канала, а
  читатель не отличил бы. Теперь автор подписывает пост, а мы проверяем.

  С чем сверять, берётся из закрепления: ключ владельца запоминается при первой
  встрече с каналом, как и ключ собеседника. Сменился — говорим вслух, а не
  принимаем молча.

  Пост помечается `verified` для интерфейса. Пометка честная: она означает
  «подписано ключом, который мы за этим каналом помним», а не «правда».
*/
fn check_channel_report(store: &Store, sink: &EventSink, report: &mut serde_json::Value) {
    let Some(handle) = report.get("handle").and_then(|v| v.as_str()).map(str::to_owned) else {
        return;
    };
    let Some(owner) = report
        .get("ownerIdentity")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
    else {
        return;
    };

    // Закрепление живёт в той же книге, что и ключи собеседников: имя канала
    // не может совпасть с юзернеймом — у него свой префикс.
    let pin_name = format!("channel:{handle}");
    let mut pins = match load_pins_checked(store) {
        Ok(pins) => pins,
        Err(err) => {
            fail(sink, "storage", &err.to_string());
            return;
        }
    };
    let state = pins.check(&pin_name, &owner, now_millis());
    if state != crate::pins::PinState::Same {
        if let Err(err) = save_pins(store, &pins) {
            fail(sink, "storage", &err.to_string());
        }
    }
    if state == crate::pins::PinState::Changed {
        sink(Event::Anomaly {
            kind: "channel_owner_changed".into(),
            detail: format!("у канала @{handle} другой ключ владельца, чем прежде"),
        });
    }

    let trusted = pins.names.get(&pin_name).map(|pin| pin.device.clone()).unwrap_or(owner);
    let channel_id = report
        .get("channel")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .unwrap_or_default();

    if let Some(post) = report.get_mut("post") {
        mark_post(post, &channel_id, &trusted);
    }
    if let Some(posts) = report.get_mut("posts").and_then(|v| v.as_array_mut()) {
        for post in posts {
            mark_post(post, &channel_id, &trusted);
        }
    }
}

/// Проставляет посту `verified`: подписан ли он тем, кого мы помним владельцем.
fn mark_post(post: &mut serde_json::Value, channel: &str, trusted_owner: &str) {
    let verified = match crate::channels::author_of(post) {
        Some((author, signature)) if author == trusted_owner => {
            let id = post.get("id").and_then(|v| v.as_str()).unwrap_or_default();
            let created_at = post.get("createdAt").and_then(|v| v.as_i64()).unwrap_or_default();
            let body = post.get("body").and_then(|v| v.as_str()).unwrap_or_default();
            crate::channels::verify(&author, &signature, channel, id, created_at, body)
        }
        // Подписи нет вовсе (старый клиент) или подписал не владелец —
        // в обоих случаях подтвердить авторство нечем.
        _ => false,
    };
    if let Some(object) = post.as_object_mut() {
        object.insert("verified".into(), serde_json::Value::Bool(verified));
    }
}

/// Открывает значок и цвет, если они приехали запечатанными.
///
/// Ключа нет — украшений для нас просто не существует: показываем то, что
/// пришло открытым (у старого собеседника это по-прежнему обычные значения).
fn unseal_decor(
    store: &Store,
    device: Option<&str>,
    sealed: Option<&str>,
    emblem: Option<String>,
    color: Option<String>,
) -> (Option<String>, Option<String>) {
    let Some(sealed) = sealed else { return (emblem, color) };
    let Some(device) = device else { return (None, None) };
    let Some(key) = profile_key_for_device(store, device) else { return (None, None) };
    crate::profile::open_decor(&key, sealed).unwrap_or((None, None))
}

fn load_pins_checked(store: &Store) -> Result<crate::pins::Pins> {
    match store.load_setting(PINS_KEY)? {
        Some(raw) => Ok(serde_json::from_slice(&raw)?),
        None => Ok(crate::pins::Pins::default()),
    }
}

fn ensure_pin_allows(store: &Store, device: &str) -> Result<()> {
    let pins = load_pins_checked(store)?;
    if pins.blocks_device(device) {
        return Err(CoreError::Anomaly(
            "ключ устройства под этим именем изменился и ещё не подтверждён".into(),
        ));
    }
    Ok(())
}

fn pin_allows_or_reports(store: &Store, sink: &EventSink, device: &[u8; KEY_LEN]) -> Result<bool> {
    match ensure_pin_allows(store, &hex::encode(device)) {
        Ok(()) => Ok(true),
        Err(CoreError::Anomaly(detail)) => {
            sink(Event::Anomaly { kind: "send_blocked".into(), detail });
            Ok(false)
        }
        Err(other) => Err(other),
    }
}

fn save_pins(store: &Store, pins: &crate::pins::Pins) -> Result<()> {
    store.save_setting(PINS_KEY, &serde_json::to_vec(pins)?)
}

fn load_directory(store: &Store) -> crate::directory::Directory {
    store
        .load_setting(DIRECTORY_KEY)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}

fn save_directory(store: &Store, directory: &crate::directory::Directory) -> Result<()> {
    store.save_setting(DIRECTORY_KEY, &serde_json::to_vec(directory)?)
}

fn load_access(store: &Store) -> crate::access::Access {
    store
        .load_setting(ACCESS_KEY)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}

fn save_access(store: &Store, access: &crate::access::Access) -> Result<()> {
    store.save_setting(ACCESS_KEY, &serde_json::to_vec(access)?)
}

fn access_event(store: &Store) -> Event {
    let access = load_access(store);
    Event::Access {
        policy: access.policy,
        invites: access.invites.clone(),
        granted: access.granted.len(),
    }
}

/// Кому полагается пропуск.
///
/// Решает то же самое правило «личные сообщения», по которому интерфейс рисует
/// настройку, — вместе с именными исключениями. Благодаря этому «всегда
/// разрешать» действительно выдаёт пропуск, а «никогда» его отбирает, и
/// поведение не расходится с тем, что человек видит на экране.
/// Стоит ли прятать аватар от сервера.
///
/// Решает то же правило, которым человек уже пользуется: «аватар видят все» —
/// прятать не от кого, всё остальное — прятать. Отдельной настройки нет
/// намеренно: две настройки об одном и том же расходятся, и объяснить разницу
/// потом невозможно.
fn avatar_is_private(store: &Store) -> bool {
    let privacy: crate::privacy::Privacy = store
        .load_setting(PRIVACY_KEY)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default();
    privacy.profile_avatar.scope != crate::privacy::Scope::Everyone
}

fn deserving(store: &Store) -> Vec<String> {
    let privacy: crate::privacy::Privacy = store
        .load_setting(PRIVACY_KEY)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default();
    let rule = &privacy.direct_messages;

    load_directory(store)
        .entries
        .iter()
        .filter(|(device, entry)| rule.permits(device, entry.standing.relation()))
        .map(|(device, _)| device.clone())
        .collect()
}

fn avatar_recipients(store: &Store) -> Vec<String> {
    let privacy: crate::privacy::Privacy = store.load_setting(PRIVACY_KEY).ok().flatten()
        .and_then(|raw| serde_json::from_slice(&raw).ok()).unwrap_or_default();
    let directory = load_directory(store);
    directory.entries.iter().filter(|(device, entry)|
        !directory.is_blocked(device) && privacy.profile_avatar.permits(device, entry.standing.relation()))
        .map(|(device, _)| device.clone()).collect()
}

// Do not close the bootstrap channel while pass gifts still need it.
// An already restricted account is never silently reopened on upgrade.
fn restrictions_ready(store: &Store) -> Result<bool> {
    // Already restricted accounts may still change transport/privacy settings;
    // this guard must never prevent switching their primary socket to onion.
    if load_access(store).policy == crate::access::Policy::Passes { return Ok(true); }
    passes_ready(store)
}

fn passes_ready(store: &Store) -> Result<bool> {
    let access = load_access(store);
    for peer in deserving(store) {
        let raw = hex::decode(&peer).map_err(|_| CoreError::BadFrame)?;
        if store.conversation_with(&raw)?.is_some()
            && (!access.granted.contains_key(&peer) || !access.held.contains_key(&peer)) {
            return Ok(false);
        }
    }
    Ok(!crate::anon_queue::load(store)?.iter().any(|item| item.kind == crate::anon_queue::Kind::Essential))
}

fn blocked(store: &Store, device: &str) -> bool {
    load_directory(store).is_blocked(device)
}

/// Заводит запрос на незнакомца, написавшего первым.
///
/// Без этого раздел «Запросы» оставался бы пустым: записи неоткуда взяться,
/// пока человек сам кого-то не добавил. Уже знакомых — контактов, одобренных,
/// отклонённых — не трогаем: их положение решено, и переписка не должна
/// возвращать их в очередь на рассмотрение.
fn remember_stranger(store: &Store, sink: &EventSink, device: &str, origin: &str) {
    let mut directory = load_directory(store);
    if directory.standing(device).is_some() {
        return;
    }
    let now = now_millis();
    directory.set(device, crate::directory::Standing::Pending, now);
    directory.note(device, None, Some(origin.to_owned()), now);
    if save_directory(store, &directory).is_ok() {
        sink(directory_event(store));
    }
}

fn directory_event(store: &Store) -> Event {
    let directory = load_directory(store);
    Event::Directory {
        entries: directory
            .entries
            .iter()
            .map(|(device, entry)| crate::command::DirectoryItem {
                device: device.clone(),
                standing: entry.standing,
                display_name: entry.display_name.clone(),
                username: entry.username.clone(),
                origin: entry.origin.clone(),
                noted_at: entry.noted_at,
            })
            .collect(),
    }
}

/// Правила из базы либо значения по умолчанию.
///
/// Испорченная запись не должна запирать человека снаружи собственных
/// настроек: в этом случае берётся набор по умолчанию — он безопасный, а не
/// разрешающий, — и правила можно перезадать.
fn privacy_event(store: &Store) -> Event {
    let privacy = store
        .load_setting(PRIVACY_KEY)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default();
    Event::Privacy { privacy }
}

const DELIVERY_POLICY_KEY: &str = "delivery_policy_v1";

fn anonymous_required(store: &Store) -> Result<bool> {
    // Missing/unknown policy is private; only a recorded explicit route opts out.
    Ok(store.load_setting(DELIVERY_POLICY_KEY)?.as_deref() != Some(b"explicit_relay"))
}

fn load_privacy_checked(store: &Store) -> Result<crate::privacy::Privacy> {
    match store.load_setting(PRIVACY_KEY)? {
        Some(raw) => Ok(serde_json::from_slice(&raw)?),
        None => Ok(crate::privacy::Privacy::default()),
    }
}

fn content_json(body: &str) -> Option<serde_json::Value> {
    serde_json::from_str(body.strip_prefix("\u{2063}OBS1:").unwrap_or(body)).ok()
}

fn emit_delivery(store: &Store, sink: &EventSink, conversation: &[u8], id: &str) -> Result<()> {
    if let Some(state) = crate::delivery::state(store, conversation, id)? {
        sink(Event::Delivery { conversation: hex::encode(conversation), id: id.to_owned(), state });
    }
    Ok(())
}

fn activity_rule(body: &str) -> Option<&'static str> {
    match crate::access::parse_signal(body) {
        Some(crate::access::Control::Delivered(_)) => return Some("read_receipts"),
        Some(crate::access::Control::Call(_)) => return Some("calls"),
        Some(crate::access::Control::Typing(_)) => return Some("typing"),
        Some(crate::access::Control::Online) => return Some("presence"),
        _ => {}
    }
    match content_json(body)?.get("type")?.as_str()? {
        "read" => Some("read_receipts"),
        _ => None,
    }
}

fn permits_incoming_call(store:&Store, device:&[u8;KEY_LEN])->Result<bool>{
    let peer=hex::encode(device);
    let relation=load_directory(store).standing(&peer).map(|standing|standing.relation()).unwrap_or(crate::privacy::Relation::Unknown);
    Ok(load_privacy_checked(store)?.calls.permits(&peer,relation))
}

fn permits_activity(store: &Store, body: &str, device: &[u8; KEY_LEN]) -> Result<bool> {
    if matches!(crate::access::parse_signal(body),Some(crate::access::Control::Call(_))){return Ok(true)}
    let Some(name) = activity_rule(body) else { return Ok(true) };
    let privacy = load_privacy_checked(store)?;
    let peer = hex::encode(device);
    let directory = load_directory(store);
    let relation = directory.standing(&peer).map(|standing| standing.relation())
        .unwrap_or(crate::privacy::Relation::Unknown);
    Ok(privacy.rule(name).is_some_and(|rule| rule.permits(&peer, relation)))
}

/// Restore server-side restrictions after reconnect, including changes made offline.
async fn sync_server_privacy(socket: &mut Socket, store: &Store,
    pending_username: &mut Option<(String, bool)>) -> Result<()> {
    let privacy = match store.load_setting(PRIVACY_KEY)? {
        Some(raw) => Some(serde_json::from_slice::<crate::privacy::Privacy>(&raw)?),
        None => None,
    };
    if let Some(privacy) = &privacy {
        let restricted = privacy.direct_messages.scope != crate::privacy::Scope::Everyone;
        send(socket, proto::access_set_frame(if restricted { "passes" } else { "everyone" })?).await?;
        let mut access = load_access(store);
        access.policy = if restricted { crate::access::Policy::Passes } else { crate::access::Policy::Everyone };
        save_access(store, &access)?;
    }
    let own = load_username(store);
    // Hide legacy names on upgrade without releasing ownership or destroying backups.
    let discoverable = false;
    if pending_username.is_none() {
        if let Some(name) = own.name.filter(|_| !own.strong_hash || own.discoverable != discoverable) {
            let hash = crate::directory::username_hash(&name);
            let hash2 = crate::directory::username_hash_v2(&name)?;
            *pending_username = Some((name, discoverable));
            send(socket, proto::username_set_frame(&hash, &hash2, discoverable)?).await?;
        }
    }
    Ok(())
}

/// Свой адрес нужен интерфейсу до всякого подключения: его показывают
/// собеседнику, чтобы тот мог написать первым.
fn status(store: &Store) -> Event {
    match store.load_credentials() {
        Ok(credentials) => Event::Status {
            has_identity: true,
            identity: hex::encode(credentials.identity_pub()),
            device: hex::encode(credentials.device_pub()),
        },
        Err(_) => Event::Status { has_identity: false, identity: String::new(), device: String::new() },
    }
}

/// Держит соединение и переподключается, пока не придёт Disconnect.
async fn session(
    url: &str,
    entry: Entry,
    store: &Store,
    sink: &EventSink,
    commands: &mut mpsc::UnboundedReceiver<Command>,
) {
    let policy = if url == ONION_ROUTE_URL || url.contains(".onion") { b"onion".as_slice() }
        else { b"explicit_relay".as_slice() };
    if let Err(error) = store.save_setting(DELIVERY_POLICY_KEY, policy) {
        return fail(sink, "storage", &error.to_string());
    }
    let credentials = match load_or_create(store, &entry) {
        Ok(credentials) => credentials,
        Err(err) => return fail(sink, "keys", &err.to_string()),
    };
    let mut mls = match load_or_create_mls(store, &credentials) {
        Ok(mls) => mls,
        Err(err) => return fail(sink, "mls", &err.to_string()),
    };

    let mut entry = entry;
    let mut backoff = 1u64;
    let mut live = Live::default();
    live.require_anonymous = url == ONION_ROUTE_URL || url.contains(".onion");
    // Один канал на всю сессию, а не на каждое соединение: исход анонимной
    // отправки может прийти уже после того, как основное соединение оборвалось
    // и pump() успел вернуться. Канал переживает это молча — следующий заход
    // pump() получит тот же приёмник и разберёт то, что накопилось.
    let (anon_tx, mut anon_rx) = mpsc::unbounded_channel::<AnonOutcome>();
    // Список маршрутов считается один раз на сессию: он меняется только вместе
    // с настройкой, а её смена и так пересоздаёт соединение.
    let routes = routes_for(url, store);
    let rotating = routes.len() > 1;
    let mut route = 0usize;

    loop {
        // Дошли ли мы в этот раз до рабочего состояния. Нужно, чтобы отличить
        // «сервер недоступен» от «связь была и оборвалась».
        let mut established = false;

        // Пустым список быть не может: `routes_for` всегда возвращает хотя бы
        // сам адрес, а для onion-режима — хотя бы запасные входы.
        let attempt_url = routes.get(route).map(String::as_str).unwrap_or(url);
        match connect_once(
            attempt_url, &credentials, &entry, store, &mut mls, sink, commands, &mut live,
            &mut established, anon_tx.clone(), &mut anon_rx,
        )
        .await
        {
            Ok(Outcome::Closed) => return,
            Ok(Outcome::Retry) => {}
            Err(CoreError::Rejected(code)) => {
                // Отказ сервера повтором не лечится: ждём новой команды.
                fail(sink, &code, "server rejected the entry attempt");
                return;
            }
            Err(err) => sink(Event::Disconnected { reason: err.to_string() }),
        }

        // Задержка растёт только пока сервер недостижим. Разрыв уже рабочего
        // соединения — обычное дело за Cloudflare, и наказывать за него
        // минутой ожидания нельзя: именно так «доставка в реальном времени»
        // превращается в «увижу после перезапуска».
        if established {
            // Только завершённый handshake означает, что регистрация дошла до
            // сервера. Сетевой сбой до HELLO/AUTH не должен выбрасывать инвайт
            // при переходе Auto на следующий маршрут.
            entry = Entry::Existing;
            backoff = 1;
        } else if rotating {
            // Не открылось — пробуем следующий вход. После успешного соединения
            // остаёмся на выбранном пути; вращение возобновится лишь если он
            // перестанет открываться.
            route = (route + 1) % routes.len();
        }
        invalidate_network(&mut live,&mut anon_rx);
        if wait_before_retry(backoff, store, sink, commands).await == Pause::Closed {
            return;
        }
        // Offline sends may advance the saved ratchet while reconnect waits.
        mls = match load_or_create_mls(store, &credentials) {
            Ok(fresh) => fresh,
            Err(error) => return fail(sink, "mls", &error.to_string()),
        };
        if !established {
            backoff = (backoff * 2).min(MAX_BACKOFF_SEC);
        }
    }
}

#[derive(PartialEq, Eq)]
enum Pause {
    /// Пауза кончилась — пробуем снова.
    Elapsed,
    /// Интерфейс попросил закончить.
    Closed,
}

/// Пауза между попытками, которая слышит интерфейс.
///
/// Раньше здесь стоял простой сон, и всё это время ядро не читало команды
/// вовсе. Стоило телефону уснуть и потерять связь, как заново открытый экран
/// не получал ответа даже на вопрос «кто я» и висел на заставке до конца
/// паузы — а она растёт до минуты. Выглядело это как намертво зависшее
/// приложение, хотя ядро просто ждало.
///
/// Поэтому во время паузы отвечаем на всё, что можно ответить без сети, а
/// просьбу подключиться понимаем как «попробуй прямо сейчас».
async fn wait_before_retry(
    seconds: u64,
    store: &Store,
    sink: &EventSink,
    commands: &mut mpsc::UnboundedReceiver<Command>,
) -> Pause {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => return Pause::Elapsed,
            command = commands.recv() => {
                let Some(command) = command else { return Pause::Closed };
                if handle_local(&command, store, sink) {
                    if matches!(command, Command::PrivacyMaximize) && restrictions_ready(store).unwrap_or(false) { return Pause::Closed; }
                    continue;
                }
                match command {
                    Command::Send { recipient_device, body } => queue_offline(store, sink, &recipient_device, &body),
                    // «Подключись» во время паузы — это «не жди, попробуй сейчас».
                    Command::Connect { .. } | Command::NetworkChanged | Command::ProbeConnection => return Pause::Elapsed,
                    Command::Disconnect => {
                        sink(Event::Disconnected { reason: "by request".into() });
                        return Pause::Closed;
                    }
                    other => {
                        let _ = other;
                        fail(sink, "not_connected", "нет связи с сервером, пробуем ещё раз");
                    }
                }
            }
        }
    }
}

enum Outcome {
    /// UI попросил отключиться.
    Closed,
    /// Соединение оборвалось — переподключаемся.
    Retry,
}

/// Итог рукопожатия: с чем мы остались на этом соединении.
enum Handshake {
    /// `key_packages` — сколько наших пакетов сервер уже держит. `None` —
    /// сервер старый и числа не сообщает. `device_id` и `admin` хранятся, чтобы
    /// ответить на повторный вопрос «кто я» без нового рукопожатия.
    Authenticated { key_packages: Option<usize>, device_id: String, admin: bool },
    /// Счёт выставлен, ждём оплаты — писать ещё нельзя.
    #[cfg(feature = "ton")]
    InvoicePending,
}

fn load_or_create(store: &Store, entry: &Entry) -> Result<Credentials> {
    if store.has_credentials()? {
        return store.load_credentials();
    }
    if matches!(entry, Entry::Existing) {
        return Err(CoreError::NoCredentials);
    }
    let credentials = Credentials::generate();
    store.save_credentials(&credentials)?;
    Ok(credentials)
}

fn load_or_create_mls(store: &Store, credentials: &Credentials) -> Result<Mls> {
    match store.load_mls()? {
        Some((signer_public, snapshot)) => Mls::restore(&credentials.device, &signer_public, &snapshot),
        None => {
            let mls = Mls::create(&credentials.device)?;
            store.save_mls(&mls.signer_public(), &mls.snapshot())?;
            Ok(mls)
        }
    }
}

/// Снимок состояния MLS переживает любое изменение: пропущенная запись — это
/// потерянная эпоха и нерасшифровываемая переписка после перезапуска.
fn persist(store: &Store, mls: &Mls, sink: &EventSink) {
    if let Err(err) = store.save_mls(&mls.signer_public(), &mls.snapshot()) {
        fail(sink, "mls_persist", &err.to_string());
    }
}

/// Что сервер сообщил о себе при встрече.
///
/// Хранится на всё соединение: те же слова понадобятся ещё раз, когда экран
/// пересоздадут и он спросит, где мы. Придумывать ответ заново нельзя — про
/// возможности сервера знает только его приветствие.
#[derive(Clone, Copy)]
struct Greeting {
    heartbeat_sec: u64,
    invite_entry: bool,
    ton_entry: bool,
    profiles: bool,
    decor: bool,
    /// Умеет ли сервер отдавать наш собственный список устройств.
    devices: bool,
    /// Умеет ли сервер TICKET_REQUEST/SEND_ANON (ARCHITECTURE.md §7a).
    sealed_sender: bool,
}

#[allow(clippy::too_many_arguments)]
async fn connect_once(
    url: &str,
    credentials: &Credentials,
    entry: &Entry,
    store: &Store,
    mls: &mut Mls,
    sink: &EventSink,
    commands: &mut mpsc::UnboundedReceiver<Command>,
    live: &mut Live,
    established: &mut bool,
    anon_tx: mpsc::UnboundedSender<AnonOutcome>,
    anon_rx: &mut mpsc::UnboundedReceiver<AnonOutcome>,
) -> Result<Outcome> {
    // Заголовки Cloudflare Access, если сборка их знает: без них закрытый
    // периметр пришлось бы держать выключенным (§10.1).
    let Some(mut socket) = while_connecting(open_socket(url), store, sink, commands).await? else {
        return Ok(Outcome::Closed);
    };

    let Some(hello) = while_connecting(expect_hello(&mut socket, sink), store, sink, commands).await? else {
        return Ok(Outcome::Closed);
    };
    remember_onion_hosts(
        store,
        &hello.onion,
        &hello.onion_sig,
        hello.onion_issued_at,
        onion::PUBLIC_KEY,
        sink,
    );
    let heartbeat = if hello.heartbeat_sec == 0 { FALLBACK_HEARTBEAT_SEC } else { hello.heartbeat_sec.clamp(5, 60) };

    let nonce = hex::decode(&hello.nonce).map_err(|_| CoreError::BadFrame)?;
    let Some(handshake) = while_connecting(handshake(&mut socket, credentials, entry, nonce, sink), store, sink, commands).await? else {
        return Ok(Outcome::Closed);
    };
    *mls = load_or_create_mls(store, credentials)?;
    *established = true;

    let greeting = Greeting {
        heartbeat_sec: heartbeat,
        invite_entry: hello.entry.invite,
        ton_entry: hello.entry.ton,
        profiles: hello.features.profiles,
        decor: hello.features.decor,
        devices: hello.features.devices,
        sealed_sender: hello.features.sealed_sender,
    };

    pump(socket, store, mls, sink, commands, greeting, handshake, live, anon_tx, anon_rx).await
}

async fn expect_hello(socket: &mut Socket, sink: &EventSink) -> Result<Hello> {
    loop {
        let message = socket
            .next()
            .await
            .ok_or_else(|| CoreError::Transport("closed before hello".into()))?
            .map_err(|err| CoreError::Transport(err.to_string()))?;

        if let Message::Binary(data) = message {
            let (opcode, body) = proto::split(&data)?;
            if opcode != op::HELLO {
                return Err(CoreError::UnknownOpcode(opcode));
            }
            let hello: Hello = proto::parse_json(body)?;
            sink(Event::Connected {
                heartbeat_sec: hello.heartbeat_sec,
                invite_entry: hello.entry.invite,
                ton_entry: hello.entry.ton,
                profiles: hello.features.profiles,
                decor: hello.features.decor,
            });
            return Ok(hello);
        }
    }
}

/// Tor bootstrap/HELLO/AUTH must not prevent an offline message from reaching
/// durable storage, or make local privacy controls wait for a network timeout.
async fn while_connecting<T>(future: impl std::future::Future<Output = Result<T>>, store: &Store,
    sink: &EventSink, commands: &mut mpsc::UnboundedReceiver<Command>) -> Result<Option<T>> {
    while_connecting_with_timeout(future, store, sink, commands, Duration::from_secs(60)).await
}

async fn while_connecting_with_timeout<T>(future: impl std::future::Future<Output = Result<T>>, store: &Store,
    sink: &EventSink, commands: &mut mpsc::UnboundedReceiver<Command>, timeout: Duration) -> Result<Option<T>> {
    tokio::pin!(future);
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => return Err(CoreError::Transport("Connection timed out; trying another route".into())),
            result = &mut future => return result.map(Some),
            command = commands.recv() => {
                let Some(command) = command else { return Ok(None); };
                if handle_local(&command, store, sink) {
                    if matches!(command, Command::PrivacyMaximize) && restrictions_ready(store).unwrap_or(false) { return Ok(None); }
                    continue;
                }
                match command {
                    Command::Send { recipient_device, body } => queue_offline(store, sink, &recipient_device, &body),
                    Command::Disconnect => {
                        sink(Event::Disconnected { reason: "by request".into() });
                        return Ok(None);
                    }
                    Command::Connect { .. } | Command::ProbeConnection => {},
                    Command::NetworkChanged => return Err(CoreError::Transport("Network changed; reconnecting".into())),
                    _ => fail(sink, "not_connected", "Соединение ещё устанавливается"),
                }
            }
        }
    }
}

/// Проходит AUTH или PAY_REQUEST.
#[cfg_attr(not(feature = "ton"), allow(unused_mut))]
async fn handshake(
    socket: &mut Socket,
    credentials: &Credentials,
    entry: &Entry,
    mut nonce: Vec<u8>,
    sink: &EventSink,
) -> Result<Handshake> {
    let identity = hex::encode(credentials.identity_pub());
    let device = hex::encode(credentials.device_pub());
    let cert = hex::encode(credentials.device_cert());

    let build = |nonce: &[u8], entry: &Entry| -> AuthRequest {
        let (invite, payment_ref, handle) = match entry {
            Entry::Register { handle, invite, payment_ref } => {
                (invite.clone(), payment_ref.clone(), handle.clone())
            }
            _ => (None, None, None),
        };
        AuthRequest {
            v: 1,
            identity: identity.clone(),
            device: device.clone(),
            device_cert: cert.clone(),
            sig: hex::encode(credentials.auth_signature(nonce)),
            invite,
            payment_ref,
            handle,
        }
    };

    #[cfg(feature = "ton")]
    let opcode = if matches!(entry, Entry::Invoice) { op::PAY_REQUEST } else { op::AUTH };
    #[cfg(not(feature = "ton"))]
    let opcode = op::AUTH;

    send(socket, proto::json_frame(opcode, &build(&nonce, entry))?).await?;

    loop {
        let message = socket
            .next()
            .await
            .ok_or_else(|| CoreError::Transport("closed during handshake".into()))?
            .map_err(|err| CoreError::Transport(err.to_string()))?;

        let Message::Binary(data) = message else { continue };
        let (opcode, body) = proto::split(&data)?;
        match opcode {
            op::AUTH_OK => {
                let ok: AuthOk = proto::parse_json(body)?;
                sink(Event::Authenticated {
                    device_id: ok.device_id.clone(),
                    queued: ok.queued,
                    admin: ok.admin,
                });
                sink(Event::Registered { identity, device });
                return Ok(Handshake::Authenticated {
                    key_packages: ok.key_packages,
                    device_id: ok.device_id,
                    admin: ok.admin,
                });
            }
            #[cfg(feature = "ton")]
            op::PAY_INFO => {
                let info: PayInfo = proto::parse_json(body)?;
                nonce = hex::decode(&info.nonce).map_err(|_| CoreError::BadFrame)?;
                let _ = &nonce;
                sink(Event::Invoice {
                    reference: info.reference,
                    address: info.address,
                    amount_nano: info.amount_nano,
                    expires_at: info.expires_at,
                    paid: info.paid,
                });
                return Ok(Handshake::InvoicePending);
            }
            op::AUTH_ERR => {
                let err: AuthErr = proto::parse_json(body)?;
                sink(Event::Failed { code: err.code.clone(), message: err.message });
                return Err(CoreError::Rejected(err.code));
            }
            op::ERROR => {
                let err: ServerError = proto::parse_json(body)?;
                sink(Event::Failed { code: err.code.clone(), message: err.message });
                return Err(CoreError::Rejected(err.code));
            }
            // Всё остальное на этом шаге неинтересно: ждём своего ответа
            // дальше. Рвать рукопожатие из-за постороннего кадра нельзя —
            // сервер вправе прислать что-то, чего эта версия ещё не знает.
            _ => {}
        }
    }
}

/// Рабочий цикл соединения: входящие кадры, команды UI и heartbeat.
#[allow(clippy::too_many_arguments)]
async fn pump(
    mut socket: Socket,
    store: &Store,
    mls: &mut Mls,
    sink: &EventSink,
    commands: &mut mpsc::UnboundedReceiver<Command>,
    greeting: Greeting,
    handshake: Handshake,
    live: &mut Live,
    anon_tx: mpsc::UnboundedSender<AnonOutcome>,
    anon_rx: &mut mpsc::UnboundedReceiver<AnonOutcome>,
) -> Result<Outcome> {
    let authenticated = matches!(handshake, Handshake::Authenticated { .. });
    let device_id = match &handshake {
        Handshake::Authenticated { device_id, .. } => device_id.clone(),
        #[cfg(feature = "ton")]
        Handshake::InvoicePending => String::new(),
    };
    let admin = matches!(&handshake, Handshake::Authenticated { admin: true, .. });
    let mut pending: HashMap<[u8; ID_LEN], Claim> = HashMap::new();
    // Логин, посылку по которому сервер ещё не подтвердил. Пустое значение при
    // RECOVERY_OK означает, что подтверждают удаление, а не установку.
    // Логин и «включался ли второй фактор» — чтобы ответить о том, что
    // именно сохранилось, а не просто «сохранено».
    let mut pending_recovery: Option<(String, bool)> = None;
    // Имя, которое сервер ещё не подтвердил, и имя, по которому идёт поиск.
    // Сервер видит только хеши и вернуть читаемое имя не может — его помнит
    // эта сторона.
    let mut pending_username: Option<(String, bool)> = None;
    let mut looking_for: Option<String> = None;
    let mut lookup_queue: VecDeque<(String, [u8; 32], [u8; 32])> = VecDeque::new();

    if authenticated {
        repair_missing_channels(&mut socket,store,mls,sink,&mut pending).await?;
        // Докладываем ровно недостающее. Раньше полная пачка уходила на каждый
        // вход: пакеты копились, упирались в потолок хранилища, и сервер начинал
        // рвать соединение на каждом подключении — связь не жила дольше одного
        // захода. Старый сервер числа не сообщает, тогда ведём себя как прежде.
        let stored = match &handshake {
            Handshake::Authenticated { key_packages, .. } => key_packages.unwrap_or(0),
            #[cfg(feature = "ton")]
            Handshake::InvoicePending => 0,
        };
        let needed = KEY_PACKAGES_PER_CONNECT.saturating_sub(stored);

        // Без выложенных KeyPackages нам просто некому написать первым.
        if needed > 0 {
            match mls.key_packages(needed) {
                Ok(packages) => {
                    send(&mut socket, proto::keypkg_publish_frame(&packages)).await?;
                    persist(store, mls, sink);
                }
                Err(err) => fail(sink, "mls_key_packages", &err.to_string()),
            }
        }

        // Предъявляем всё, что нам выдали: допуск живёт только в пределах
        // соединения, поэтому повторять его надо на каждом заходе.
        for (recipient, pass) in load_access(store).to_present() {
            send(&mut socket, proto::pass_present_frame(&recipient, &pass)?).await?;
        }

        // Спрашиваем свои же устройства. Ответ придёт отдельным кадром и сам
        // разошлёт список собеседникам: до него мы не знаем, появилось ли
        // где-то новое устройство и не отозвали ли старое.
        //
        // Только если сервер сказал, что умеет. Неизвестный код кадра он считает
        // битым кадром и закрывает соединение — спросив наугад, мы отвалились бы
        // сразу после входа на любом сервере старее этой строки. А свои узлы люди
        // обновляют когда захотят.
        if greeting.devices {
            send(&mut socket, proto::device_list_frame()?).await?;
        }

        live.ticket_request_until = None;
        // Sealed sender: прогреваем пул заранее, а не в момент первой отправки —
        // иначе первое сообщение после каждого подключения ждало бы пачки.
        if greeting.sealed_sender {
            request_anonymous_tickets(&mut socket, live).await?;
        }

        // Выдаём пропуска тем, кому они полагаются, но ещё не достались.
        // Сверка идёт при каждом подключении, поэтому включение политики никого
        // не отрезает: знакомые получают пропуска тем же заходом. Это же чинит
        // и выдачу, прерванную обрывом связи.
        // Пропуск — любезность, а не условие переписки: его неудача не повод
        // рвать связь. Обрыв разберётся сам на следующей отправке.
        if let Err(err) = grant_missing(&mut socket, store, mls, sink, live).await {
            fail(sink, "pass_grant", &err.to_string());
        }

        sync_server_privacy(&mut socket, store, &mut pending_username).await?;

        // Меняем свой ключ в беседах, где он давно не менялся.
        //
        // Здесь же, при подключении, а не по таймеру в фоне: коммит всё равно
        // нужно отправить, а отправлять его некуда, пока нет соединения.
        if let Err(err) = rekey_stale(&mut socket, store, mls, sink).await {
            if is_transport(&err) {
                return Ok(Outcome::Retry);
            }
            // Не повод рвать связь: переписка работает и на прежней эпохе,
            // а попытка повторится на следующем заходе.
            fail(sink, "rekey", &err.to_string());
        }

        // Дозанимаем своё имя дорогим хешем, если этого ещё не делали.
        //
        // Сервер не может пересчитать хеш сам — имени он не знает и знать не
        // должен. Значит, обновить строку способен только тот, у кого имя
        // есть, то есть клиент, и делает он это молча при первом же заходе
        // после обновления. Один раз: отметка лежит рядом с самим именем.

        // Объявляемся тем, кому это разрешено правилом «сейчас в сети».
        if let Err(err) = announce_presence(&mut socket, store, mls, sink, &mut live.outbox).await {
            fail(sink, "presence", &err.to_string());
        }

        // То, что не ушло из-за обрыва. Досылаем до всего остального: порядок
        // сообщений для человека важнее, чем свежесть.
        for waiting in std::mem::take(&mut live.outbox) {
            let mut sealed = Some(SealedSenderCtx { required: live.require_anonymous });
            if let Err(err) = deliver(
                &mut socket, store, mls, sink, &mut pending, waiting, &mut live.outbox,
                sealed.as_mut(),
            )
            .await
            {
                if is_transport(&err) {
                    return Ok(Outcome::Retry);
                }
                fail(sink, "send", &err.to_string());
            }
        }
    }

    if authenticated { flush_anonymous(&mut socket, store, sink, live, greeting.sealed_sender, &anon_tx).await?; }

    let mut ticker = tokio::time::interval(Duration::from_secs(greeting.heartbeat_sec));
    ticker.tick().await; // первый тик срабатывает сразу — пропускаем
    let idle_timeout = Duration::from_secs((greeting.heartbeat_sec * 3).clamp(30, 180));
    let mut last_received = tokio::time::Instant::now();
    let mut probe: Option<(tokio::time::Instant,Vec<u8>)> = None;

    loop {
        let anonymous_deadline = if authenticated && greeting.sealed_sender {
            next_anonymous_maintenance(live, live.anonymous_queued)
        } else { None };
        let prewarming_active = !live.prewarming.is_empty();
        tokio::select! {
            _ = tokio::time::sleep_until(probe.as_ref().map(|p|p.0).unwrap_or(last_received+idle_timeout)), if probe.is_some() => {
                return Err(CoreError::Transport("Resume probe timed out; reconnecting".into()));
            }
            Some(outcome) = live.prewarming.join_next(), if prewarming_active => {
                accept_anonymous_prewarm(live, outcome);
                if authenticated {
                    flush_anonymous(&mut socket, store, sink, live, greeting.sealed_sender, &anon_tx).await?;
                }
            }
            _ = tokio::time::sleep_until(last_received + idle_timeout) => {
                return Err(CoreError::Transport("Server stopped responding; reconnecting".into()));
            }
            incoming = socket.next() => {
                let Some(message) = incoming else { return Ok(Outcome::Retry) };
                let message = message.map_err(|err| CoreError::Transport(err.to_string()))?;
                last_received = tokio::time::Instant::now();
                if let Message::Pong(payload)=&message {
                    if probe.as_ref().is_some_and(|p|p.1.as_slice()==payload.as_slice()) {probe=None;}
                }

                // Закрытие сервером надо заметить здесь. Дальше сокет только
                // выглядит живым: отправка в него вернёт «sending after
                // closing», а мы будем считать, что всё в порядке, — и
                // доставка встанет до перезапуска.
                if let Message::Close(frame) = &message {
                    let reason = frame
                        .as_ref()
                        .map(|f| f.reason.to_string())
                        .filter(|reason| !reason.is_empty())
                        .unwrap_or_else(|| "сервер закрыл соединение".into());
                    sink(Event::Disconnected { reason });
                    return Ok(Outcome::Retry);
                }

                if let Message::Binary(data) = message {
                    // Ответ на восстановление разбирается здесь, а не в on_frame:
                    // логин знает только эта сторона — сервер его не видел.
                    if matches!(proto::split(&data), Ok((op::USERNAME_OK, _))) {
                        let own = match pending_username.take() {
                            // Имя, занятое этим клиентом, всегда занято обеими
                            // формами хеша: дозанимать его потом не нужно.
                            Some((name, discoverable)) => OwnUsername {
                                name: Some(name),
                                discoverable,
                                strong_hash: true,
                            },
                            None => OwnUsername {
                                name: None,
                                discoverable: true,
                                strong_hash: false,
                            },
                        };
                        match serde_json::to_vec(&own)
                            .map_err(CoreError::from)
                            .and_then(|raw| store.save_setting(USERNAME_KEY, &raw))
                        {
                            Ok(()) => sink(Event::Username {
                                name: own.name,
                                discoverable: own.discoverable,
                            }),
                            Err(err) => fail(sink, "storage", &err.to_string()),
                        }
                        sync_server_privacy(&mut socket, store, &mut pending_username).await?;
                        continue;
                    }
                    if matches!(proto::split(&data), Ok((op::USERNAME_FOUND, _))) {
                        let (_, body) = proto::split(&data)?;
                        let found: proto::UsernameFound = proto::parse_json(body)?;
                        let query = looking_for.take().unwrap_or_default();
                        let device = if found.found { found.device } else { None };

                        // Ответ сервера сверяется с тем, что мы помним об этом
                        // имени. Промолчать здесь нельзя: подмена ключа до
                        // первого письма выглядит ровно как обычная находка.
                        let mut trust_error = None;
                        let pin = match (&query.is_empty(), &device) {
                            (false, Some(device)) => {
                                let mut pins = match load_pins_checked(store) {
                                    Ok(pins) => pins,
                                    Err(err) => {
                                        trust_error = Some(err);
                                        crate::pins::Pins::default()
                                    }
                                };
                                let state = if trust_error.is_none() {
                                    Some(pins.check(&query, device, now_millis()))
                                } else {
                                    None
                                };
                                if matches!(state, Some(value) if value != crate::pins::PinState::Same) {
                                    if let Err(err) = save_pins(store, &pins) {
                                        trust_error = Some(err);
                                    }
                                }
                                if state == Some(crate::pins::PinState::Changed) && trust_error.is_none() {
                                    sink(Event::Anomaly {
                                        kind: "pinned_key_changed".into(),
                                        detail: format!(
                                            "у имени @{query} другой ключ устройства, чем прежде",
                                        ),
                                    });
                                }
                                state
                            }
                            _ => None,
                        };

                        if let Some(err) = trust_error {
                            // Не отдаём неприкреплённый ключ UI: ошибка диска
                            // не должна превращать TOFU в незаметный первый контакт.
                            fail(sink, "storage", &err.to_string());
                        } else {
                            let (avatar_mime, avatar_base64) = unseal_avatar(
                                store,
                                device.as_deref(),
                                found.avatar_mime,
                                found.avatar_base64,
                            );
                            let (emblem, color) = unseal_decor(
                                store,
                                device.as_deref(),
                                found.decor.as_deref(),
                                found.emblem,
                                found.color,
                            );
                            sink(Event::UsernameFound {
                                query,
                                device,
                                pin,
                                chat_code: found.chat_code,
                                avatar_mime,
                                avatar_base64,
                                emblem,
                                color,
                            });
                        }
                        // На проводе нет request id, поэтому одновременно
                        // держим ровно один поиск. Следующий уходит только
                        // после разбора ответа на предыдущий.
                        if let Some((next, hash, hash2)) = lookup_queue.pop_front() {
                            looking_for = Some(next);
                            send(
                                &mut socket,
                                proto::username_lookup_frame(&hash, &hash2)?,
                            )
                            .await?;
                        }
                        continue;
                    }
                    if matches!(proto::split(&data), Ok((op::RECOVERY_OK, _))) {
                        match pending_recovery.take() {
                            Some((login, totp)) => sink(Event::RecoverySaved { login, totp }),
                            None => sink(Event::RecoveryForgotten),
                        }
                        continue;
                    }
                    if let Err(err) =
                        on_frame(
                            &mut socket, &data, store, mls, sink, &mut pending, live,
                            greeting.sealed_sender, &anon_tx,
                        )
                        .await
                    {
                        // Битый кадр — рвём соединение, а не гадаем.
                        sink(Event::Disconnected { reason: err.to_string() });
                        return Ok(Outcome::Retry);
                    }
                    if authenticated && matches!(proto::split(&data), Ok((op::ENVELOPE | op::KEYPKG, _))) {
                        if let Err(err) = grant_missing(&mut socket, store, mls, sink, live).await {
                            fail(sink, "pass_grant", &err.to_string());
                        }
                        flush_anonymous(&mut socket, store, sink, live, greeting.sealed_sender, &anon_tx).await?;
                    }
                }
            }
            command = commands.recv() => {
                let Some(command) = command else { return Ok(Outcome::Closed) };

                // Локальные команды отвечают одинаково, подключены мы или нет.
                if handle_local(&command, store, sink) {
                    if matches!(command, Command::PrivacyMaximize) && restrictions_ready(store).unwrap_or(false) {
                        cancel_removed_anonymous(store, live)?;
                        let _ = socket.close(None).await;
                        sink(Event::Disconnected { reason: "Профиль сохранён; подключаемся через Onion".into() });
                        return Ok(Outcome::Closed);
                    }
                    if authenticated && matches!(command, Command::PrivacySet { .. }) {
                        grant_missing(&mut socket, store, mls, sink, live).await?;
                        sync_server_privacy(&mut socket, store, &mut pending_username).await?;
                        flush_anonymous(&mut socket, store, sink, live, greeting.sealed_sender, &anon_tx).await?;
                    }
                    if matches!(command, Command::ClearConversation { .. } | Command::DeleteConversation { .. }) {
                        cancel_removed_anonymous(store, live)?;
                    }
                    continue;
                }

                match command {
                    Command::Disconnect => {
                        let _ = socket.close(None).await;
                        sink(Event::Disconnected { reason: "by request".into() });
                        return Ok(Outcome::Closed);
                    }
                    Command::Verify { peer_device } => {
                        // Здесь MLS живой, поэтому эпоха свежая — в отличие от
                        // сверки вне соединения, где состояние поднимается из снимка.
                        sink(verification(mls, store, &peer_device));
                    }
                    Command::DeleteMessage { conversation, id, for_both } => {
                        match delete_locally(store, sink, &conversation, &id) {
                            Ok(peer) => {
                                cancel_removed_anonymous(store, live)?;
                                // Просьба уходит после удаления у себя: если
                                // связь оборвётся, у нас оно уже исчезло, а
                                // повторить просьбу человек сможет.
                                if for_both {
                                    if let Some(device) = peer {
                                        // На все устройства: удаление,
                                        // доехавшее до одного, оставляет
                                        // сообщение на остальных, и «удалить у
                                        // обоих» оказывается неправдой.
                                        let body = crate::access::delete_request(&[id.clone()]);
                                        if let Err(err) = deliver_to_person(
                                            &mut socket, store, mls, sink, &mut pending,
                                            device, body, true, &mut live.outbox, None,
                                        ).await {
                                            if is_transport(&err) {
                                                return Ok(Outcome::Retry);
                                            }
                                            fail(sink, "delete_request", &err.to_string());
                                        }
                                    }
                                }
                            }
                            Err(err) => fail(sink, "storage", &err.to_string()),
                        }
                    }
                    Command::EditMessage { conversation, id, body, for_both } => {
                        match edit_locally(store, sink, &conversation, &id, &body) {
                            Ok(peer) => {
                                // Как и с удалением: сначала у себя, потом
                                // просьба. Оборвётся связь — своё уже
                                // исправлено, а повторить человек сможет.
                                if for_both {
                                    if let Some(device) = peer {
                                        // Как и удаление — на все устройства.
                                        let request = crate::access::edit_request(&id, &body);
                                        if let Err(err) = deliver_to_person(
                                            &mut socket, store, mls, sink, &mut pending,
                                            device, request, true, &mut live.outbox, None,
                                        ).await {
                                            if is_transport(&err) {
                                                return Ok(Outcome::Retry);
                                            }
                                            fail(sink, "edit_request", &err.to_string());
                                        }
                                    }
                                }
                            }
                            Err(err) => fail(sink, "storage", &err.to_string()),
                        }
                    }
                    Command::CallProbe { recipient_device } => {
                        let device=hex::decode(&recipient_device).map_err(|_|CoreError::BadFrame)?;
                        let device:[u8;KEY_LEN]=device.try_into().map_err(|_|CoreError::BadKeyLength)?;
                        let Some(group_id)=store.conversation_with(&device)? else {fail(sink,"call_no_conversation","Open a conversation first");continue};
                        let Some(signal)=live.calls.start(&recipient_device,now_millis()) else {fail(sink,"call_probe_pending","A capability check is already pending");continue};
                        if !permits_activity(store,&signal.body(),&device)? {fail(sink,"call_privacy","Calls are disabled by your privacy rule");continue}
                        let id=signal.id.clone();
                        encrypt_and_send(&mut socket,store,mls,sink,&group_id,PendingSend{device,body:signal.body(),stored:true},&mut live.outbox,None).await?;
                        sink(Event::CallStatus{peer_device:recipient_device,call_id:id,status:"checking".into()});
                    }
                    Command::Typing { recipient_device, active } => {
                        // Ошибку не показываем: индикатор набора — вещь
                        // необязательная, и ругаться на него посреди переписки
                        // хуже, чем тихо его не отправить.
                        if let Err(err) = send_typing(
                            &mut socket, store, mls, sink, &recipient_device, active,
                            &mut live.outbox,
                        ).await {
                            if is_transport(&err) {
                                return Ok(Outcome::Retry);
                            }
                        }
                    }
                    Command::AccessSet { policy } => {
                        if policy == crate::access::Policy::Passes && !restrictions_ready(store)? {
                            fail(sink, "privacy_pass_exchange_pending", "Дождитесь обмена пропусками с контактами онлайн и повторите.");
                            continue;
                        }
                        let label = match policy {
                            crate::access::Policy::Everyone => "everyone",
                            crate::access::Policy::Passes => "passes",
                        };
                        // Сначала раздаём пропуска, потом запираем дверь: в
                        // обратном порядке знакомые остались бы снаружи до
                        // следующего подключения.
                        if let Err(err) = grant_missing(&mut socket, store, mls, sink, live).await {
                            fail(sink, "pass_grant", &err.to_string());
                        }
                        send(&mut socket, proto::access_set_frame(label)?).await?;

                        let mut access = load_access(store);
                        access.policy = policy;
                        match save_access(store, &access) {
                            Ok(()) => sink(access_event(store)),
                            Err(err) => fail(sink, "storage", &err.to_string()),
                        }
                    }
                    Command::PassInvite { label, one_time, ttl_sec } => {
                        let (pass, hash) = crate::access::new_pass();
                        send(&mut socket, proto::pass_create_frame(&hash, one_time, ttl_sec)?).await?;

                        let mut access = load_access(store);
                        access.invites.push(crate::access::Invite {
                            pass,
                            hash,
                            label,
                            one_time,
                            ttl_sec,
                            created_at: now_millis(),
                        });
                        match save_access(store, &access) {
                            Ok(()) => sink(access_event(store)),
                            Err(err) => fail(sink, "storage", &err.to_string()),
                        }
                    }
                    Command::PassRevoke { hash } => {
                        send(&mut socket, proto::pass_revoke_frame(&hash)?).await?;

                        let mut access = load_access(store);
                        access.invites.retain(|invite| invite.hash != hash);
                        access.granted.retain(|_, granted| *granted != hash);
                        match save_access(store, &access) {
                            Ok(()) => sink(access_event(store)),
                            Err(err) => fail(sink, "storage", &err.to_string()),
                        }
                    }
                    Command::UsernameSet { name, discoverable } => {
                        match crate::directory::normalize_username(&name) {
                            Ok(normalized) => {
                                // Argon2id считается здесь и держит поток ядра
                                // около десятой доли секунды. Это цена за то,
                                // чтобы утёкшая таблица имён не перебиралась
                                // словарём, и платится она дважды за имя.
                                let hash = crate::directory::username_hash(&normalized);
                                let hash2 = crate::directory::username_hash_v2(&normalized)?;
                                pending_username = Some((normalized, discoverable));
                                send(
                                    &mut socket,
                                    proto::username_set_frame(&hash, &hash2, discoverable)?,
                                )
                                .await?;
                            }
                            Err(err) => fail(sink, "bad_username", &err.to_string()),
                        }
                    }
                    Command::UsernameClear => {
                        pending_username = None;
                        send(&mut socket, proto::username_clear_frame()?).await?;
                    }
                    Command::UsernameLookup { name } => {
                        match crate::directory::normalize_username(&name) {
                            Ok(normalized) => {
                                let hash = crate::directory::username_hash(&normalized);
                                let hash2 = crate::directory::username_hash_v2(&normalized)?;
                                if looking_for.is_some() {
                                    lookup_queue.push_back((normalized, hash, hash2));
                                } else {
                                    looking_for = Some(normalized);
                                    send(
                                        &mut socket,
                                        proto::username_lookup_frame(&hash, &hash2)?,
                                    )
                                    .await?;
                                }
                            }
                            Err(err) => fail(sink, "bad_username", &err.to_string()),
                        }
                    }
                    Command::ProfileGet { query } => {
                        if greeting.profiles {
                            send(&mut socket, proto::profile_get_frame(&query)?).await?;
                        } else {
                            fail(sink, "profiles_unavailable", "server does not support profiles yet");
                        }
                    }
                    Command::ProfileDecor { emblem, color } => {
                        if greeting.decor {
                            // Значок и цвет прячутся тем же правилом, что и
                            // аватар: они про то же — как человек выглядит
                            // рядом со своим именем.
                            let frame = if avatar_is_private(store) {
                                let key = own_profile_key(store)?;
                                let sealed = crate::profile::seal_decor(
                                    &key,
                                    emblem.as_deref().filter(|value| *value != "none"),
                                    color.as_deref().filter(|value| *value != "none"),
                                )?;
                                proto::profile_decor_sealed_frame(&sealed)?
                            } else {
                                proto::profile_decor_frame(&emblem, &color)?
                            };
                            send(&mut socket, frame).await?;
                        } else {
                            fail(sink, "decor_unavailable", "сервер ещё не умеет значки и цвета");
                        }
                    }
                    Command::GroupCreate { title, kind, members } => {
                        if let Err(err) = members.iter().try_for_each(|member| ensure_pin_allows(store, member)) {
                            match err {
                                CoreError::Anomaly(detail) => sink(Event::Anomaly {
                                    kind: "send_blocked".into(),
                                    detail,
                                }),
                                other => fail(sink, "storage", &other.to_string()),
                            }
                            continue;
                        }
                        let kind = if kind == "channel" { "channel" } else { "chat" };
                        match mls.create_group() {
                            Ok(group_id) => {
                                let meta = GroupMeta {
                                    title,
                                    kind: kind.to_string(),
                                    owner: hex::encode(mls.device_pub()),
                                    members: Vec::new(),
                                };
                                let raw = serde_json::to_vec(&meta).unwrap_or_default();
                                store.save_group(&group_id, kind, &raw, now_millis())?;
                                persist(store, mls, sink);
                                sink(group_event(mls, store, &group_id, &meta));
                                for member in members {
                                    request_invite(&mut socket, sink, &mut pending, &group_id, &member).await?;
                                }
                            }
                            Err(err) => fail(sink, "group_create", &err.to_string()),
                        }
                    }
                    Command::GroupInvite { group, members } => {
                        if let Err(err) = members.iter().try_for_each(|member| ensure_pin_allows(store, member)) {
                            match err {
                                CoreError::Anomaly(detail) => sink(Event::Anomaly {
                                    kind: "send_blocked".into(),
                                    detail,
                                }),
                                other => fail(sink, "storage", &other.to_string()),
                            }
                            continue;
                        }
                        match hex::decode(&group) {
                            Ok(group_id) => {
                                for member in members {
                                    request_invite(&mut socket, sink, &mut pending, &group_id, &member).await?;
                                }
                            }
                            Err(_) => fail(sink, "bad_group", "идентификатор группы неразборчив"),
                        }
                    }
                    Command::GroupRemove { group, device } => {
                        let (Ok(group_id), Ok(raw)) = (hex::decode(&group), hex::decode(&device))
                        else {
                            fail(sink, "bad_group", "идентификатор неразборчив");
                            continue;
                        };
                        let Ok(target): std::result::Result<[u8; KEY_LEN], _> = raw.try_into()
                        else {
                            fail(sink, "bad_device", "адрес устройства неверной длины");
                            continue;
                        };
                        match mls.remove_member(&group_id, &target) {
                            Ok(commit) => {
                                persist(store, mls, sink);
                                fan_out(&mut socket, store, mls, &group_id, &commit, None, None).await?;
                                if let Some((_, meta)) = load_meta(store, &group_id) {
                                    sink(group_event(mls, store, &group_id, &meta));
                                }
                            }
                            Err(err) => fail(sink, "group_remove", &err.to_string()),
                        }
                    }
                    Command::GroupSend { group, body } => {
                        let Ok(group_id) = hex::decode(&group) else {
                            fail(sink, "bad_group", "идентификатор группы неразборчив");
                            continue;
                        };
                        // В своём канале пишет только владелец — и здесь тоже:
                        // отправлять то, что получатели отвергнут, незачем.
                        if let Some((kind, meta)) = load_meta(store, &group_id) {
                            if kind == "channel" && meta.owner != hex::encode(mls.device_pub()) {
                                fail(sink, "channel_readonly", "в этот канал пишет только владелец");
                                continue;
                            }
                        }
                        match mls.encrypt_group(&group_id, &crate::padding::pad(body.as_bytes())) {
                            Ok(ciphertext) => {
                                persist(store, mls, sink);
                                let mut id = [0u8; ID_LEN];
                                id.copy_from_slice(&random_bytes(ID_LEN));
                                store.insert_message(&id, &group_id, true, now_millis(), body.as_bytes())?;
                                let logical_id = content_json(&body)
                                    .and_then(|v| v.get("id").and_then(|id| id.as_str()).map(str::to_owned))
                                    .unwrap_or_else(|| hex::encode(id));
                                fan_out(&mut socket, store, mls, &group_id, &ciphertext, None, Some(&logical_id)).await?;
                            }
                            Err(CoreError::Anomaly(detail)) => {
                                sink(Event::Anomaly { kind: "send_blocked".into(), detail });
                            }
                            Err(err) => fail(sink, "group_send", &err.to_string()),
                        }
                    }
                    Command::GroupForget { group } => {
                        if let Ok(group_id) = hex::decode(&group) {
                            crate::anon_queue::forget_thread(store, &group_id)?;
                            cancel_removed_anonymous(store, live)?;
                            store.forget_group(&group_id)?;
                            sink(Event::GroupForgotten { group });
                        }
                    }
                    Command::ChannelCreate { handle, title, about } => {
                        send(&mut socket, proto::channel_frame(op::CHANNEL_CREATE,
                            &serde_json::json!({ "handle": handle, "title": title, "about": about }))?).await?;
                    }
                    Command::ChannelPublish { channel, body } => {
                        /*
                          Идентификатор и время придумываем здесь.

                          Подпись должна покрывать то, что увидит читатель, —
                          а увидит он время и то, в каком канале пост стоит.
                          Придумай их сервер, подписывать было бы нечего:
                          автор не знает, что там появится после него.
                        */
                        let post_id = hex::encode(random_bytes(ID_LEN));
                        let created_at = now_millis();
                        // Ключ личности берём из базы: держать его копию в
                        // цикле соединения незачем, а подписываем мы редко.
                        let signature = match store.load_credentials() {
                            Ok(credentials) => crate::channels::sign(
                                &credentials.identity, &channel, &post_id, created_at, &body,
                            ),
                            Err(err) => {
                                fail(sink, "no_identity", &err.to_string());
                                continue;
                            }
                        };
                        send(&mut socket, proto::channel_frame(op::CHANNEL_PUBLISH,
                            &serde_json::json!({
                                "channel": channel,
                                "body": body,
                                "id": post_id,
                                "createdAt": created_at,
                                "signature": signature,
                            }))?).await?;
                    }
                    Command::ChannelList => {
                        send(&mut socket, proto::channel_frame(op::CHANNEL_LIST,
                            &serde_json::json!({}))?).await?;
                    }
                    Command::ChannelFeed { channel, before } => {
                        send(&mut socket, proto::channel_frame(op::CHANNEL_FEED,
                            &serde_json::json!({ "channel": channel, "before": before }))?).await?;
                    }
                    Command::ChannelSubscribe { channel, subscribe } => {
                        send(&mut socket, proto::channel_frame(op::CHANNEL_SUB,
                            &serde_json::json!({ "channel": channel, "subscribe": subscribe }))?).await?;
                    }
                    Command::ChannelFind { handle } => {
                        send(&mut socket, proto::channel_frame(op::CHANNEL_FIND,
                            &serde_json::json!({ "handle": handle }))?).await?;
                    }
                    Command::ChannelDeletePost { channel, post } => {
                        send(&mut socket, proto::channel_frame(op::CHANNEL_DELETE_POST,
                            &serde_json::json!({ "channel": channel, "post": post }))?).await?;
                    }
                    Command::ChannelDelete { channel } => {
                        send(&mut socket, proto::channel_frame(op::CHANNEL_DELETE,
                            &serde_json::json!({ "channel": channel }))?).await?;
                    }
                    Command::ChannelUpdate { channel, title, about, icon } => {
                        // Только тронутые поля: сервер отличает «не менять» от
                        // «очистить» по наличию ключа, а не по пустому значению.
                        let mut payload = serde_json::Map::new();
                        payload.insert("channel".into(), channel.into());
                        if let Some(title) = title {
                            payload.insert("title".into(), title.into());
                        }
                        if let Some(about) = about {
                            payload.insert("about".into(), about.into());
                        }
                        if let Some(icon) = icon {
                            payload.insert("icon".into(), icon);
                        }
                        send(&mut socket, proto::channel_frame(op::CHANNEL_UPDATE,
                            &serde_json::Value::Object(payload))?).await?;
                    }
                    Command::ChannelAdmin { channel, who, admin } => {
                        send(&mut socket, proto::channel_frame(op::CHANNEL_ADMIN,
                            &serde_json::json!({ "channel": channel, "who": who, "admin": admin }))?).await?;
                    }
                    Command::RevokeDevice { device } => {
                        // Своё текущее сервер откажется отзывать, и правильно
                        // сделает. Проверять это ещё и здесь незачем: два
                        // места, решающих одно, расходятся, и расходится
                        // обычно то, что снаружи.
                        let credentials = match store.load_credentials() {
                            Ok(credentials) => credentials,
                            Err(err) => { fail(sink, "no_identity", &err.to_string()); continue; }
                        };
                        let Ok(target) = hex::decode(&device) else {
                            fail(sink, "bad_device", "device must be hex");
                            continue;
                        };
                        let message = crate::keys::revoke_device_message(
                            &credentials.identity_pub(), &target,
                        );
                        let signature = credentials.identity.sign(&message);
                        send(&mut socket, proto::json_frame(op::DEVICE_REVOKE,
                            &serde_json::json!({
                                "device": device,
                                "signature": hex::encode(signature),
                            }))?).await?;
                    }
                    Command::RevokeOtherDevices => {
                        let credentials = match store.load_credentials() {
                            Ok(credentials) => credentials,
                            Err(err) => { fail(sink, "no_identity", &err.to_string()); continue; }
                        };
                        let message = crate::keys::revoke_other_devices_message(
                            &credentials.identity_pub(), &credentials.device_pub(),
                        );
                        let signature = credentials.identity.sign(&message);
                        send(&mut socket, proto::json_frame(op::DEVICE_REVOKE_OTHERS,
                            &serde_json::json!({ "signature": hex::encode(signature) }))?).await?;
                    }
                    Command::AdminGet { offset } => {
                        send(&mut socket, proto::admin_get_frame(offset)?).await?;
                    }
                    Command::AdminAction { action, reference } => {
                        send(&mut socket, proto::admin_action_frame(&action, &reference)?).await?;
                    }
                    Command::SupportGet { offset, thread } => {
                        send(&mut socket, proto::support_get_frame(offset, thread.as_deref())?).await?;
                    }
                    Command::SupportMark { thread, closed } => {
                        send(&mut socket, proto::support_mark_frame(&thread, closed)?).await?;
                    }
                    Command::ProfileSet { avatar_mime, avatar_base64 } => {
                        if greeting.profiles {
                            // Прячем аватар от сервера ровно тогда, когда
                            // человек и так велел показывать его не всем.
                            // Правило `profile_avatar` до сих пор соблюдал
                            // только наш собственный интерфейс — теперь его
                            // соблюдает и хранилище.
                            let (mime, data) = match (&avatar_mime, &avatar_base64) {
                                (Some(mime), Some(data)) if avatar_is_private(store) => {
                                    let key = own_profile_key(store)?;
                                    (
                                        Some(crate::profile::SEALED_MIME.to_owned()),
                                        Some(crate::profile::seal(&key, mime, data)?),
                                    )
                                }
                                _ => (avatar_mime.clone(), avatar_base64.clone()),
                            };
                            store.save_setting("pending_avatar_publication",&serde_json::to_vec(&(mime.clone(),data.clone()))?)?;
                            send(&mut socket, proto::profile_set_frame(&mime, &data)?).await?;
                        } else {
                            fail(sink, "profiles_unavailable", "server does not support profiles yet");
                        }
                    }
                    Command::RecoverySetup { login, password, totp, code } => {
                        if !authenticated {
                            fail(sink, "not_authenticated", "войдите, прежде чем включать восстановление");
                        } else if totp.is_some() && code.as_deref().unwrap_or("").is_empty() {
                            // Включать второй фактор без подтверждения нельзя:
                            // ошибка при переносе секрета обнаружилась бы только
                            // тогда, когда восстановление уже понадобилось.
                            fail(sink, "totp_code_required", "подтвердите код из приложения");
                        } else {
                            match seal_recovery(
                                store, &login, &password, totp.as_deref(), code.as_deref(),
                            ) {
                                Ok((normalized, frame)) => {
                                    pending_recovery = Some((normalized, totp.is_some()));
                                    send(&mut socket, frame).await?;
                                }
                                Err(err) => fail(sink, password_code_of(&err), &err.to_string()),
                            }
                        }
                    }
                    Command::RecoveryForget => {
                        if !authenticated {
                            fail(sink, "not_authenticated", "войдите, прежде чем менять восстановление");
                        } else {
                            pending_recovery = None;
                            send(&mut socket, proto::recovery_forget_frame()?).await?;
                        }
                    }
                    Command::Send { recipient_device, body } => {
                        if !authenticated {
                            fail(sink, "not_authenticated", "invoice is not funded yet");
                        } else if let Err(err) = ensure_pin_allows(store, &recipient_device) {
                            match err {
                                CoreError::Anomaly(detail) => sink(Event::Anomaly {
                                    kind: "send_blocked".into(),
                                    detail,
                                }),
                                other => fail(sink, "storage", &other.to_string()),
                            }
                        } else if let Err(err) = on_send(
                            &mut socket, store, mls, sink, &mut pending, &recipient_device, body.clone(),
                            &mut live.outbox,
                            Some(SealedSenderCtx { required: live.require_anonymous }),
                        )
                        .await
                        {
                            if let Some(id) = content_json(&body).and_then(|v| v.get("id")?.as_str().map(str::to_owned)) {
                                let conversation = hex::decode(&recipient_device).ok()
                                    .and_then(|device| store.conversation_with(&device).ok().flatten())
                                    .and_then(|group| store.thread_of(&group).ok()).map(hex::encode).unwrap_or_default();
                                sink(Event::Delivery { conversation, id, state: "failed".into() });
                            }
                            if is_transport(&err) {
                                // Сообщение уже в ящике — переподключаемся и
                                // досылаем, а не пишем в закрытый сокет.
                                sink(Event::Disconnected { reason: err.to_string() });
                                return Ok(Outcome::Retry);
                            }
                            fail(sink, "send", &err.to_string());
                        }
                    }
                    Command::NetworkChanged => return Ok(Outcome::Retry),
                    Command::ProbeConnection => {
                        if probe.is_none() {
                            let nonce=random_bytes(16);
                            tokio::time::timeout(Duration::from_secs(8),socket.send(Message::Ping(nonce.clone().into())))
                                .await.map_err(|_|CoreError::Transport("Resume probe write timed out".into()))?
                                .map_err(|e|CoreError::Transport(e.to_string()))?;
                            probe=Some((tokio::time::Instant::now()+Duration::from_secs(8),nonce));
                        }
                    }
                    Command::Connect { .. } => {
                        // Уже подключены — и это не ошибка, а вопрос «где я».
                        // Так спрашивает заново открытый экран: приложение
                        // свернули, окно пересоздали, а соединение всё это время
                        // жило. Раньше в ответ уходил отказ «busy», интерфейс
                        // ждал «вошли» и навсегда оставался на заставке.
                        //
                        // Пересказываем встречу целиком, а не одно «вошли»: без
                        // возможностей сервера заново открытый экран не знает,
                        // можно ли спрашивать профили, и показывает вместо имён
                        // шестнадцатеричные адреса.
                        //
                        // Очередь при этом ноль: всё накопленное уже отдано на
                        // входе, обещать «столько-то ждёт» было бы неправдой.
                        sink(Event::Connected {
                            heartbeat_sec: greeting.heartbeat_sec,
                            invite_entry: greeting.invite_entry,
                            ton_entry: greeting.ton_entry,
                            profiles: greeting.profiles,
                            decor: greeting.decor,
                        });
                        sink(Event::Authenticated {
                            device_id: device_id.clone(),
                            queued: 0,
                            admin,
                        });
                    }
                    other => {
                        // Остальное — вход и восстановление — имеет смысл только
                        // вне соединения.
                        fail(sink, "busy", &format!("already connected: {other:?}"));
                    }
                }
                if authenticated { flush_anonymous(&mut socket, store, sink, live, greeting.sealed_sender, &anon_tx).await?; }
            }
            _ = wait_anonymous_retry(anonymous_deadline), if authenticated => {
                flush_anonymous(&mut socket, store, sink, live, greeting.sealed_sender, &anon_tx).await?;
            }
            _ = ticker.tick() => {
                if authenticated { repair_missing_channels(&mut socket,store,mls,sink,&mut pending).await?; }
                send(&mut socket, proto::frame(op::PING, &[])).await?;
                if authenticated {
                    flush_anonymous(&mut socket, store, sink, live, greeting.sealed_sender, &anon_tx).await?;
                }
            }
            // An anonymous outcome is never a reason to send via AUTH.
            Some(AnonOutcome { route, client_ref, result }) = anon_rx.recv() => {
                live.anon_inflight.remove(&client_ref);
                match result {
                    Ok(envelope_id) => {
                        live.anon_retries.remove(&client_ref);
                        live.route_cooldowns.remove(&route);
                        let completed = crate::anon_queue::load(store)?.into_iter()
                            .find(|item| item.client_ref == client_ref);
                        crate::anon_queue::acknowledge(store, &client_ref)?;
                        if let Some(item) = completed {
                            if let Some(id) = item.logical_id {
                                emit_delivery(store, sink, &item.conversation, &id)?;
                            }
                        }
                        sink(Event::Accepted {
                            client_ref: hex::encode(client_ref),
                            envelope_id: hex::encode(envelope_id),
                        });
                    }
                    Err(()) => {
                        if let Some(item) = crate::anon_queue::load(store)?.into_iter().find(|item| item.client_ref == client_ref) {
                            if let Some(id) = item.logical_id { emit_delivery(store, sink, &item.conversation, &id)?; }
                        }
                        let attempts = live.anon_retries.get(&client_ref).map_or(1, |(_, n)| n.saturating_add(1));
                        let delay = Duration::from_secs((1u64 << attempts.min(3)).min(10));
                        live.anon_retries.insert(client_ref, (tokio::time::Instant::now() + delay, attempts));
                        live.route_cooldowns.insert(route, tokio::time::Instant::now() + Duration::from_secs(30));
                        fail(sink, "anonymous_delivery_waiting",
                            "Анонимная доставка ожидает Tor. Сообщение сохранено; обычная отправка отключена.");
                    }
                }
                flush_anonymous(&mut socket, store, sink, live, greeting.sealed_sender, &anon_tx).await?;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn on_frame(
    socket: &mut Socket,
    data: &[u8],
    store: &Store,
    mls: &mut Mls,
    sink: &EventSink,
    pending: &mut HashMap<[u8; ID_LEN], Claim>,
    live: &mut Live,
    sealed_sender_available: bool,
    anon_tx: &mpsc::UnboundedSender<AnonOutcome>,
) -> Result<()> {
    let (opcode, body) = proto::split(data)?;
    match opcode {
        op::PONG => {}
        op::QUEUE_DONE => sink(Event::QueueDone),
        op::ENVELOPE => on_envelope(socket, body, store, mls, sink, &mut live.calls).await?,
        op::KEYPKG => {
            let sealed = Some(SealedSenderCtx { required: live.require_anonymous });
            on_key_package(socket, body, store, mls, sink, pending, &mut live.outbox, sealed).await?;
            flush_anonymous(socket, store, sink, live, sealed_sender_available, anon_tx).await?
        }
        // Sealed sender: пачка билетов, заказанная encrypt_and_send. Спрятать
        // их негде, кроме как здесь — TICKET_REQUEST не привязан к конкретной
        // отправке, и ответ может прийти в любой момент между ними.
        op::TICKET_GRANT => {
            let tickets = proto::parse_ticket_grant(body)?;
            accept_anonymous_tickets(live, tickets, now_millis());
            flush_anonymous(socket, store, sink, live, sealed_sender_available, anon_tx).await?;
        }
        op::PROFILE => {
            let profile: proto::ProfilePayload = proto::parse_json(body)?;
            let published: Option<(Option<String>,Option<String>)> = store.load_setting("pending_avatar_publication")?
                .and_then(|raw|serde_json::from_slice(&raw).ok());
            if profile.device==hex::encode(store.load_credentials()?.device_pub())
                && published.as_ref()==Some(&(profile.avatar_mime.clone(),profile.avatar_base64.clone())) {
                let recipients=avatar_recipients(store);
                for (device,group) in store.list_conversations()? {
                    if !recipients.contains(&hex::encode(&device)) || mls.inspect(&group).is_err() {continue;}
                    let Ok(device)=<[u8;KEY_LEN]>::try_from(device.as_slice()) else{continue};
                    encrypt_and_send(socket,store,mls,sink,&group,PendingSend{device,
                        body:"\u{2063}VALCTL1:{\"profileChanged\":true}".into(),stored:true},
                        &mut live.outbox,Some(&mut SealedSenderCtx{required:live.require_anonymous})).await?;
                }
                store.save_setting("pending_avatar_publication",b"null")?;
                flush_anonymous(socket,store,sink,live,sealed_sender_available,anon_tx).await?;
            }
            // Аватар мог приехать запечатанным. Ключ есть — открываем, нет —
            // отдаём наверх пустоту: интерфейс покажет инициалы.
            let (avatar_mime, avatar_base64) = unseal_avatar(
                store,
                Some(profile.device.as_str()),
                profile.avatar_mime,
                profile.avatar_base64,
            );
            let (emblem, color) = unseal_decor(
                store,
                Some(profile.device.as_str()),
                profile.decor.as_deref(),
                profile.emblem,
                profile.color,
            );
            sink(Event::Profile {
                device: profile.device,
                chat_code: profile.chat_code,
                handle: profile.handle,
                avatar_mime,
                avatar_base64,
                emblem,
                color,
                updated_at: profile.updated_at,
            });
        }
        op::SEND_OK => {
            let (client_ref, envelope_id) = proto::parse_send_ok(body)?;
            if let Some(raw) = store.load_setting(&format!("delivery-ref:{}", hex::encode(client_ref)))? {
                let (thread, id, device): (Vec<u8>, String, Vec<u8>) = serde_json::from_slice(&raw)?;
                crate::delivery::accepted(store, &thread, &id, &device)?;
                emit_delivery(store, sink, &thread, &id)?;
            }
            sink(Event::Accepted {
                client_ref: hex::encode(client_ref),
                envelope_id: hex::encode(envelope_id),
            });
        }
        #[cfg(feature = "ton")]
        op::PAY_OK => {
            let info: serde_json::Value = proto::parse_json(body)?;
            sink(Event::InvoicePaid {
                reference: info.get("ref").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
            });
        }
        op::ERROR => {
            let err: ServerError = proto::parse_json(body)?;
            sink(Event::Failed { code: err.code, message: err.message });
        }

        op::CHANNEL_OK => {
            let mut report: serde_json::Value = proto::parse_json(body)?;
            check_channel_report(store, sink, &mut report);
            sink(Event::Channels { report });
        }
        op::CHANNEL_POST => {
            let mut report: serde_json::Value = proto::parse_json(body)?;
            check_channel_report(store, sink, &mut report);
            sink(Event::ChannelPost { report });
        }
        op::DEVICE_OK => {
            // Одним кодом отвечают и на отзыв, и на запрос списка. Различаем по
            // полю: у отзыва его нет, а придумывать второй код ради этого
            // значило бы держать в протоколе лишнюю запись.
            let report: serde_json::Value = proto::parse_json(body)?;
            if report.get("devices").is_some() {
                let own: proto::OwnDevices = proto::parse_json(body)?;
                own_devices_arrived(socket, store, mls, sink, live, own).await?;
            } else {
                sink(Event::DevicesRevoked {
                    count: report.get("revoked").and_then(|v| v.as_u64()).unwrap_or(0),
                });
            }
        }
        op::SUPPORT_OK => {
            // Как и ADMIN_OK: набор полей задаёт сервер, разбирать их здесь
            // значило бы ломать клиент при каждом новом поле.
            sink(Event::Support { report: proto::parse_json(body)? });
        }
        op::ADMIN_OK => {
            // Отчёт пересылается как есть: набор счётчиков задаёт сервер, и
            // разбирать его по полям здесь значило бы ломать клиент при каждом
            // новом счётчике.
            sink(Event::Admin { report: proto::parse_json(body)? });
        }
        op::ACCESS_OK => {
            // Подтверждение операции с доступом. Разбирать почти нечего, кроме
            // одного случая: наш пропуск не приняли. Тогда писать этому
            // человеку не выйдет, и он должен узнать причину, а не гадать.
            let ok: proto::AccessOk = proto::parse_json(body)?;
            if ok.admitted == Some(false) {
                fail(sink, "pass_rejected", "пропуск не принят: возможно, он отозван или истёк");
            }
        }

        // Незнакомый кадр пропускаем, а не рвём связь.
        //
        // Раньше здесь был отказ, и он ронял соединение. Это стоило
        // бесконечного цикла переподключений: сервер отвечал на выкладку
        // пропусков кадром 0x2a, которого клиент не знал, тот рвал связь, на
        // новом соединении всё повторялось — и так до перезапуска. Защищать
        // тут нечего: сервер и так может прислать что угодно, а клиент, не
        // переживающий нового кадра, ломается от любого обновления сервера.
        _ => {}
    }
    Ok(())
}

/// Конверты, промахнувшиеся мимо расшифровки один раз, но ещё не подтверждённые.
///
/// Лежит на диске, а не в памяти сессии, — и это важнее, чем кажется. Мобильная
/// ОС убивает процесс приложения между чем угодно: сворачиванием, нехваткой
/// памяти, обычным «смахнул из списка задач». Если бы список первых промахов
/// жил в `Live` (как было раньше), каждый новый процесс видел бы любой такой
/// конверт впервые, и «первый промах прощается» превращалось бы в «прощается
/// каждый раз» — письмо застревало бы в очереди сервера до истечения TTL,
/// вообще никогда не подтверждаясь. Это и происходило на практике: живое
/// устройство, подключавшееся регулярно, годами не разгружало один и тот же
/// конверт (правило про ACK в ARCHITECTURE.md §7 «Правила»).
const FAILED_ENVELOPES_KEY: &str = "failed_envelopes";
/// Не даёт множеству расти без границ при потоке действительно неразбираемых
/// конвертов — потолок спасает от накопления, а не заменяет TTL на сервере:
/// конверт, который так и не прочитается, всё равно рано или поздно протухнет
/// там сам.
const MAX_TRACKED_FAILURES: usize = 500;

fn load_failed_envelopes(store: &Store) -> std::collections::HashSet<String> {
    store
        .load_setting(FAILED_ENVELOPES_KEY)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}

fn save_failed_envelopes(store: &Store, set: &std::collections::HashSet<String>) {
    if let Ok(encoded) = serde_json::to_vec(set) {
        let _ = store.save_setting(FAILED_ENVELOPES_KEY, &encoded);
    }
}

/// Расшифровывает конверт и раскладывает по беседам.
///
/// ACK отправляется в любом случае, даже если расшифровать не удалось: сервер
/// хранит конверт до подтверждения, и без ACK нерасшифровываемый кадр приезжал
/// бы заново при каждом подключении. Ошибка при этом видна событием.
async fn on_envelope(
    socket: &mut Socket,
    body: &[u8],
    store: &Store,
    mls: &mut Mls,
    sink: &EventSink,
    calls: &mut crate::calls::Pending,
) -> Result<()> {
    let envelope = proto::parse_envelope(body)?;

    match mls.process(&envelope.ciphertext) {
        Ok(Incoming::Joined { group_id, peer_device }) => {
            let device = hex::encode(&peer_device);
            if blocked(store, &device) {
                // Приглашение от заблокированного не заводит беседу. Конверт
                // подтверждаем: иначе он будет приезжать снова и снова.
                return send(socket, proto::ack_frame(&envelope.id)).await;
            }
            remember_stranger(store, sink, &device, "написал первым");

            let thread = store.atomic(|| {
                let thread=attach_channel(store,&peer_device,&group_id)?;
                store.save_mls(&mls.signer_public(),&mls.snapshot())?;
                Ok(thread)
            })?;
            check_membership(mls, store, &group_id, sink);
            sink(Event::ConversationStarted {
                peer_device: device,
                conversation: hex::encode(&thread),
            });
        }
        Ok(Incoming::Message { group_id, sender_device, plaintext }) => {
            // Снимаем добивку сразу: дальше по коду тело считается тем, что
            // написал человек, и хвост из пробелов там был бы лишним — в том
            // числе в базе и в поиске по переписке.
            let plaintext = crate::padding::strip(&plaintext).to_vec();
            let device = hex::encode(&sender_device);
            if blocked(store, &device) {
                // Заблокированный не попадает ни в базу, ни на экран. Проверка
                // стоит здесь, а не в интерфейсе, именно поэтому: спрятать
                // сообщение мало, его не должно быть на диске.
                store.save_mls(&mls.signer_public(),&mls.snapshot())?;
                return send(socket, proto::ack_frame(&envelope.id)).await;
            }
            let text = String::from_utf8_lossy(&plaintext);
            if crate::access::is_control(&text) {
                // Служебное сообщение в переписке не сохраняется: в ленте ему
                // не место. Наружу выходит только то, что видно человеку, —
                // «печатает» и исчезнувшие сообщения.
                use crate::access::Control;
                match crate::access::parse_signal(&text) {
                    Some(Control::Delivered(ids)) => {
                        let thread = store.thread_of(&group_id)?;
                        for id in ids {
                            crate::delivery::delivered(store, &thread, &id, &sender_device)?;
                            emit_delivery(store, sink, &thread, &id)?;
                        }
                        store.save_mls(&mls.signer_public(), &mls.snapshot())?;
                    }
                    Some(Control::Pass(pass)) => {
                        let mut access = load_access(store);
                        access.hold(&device, &pass);
                        let _ = save_access(store, &access);
                    }
                    Some(Control::ProfileKey(key)) => {
                        // Ключ от аватара собеседника. Кладём его рядом с
                        // устройством, от которого он пришёл: чужой ключ на
                        // чужой аватар всё равно не подойдёт.
                        let mut keys = load_peer_profile_keys(store);
                        keys.insert(device.clone(), key);
                        store.save_setting(PEER_PROFILE_KEYS, &serde_json::to_vec(&keys)?)?;
                        store.save_mls(&mls.signer_public(), &mls.snapshot())?;
                        send(socket, proto::profile_get_frame(&device)?).await?;
                    }
                    Some(Control::ProfileChanged) => {
                        store.save_mls(&mls.signer_public(), &mls.snapshot())?;
                        send(socket,proto::profile_get_frame(&device)?).await?;
                    }
                    Some(Control::Delete(ids)) => {
                        // Нить, а не группа: сообщения лежат под нитью, и
                        // просьба удалить, пришедшая со второго устройства
                        // собеседника, обязана находить то же самое.
                        let thread = store.thread_of(&group_id)?;
                        let mut removed = Vec::new();
                        for id in ids {
                            if store.delete_message_by_id(&thread, &id).unwrap_or(false) {
                                removed.push(id);
                            }
                        }
                        if !removed.is_empty() {
                            sink(Event::Deleted {
                                conversation: hex::encode(&thread),
                                ids: removed,
                            });
                        }
                    }
                    Some(Control::Edit { id, body }) => {
                        let thread = store.thread_of(&group_id)?;
                        if store.update_message_by_id(&thread, &id, body.as_bytes())
                            .unwrap_or(false)
                        {
                            sink(Event::Edited {
                                conversation: hex::encode(&thread),
                                id,
                                body,
                            });
                        }
                    }
                    Some(Control::Devices(announcement)) => {
                        /*
                          Список устройств собеседника.

                          `accept` берёт устройство, от которого пришло
                          сообщение, и требует, чтобы оно нашлось в самом
                          списке: иначе собеседник объявляет список, в котором
                          его нет, и переписка целиком уезжает к тем, кто в нём
                          перечислен. Строки с несошедшейся подписью `accept`
                          выбрасывает молча.

                          Пока список только запоминается. Веером по нему
                          отправка пойдёт следующим шагом — сейчас доставка
                          по-прежнему идёт на то устройство, с которого пришло
                          сообщение.
                        */
                        if let Ok(from) = <[u8; KEY_LEN]>::try_from(sender_device.as_slice()) {
                            if let Some((identity, devices)) = announcement.accept(&from) {
                                remember_peer_devices(store, &devices);
                                let _ = store.remember_peer_identity(&identity, &devices);
                                // Беседы с разными устройствами одного человека
                                // сводятся в одну нить прямо здесь: узнали, что
                                // это один человек, — значит и переписка одна.
                                let _ = merge_threads(store, &identity);
                            }
                        }
                    }
                    Some(Control::Call(signal)) => {
                        let now=now_millis();
                        if signal.fresh(now) {
                            if signal.action==crate::calls::Action::Probe && permits_incoming_call(store,&sender_device)? && calls.answer(&device,&signal,now) {
                                let response=crate::calls::Signal{action:crate::calls::Action::Unavailable,..signal};
                                queue_private_message(store,mls,&group_id,&PendingSend{device:sender_device,body:response.body(),stored:true})?;
                            } else if calls.complete(&device,&signal,now) {
                                sink(Event::CallStatus{peer_device:device.clone(),call_id:signal.id,status:"media_unavailable".into()});
                            }
                        }
                    }
                    Some(Control::Typing(active)) => {
                        sink(Event::PeerTyping { peer_device: device.clone(), active });
                    }
                    Some(Control::Online) => {
                        sink(Event::PeerOnline { peer_device: device.clone() });
                    }
                    Some(Control::Group { title, kind, owner }) => {
                        // Название присылает тот, кто позвал. Верить ему на слово
                        // тут можно: состав всё равно задаёт MLS, а подпись под
                        // сообщением уже проверена.
                        let meta = GroupMeta {
                            title,
                            kind: kind.clone(),
                            owner,
                            members: Vec::new(),
                        };
                        if let Ok(raw) = serde_json::to_vec(&meta) {
                            let _ = store.save_group(&group_id, &kind, &raw, now_millis());
                        }
                        sink(group_event(mls, store, &group_id, &meta));
                    }
                    None => {}
                }
                // Control frames advance the receive ratchet too. ACK only
                // after that state is durable, including unknown extensions.
                store.save_mls(&mls.signer_public(),&mls.snapshot())?;
                return send(socket, proto::ack_frame(&envelope.id)).await;
            }
            // В канале пишет только владелец. Это соглашение клиентов, а не
            // запрет криптографии: MLS разрешает говорить любому участнику.
            // Поэтому проверка стоит на приёме — чужой пост не ляжет в базу,
            // даже если его собрали изменённым клиентом.
            if let Some((kind, meta)) = load_meta(store, &group_id) {
                if kind == "channel" && meta.owner != device {
                    sink(Event::Anomaly {
                        kind: "channel_post_rejected".into(),
                        detail: format!("в канале «{}» писать может только владелец", meta.title),
                    });
                    return send(socket, proto::ack_frame(&envelope.id)).await;
                }
            }

            remember_stranger(store, sink, &device, "написал первым");

            // Под нить, а не под группу: у собеседника с телефоном и ноутбуком
            // групп две, и переписка иначе легла бы в две разные ленты.
            let thread = store.thread_of(&group_id)?;
            store.atomic(|| {
                store.insert_message(&envelope.id, &thread, false, envelope.server_ts as i64, &plaintext)?;
                store.save_mls(&mls.signer_public(), &mls.snapshot())
            })?;
            check_membership(mls, store, &group_id, sink);
            sink(Event::Message {
                envelope_id: hex::encode(envelope.id),
                conversation: hex::encode(&thread),
                sender_device: device,
                server_ts: envelope.server_ts,
                body: String::from_utf8_lossy(&plaintext).into_owned(),
            });
            // Optional encrypted receipt: disabled by default together with
            // read receipts. Never emit it for controls, groups or read events.
            if load_meta(store, &group_id).is_none() {
                if let Some(value) = content_json(&text) {
                    if value.get("type").and_then(|v| v.as_str()) != Some("read") {
                        if let Some(id) = value.get("id").and_then(|v| v.as_str()).filter(|id| !id.is_empty() && id.len() <= 128) {
                            let body = format!("{}{}", "\u{2063}VALCTL1:",
                                serde_json::json!({"delivered": [id]}));
                            if permits_activity(store, &body, &sender_device)? {
                                encrypt_and_send(socket, store, mls, sink, &group_id,
                                    PendingSend { device: sender_device, body, stored: true },
                                    &mut Vec::new(), None).await?;
                            }
                        }
                    }
                }
            }
        }
        Ok(Incoming::Handled) => persist(store, mls, sink),

        // Повторная доставка после потерянного подтверждения. Ключ израсходован,
        // второй раз это сообщение не прочитается никогда — подтверждаем и
        // забываем. Человеку показывать нечего: он это сообщение уже видел.
        Err(CoreError::AlreadyProcessed(_)) => {}

        Err(err) => {
            fail(sink, "decrypt", &err.to_string());

            // Подтверждённый конверт сервер удаляет навсегда, поэтому с первого
            // промаха ACK не шлём: сообщение могло опередить приглашение, и на
            // следующем подключении оно разберётся. Но и держать его вечно
            // нельзя — конверт, не читаемый дважды, не прочитается уже никогда,
            // а очередь занимать будет до истечения срока.
            //
            // «Первый раз» проверяется по диску, а не по памяти этого запуска:
            // приложение может быть убито и перезапущено между двумя попытками,
            // и тогда в памяти это снова выглядело бы как первый промах —
            // конверт прощался бы бесконечно и никогда не подтверждался.
            let id_hex = hex::encode(envelope.id);
            let mut seen_failing = load_failed_envelopes(store);
            if seen_failing.insert(id_hex.clone()) {
                if seen_failing.len() <= MAX_TRACKED_FAILURES {
                    save_failed_envelopes(store, &seen_failing);
                }
                return Ok(());
            }
            seen_failing.remove(&id_hex);
            save_failed_envelopes(store, &seen_failing);
            sink(Event::Failed {
                code: "undecryptable".into(),
                message: "сообщение не удалось прочитать — снято с очереди".into(),
            });
        }
    }

    send(socket, proto::ack_frame(&envelope.id)).await
}

/// Описание группы: то, чего нет в самом MLS.
///
/// Состав здесь — снимок, а не второй источник правды: настоящий состав знает
/// MLS, и `group_event` каждый раз переписывает снимок его ответом. Нужен он
/// ради списка групп без сети: MLS живёт только внутри соединения, а показать
/// список надо сразу после запуска.
#[derive(serde::Serialize, serde::Deserialize)]
struct GroupMeta {
    title: String,
    kind: String,
    owner: String,
    #[serde(default)]
    members: Vec<String>,
}

fn load_meta(store: &Store, group_id: &[u8]) -> Option<(String, GroupMeta)> {
    let (kind, raw) = store.group(group_id).ok().flatten()?;
    let meta: GroupMeta = serde_json::from_slice(&raw).ok()?;
    Some((kind, meta))
}

fn group_event(mls: &Mls, store: &Store, group_id: &[u8], meta: &GroupMeta) -> Event {
    let members: Vec<String> = mls
        .members(group_id)
        .unwrap_or_default()
        .into_iter()
        .map(hex::encode)
        .collect();

    // Снимок состава обновляется тем же ответом MLS, которым отвечаем наружу:
    // разойтись им негде.
    let fresh = GroupMeta {
        title: meta.title.clone(),
        kind: meta.kind.clone(),
        owner: meta.owner.clone(),
        members: members.clone(),
    };
    if let Ok(raw) = serde_json::to_vec(&fresh) {
        let _ = store.save_group(group_id, &fresh.kind, &raw, now_millis());
    }

    Event::Group {
        group: hex::encode(group_id),
        kind: meta.kind.clone(),
        title: meta.title.clone(),
        owner: meta.owner.clone(),
        members,
    }
}

/// Отправляет один и тот же шифротекст каждому участнику, кроме себя.
///
/// Групп на сервере нет: он принимает конверты, адресованные устройствам. Это
/// и хорошо (состав группы ему негде хранить), и честно говоря дорого —
/// отправка в группу из двадцати человек это двадцать конвертов. Поэтому
/// группы здесь про десятки участников, а не про тысячи.
async fn fan_out(
    socket: &mut Socket,
    store: &Store,
    mls: &Mls,
    group_id: &[u8],
    payload: &[u8],
    skip: Option<[u8; KEY_LEN]>,
    message_id: Option<&str>,
) -> Result<()> {
    let me = mls.device_pub();
    let members: Vec<_> = mls.members(group_id)?.into_iter()
        .filter(|member| *member != me && Some(*member) != skip).collect();
    store.save_mls(&mls.signer_public(), &mls.snapshot())?;
    if anonymous_required(store)? {
        let envelopes = members.into_iter().map(|device| crate::anon_queue::Envelope {
            client_ref: random_bytes(ID_LEN).try_into().expect("random id length"),
            device, conversation: group_id.to_vec(), logical_id: message_id.map(str::to_owned),
            ciphertext: payload.to_vec(), expires_at: Some(now_millis() + i64::from(DEFAULT_TTL_SEC) * 1000),
            kind: if message_id.is_some() { crate::anon_queue::Kind::Message } else { crate::anon_queue::Kind::Essential },
        }).collect();
        return crate::anon_queue::enqueue_batch(store, envelopes);
    }
    for member in members {
        if member == me || Some(member) == skip {
            continue;
        }
        let mut client_ref = [0u8; ID_LEN];
        client_ref.copy_from_slice(&random_bytes(ID_LEN));
        send(
            socket,
            proto::send_frame(&client_ref, &member, DEFAULT_TTL_SEC, payload),
        )
        .await?;
    }
    Ok(())
}

/// Рассказывает участникам, как группа называется.
///
/// В самом Welcome названия нет, и придумать его получатель не может. Поэтому
/// описание едет обычным шифрованным сообщением внутри группы: сервер видит
/// такой же непрозрачный конверт, как и у любого другого.
async fn announce_group(
    socket: &mut Socket,
    store: &Store,
    mls: &mut Mls,
    group_id: &[u8],
    meta: &GroupMeta,
) -> Result<()> {
    let body = crate::access::group_signal(&meta.title, &meta.kind, &meta.owner);
    // Добивается только то, что уходит в шифр: в базе остаётся чистое тело,
    // иначе ступени раздували бы хранилище на обоих устройствах.
    let ciphertext = mls.encrypt_group(group_id, &crate::padding::pad(body.as_bytes()))?;
    fan_out(socket, store, mls, group_id, &ciphertext, None, None).await
}

/// Досылает приглашение, когда приехал KeyPackage приглашённого.
async fn finish_invite(
    socket: &mut Socket,
    store: &Store,
    mls: &mut Mls,
    sink: &EventSink,
    group_id: &[u8],
    device: [u8; KEY_LEN],
    package: &[u8],
) -> Result<()> {
    if !pin_allows_or_reports(store, sink, &device)? {
        return Ok(());
    }
    let (commit, welcome) = match mls.add_members(group_id, &[(package.to_vec(), device)]) {
        Ok(pair) => pair,
        Err(err) => {
            fail(sink, "group_invite", &err.to_string());
            return Ok(());
        }
    };
    persist(store, mls, sink);

    // Коммит — тем, кто уже был в группе; приглашение — новичку. Порядок важен:
    // без Welcome ему нечем разобрать даже следующий коммит.
    fan_out(socket, store, mls, group_id, &commit, Some(device), None).await?;
    send_envelope(socket, store, &device, &welcome, group_id).await?;

    if let Some((_, meta)) = load_meta(store, group_id) {
        announce_group(socket, store, mls, group_id, &meta).await?;
        sink(group_event(mls, store, group_id, &meta));
    }
    Ok(())
}

/// Просит KeyPackage приглашаемого: без него добавить лист нечем.
async fn request_invite(
    socket: &mut Socket,
    sink: &EventSink,
    pending: &mut HashMap<[u8; ID_LEN], Claim>,
    group_id: &[u8],
    member: &str,
) -> Result<()> {
    let Ok(raw) = hex::decode(member) else {
        fail(sink, "bad_device", "адрес устройства неразборчив");
        return Ok(());
    };
    let Ok(device): std::result::Result<[u8; KEY_LEN], _> = raw.try_into() else {
        fail(sink, "bad_device", "адрес устройства неверной длины");
        return Ok(());
    };

    let mut client_ref = [0u8; ID_LEN];
    client_ref.copy_from_slice(&random_bytes(ID_LEN));
    pending.insert(client_ref, Claim::Invite { group_id: group_id.to_vec(), device });
    send(socket, proto::keypkg_claim_frame(&client_ref, &device)).await
}

/// Приехал KeyPackage собеседника — заводим группу и досылаем отложенное.
#[allow(clippy::too_many_arguments)]
async fn on_key_package(
    socket: &mut Socket,
    body: &[u8],
    store: &Store,
    mls: &mut Mls,
    sink: &EventSink,
    pending: &mut HashMap<[u8; ID_LEN], Claim>,
    outbox: &mut Outbox,
    mut sealed: Option<SealedSenderCtx>,
) -> Result<()> {
    let (client_ref, package) = proto::parse_keypkg(body)?;
    let Some(claim) = pending.remove(&client_ref) else {
        // Ответ на запрос, который мы уже не ждём. Не повод рвать соединение.
        return Ok(());
    };

    let Some(package) = package else {
        fail(sink, "no_key_packages", "recipient has no key packages left");
        return Ok(());
    };

    let waiting = match claim {
        Claim::Start(waiting) => {
            if !pin_allows_or_reports(store, sink, &waiting.device)? {
                return Ok(());
            }
            waiting
        }
        Claim::Invite { group_id, device } => {
            return finish_invite(socket, store, mls, sink, &group_id, device, &package).await;
        }
    };

    // Привязку пакета к устройству проверяет сам MLS-слой: сервер мог подсунуть
    // чужой, и тогда start_conversation откажется его брать.
    let (group_id, welcome) = match mls.start_conversation(&package, &waiting.device) {
        Ok(result) => result,
        Err(err) => {
            fail(sink, "key_package", &err.to_string());
            return Ok(());
        }
    };
    let thread=store.atomic(|| {
        let thread=attach_channel(store,&waiting.device,&group_id)?;
        store.save_mls(&mls.signer_public(),&mls.snapshot())?;
        Ok(thread)
    })?;
    // Отправитель узнаёт идентификатор беседы тем же событием, что и
    // получатель: интерфейсу иначе некуда класть исходящие сообщения.
    sink(Event::ConversationStarted {
        peer_device: hex::encode(waiting.device),
        conversation: hex::encode(&thread),
    });

    // Сначала приглашение, потом само сообщение — порядок важен: без Welcome
    // получателю нечем расшифровать.
    if let Err(err) = send_envelope(socket, store, &waiting.device, &welcome, &thread).await {
        outbox.push(waiting);
        return Err(err);
    }
    encrypt_and_send(socket, store, mls, sink, &group_id, waiting, outbox, sealed.as_mut()).await
}

/// Все устройства человека, которому адресовано сообщение.
///
/// Названное устройство остаётся в списке всегда — даже если о личности мы
/// ничего не знаем: тогда список из него одного, и отправка ведёт себя ровно
/// как прежде.
///
/// Порядок не случайный: сначала те, с кем беседа уже заведена. Своя копия
/// сообщения ложится в базу первой отправкой, а заведение новой беседы —
/// это поход за KeyPackage и ответ отдельным кадром. Поставь мы такое
/// устройство первым, копия ждала бы ответа сервера, и при отказе человек
/// увидел бы, что его сообщение исчезло.
fn recipients(store: &Store, device: &[u8; KEY_LEN]) -> Vec<[u8; KEY_LEN]> {
    let Ok(Some(identity)) = store.identity_of_device(device) else {
        return vec![*device];
    };
    let known: std::collections::BTreeMap<String, Vec<String>> = store
        .load_setting(PEER_DEVICES)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default();
    let Some(list) = known.get(&hex::encode(device)) else {
        return vec![*device];
    };

    let mut devices: Vec<[u8; KEY_LEN]> = Vec::new();
    for entry in list {
        let Ok(raw) = hex::decode(entry) else { continue };
        let Ok(key): std::result::Result<[u8; KEY_LEN], _> = raw.clone().try_into() else {
            continue;
        };
        // Список хранится под каждым устройством человека, но принадлежать он
        // мог другому: сверяем личность, а не верим ключу настройки.
        if store.identity_of_device(&raw).ok().flatten().as_deref() != Some(&identity) {
            continue;
        }
        devices.push(key);
    }
    if !devices.contains(device) {
        devices.push(*device);
    }
    devices.sort_by_key(|entry| {
        matches!(store.conversation_with(entry), Ok(None) | Err(_))
    });
    devices
}

/// Отправка человеку, а не устройству.
///
/// Одно сообщение — по конверту на каждое его устройство. Иначе оно уходит
/// только туда, где человек был активнее, и до телефона, лежащего в кармане,
/// не доезжает никогда: выглядит это не как потеря, а как «ему не пришло».
///
/// Своя копия в базу ложится **один раз** — с первой отправки. Остальные
/// помечены как уже сохранённые, иначе одно написанное сообщение появилось бы
/// в собственной переписке столько раз, сколько у собеседника устройств.
#[allow(clippy::too_many_arguments)]
async fn deliver_to_person(
    socket: &mut Socket,
    store: &Store,
    mls: &mut Mls,
    sink: &EventSink,
    pending: &mut HashMap<[u8; ID_LEN], Claim>,
    device: [u8; KEY_LEN],
    body: String,
    stored: bool,
    outbox: &mut Outbox,
    mut sealed: Option<SealedSenderCtx>,
) -> Result<()> {
    let devices = recipients(store, &device);
    let mut saved = stored;
    for target in devices {
        let waiting = PendingSend { device: target, body: body.clone(), stored: saved };
        // Реborrow на каждый круг: у человека может быть несколько устройств,
        // и пул билетов/канал исхода у них общий на всё сообщение целиком.
        deliver(socket, store, mls, sink, pending, waiting, outbox, sealed.as_mut()).await?;
        saved = true;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn on_send(
    socket: &mut Socket,
    store: &Store,
    mls: &mut Mls,
    sink: &EventSink,
    pending: &mut HashMap<[u8; ID_LEN], Claim>,
    recipient_device: &str,
    body: String,
    outbox: &mut Outbox,
    sealed: Option<SealedSenderCtx>,
) -> Result<()> {
    let device = hex::decode(recipient_device).map_err(|_| CoreError::BadFrame)?;
    let device: [u8; KEY_LEN] = device.try_into().map_err(|_| CoreError::BadKeyLength)?;

    deliver_to_person(socket, store, mls, sink, pending, device, body, false, outbox, sealed).await
}

/// Общий путь для новой отправки и для досылки из ящика.
///
/// При обрыве сообщение возвращается в ящик, а не теряется: до этой правки
/// неудачная отправка оставляла человеку одну строку в журнале ошибок и
/// собственную копию в базе, которой собеседник никогда не увидит.
#[allow(clippy::too_many_arguments)]
async fn deliver(
    socket: &mut Socket,
    store: &Store,
    mls: &mut Mls,
    sink: &EventSink,
    pending: &mut HashMap<[u8; ID_LEN], Claim>,
    waiting: PendingSend,
    outbox: &mut Outbox,
    sealed: Option<&mut SealedSenderCtx>,
) -> Result<()> {
    if !pin_allows_or_reports(store, sink, &waiting.device)? {
        return Ok(());
    }
    match store.conversation_with(&waiting.device)? {
        Some(group_id) if !matches!(mls.inspect(&group_id),Err(CoreError::MissingGroup)) => {
            encrypt_and_send(socket, store, mls, sink, &group_id, waiting, outbox, sealed).await
        }
        _ => {
            // Беседы ещё нет: просим KeyPackage и досылаем сообщение по ответу.
            // Билет здесь никак не поможет — сообщение уйдёт уже потом, из
            // on_key_package, своим отдельным заходом за sealed sender.
            let mut client_ref = [0u8; ID_LEN];
            client_ref.copy_from_slice(&random_bytes(ID_LEN));
            let device = waiting.device;
            pending.insert(client_ref, Claim::Start(waiting));
            if let Err(err) = send(socket, proto::keypkg_claim_frame(&client_ref, &device)).await {
                if let Some(Claim::Start(lost)) = pending.remove(&client_ref) {
                    outbox.push(lost);
                }
                return Err(err);
            }
            Ok(())
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn encrypt_and_send(
    socket: &mut Socket,
    store: &Store,
    mls: &mut Mls,
    sink: &EventSink,
    group_id: &[u8],
    waiting: PendingSend,
    outbox: &mut Outbox,
    sealed: Option<&mut SealedSenderCtx>,
) -> Result<()> {
    if !permits_activity(store, &waiting.body, &waiting.device)? { return Ok(()) }
    if !pin_allows_or_reports(store, sink, &waiting.device)? {
        return Ok(());
    }
    if sealed.as_ref().is_some_and(|ctx| ctx.required) || anonymous_required(store)? {
        queue_private_message(store, mls, group_id, &waiting)?;
        if let Some(id) = content_json(&waiting.body).and_then(|v| v.get("id")?.as_str().map(str::to_owned)) {
            emit_delivery(store, sink, &store.thread_of(group_id)?, &id)?;
        }
        return Ok(());
    }
    let device = &waiting.device;
    let body = waiting.body.as_str();

    // Отказ здесь — не сбой отправки, а сигнал: состав беседы не тот, кому мы
    // собирались писать. Открытый текст в такую группу уходить не должен.
    let ciphertext = match mls.encrypt(group_id, &crate::padding::pad(body.as_bytes()), device) {
        Ok(ciphertext) => ciphertext,
        Err(CoreError::Anomaly(detail)) => {
            sink(Event::Anomaly { kind: "send_blocked".into(), detail });
            return Ok(());
        }
        Err(other) => return Err(other),
    };
    store.save_mls(&mls.signer_public(), &mls.snapshot())?;
    let client_ref: [u8; ID_LEN] = random_bytes(ID_LEN).try_into().unwrap();
    let thread = store.thread_of(group_id)?;
    if !waiting.stored {
        store.insert_message(&client_ref, &thread, true, now_millis(), body.as_bytes())?;
    }
    if let Some(id) = content_json(body).and_then(|v| v.get("id")?.as_str().map(str::to_owned)) {
        crate::delivery::track(store, &thread, &id, device, None)?;
        store.save_setting(&format!("delivery-ref:{}", hex::encode(client_ref)),
            &serde_json::to_vec(&(thread.clone(), id, device.to_vec()))?)?;
    }

    if let Err(err) =
        send(socket, proto::send_frame(&client_ref, device, DEFAULT_TTL_SEC, &ciphertext)).await
    {
        // Шифротекст этой эпохи уже не пригодится — при досылке текст будет
        // зашифрован заново, поэтому в ящик кладётся именно открытый текст.
        outbox.push(PendingSend { device: *device, body: waiting.body, stored: true });
        return Err(err);
    }
    Ok(())
}

/// Persist ratchet, own history and exact ciphertext as one durable change.
fn queue_private_message(store: &Store, mls: &mut Mls, group_id: &[u8], waiting: &PendingSend) -> Result<()> {
    if !permits_activity(store, &waiting.body, &waiting.device)? { return Ok(()); }
    ensure_pin_allows(store, &hex::encode(waiting.device))?;
    let before = mls.snapshot();
    let signer = mls.signer_public();
    let ciphertext = mls.encrypt(group_id, &crate::padding::pad(waiting.body.as_bytes()), &waiting.device)?;
    let now = now_millis();
    let client_ref: [u8; ID_LEN] = random_bytes(ID_LEN).try_into().unwrap();
    let thread = store.thread_of(group_id)?;
    let ephemeral = activity_rule(&waiting.body).is_some();
    let control = crate::access::is_control(&waiting.body);
    let logical_id = content_json(&waiting.body).and_then(|v| v.get("id")?.as_str().map(str::to_owned));
    let result = store.atomic(|| {
        store.save_mls(&mls.signer_public(), &mls.snapshot())?;
        if !waiting.stored && !control && !ephemeral {
            store.insert_message(&client_ref, &thread, true, now, waiting.body.as_bytes())?;
        }
        crate::anon_queue::enqueue(store, crate::anon_queue::Envelope {
            client_ref, device: waiting.device, conversation: thread, logical_id, ciphertext,
            kind: if ephemeral { crate::anon_queue::Kind::Ephemeral } else if control {
                crate::anon_queue::Kind::Essential } else { crate::anon_queue::Kind::Message },
            expires_at: Some(now + if matches!(crate::access::parse_signal(&waiting.body),Some(crate::access::Control::Call(_))) { crate::calls::TTL_MS } else if ephemeral { 15_000 } else { i64::from(DEFAULT_TTL_SEC) * 1000 }),
        })
    });
    if result.is_err() {
        // The database savepoint rolled back; the in-memory ratchet must too.
        *mls = Mls::restore(&store.load_credentials()?.device, &signer, &before)?;
    }
    result
}

fn queue_offline(store: &Store, sink: &EventSink, recipient: &str, body: &str) {
    let result = (|| -> Result<()> {
        if !anonymous_required(store)? { return Err(CoreError::Rejected("offline_requires_onion".into())); }
        ensure_pin_allows(store, recipient)?;
        let device: [u8; KEY_LEN] = hex::decode(recipient).map_err(|_| CoreError::BadFrame)?
            .try_into().map_err(|_| CoreError::BadKeyLength)?;
        let credentials = store.load_credentials()?;
        let mut mls = load_or_create_mls(store, &credentials)?;
        let devices = recipients(store, &device);
        let mut targets = Vec::new();
        for device in devices {
            let group = store.conversation_with(&device)?.ok_or_else(|| CoreError::Rejected("offline_first_contact".into()))?;
            targets.push((device, group));
        }
        store.atomic(|| {
            for (index, (device, group)) in targets.iter().enumerate() {
                queue_private_message(store, &mut mls, group, &PendingSend {
                    device: *device, body: body.to_owned(), stored: index > 0 })?;
            }
            Ok(())
        })?;
        if let Some(id) = content_json(body).and_then(|v| v.get("id")?.as_str().map(str::to_owned)) {
            for (_, group) in targets { emit_delivery(store, sink, &store.thread_of(&group)?, &id)?; }
        }
        Ok(())
    })();
    if let Err(error) = result {
        if let Some(id) = content_json(body).and_then(|v| v.get("id")?.as_str().map(str::to_owned)) {
            let conversation = hex::decode(recipient).ok().and_then(|d| store.conversation_with(&d).ok().flatten())
                .and_then(|g| store.thread_of(&g).ok()).map(hex::encode).unwrap_or_default();
            sink(Event::Delivery { conversation, id, state: "failed".into() });
        }
        fail(sink, "offline_send", &format!("Сообщение не сохранено: {error}. Для новой беседы сначала нужно подключение."));
    }
}

/// Один из известных onion-входов — специально для sealed sender.
///
/// Обычное соединение выбирает путь один раз на сессию и держится его, пока
/// он открывается (см. `routes_for`/`session`); анонимной отправке это не
/// подходит: если она год за годом ходит тем же путём, что и основное
/// соединение, наблюдатель сети свяжет их по одному этому факту, даже не
/// заглядывая внутрь протокола. Поэтому вход выбирается заново на каждую
/// попытку и намеренно не через `session`-маршрутизацию.
///
/// Только Tor: спрятать отправителя от сервера, но не от сети, — не та защита,
/// которую sealed sender обещает (ARCHITECTURE.md §7a). Список онион-входов
/// почти никогда не пуст — три запасных зашиты в сборку, — но défensive-`None`
/// оставлен на случай, если это когда-нибудь перестанет быть так.
fn pick_onion_route(store: &Store, live: &Live) -> Option<String> {
    let mut hosts = load_onion_hosts(store);
    let now = tokio::time::Instant::now();
    let healthy: Vec<_> = hosts.iter().filter(|route|
        !live.route_cooldowns.get(*route).is_some_and(|until| *until > now)).cloned().collect();
    if !healthy.is_empty() { hosts = healthy; }
    if hosts.is_empty() {
        return None;
    }
    let index = (random_bytes(1)[0] as usize) % hosts.len();
    hosts.into_iter().nth(index)
}

/// Один анонимный SEND: своё, отдельное соединение, которое никогда не
/// проходит AUTH — открывает его, ждёт HELLO, тратит билет и закрывается.
///
/// Не различает исход отказа: просроченный билет, неверная подпись, отказ
/// сервера, оборвавшийся Tor-circuit — для вызывающего все они означают одно
/// и то же: письмо остаётся в очереди для следующей onion-попытки. Разбор
/// причины не добавляет здесь ничего: подделывающему билет не должно быть
/// виднее, что именно он не угадал (это же верно и на сервере, см.
/// `valanium-server/src/auth/tickets.ts`), а настоящему отказу — что билет,
/// что обрыв связи — одинаково нужно просто попробовать иначе.
async fn send_anon_once(
    route: String,
    client_ref: [u8; ID_LEN],
    ticket: [u8; proto::TICKET_LEN],
    device: [u8; KEY_LEN],
    ciphertext: Vec<u8>,
    pass: Option<String>,
) -> Result<[u8; ID_LEN]> {
    // A sealed-sender task may only open an onion destination, even if a
    // future caller accidentally passes a normal WebSocket address.
    let request = crate::edge::ws_request(&route)?;
    if !request.uri().host().is_some_and(valid_onion_host) {
        return Err(CoreError::Transport("анонимная доставка требует onion-маршрут".into()));
    }
    let (socket, supports_passes) = prepare_anonymous_socket(&route).await?;
    send_on_anonymous_socket(socket, supports_passes, client_ref, ticket, device, ciphertext, pass).await
}

// Contains no identity, recipient, ticket or message while waiting in the pool.
async fn prepare_anonymous_socket(route: &str) -> Result<(Socket, bool)> {
    let started = std::time::Instant::now();
    let request = crate::edge::ws_request(route)?;
    if !request.uri().host().is_some_and(valid_onion_host) { return Err(CoreError::BadFrame); }
    let mut socket = open_socket(route).await?;
    delivery_timing("socks_ws_connect", started);
    let started = std::time::Instant::now();
    loop {
        let message = socket.next().await
            .ok_or_else(|| CoreError::Transport("closed before hello".into()))?
            .map_err(|err| CoreError::Transport(err.to_string()))?;
        if let Message::Binary(data) = message {
            let (opcode, body) = proto::split(&data)?;
            if opcode == op::HELLO {
                let hello: Hello = proto::parse_json(body)?;
                if !hello.features.sealed_sender { return Err(CoreError::Rejected("anonymous_unsupported".into())); }
                delivery_timing("ws_hello", started);
                return Ok((socket, hello.features.sealed_sender_passes));
            }
        }
    }
}

fn delivery_timing(stage: &str, started: std::time::Instant) {
    if std::env::var_os("VALANIUM_DELIVERY_TIMINGS").is_some() {
        eprintln!("valanium_delivery stage={stage} elapsed_ms={}", started.elapsed().as_millis());
    }
}

async fn send_on_anonymous_socket(
    mut socket: Socket, supports_passes: bool,
    client_ref: [u8; ID_LEN], ticket: [u8; proto::TICKET_LEN],
    device: [u8; KEY_LEN], ciphertext: Vec<u8>, pass: Option<String>,
) -> Result<[u8; ID_LEN]> {
    if let Some(pass) = &pass {
        if !supports_passes { return Err(CoreError::Rejected("anonymous_pass_unsupported".into())); }
        send(&mut socket, proto::pass_present_frame(&hex::encode(device), pass)?).await?;
    }
    let started = std::time::Instant::now();
    send(
        &mut socket,
        proto::send_anon_frame(&client_ref, &ticket, &device, DEFAULT_TTL_SEC, &ciphertext),
    )
    .await?;

    let envelope_id = loop {
        let message = socket
            .next()
            .await
            .ok_or_else(|| CoreError::Transport("closed before response".into()))?
            .map_err(|err| CoreError::Transport(err.to_string()))?;
        let Message::Binary(data) = message else { continue };
        let (opcode, body) = proto::split(&data)?;
        match opcode {
            op::SEND_OK => {
                let (ack_ref, envelope_id) = proto::parse_send_ok(body)?;
                if ack_ref != client_ref { return Err(CoreError::BadFrame); }
                break envelope_id;
            }
            op::ERROR => {
                let err: ServerError = proto::parse_json(body)?;
                return Err(CoreError::Rejected(err.code));
            }
            // Постороннее (например, чужой PONG) — соединение только на это и
            // заведено, но неизвестный кадр рвать его не должен.
            _ => continue,
        }
    };
    delivery_timing("server_ack", started);
    // ACK is final; do not delay queue progress for the close handshake.
    drop(socket);
    Ok(envelope_id)
}

/// Заводит `send_anon_once` отдельной задачей и не ждёт её здесь: `pump` не
/// должен стоять всё это время, особенно если путь идёт через Tor.
///
/// Успех сообщает о себе сам — `sink` можно звать из любой задачи. Неудача
/// возвращается каналом: pump обновляет очередь только после результата.
#[allow(clippy::too_many_arguments)]
fn spawn_anon_send(
    route: String,
    client_ref: [u8; ID_LEN],
    ticket: [u8; proto::TICKET_LEN],
    device: [u8; KEY_LEN],
    ciphertext: Vec<u8>,
    outcomes: mpsc::UnboundedSender<AnonOutcome>,
    expires_at: Option<i64>,
    pass: Option<String>,
    prepared: Option<(Socket, bool)>,
) -> tokio::task::AbortHandle {
    tokio::spawn(async move {
        let result = match tokio::time::timeout(
            expires_at.map(|at| Duration::from_millis(at.saturating_sub(now_millis()).max(0) as u64).min(ANON_SEND_TIMEOUT)).unwrap_or(ANON_SEND_TIMEOUT),
            async {
                match prepared {
                    Some((socket, supports_passes)) => send_on_anonymous_socket(socket, supports_passes,
                        client_ref, ticket, device, ciphertext, pass).await,
                    None => send_anon_once(route.clone(), client_ref, ticket, device, ciphertext, pass).await,
                }
            },
        ).await {
            Ok(Ok(envelope_id)) => Ok(envelope_id),
            _ => Err(()),
        };
        let _ = outcomes.send(AnonOutcome { route, client_ref, result });
    }).abort_handle()
}

fn cancel_removed_anonymous(store: &Store, live: &mut Live) -> Result<()> {
    if live.anon_inflight.is_empty() { return Ok(()); }
    let queued: HashSet<_> = crate::anon_queue::load(store)?.iter().map(|item| item.client_ref).collect();
    live.anon_inflight.retain(|id, task| {
        if queued.contains(id) { true } else { task.abort(); false }
    });
    Ok(())
}

fn ticket_usable(ticket: &[u8; proto::TICKET_LEN], now: i64) -> bool {
    // Public expiry is only a local scheduling hint; the server verifies the signature.
    let expiry = u64::from_be_bytes(ticket[16..24].try_into().unwrap());
    expiry > now.max(0).saturating_add(TICKET_SEND_MARGIN_MS) as u64
}

fn prune_anonymous_tickets(live: &mut Live, now: i64) {
    live.tickets.retain(|ticket| ticket_usable(ticket, now));
}

fn accept_anonymous_tickets(live: &mut Live, tickets: Vec<[u8; proto::TICKET_LEN]>, now: i64) {
    // Empty/expired grants must not cause an immediate request/response loop.
    if tickets.iter().any(|ticket| ticket_usable(ticket, now)) { live.ticket_request_until = None; }
    live.tickets.extend(tickets);
    prune_anonymous_tickets(live, now);
    live.tickets.truncate(32);
}

async fn request_anonymous_tickets(socket: &mut Socket, live: &mut Live) -> Result<()> {
    prune_anonymous_tickets(live, now_millis());
    let now = tokio::time::Instant::now();
    if live.tickets.len() < TICKET_LOW_WATERMARK
        && !live.ticket_request_until.is_some_and(|until| until > now) {
        send(socket, proto::ticket_request_frame(TICKET_BATCH_SIZE)).await?;
        live.ticket_request_until = Some(tokio::time::Instant::now() + TICKET_REQUEST_TIMEOUT);
    }
    Ok(())
}

fn maintain_anonymous_prewarm(store: &Store, live: &mut Live) {
    if !live.require_anonymous { return; }
    let now = tokio::time::Instant::now();
    live.prepared.retain(|ready| now.duration_since(ready.started) < ANON_READY_LIFETIME
        && !live.route_cooldowns.get(&ready.route).is_some_and(|until| *until > now));
    if live.prewarm_retry_at.is_some_and(|until| until > now) { return; }
    live.prewarm_retry_at = None;
    while live.prepared.len() + live.prewarming.len() < ANON_POOL_SIZE {
        let Some(route) = pick_onion_route(store, live) else { break };
        live.prewarming.spawn(async move {
            let result = async {
                let (socket, supports_passes) = tokio::time::timeout(
                    ANON_SEND_TIMEOUT, prepare_anonymous_socket(&route)).await
                    .map_err(|_| CoreError::Transport("anonymous prewarm timeout".into()))??;
                Ok(PreparedAnonymous { route: route.clone(), started: now, socket, supports_passes })
            }.await;
            (route, result)
        });
    }
}

fn accept_anonymous_prewarm(live: &mut Live,
    outcome: std::result::Result<(String, Result<PreparedAnonymous>), tokio::task::JoinError>) {
    match outcome {
        Ok((_, Ok(ready))) => live.prepared.push_back(ready),
        other => {
            let now = tokio::time::Instant::now();
            live.prewarm_retry_at = Some(now + Duration::from_secs(2));
            if let Ok((route, Err(_))) = other {
                live.route_cooldowns.insert(route, now + Duration::from_secs(30));
            }
        }
    }
}

fn next_anonymous_maintenance(live: &Live, sending: bool) -> Option<tokio::time::Instant> {
    let retry = if sending && !live.tickets.is_empty()
        && (!live.require_anonymous || !live.prepared.is_empty()) { next_anonymous_retry(live) } else { None };
    let ticket = if sending { live.ticket_request_until } else { None };
    [retry, ticket, live.prewarm_retry_at,
        live.prepared.iter().map(|r| r.started + ANON_READY_LIFETIME).min()]
        .into_iter().flatten().min()
}

// Ignore active sends: their old retry deadline must not spin the select loop.
fn next_anonymous_retry(live: &Live) -> Option<tokio::time::Instant> {
    if live.anon_inflight.len() >= 4 { return None; }
    live.anon_retries.iter().filter(|(id, _)| !live.anon_inflight.contains_key(*id))
        .map(|(_, (until, _))| *until).min()
}

async fn wait_anonymous_retry(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending::<()>().await,
    }
}

/// Retry only through anonymous onion connections; keep storage until ACK.
async fn flush_anonymous(
    socket: &mut Socket,
    store: &Store,
    sink: &EventSink,
    live: &mut Live,
    available: bool,
    outcomes: &mpsc::UnboundedSender<AnonOutcome>,
) -> Result<()> {
    if !available {
        return Ok(());
    }
    crate::anon_queue::bound_legacy_lifetimes(store, now_millis(), i64::from(DEFAULT_TTL_SEC) * 1000)?;
    let pending = crate::anon_queue::load(store)?;
    for item in &pending {
        if item.kind == crate::anon_queue::Kind::Message {
            if let Some(id) = &item.logical_id { emit_delivery(store, sink, &item.conversation, id)?; }
        }
    }
    let queue = crate::anon_queue::prune_expired(store, now_millis())?;
    live.anonymous_queued = !queue.is_empty();
    maintain_anonymous_prewarm(store, live);
    let queued: HashSet<_> = queue.iter().map(|item| item.client_ref).collect();
    live.anon_retries.retain(|id, _| queued.contains(id));
    if queue.is_empty() { return Ok(()); }
    request_anonymous_tickets(socket, live).await?;
    let now = tokio::time::Instant::now();
    let inflight = live.anon_inflight.keys().copied().chain(live.anon_retries.iter()
        .filter(|(_, (until, _))| *until > now).map(|(id, _)| *id)).collect();
    for envelope in crate::anon_queue::ready(&queue, &inflight, 4usize.saturating_sub(live.anon_inflight.len())) {
        if live.tickets.is_empty() { break; }
        let ready = live.prepared.pop_front();
        // A building socket will wake pump when ready. Do not start another
        // cold connection and throw away the work already in progress.
        if live.require_anonymous && ready.is_none() { break; }
        let (route, prepared) = match ready {
            Some(ready) => (Some(ready.route), Some((ready.socket, ready.supports_passes))),
            None => (pick_onion_route(store, live), None),
        };
        if let (Some(route), Some(ticket)) = (route, live.tickets.pop_front()) {
            if envelope.kind == crate::anon_queue::Kind::Message {
                if let Some(id) = &envelope.logical_id {
                    sink(Event::Delivery { conversation: hex::encode(&envelope.conversation), id: id.clone(), state: "sending".into() });
                }
            }
            let task = spawn_anon_send(route, envelope.client_ref, ticket, envelope.device,
                envelope.ciphertext, outcomes.clone(), envelope.expires_at,
                load_access(store).held.get(&hex::encode(envelope.device)).cloned(), prepared);
            live.anon_inflight.insert(envelope.client_ref, task);
        } else { break; }
    }
    maintain_anonymous_prewarm(store, live);
    Ok(())
}

/// Служебный кадр MLS (Welcome, коммит) едет тем же конвертом, что и сообщения.
async fn send_envelope(socket: &mut Socket, store: &Store, device: &[u8; KEY_LEN], payload: &[u8], conversation: &[u8]) -> Result<()> {
    let mut client_ref = [0u8; ID_LEN];
    client_ref.copy_from_slice(&random_bytes(ID_LEN));
    if anonymous_required(store)? {
        return crate::anon_queue::enqueue(store, crate::anon_queue::Envelope {
            client_ref, device: *device, ciphertext: payload.to_vec(), conversation: conversation.to_vec(),
            logical_id: None, kind: crate::anon_queue::Kind::Essential,
            expires_at: Some(now_millis() + i64::from(DEFAULT_TTL_SEC) * 1000),
        });
    }
    send(socket, proto::send_frame(&client_ref, device, DEFAULT_TTL_SEC, payload)).await
}

async fn send(socket: &mut Socket, frame: Vec<u8>) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(30), socket.send(Message::Binary(frame)))
        .await
        .map_err(|_| CoreError::Transport("Server write timed out; reconnecting".into()))?
        .map_err(|err| CoreError::Transport(err.to_string()))
}

fn fail(sink: &EventSink, code: &str, message: &str) {
    sink(Event::Failed { code: code.to_string(), message: message.to_string() });
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn resume_probe_detects_a_silent_socket_before_the_idle_timeout() {
        use tokio_tungstenite::tungstenite::protocol::Role;
        let store=Store::open(":memory:",b"resume-test").unwrap();
        let credentials=Credentials::generate();store.save_credentials(&credentials).unwrap();
        let mut mls=load_or_create_mls(&store,&credentials).unwrap();
        let (pipe,peer)=tokio::io::duplex(65536);let transport:Box<dyn NetworkStream>=Box::new(pipe);
        let socket=WebSocketStream::from_raw_socket(MaybeTlsStream::Plain(transport),Role::Client,None).await;
        let _silent_server=WebSocketStream::from_raw_socket(peer,Role::Server,None).await;
        let (tx,mut commands)=mpsc::unbounded_channel();tx.send(Command::ProbeConnection).unwrap();
        let (outcomes,mut incoming)=mpsc::unbounded_channel();
        let greeting=Greeting{heartbeat_sec:30,invite_entry:true,ton_entry:false,profiles:false,decor:false,devices:false,sealed_sender:false};
        let handshake=Handshake::Authenticated{key_packages:Some(KEY_PACKAGES_PER_CONNECT),device_id:hex::encode(credentials.device_pub()),admin:false};
        let sink:EventSink=Arc::new(|_|{});let mut live=Live::default();let started=tokio::time::Instant::now();
        let result=pump(socket,&store,&mut mls,&sink,&mut commands,greeting,handshake,&mut live,outcomes,&mut incoming).await;
        assert!(matches!(result,Err(CoreError::Transport(ref message)) if message.contains("Resume probe")));
        assert!(started.elapsed()<Duration::from_secs(10));
    }

    #[tokio::test]
    async fn changed_network_cancels_transient_tasks_and_resets_retry_delays() {
        let mut live=Live::default();let (tx,mut rx)=mpsc::unbounded_channel();
        live.anon_retries.insert([1;ID_LEN],(tokio::time::Instant::now(),1));
        live.route_cooldowns.insert("old.onion".into(),tokio::time::Instant::now());
        live.ticket_request_until=Some(tokio::time::Instant::now());
        let task=tokio::spawn(std::future::pending::<()>());live.anon_inflight.insert([1;ID_LEN],task.abort_handle());
        invalidate_network(&mut live,&mut rx);drop(tx);
        assert!(live.anon_inflight.is_empty());assert!(live.anon_retries.is_empty());
        assert!(live.route_cooldowns.is_empty());assert!(live.ticket_request_until.is_none());
        assert!(task.await.unwrap_err().is_cancelled());
    }

    #[test]
    fn private_avatars_open_for_the_owner_and_existing_legacy_peer_keys() {
        let store=TempStore::new("avatar-keys");
        let credentials=Credentials::generate();store.0.save_credentials(&credentials).unwrap();
        let key=own_profile_key(&store.0).unwrap();
        let sealed=crate::profile::seal(&key,"image/jpeg","cGljdHVyZQ==").unwrap();
        let open=|device:&str|unseal_avatar(&store.0,Some(device),Some(crate::profile::SEALED_MIME.into()),Some(sealed.clone()));
        assert_eq!(open(&hex::encode(credentials.device_pub())),(Some("image/jpeg".into()),Some("cGljdHVyZQ==".into())));
        let peer=hex::encode([73;32]);
        assert_eq!(open(&peer),(None,None));
        let keys=std::collections::BTreeMap::from([(hex::encode(&peer),hex::encode(key))]);
        store.0.save_setting(PEER_PROFILE_KEYS,&serde_json::to_vec(&keys).unwrap()).unwrap();
        assert_eq!(open(&peer).1.as_deref(),Some("cGljdHVyZQ=="));
    }

    #[test]
    fn avatar_key_recipients_follow_avatar_rules_instead_of_message_rules() {
        let store=TempStore::new("avatar-recipients");let peer=hex::encode([74;32]);
        let mut directory=load_directory(&store.0);directory.set(&peer,crate::directory::Standing::Contact,1);
        save_directory(&store.0,&directory).unwrap();
        let mut privacy=crate::privacy::Privacy::default();
        privacy.direct_messages.scope=crate::privacy::Scope::Everyone;
        privacy.profile_avatar.scope=crate::privacy::Scope::Nobody;
        store.0.save_setting(PRIVACY_KEY,&serde_json::to_vec(&privacy).unwrap()).unwrap();
        assert!(avatar_recipients(&store.0).is_empty());
        privacy.profile_avatar.allow.push(peer.clone());
        store.0.save_setting(PRIVACY_KEY,&serde_json::to_vec(&privacy).unwrap()).unwrap();
        assert_eq!(avatar_recipients(&store.0),vec![peer.clone()]);
        directory.set(&peer,crate::directory::Standing::Blocked,2);save_directory(&store.0,&directory).unwrap();
        assert!(avatar_recipients(&store.0).is_empty());
    }

    #[test]
    fn only_one_engine_can_write_a_profile_and_shutdown_releases_it() {
        let directory=std::env::temp_dir().join(format!("valanium-one-writer-{}",std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();let path=directory.join("account.db").to_string_lossy().into_owned();
        let sink:EventSink=Arc::new(|_|{});
        let first=Engine::start(path.clone(),b"test-only".to_vec(),sink.clone()).unwrap();
        assert!(matches!(Engine::start(path.clone(),b"test-only".to_vec(),sink.clone()),Err(CoreError::StoreInUse)));
        first.shutdown();
        Engine::start(path,b"test-only".to_vec(),sink).unwrap().shutdown();
    }

    #[tokio::test]
    async fn a_missing_channel_is_repaired_without_losing_history_or_the_other_group() {
        use tokio_tungstenite::tungstenite::protocol::Role;
        let store=TempStore::new("missing-channel");
        let (credentials,_old,mut bob,device,old_group)=established_pair(&store.0);
        store.0.insert_message(&[91;ID_LEN],&old_group,true,1,b"saved history").unwrap();
        let mut lost=Mls::create(&credentials.device).unwrap();
        let other_key=keys::SecretKey::generate();let other=Mls::create(&other_key).unwrap();
        let other_package=other.key_packages(1).unwrap().remove(0);
        let (healthy_group,_)=lost.start_conversation(&other_package,&other_key.public()).unwrap();
        store.0.set_conversation(&other_key.public(),&healthy_group).unwrap();
        store.0.save_mls(&lost.signer_public(),&lost.snapshot()).unwrap();
        let (pipe,peer)=tokio::io::duplex(65536);let transport:Box<dyn NetworkStream>=Box::new(pipe);
        let mut socket=WebSocketStream::from_raw_socket(MaybeTlsStream::Plain(transport),Role::Client,None).await;
        let mut observer=WebSocketStream::from_raw_socket(peer,Role::Server,None).await;
        let sink:EventSink=Arc::new(|_|{});let mut pending=HashMap::new();
        repair_missing_channels(&mut socket,&store.0,&lost,&sink,&mut pending).await.unwrap();
        assert_eq!(pending.len(),1);
        repair_missing_channels(&mut socket,&store.0,&lost,&sink,&mut pending).await.unwrap();assert_eq!(pending.len(),1);
        let request=observer.next().await.unwrap().unwrap().into_data();assert_eq!(request[0],op::KEYPKG_CLAIM);
        let reference=*pending.keys().next().unwrap();let package=bob.key_packages(1).unwrap().remove(0);
        let mut response=reference.to_vec();response.push(1);response.extend_from_slice(&package);
        on_key_package(&mut socket,&response,&store.0,&mut lost,&sink,&mut pending,&mut Vec::new(),Some(SealedSenderCtx{required:true})).await.unwrap();
        let repaired=store.0.conversation_with(&device).unwrap().unwrap();assert_ne!(repaired,old_group);
        assert_eq!(store.0.thread_of(&repaired).unwrap(),old_group);
        assert_eq!(store.0.list_messages(&old_group,100,None).unwrap()[0].body,b"saved history");
        assert!(lost.inspect(&healthy_group).is_ok());
        let queue=crate::anon_queue::load(&store.0).unwrap();assert_eq!(queue.len(),2);
        assert!(matches!(bob.process(&queue[0].ciphertext).unwrap(),Incoming::Joined{..}));
        assert!(matches!(bob.process(&queue[1].ciphertext).unwrap(),Incoming::Message{..}));
        let reply=bob.encrypt(&repaired,b"phone to desktop after repair",&credentials.device_pub()).unwrap();
        let mut restored=load_or_create_mls(&store.0,&credentials).unwrap();
        assert!(matches!(restored.process(&reply).unwrap(),Incoming::Message{plaintext,..} if plaintext==b"phone to desktop after repair"));
        assert!(restored.inspect(&healthy_group).is_ok());
    }

    #[tokio::test]
    async fn acknowledged_controls_save_the_receive_ratchet_before_restart() {
        use tokio_tungstenite::tungstenite::protocol::Role;
        let store=TempStore::new("durable-control");
        let (credentials,mut alice,mut bob,_device,group)=established_pair(&store.0);
        let original=store.0.load_mls().unwrap().unwrap().1;
        let gift=crate::access::profile_key_gift(&hex::encode([47;32]));
        let ciphertext=bob.encrypt(&group,gift.as_bytes(),&credentials.device_pub()).unwrap();
        let (pipe,_peer)=tokio::io::duplex(65536);let transport:Box<dyn NetworkStream>=Box::new(pipe);
        let mut socket=WebSocketStream::from_raw_socket(MaybeTlsStream::Plain(transport),Role::Client,None).await;
        let sink:EventSink=Arc::new(|_|{});
        let mut envelope=vec![82;ID_LEN];envelope.extend_from_slice(&1u64.to_be_bytes());envelope.extend_from_slice(&ciphertext);
        on_envelope(&mut socket,&envelope,&store.0,&mut alice,&sink,&mut crate::calls::Pending::default()).await.unwrap();
        assert_eq!(load_peer_profile_keys(&store.0).get(&hex::encode(_device)),Some(&hex::encode([47;32])));
        let saved=store.0.load_mls().unwrap().unwrap().1;assert_ne!(saved,original);
        let reply=bob.encrypt(&group,b"after restart",&credentials.device_pub()).unwrap();
        let mut restored=load_or_create_mls(&store.0,&credentials).unwrap();
        assert!(matches!(restored.process(&reply).unwrap(),Incoming::Message{plaintext,..} if plaintext==b"after restart"));
        assert!(store.0.list_messages(&group,100,None).unwrap().is_empty());
    }

    fn usable_test_ticket(expiry: i64) -> [u8; proto::TICKET_LEN] {
        let mut ticket = [0; proto::TICKET_LEN];
        ticket[16..24].copy_from_slice(&(expiry as u64).to_be_bytes());
        ticket
    }

    #[test]
    fn anonymous_ticket_expiry_has_a_send_margin_and_preserves_valid_order() {
        let now = 1_000_000;
        let fresh = usable_test_ticket(now + 300_000);
        let later = usable_test_ticket(now + 400_000);
        let mut live = Live::default();
        live.tickets.extend([usable_test_ticket(now - 1), fresh,
            usable_test_ticket(now + TICKET_SEND_MARGIN_MS), later]);
        prune_anonymous_tickets(&mut live, now);
        assert_eq!(live.tickets.iter().copied().collect::<Vec<_>>(), vec![fresh, later]);
    }

    #[tokio::test]
    async fn anonymous_ticket_grants_are_bounded_and_invalid_grants_do_not_spin() {
        let now = 1_000_000;
        let mut live = Live::default();
        let deadline = tokio::time::Instant::now() + TICKET_REQUEST_TIMEOUT;
        live.ticket_request_until = Some(deadline);
        accept_anonymous_tickets(&mut live, vec![usable_test_ticket(now - 1)], now);
        assert_eq!(live.ticket_request_until, Some(deadline));
        assert!(live.tickets.is_empty());
        accept_anonymous_tickets(&mut live, vec![], now);
        assert_eq!(live.ticket_request_until, Some(deadline));
        accept_anonymous_tickets(&mut live, vec![usable_test_ticket(now + 300_000); 100], now);
        assert_eq!(live.ticket_request_until, None);
        assert_eq!(live.tickets.len(), 32);
    }

    #[tokio::test(start_paused = true)]
    async fn anonymous_ticket_refresh_is_single_flight_and_retries_without_heartbeat() {
        use tokio_tungstenite::tungstenite::protocol::Role;
        let (pipe, peer) = tokio::io::duplex(4096);
        let stream: Box<dyn NetworkStream> = Box::new(pipe);
        let mut socket = WebSocketStream::from_raw_socket(MaybeTlsStream::Plain(stream), Role::Client, None).await;
        let mut observer = WebSocketStream::from_raw_socket(peer, Role::Server, None).await;
        let mut live = Live::default();
        live.tickets.push_back(usable_test_ticket(now_millis() - 1));
        request_anonymous_tickets(&mut socket, &mut live).await.unwrap();
        let deadline = live.ticket_request_until.unwrap();
        request_anonymous_tickets(&mut socket, &mut live).await.unwrap();
        assert_eq!(live.ticket_request_until, Some(deadline));
        assert!(live.tickets.is_empty());
        assert_eq!(next_anonymous_maintenance(&live, true), Some(deadline));
        assert_eq!(proto::split(&observer.next().await.unwrap().unwrap().into_data()).unwrap().0, op::TICKET_REQUEST);
        assert!(tokio::time::timeout(Duration::from_millis(1), observer.next()).await.is_err());
        tokio::time::advance(TICKET_REQUEST_TIMEOUT).await;
        request_anonymous_tickets(&mut socket, &mut live).await.unwrap();
        assert_eq!(proto::split(&observer.next().await.unwrap().unwrap().into_data()).unwrap().0, op::TICKET_REQUEST);
    }

    #[tokio::test]
    async fn anonymous_send_waits_for_existing_prewarm_and_uses_it_without_auth() {
        use tokio_tungstenite::tungstenite::protocol::Role;
        let store = Store::open(":memory:", b"prewarm-test").unwrap();
        let envelope = crate::anon_queue::Envelope { client_ref: [1; ID_LEN], device: [2; KEY_LEN],
            conversation: vec![4], logical_id: None, ciphertext: vec![3; 256],
            kind: crate::anon_queue::Kind::Essential, expires_at: Some(now_millis() + 60_000) };
        crate::anon_queue::enqueue(&store, envelope.clone()).unwrap();
        let mut live = Live::default(); live.require_anonymous = true;
        live.tickets.extend([usable_test_ticket(now_millis()+300_000); 4]);
        // Occupied slots prevent any real network access in this test.
        for _ in 0..ANON_POOL_SIZE { live.prewarming.spawn(std::future::pending()); }
        let (pipe, peer) = tokio::io::duplex(4096);
        let stream: Box<dyn NetworkStream> = Box::new(pipe);
        let mut auth = WebSocketStream::from_raw_socket(MaybeTlsStream::Plain(stream), Role::Client, None).await;
        let mut auth_peer = WebSocketStream::from_raw_socket(peer, Role::Server, None).await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let sink: EventSink = Arc::new(|_| {});
        flush_anonymous(&mut auth, &store, &sink, &mut live, true, &tx).await.unwrap();
        assert!(live.anon_inflight.is_empty(), "do not open another cold socket while prewarming");
        live.anon_retries.insert(envelope.client_ref, (tokio::time::Instant::now(), 1));
        assert_eq!(next_anonymous_maintenance(&live, true), None, "no expired retry timer spin while waiting for a socket");
        let (pipe, peer) = tokio::io::duplex(4096);
        let stream: Box<dyn NetworkStream> = Box::new(pipe);
        let socket = WebSocketStream::from_raw_socket(MaybeTlsStream::Plain(stream), Role::Client, None).await;
        let server = tokio::spawn(async move {
            let mut ws = WebSocketStream::from_raw_socket(peer, Role::Server, None).await;
            let data = ws.next().await.unwrap().unwrap().into_data();
            assert_eq!(proto::split(&data).unwrap().0, op::SEND_ANON);
            let mut ack = vec![1; ID_LEN]; ack.extend_from_slice(&[8; ID_LEN]);
            ws.send(Message::Binary(proto::frame(op::SEND_OK, &ack))).await.unwrap();
        });
        accept_anonymous_prewarm(&mut live, Ok(("test.onion".into(), Ok(PreparedAnonymous {
            route: "test.onion".into(), started: tokio::time::Instant::now(), socket, supports_passes: true }))));
        flush_anonymous(&mut auth, &store, &sink, &mut live, true, &tx).await.unwrap();
        assert_eq!(rx.recv().await.unwrap().result.unwrap(), [8; ID_LEN]);
        assert!(live.prepared.is_empty(), "one socket is consumed by exactly one send");
        assert!(tokio::time::timeout(Duration::from_millis(10), auth_peer.next()).await.is_err());
        server.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn anonymous_prewarm_completion_wakes_pump_before_heartbeat() {
        use tokio_tungstenite::tungstenite::protocol::Role;
        let store = Store::open(":memory:", b"prewarm-wakeup").unwrap();
        let credentials = Credentials::generate(); store.save_credentials(&credentials).unwrap();
        let mut mls = load_or_create_mls(&store, &credentials).unwrap();
        crate::anon_queue::enqueue(&store, crate::anon_queue::Envelope {
            client_ref: [1; ID_LEN], device: [2; KEY_LEN], conversation: vec![4],
            logical_id: None, ciphertext: vec![3; 256], kind: crate::anon_queue::Kind::Essential,
            expires_at: Some(now_millis() + 60_000),
        }).unwrap();
        let (pipe, peer) = tokio::io::duplex(65536);
        let stream: Box<dyn NetworkStream> = Box::new(pipe);
        let socket = WebSocketStream::from_raw_socket(MaybeTlsStream::Plain(stream), Role::Client, None).await;
        let _auth_peer = WebSocketStream::from_raw_socket(peer, Role::Server, None).await;
        let (pipe, peer) = tokio::io::duplex(4096);
        let stream: Box<dyn NetworkStream> = Box::new(pipe);
        let prepared_socket = WebSocketStream::from_raw_socket(MaybeTlsStream::Plain(stream), Role::Client, None).await;
        let server = tokio::spawn(async move {
            let mut ws = WebSocketStream::from_raw_socket(peer, Role::Server, None).await;
            assert_eq!(proto::split(&ws.next().await.unwrap().unwrap().into_data()).unwrap().0, op::SEND_ANON);
            let mut ack = vec![1; ID_LEN]; ack.extend_from_slice(&[8; ID_LEN]);
            ws.send(Message::Binary(proto::frame(op::SEND_OK, &ack))).await.unwrap();
        });
        let mut live = Live::default(); live.require_anonymous = true;
        live.tickets.extend([usable_test_ticket(now_millis()+300_000); 4]);
        // A previous failed preparation can back off replenishment while another
        // preparation completes. Avoid real network access during this test.
        live.prewarm_retry_at = Some(tokio::time::Instant::now()+Duration::from_secs(60));
        live.prewarming.spawn(async move {
            tokio::time::sleep(Duration::from_secs(1)).await;
            ("mock.onion".into(), Ok(PreparedAnonymous { route: "mock.onion".into(),
                started: tokio::time::Instant::now(), socket: prepared_socket, supports_passes: true }))
        });
        let (tx, mut commands) = mpsc::unbounded_channel();
        let (accepted, mut received) = mpsc::unbounded_channel();
        let sink: EventSink = Arc::new(move |event| {
            if matches!(event, Event::Accepted { .. }) { let _ = accepted.send(()); }
        });
        let controller = tokio::spawn(async move {
            received.recv().await.unwrap(); tx.send(Command::Disconnect).unwrap();
        });
        let greeting = Greeting { heartbeat_sec: 30, invite_entry: true, ton_entry: false,
            profiles: false, decor: false, devices: false, sealed_sender: true };
        let handshake = Handshake::Authenticated { key_packages: Some(KEY_PACKAGES_PER_CONNECT),
            device_id: hex::encode(credentials.device_pub()), admin: false };
        let (outcomes, mut incoming) = mpsc::unbounded_channel();
        let result = tokio::time::timeout(Duration::from_secs(10), pump(socket, &store, &mut mls,
            &sink, &mut commands, greeting, handshake, &mut live, outcomes, &mut incoming)).await.unwrap().unwrap();
        assert!(matches!(result, Outcome::Closed));
        controller.await.unwrap(); server.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn anonymous_prewarm_failure_is_backed_off_and_pool_is_bounded() {
        let store = Store::open(":memory:", b"prewarm-failure").unwrap();
        let mut live = Live::default(); live.require_anonymous = true;
        accept_anonymous_prewarm(&mut live, Ok(("failed.onion".into(), Err(CoreError::BadFrame))));
        maintain_anonymous_prewarm(&store, &mut live);
        assert_eq!(live.prewarming.len(), 0);
        assert!(next_anonymous_maintenance(&live, false).is_some());
        tokio::time::advance(Duration::from_secs(2)).await;
        maintain_anonymous_prewarm(&store, &mut live);
        maintain_anonymous_prewarm(&store, &mut live);
        assert_eq!(live.prewarming.len(), ANON_POOL_SIZE);
        live.prewarming.abort_all();
    }

    #[tokio::test(start_paused = true)]
    async fn silent_open_socket_is_reconnected_instead_of_staying_online() {
        use tokio_tungstenite::tungstenite::protocol::Role;
        let store=Store::open(":memory:",b"idle-test").unwrap();
        let credentials=Credentials::generate();store.save_credentials(&credentials).unwrap();
        let mut mls=load_or_create_mls(&store,&credentials).unwrap();
        let (pipe,peer)=tokio::io::duplex(65536);
        let transport: Box<dyn NetworkStream>=Box::new(pipe);
        let socket=WebSocketStream::from_raw_socket(MaybeTlsStream::Plain(transport),Role::Client,None).await;
        let _silent_server=WebSocketStream::from_raw_socket(peer,Role::Server,None).await;
        let (_commands,mut rx)=mpsc::unbounded_channel();
        let (anon_tx,mut anon_rx)=mpsc::unbounded_channel();
        let mut live=Live::default();let sink:EventSink=Arc::new(|_|{});
        let greeting=Greeting{heartbeat_sec:5,invite_entry:true,ton_entry:false,profiles:false,decor:false,devices:false,sealed_sender:false};
        let handshake=Handshake::Authenticated{key_packages:Some(KEY_PACKAGES_PER_CONNECT),device_id:hex::encode(credentials.device_pub()),admin:false};
        let result=tokio::time::timeout(Duration::from_secs(35),pump(socket,&store,&mut mls,&sink,&mut rx,greeting,handshake,&mut live,anon_tx,&mut anon_rx)).await.unwrap();
        assert!(matches!(result,Err(CoreError::Transport(ref reason)) if reason.contains("stopped responding")));
    }

    #[tokio::test]
    async fn stalled_connection_has_a_deadline_and_still_accepts_disconnect() {
        let store=Store::open(":memory:",b"timeout-test").unwrap();
        let sink: EventSink=Arc::new(|_|{});
        let (tx,mut rx)=mpsc::unbounded_channel();
        let outcome=while_connecting_with_timeout(std::future::pending::<Result<()>>(),&store,&sink,&mut rx,Duration::from_millis(20)).await;
        assert!(matches!(outcome,Err(CoreError::Transport(_))));
        tx.send(Command::Disconnect).unwrap();
        let outcome=while_connecting_with_timeout(std::future::pending::<Result<()>>(),&store,&sink,&mut rx,Duration::from_secs(60)).await;
        assert!(matches!(outcome,Ok(None)));
    }

    #[test]
    fn incoming_call_checks_obey_approval_and_explicit_privacy_blocks() {
        let store=Store::open(":memory:",b"calls-test").unwrap();
        let device=[2;KEY_LEN];let peer=hex::encode(device);
        assert!(!permits_incoming_call(&store,&device).unwrap());
        let mut directory=load_directory(&store);
        directory.set(&peer,crate::directory::Standing::Approved,1);
        save_directory(&store,&directory).unwrap();
        assert!(permits_incoming_call(&store,&device).unwrap());
        let mut privacy=load_privacy_checked(&store).unwrap();
        privacy.calls.deny.push(peer.clone());
        store.save_setting(PRIVACY_KEY,&serde_json::to_vec(&privacy).unwrap()).unwrap();
        assert!(!permits_incoming_call(&store,&device).unwrap());
    }

    #[tokio::test]
    async fn retry_deadline_wakes_without_heartbeat_and_ignores_active_sends() {
        let mut live = Live::default();
        let deadline = tokio::time::Instant::now() + Duration::from_millis(20);
        live.anon_retries.insert([1; ID_LEN], (deadline, 1));
        assert_eq!(next_anonymous_retry(&live), Some(deadline));
        tokio::time::timeout(Duration::from_secs(1), wait_anonymous_retry(next_anonymous_retry(&live))).await.unwrap();
        let task = tokio::spawn(std::future::pending::<()>());
        live.anon_inflight.insert([1; ID_LEN], task.abort_handle());
        assert_eq!(next_anonymous_retry(&live), None);
    }

    #[test]
    fn restrictions_wait_for_mutual_pass_exchange_without_changing_settings() {
        let store = TempStore::new("pass-barrier");
        let (_, _, _, device, _) = established_pair(&store.0);
        let peer = hex::encode(device);
        let mut directory = load_directory(&store.0);
        directory.set(&peer, crate::directory::Standing::Contact, now_millis());
        save_directory(&store.0, &directory).unwrap();
        assert!(!passes_ready(&store.0).unwrap());
        let mut restricted = load_access(&store.0);
        restricted.policy = crate::access::Policy::Passes;
        save_access(&store.0, &restricted).unwrap();
        assert!(restrictions_ready(&store.0).unwrap());
        restricted.policy = crate::access::Policy::Everyone;
        save_access(&store.0, &restricted).unwrap();
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = events.clone();
        let sink: EventSink = Arc::new(move |event| captured.lock().unwrap().push(event));
        assert!(handle_local(&Command::PrivacyMaximize, &store.0, &sink));
        assert!(matches!(events.lock().unwrap().last(), Some(Event::Failed { code, .. }) if code == "privacy_pass_exchange_pending"));
        assert!(store.0.load_setting(PRIVACY_KEY).unwrap().is_none());
        let mut access = load_access(&store.0);
        let (pass, hash) = crate::access::new_pass();
        access.remember_grant(&peer, &hash);
        save_access(&store.0, &access).unwrap();
        assert!(!passes_ready(&store.0).unwrap());
        access.hold(&peer, &pass);
        save_access(&store.0, &access).unwrap();
        assert!(passes_ready(&store.0).unwrap());
    }

    #[tokio::test]
    async fn prepared_socket_sends_one_anonymous_envelope_and_never_authenticates() {
        use tokio_tungstenite::tungstenite::protocol::Role;
        let (pipe, peer) = tokio::io::duplex(4096);
        let stream: Box<dyn NetworkStream> = Box::new(pipe);
        let socket = WebSocketStream::from_raw_socket(MaybeTlsStream::Plain(stream), Role::Client, None).await;
        let server = tokio::spawn(async move {
            let mut observer = WebSocketStream::from_raw_socket(peer, Role::Server, None).await;
            let pass = observer.next().await.unwrap().unwrap().into_data();
            assert_eq!(proto::split(&pass).unwrap().0, op::PASS_PRESENT);
            let data = observer.next().await.unwrap().unwrap().into_data();
            assert_eq!(proto::split(&data).unwrap().0, op::SEND_ANON);
            let mut body = vec![1; ID_LEN];
            body.extend_from_slice(&[8; ID_LEN]);
            observer.send(Message::Binary(proto::frame(op::SEND_OK, &body))).await.unwrap();
        });
        let result = send_on_anonymous_socket(socket, true, [1; ID_LEN], [0; proto::TICKET_LEN],
            [2; KEY_LEN], vec![3; 256], Some(hex::encode([4; 32]))).await.unwrap();
        assert_eq!(result, [8; ID_LEN]);
        server.await.unwrap();
    }

    #[test]
    fn retired_account_commands_are_rejected_locally_without_network_or_mutation() {
        let store = TempStore::new("seed-only-retired");
        let credentials = Credentials { identity: keys::SecretKey::generate(), device: keys::SecretKey::generate() };
        store.0.save_credentials(&credentials).unwrap();
        let before = store.0.load_credentials().unwrap().identity_pub();
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let received = events.clone();
        let sink: EventSink = Arc::new(move |event| received.lock().unwrap().push(event));
        let commands = [
            Command::UsernameSet { name: "legacy".into(), discoverable: true },
            Command::UsernameLookup { name: "legacy".into() },
            Command::RecoverySetup { login: "legacy".into(), password: "unused-password".into(), totp: None, code: None },
            Command::RecoverPassword { url: "wss://unused.invalid".into(), login: "legacy".into(), password: "unused-password".into(), code: None },
            Command::TotpSecret { login: "legacy".into() },
        ];
        for command in commands {
            assert!(handle_local(&command, &store.0, &sink));
            assert!(matches!(events.lock().unwrap().pop(), Some(Event::Failed { code, .. }) if code == "seed_only"));
            assert_eq!(store.0.load_credentials().unwrap().identity_pub(), before);
        }
        assert!(handle_local(&Command::RecoveryCode, &store.0, &sink));
        let phrase = match events.lock().unwrap().pop().unwrap() {
            Event::RecoveryCode { words, .. } => words,
            event => panic!("{event:?}"),
        };
        assert_eq!(phrase.split_whitespace().count(), 24);
        let restored = TempStore::new("seed-only-restored");
        recover(&restored.0, &phrase).unwrap();
        assert_eq!(restored.0.load_credentials().unwrap().identity_pub(), before);
        assert!(recover(&restored.0, &phrase).is_err());
    }

    fn established_pair(store: &Store) -> (Credentials, Mls, Mls, [u8; KEY_LEN], Vec<u8>) {
        let credentials = Credentials { identity: keys::SecretKey::generate(), device: keys::SecretKey::generate() };
        store.save_credentials(&credentials).unwrap();
        let bob_key = keys::SecretKey::generate();
        let mut alice = Mls::create(&credentials.device).unwrap();
        let mut bob = Mls::create(&bob_key).unwrap();
        let package = bob.key_packages(1).unwrap().remove(0);
        let (group, welcome) = alice.start_conversation(&package, &bob_key.public()).unwrap();
        bob.process(&welcome).unwrap();
        store.set_conversation(&bob_key.public(), &group).unwrap();
        adopt_thread(store, &bob_key.public(), &group).unwrap();
        store.save_mls(&alice.signer_public(), &alice.snapshot()).unwrap();
        (credentials, alice, bob, bob_key.public(), group)
    }

    #[test]
    fn offline_sends_survive_reopen_keep_fifo_and_decrypt_after_reconnect() {
        let store = TempStore::new("offline-durable");
        let (_, _, mut bob, device, group) = established_pair(&store.0);
        let sink: EventSink = Arc::new(|_| {});
        for id in ["first", "second"] {
            queue_offline(&store.0, &sink, &hex::encode(device),
                &format!("\u{2063}OBS1:{{\"type\":\"text\",\"id\":\"{id}\",\"text\":\"private\"}}"));
        }
        let reopened = Store::open(&store.1, b"pw").unwrap();
        let queue = crate::anon_queue::load(&reopened).unwrap();
        assert_eq!(queue.len(), 2);
        assert_eq!(crate::anon_queue::ready(&queue, &Default::default(), 4).len(), 1);
        let thread = reopened.thread_of(&group).unwrap();
        assert_eq!(reopened.list_messages(&thread, 10, None).unwrap().len(), 2);
        for (item, expected) in queue.iter().zip(["first", "second"]) {
            match bob.process(&item.ciphertext).unwrap() {
                Incoming::Message { plaintext, .. } => assert_eq!(content_json(&String::from_utf8_lossy(crate::padding::strip(&plaintext))).unwrap()["id"], expected),
                other => panic!("{other:?}"),
            }
            crate::anon_queue::acknowledge(&reopened, &item.client_ref).unwrap();
            crate::anon_queue::acknowledge(&reopened, &item.client_ref).unwrap();
            assert_eq!(crate::delivery::state(&reopened, &thread, expected).unwrap().as_deref(), Some("accepted"));
        }
        assert!(crate::anon_queue::load(&reopened).unwrap().is_empty());
    }

    #[test]
    fn full_queue_rolls_back_ratchet_and_own_history_as_well_as_delivery_state() {
        let store = TempStore::new("atomic-send");
        let (_, mut alice, mut bob, device, group) = established_pair(&store.0);
        let filler: Vec<_> = (0..128).map(|n| crate::anon_queue::Envelope {
            client_ref: [n; ID_LEN], device, conversation: group.clone(), logical_id: None,
            ciphertext: vec![0; 256], kind: crate::anon_queue::Kind::Essential, expires_at: None,
        }).collect();
        crate::anon_queue::enqueue_batch(&store.0, filler).unwrap();
        let saved = store.0.load_mls().unwrap().unwrap();
        let waiting = PendingSend { device, body: "\u{2063}OBS1:{\"type\":\"text\",\"id\":\"m1\"}".into(), stored: false };
        assert!(queue_private_message(&store.0, &mut alice, &group, &waiting).is_err());
        assert_eq!(store.0.load_mls().unwrap().unwrap(), saved);
        assert!(store.0.list_messages(&store.0.thread_of(&group).unwrap(), 10, None).unwrap().is_empty());
        crate::anon_queue::forget_thread(&store.0, &group).unwrap();
        queue_private_message(&store.0, &mut alice, &group, &waiting).unwrap();
        let item = crate::anon_queue::load(&store.0).unwrap().remove(0);
        assert!(matches!(bob.process(&item.ciphertext).unwrap(), Incoming::Message { .. }));
    }

    #[tokio::test]
    async fn stalled_network_still_persists_send_and_obeys_disconnect() {
        let store = TempStore::new("stalled-network");
        let (_, _, _, device, _) = established_pair(&store.0);
        let sink: EventSink = Arc::new(|_| {});
        let (tx, mut rx) = mpsc::unbounded_channel();
        tx.send(Command::Send { recipient_device: hex::encode(device), body: "\u{2063}OBS1:{\"type\":\"text\",\"id\":\"offline\"}".into() }).unwrap();
        tx.send(Command::Disconnect).unwrap();
        let result = while_connecting(std::future::pending::<Result<()>>(), &store.0, &sink, &mut rx).await.unwrap();
        assert!(result.is_none());
        assert_eq!(crate::anon_queue::load(&store.0).unwrap().len(), 1);
    }

    #[test]
    fn verification_requires_matching_pair_and_is_durable_and_key_specific() {
        let store = TempStore::new("checked-pair");
        let (credentials, alice, _, device, _) = established_pair(&store.0);
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let copy = events.clone();
        let sink: EventSink = Arc::new(move |event| copy.lock().unwrap().push(event));
        let peer = hex::encode(device);
        let wrong = keys::verification_payload(&credentials.device.public(), &[9; KEY_LEN]);
        handle_local(&Command::VerificationConfirm { peer_device: peer.clone(), proof: wrong }, &store.0, &sink);
        assert!(matches!(events.lock().unwrap().last(), Some(Event::VerificationChecked { verified: false, .. })));
        assert!(matches!(verification(&alice, &store.0, &peer), Event::Verification { verified: false, .. }));
        let proof = keys::verification_payload(&device, &credentials.device.public());
        handle_local(&Command::VerificationConfirm { peer_device: peer.clone(), proof }, &store.0, &sink);
        let reopened = Store::open(&store.1, b"pw").unwrap();
        assert!(matches!(verification(&alice, &reopened, &peer), Event::Verification { verified: true, .. }));
        assert!(reopened.load_setting(&format!("verified:{}", hex::encode([9; KEY_LEN]))).unwrap().is_none());
        let receipt = "\u{2063}VALCTL1:{\"delivered\":[\"m1\"]}";
        assert!(!permits_activity(&reopened, receipt, &device).unwrap());
    }

    #[tokio::test]
    async fn peer_receipt_is_encrypted_optional_and_survives_a_late_server_ack() {
        use tokio_tungstenite::tungstenite::protocol::Role;
        let alice_store = TempStore::new("receipt-alice");
        let (credentials, mut alice, mut bob, device, group) = established_pair(&alice_store.0);
        let bob_store = TempStore::new("receipt-bob");
        bob_store.0.set_conversation(&credentials.device.public(), &group).unwrap();
        adopt_thread(&bob_store.0, &credentials.device.public(), &group).unwrap();
        let mut privacy = crate::privacy::Privacy::default();
        privacy.read_receipts.scope = crate::privacy::Scope::Everyone;
        bob_store.0.save_setting(PRIVACY_KEY, &serde_json::to_vec(&privacy).unwrap()).unwrap();
        let body = "\u{2063}OBS1:{\"type\":\"text\",\"id\":\"m1\",\"text\":\"hello\"}";
        queue_private_message(&alice_store.0, &mut alice, &group,
            &PendingSend { device, body: body.into(), stored: false }).unwrap();
        let message = crate::anon_queue::load(&alice_store.0).unwrap().remove(0);
        let (pipe, _other) = tokio::io::duplex(4096);
        let stream: Box<dyn NetworkStream> = Box::new(pipe);
        let mut socket = WebSocketStream::from_raw_socket(MaybeTlsStream::Plain(stream), Role::Client, None).await;
        let sink: EventSink = Arc::new(|_| {});
        let envelope_body = |id: u8, ciphertext: &[u8]| {
            let mut body = vec![id; ID_LEN]; body.extend_from_slice(&(now_millis() as u64).to_be_bytes()); body.extend_from_slice(ciphertext); body
        };
        on_envelope(&mut socket, &envelope_body(1, &message.ciphertext), &bob_store.0, &mut bob, &sink, &mut crate::calls::Pending::default()).await.unwrap();
        let receipts = crate::anon_queue::load(&bob_store.0).unwrap();
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].kind, crate::anon_queue::Kind::Ephemeral);
        assert_eq!(bob_store.0.list_messages(&bob_store.0.thread_of(&group).unwrap(), 10, None).unwrap().len(), 1);
        on_envelope(&mut socket, &envelope_body(2, &receipts[0].ciphertext), &alice_store.0, &mut alice, &sink, &mut crate::calls::Pending::default()).await.unwrap();
        let thread = alice_store.0.thread_of(&group).unwrap();
        assert_eq!(crate::delivery::state(&alice_store.0, &thread, "m1").unwrap().as_deref(), Some("delivered"));
        crate::anon_queue::acknowledge(&alice_store.0, &message.client_ref).unwrap();
        assert_eq!(crate::delivery::state(&alice_store.0, &thread, "m1").unwrap().as_deref(), Some("delivered"));
    }

    #[test]
    fn maximum_profile_is_durable_offline_and_blocks_receipts_in_core() {
        let store = TempStore::new("maximum-profile");
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let received = events.clone();
        let sink: EventSink = Arc::new(move |event| received.lock().unwrap().push(event));
        let mut previous = crate::privacy::Privacy::default();
        previous.typing.scope = crate::privacy::Scope::Everyone;
        previous.typing.allow.push(hex::encode([2; KEY_LEN]));
        store.0.save_setting(PRIVACY_KEY, &serde_json::to_vec(&previous).unwrap()).unwrap();
        assert!(handle_local(&Command::PrivacyMaximize, &store.0, &sink));
        assert!(matches!(events.lock().unwrap().last(), Some(Event::PrivacyMaximized { .. })));
        let saved = load_privacy_checked(&store.0).unwrap();
        assert_eq!(saved.discoverable.scope, crate::privacy::Scope::Nobody);
        assert!(anonymous_required(&store.0).unwrap());
        assert!(!permits_activity(&store.0, &crate::access::typing_signal(true), &[2; KEY_LEN]).unwrap());
        assert!(!permits_activity(&store.0, "\u{2063}OBS1:{\"v\":1,\"type\":\"read\",\"ids\":[\"m1\"]}", &[2; KEY_LEN]).unwrap());
        assert_eq!(content_json("\u{2063}OBS1:{\"type\":\"text\",\"id\":\"m1\"}").unwrap()["id"], "m1");
        store.0.save_setting(PRIVACY_KEY, b"broken").unwrap();
        assert!(handle_local(&Command::PrivacyMaximize, &store.0, &sink));
        assert_eq!(store.0.load_setting(PRIVACY_KEY).unwrap().unwrap(), b"broken");
        assert!(matches!(events.lock().unwrap().last(), Some(Event::Failed { code, .. }) if code == "privacy_preset"));
    }

    #[tokio::test]
    async fn group_welcome_and_payload_use_queue_without_authenticated_send() {
        use tokio_tungstenite::tungstenite::protocol::Role;
        let store = TempStore::new("private-group");
        let alice_key = keys::SecretKey::generate();
        let bob_key = keys::SecretKey::generate();
        let carol_key = keys::SecretKey::generate();
        let mut alice = Mls::create(&alice_key).unwrap();
        let mut bob = Mls::create(&bob_key).unwrap();
        let mut carol = Mls::create(&carol_key).unwrap();
        let group = alice.create_group().unwrap();
        let (_, welcome) = alice.add_members(&group, &[
            (bob.key_packages(1).unwrap().remove(0), bob_key.public()),
            (carol.key_packages(1).unwrap().remove(0), carol_key.public()),
        ]).unwrap();
        let (pipe, peer) = tokio::io::duplex(4096);
        let transport: Box<dyn NetworkStream> = Box::new(pipe);
        let mut socket = WebSocketStream::from_raw_socket(MaybeTlsStream::Plain(transport), Role::Client, None).await;
        let mut observer = WebSocketStream::from_raw_socket(peer, Role::Server, None).await;
        send_envelope(&mut socket, &store.0, &bob_key.public(), &welcome, &group).await.unwrap();
        send_envelope(&mut socket, &store.0, &carol_key.public(), &welcome, &group).await.unwrap();
        let payload = alice.encrypt_group(&group, b"private group hello").unwrap();
        fan_out(&mut socket, &store.0, &alice, &group, &payload, None, Some("m1")).await.unwrap();
        assert!(tokio::time::timeout(Duration::from_millis(30), observer.next()).await.is_err());
        let queued = crate::anon_queue::load(&store.0).unwrap();
        assert_eq!(queued.len(), 4);
        for (recipient, key) in [(&mut bob, bob_key.public()), (&mut carol, carol_key.public())] {
            let packets: Vec<_> = queued.iter().filter(|item| item.device == key).collect();
            assert!(matches!(recipient.process(&packets[0].ciphertext).unwrap(), Incoming::Joined { .. }));
            match recipient.process(&packets[1].ciphertext).unwrap() {
                Incoming::Message { plaintext, .. } => assert_eq!(plaintext, b"private group hello"),
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn private_message_waits_for_tickets_without_authenticated_send() {
        use tokio_tungstenite::tungstenite::protocol::Role;
        let store = TempStore::new("anonymous-wait");
        let alice_key = crate::keys::SecretKey::generate();
        let bob_key = crate::keys::SecretKey::generate();
        let mut alice = Mls::create(&alice_key).unwrap();
        let bob = Mls::create(&bob_key).unwrap();
        let package = bob.key_packages(1).unwrap().remove(0);
        let (group, _) = alice.start_conversation(&package, &bob_key.public()).unwrap();
        let (pipe, peer) = tokio::io::duplex(4096);
        let transport: Box<dyn NetworkStream> = Box::new(pipe);
        let mut socket = WebSocketStream::from_raw_socket(
            MaybeTlsStream::Plain(transport), Role::Client, None).await;
        let mut observer = WebSocketStream::from_raw_socket(peer, Role::Server, None).await;
        let sink: EventSink = Arc::new(|_| {});
        let mut outbox = Vec::new();
        let (tx, _) = mpsc::unbounded_channel();
        let mut ctx = SealedSenderCtx { required: true };
        encrypt_and_send(&mut socket, &store.0, &mut alice, &sink, &group,
            PendingSend { device: bob_key.public(), body: "hello".into(), stored: false },
            &mut outbox, Some(&mut ctx)).await.unwrap();
        let mut live = Live::default();
        flush_anonymous(&mut socket, &store.0, &sink, &mut live, true, &tx).await.unwrap();
        let frame = observer.next().await.unwrap().unwrap().into_data();
        assert_eq!(proto::split(&frame).unwrap().0, op::TICKET_REQUEST);
        assert!(tokio::time::timeout(Duration::from_millis(30), observer.next()).await.is_err(),
            "authenticated socket must not receive SEND without tickets");
        let queued = crate::anon_queue::load(&store.0).unwrap();
        assert_eq!(queued.len(), 1);
        assert!(!queued[0].ciphertext.windows(5).any(|part| part == b"hello"));
        assert!(outbox.is_empty());
    }

    #[tokio::test]
    async fn anonymous_failure_preserves_durable_envelope() {
        let store = TempStore::new("anonymous-failure");
        let envelope = crate::anon_queue::Envelope {
            client_ref: [1; ID_LEN], device: [2; KEY_LEN], ciphertext: vec![3; 256],
            conversation: vec![4], logical_id: None, kind: crate::anon_queue::Kind::Message, expires_at: None,
        };
        crate::anon_queue::enqueue(&store.0, envelope.clone()).unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        spawn_anon_send("not-a-url".into(), envelope.client_ref, [0; proto::TICKET_LEN],
            envelope.device, envelope.ciphertext.clone(), tx, None, None, None);
        let outcome = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await.unwrap().unwrap();
        assert!(outcome.result.is_err());
        assert_eq!(outcome.client_ref, envelope.client_ref);
        assert_eq!(crate::anon_queue::load(&store.0).unwrap(), vec![envelope]);
    }

    #[tokio::test]
    async fn anonymous_delivery_rejects_a_direct_route_before_network_io() {
        let result = send_anon_once("wss://example.com/ws".into(), [1; ID_LEN],
            [0; proto::TICKET_LEN], [2; KEY_LEN], vec![3], None).await;
        assert!(matches!(result, Err(CoreError::Transport(reason)) if reason.contains("onion")));
    }

    #[tokio::test]
    async fn clearing_a_queue_aborts_its_active_task() {
        let store = TempStore::new("anonymous-cancel");
        let task = tokio::spawn(std::future::pending::<()>());
        let mut live = Live::default();
        live.anon_inflight.insert([1; ID_LEN], task.abort_handle());
        cancel_removed_anonymous(&store.0, &mut live).unwrap();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(live.anon_inflight.is_empty());
    }

    /// Отдельная база на каждый тест: настройки маршрутов лежат именно в ней.
    struct TempStore(Store, String);

    impl TempStore {
        fn new(name: &str) -> Self {
            // Имя уникально для каждого вызова, а не для процесса: тесты идут
            // параллельно, и общий pid однажды сведёт два из них в один файл.
            let unique = hex::encode(crate::crypto::random_bytes(8));
            let path = std::env::temp_dir()
                .join(format!("valanium-{name}-{unique}.db"))
                .to_string_lossy()
                .into_owned();
            Self::wipe(&path);
            let store = Store::open(&path, b"pw").expect("база не открылась");
            Self(store, path)
        }

        fn wipe(path: &str) {
            for suffix in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{path}{suffix}"));
            }
        }
    }

    impl Drop for TempStore {
        fn drop(&mut self) {
            Self::wipe(&self.1);
        }
    }

    /// Пост подтверждается только подписью того, кого мы помним владельцем.
    #[test]
    fn only_the_remembered_owner_confirms_a_post() {
        let owner = crate::keys::SecretKey::generate();
        let owner_hex = hex::encode(owner.public());
        let body = "новость канала";
        let signature = crate::channels::sign(&owner, "chan", "post-1", 1_700_000_000, body);

        let make = || serde_json::json!({
            "id": "post-1",
            "body": body,
            "createdAt": 1_700_000_000_i64,
            "author": owner_hex,
            "signature": signature,
        });

        let mut post = make();
        mark_post(&mut post, "chan", &owner_hex);
        assert_eq!(post["verified"], serde_json::Value::Bool(true));

        // Тот же пост, но владельцем мы помним другого — подтверждать нечем.
        let stranger = hex::encode(crate::keys::SecretKey::generate().public());
        let mut post = make();
        mark_post(&mut post, "chan", &stranger);
        assert_eq!(post["verified"], serde_json::Value::Bool(false));

        // Подменённый текст ломает подпись.
        let mut post = make();
        post["body"] = serde_json::Value::String("подмена".into());
        mark_post(&mut post, "chan", &owner_hex);
        assert_eq!(post["verified"], serde_json::Value::Bool(false));

        // И перенос в другой канал тоже.
        let mut post = make();
        mark_post(&mut post, "other", &owner_hex);
        assert_eq!(post["verified"], serde_json::Value::Bool(false));
    }

    /// Пост без подписи — обычное дело от старого клиента, но не подтверждён.
    #[test]
    fn an_unsigned_post_is_not_confirmed() {
        let mut post = serde_json::json!({
            "id": "post-1",
            "body": "текст",
            "createdAt": 1_700_000_000_i64,
        });
        mark_post(&mut post, "chan", &hex::encode([1u8; 32]));
        assert_eq!(post["verified"], serde_json::Value::Bool(false));
    }

    #[test]
    fn auto_tries_the_fast_route_first_and_tor_last() {
        let store = TempStore::new("auto");
        let routes = routes_for(AUTO_ROUTE_URL, &store.0);

        assert!(routes[0].ends_with("valanium.com/ws"), "первым — обычный relay");
        assert!(
            routes[1..].iter().all(|route| route.contains(".onion")),
            "после relay идут только живые Tor-входы: {routes:?}",
        );
        assert!(
            routes.iter().all(|route| !route.contains("/multihop/")),
            "Auto не должен пробовать недоступный второй relay: {routes:?}",
        );
        assert_eq!(
            routes.len(),
            DIRECT_ROUTES.len() + FALLBACK_ONION.len(),
            "{routes:?}",
        );
    }

    #[test]
    fn the_onion_mode_never_falls_back_to_a_clear_route() {
        let store = TempStore::new("onion");
        let routes = routes_for(ONION_ROUTE_URL, &store.0);

        assert!(!routes.is_empty(), "без входов режим был бы мёртвым");
        assert!(
            routes.iter().all(|route| route.contains(".onion")),
            "в режиме Tor не должно быть открытых маршрутов: {routes:?}",
        );
    }

    /// Ключ владельца для тестов и подпись списка им же.
    fn signing_key() -> (crate::keys::SecretKey, String) {
        let key = crate::keys::SecretKey::generate();
        let public = hex::encode(key.public());
        (key, public)
    }

    #[test]
    fn a_named_host_comes_first_and_the_spares_stay() {
        let store = TempStore::new("named");
        let sink: EventSink = Arc::new(|_| {});
        let (key, public) = signing_key();
        let fresh = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.onion";
        let list = vec![fresh.to_string()];
        let signature = onion::sign(&key, &list, 100);
        remember_onion_hosts(&store.0, &list, &signature, 100, &public, &sink);

        let routes = routes_for(ONION_ROUTE_URL, &store.0);
        assert_eq!(routes[0], format!("ws://{fresh}/ws"), "названный сервером — первым");
        // Запасные остаются: сервер мог назвать узел, до которого именно у
        // этого человека Tor не достучится.
        //
        // Проверяется весь список целиком, а не отдельные адреса по строке:
        // узлы приходят и уходят, и тест, ломающийся на выводе узла из сети,
        // приучают чинить не глядя.
        for spare in FALLBACK_ONION {
            assert!(routes.iter().any(|route| route == spare), "потерян запасной {spare}");
        }

        // Пустой список не стирает известное: иначе старая сборка сервера
        // отобрала бы у человека единственный работающий вход.
        remember_onion_hosts(&store.0, &[], "", 101, &public, &sink);
        // И мусор не запоминается, даже подписанный.
        let junk = vec!["не-адрес".to_string()];
        let junk_sig = onion::sign(&key, &junk, 101);
        remember_onion_hosts(&store.0, &junk, &junk_sig, 101, &public, &sink);
        assert_eq!(routes_for(ONION_ROUTE_URL, &store.0)[0], format!("ws://{fresh}/ws"));
    }

    #[test]
    fn retired_hosts_from_a_previous_release_are_not_retried() {
        let store = TempStore::new("retired-onion");
        let old = RETIRED_ONION_HOSTS.to_vec();
        store
            .0
            .save_setting(ONION_HOSTS_KEY, &serde_json::to_vec(&old).unwrap())
            .unwrap();

        assert_eq!(routes_for(ONION_ROUTE_URL, &store.0), FALLBACK_ONION);
    }

    #[test]
    fn an_unsigned_list_is_ignored() {
        // Ради этого всё и делается: сервер, называющий адреса без подписи,
        // не должен уметь увести режим Tor куда захочет.
        let store = TempStore::new("unsigned");
        let sink: EventSink = Arc::new(|_| {});
        let (_, public) = signing_key();
        let evil = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.onion";

        remember_onion_hosts(&store.0, &[evil.to_string()], "", 1, &public, &sink);
        let routes = routes_for(ONION_ROUTE_URL, &store.0);
        assert!(
            !routes.iter().any(|route| route.contains(evil)),
            "неподписанный адрес попал в маршруты: {routes:?}",
        );
        // И без ключа в сборке — тоже мимо, чем бы список ни был подписан.
        let (key, _) = signing_key();
        let list = vec![evil.to_string()];
        let signature = onion::sign(&key, &list, 1);
        remember_onion_hosts(&store.0, &list, &signature, 1, "", &sink);
        assert!(!routes_for(ONION_ROUTE_URL, &store.0).iter().any(|r| r.contains(evil)));
    }

    #[test]
    fn an_older_signed_list_does_not_replace_a_newer_one() {
        /*
          Подпись не устаревает сама. Без счётчика выпуска сервер отдал бы
          подлинный, но старый список и вернул человека на узел, который мы
          уже вывели из сети — возможно, потому что его изъяли.
        */
        let store = TempStore::new("rollback");
        let sink: EventSink = Arc::new(|_| {});
        let (key, public) = signing_key();
        let new_host = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccc.onion";
        let old_host = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddd.onion";

        let fresh = vec![new_host.to_string()];
        remember_onion_hosts(&store.0, &fresh, &onion::sign(&key, &fresh, 200), 200, &public, &sink);
        assert_eq!(routes_for(ONION_ROUTE_URL, &store.0)[0], format!("ws://{new_host}/ws"));

        let stale = vec![old_host.to_string()];
        remember_onion_hosts(&store.0, &stale, &onion::sign(&key, &stale, 199), 199, &public, &sink);
        assert_eq!(
            routes_for(ONION_ROUTE_URL, &store.0)[0],
            format!("ws://{new_host}/ws"),
            "старый список подменил новый",
        );

        // А более свежий — принимается, иначе список нельзя было бы обновить.
        let newer = vec![old_host.to_string()];
        remember_onion_hosts(&store.0, &newer, &onion::sign(&key, &newer, 201), 201, &public, &sink);
        assert_eq!(routes_for(ONION_ROUTE_URL, &store.0)[0], format!("ws://{old_host}/ws"));
    }

    #[test]
    fn onion_discovery_accepts_only_bare_v3_hosts() {
        let good = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.onion";
        assert!(valid_onion_host(good));
        for bad in [
            "attacker.example/path.onion",
            "attacker.example?x=.onion",
            "real.onion@attacker.example/path.onion",
            "short.onion",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA.onion",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.onion:80",
        ] {
            assert!(!valid_onion_host(bad), "accepted {bad}");
        }
    }

    #[test]
    fn a_single_address_is_used_as_given() {
        let store = TempStore::new("single");
        assert_eq!(
            routes_for("wss://example.org/ws", &store.0),
            vec!["wss://example.org/ws".to_string()],
        );
    }

    /// Регресс: список первых промахов обязан лежать на диске, а не в памяти
    /// одного запуска.
    ///
    /// Раньше он жил в `Live`, которая создаётся заново при каждом старте
    /// приложения. Мобильная ОС убивает процесс когда захочет, и «первый
    /// промах прощается» превращалось в «прощается каждый раз»: конверт
    /// приезжал бы снова и снова, каждый раз выглядел бы новым, и ACK для
    /// него не ушёл бы никогда. Тест не гоняет реальный `on_envelope` (там
    /// нужны живые `Mls`/`Socket`), а бьёт по свойству напрямую: то, что
    /// сохранено, обязано читаться заново независимо от того, есть ли рядом
    /// хоть какое-то состояние в памяти — ровно это и означает «пережить
    /// перезапуск процесса».
    #[test]
    fn failed_envelopes_survive_a_fresh_read() {
        let store = TempStore::new("failed-envelopes");
        assert!(load_failed_envelopes(&store.0).is_empty(), "начинаем с пустого");

        let mut first_run = load_failed_envelopes(&store.0);
        assert!(first_run.insert("aa".repeat(16)), "первый промах — это первая вставка");
        save_failed_envelopes(&store.0, &first_run);

        // "Новый процесс" — здесь буквально свежий вызов load, без всякой
        // связи с переменной first_run: только то, что реально легло на диск.
        let second_run = load_failed_envelopes(&store.0);
        assert!(
            second_run.contains(&"aa".repeat(16)),
            "второй заход обязан увидеть тот же конверт, что и первый",
        );
    }

    /// Сбой MLS не должен выглядеть как обрыв связи.
    ///
    /// Раньше и то и другое было `Transport`, а по нему клиент решает
    /// «переподключиться». Любая беда с состоянием группы поэтому превращалась в
    /// бесконечный цикл: связь цела, клиент рвёт её сам и на новом соединении
    /// спотыкается о то же самое. Разделение — единственное, что это держит.
    #[test]
    fn an_mls_failure_is_not_a_dead_socket() {
        assert!(!is_transport(&CoreError::Mls("mls encrypt".into())));
        assert!(!is_transport(&CoreError::AlreadyProcessed("повтор")));
        assert!(!is_transport(&CoreError::Rejected("dm_not_allowed".into())));
        assert!(is_transport(&CoreError::Transport("сокет закрыт".into())));
    }

    /// Курсор страницы читается только целиком; мусор трактуется как «сначала».
    #[test]
    fn a_broken_cursor_reads_as_the_beginning() {
        assert_eq!(parse_cursor(&Some("1700:12".into())), Some((1700, 12)));
        for bad in ["", "1700", "abc:def", "1700:", ":12"] {
            assert_eq!(parse_cursor(&Some(bad.into())), None, "принят мусор: {bad}");
        }
        assert_eq!(parse_cursor(&None), None);
    }

    #[test]
    fn an_unconfirmed_changed_key_is_blocked_after_reload() {
        let store = TempStore::new("pin-gate");
        let mut pins = crate::pins::Pins::default();
        pins.check("mira", &"aa".repeat(32), 1);
        pins.check("mira", &"bb".repeat(32), 2);
        save_pins(&store.0, &pins).unwrap();

        assert!(matches!(
            ensure_pin_allows(&store.0, &"bb".repeat(32)),
            Err(CoreError::Anomaly(_))
        ));

        let mut restored = load_pins_checked(&store.0).unwrap();
        assert!(restored.accept("mira", &"bb".repeat(32), 3));
        save_pins(&store.0, &restored).unwrap();
        assert!(ensure_pin_allows(&store.0, &"bb".repeat(32)).is_ok());
    }

    #[test]
    fn corrupt_pin_history_fails_closed() {
        let store = TempStore::new("pin-corrupt");
        store.0.save_setting(PINS_KEY, b"not-json").unwrap();
        assert!(matches!(
            ensure_pin_allows(&store.0, &"bb".repeat(32)),
            Err(CoreError::Encoding(_))
        ));
    }

    // --- веер по устройствам --------------------------------------------------

    #[test]
    fn an_unknown_person_is_still_one_recipient() {
        // Пока о личности собеседника ничего не известно, отправка обязана
        // вести себя ровно как прежде: одно устройство, один конверт. Иначе
        // первое же сообщение незнакомцу уходило бы в никуда.
        let temp = TempStore::new("fanout-unknown");
        let device = [1u8; KEY_LEN];
        assert_eq!(recipients(&temp.0, &device), vec![device]);
    }

    #[test]
    fn every_device_of_a_person_gets_a_copy() {
        /*
          То, ради чего весь этап. Сообщение, ушедшее на одно устройство, до
          телефона, лежащего в кармане, не доезжает никогда — и выглядит это не
          как потеря, а как «ему не пришло».
        */
        let temp = TempStore::new("fanout-all");
        let store = &temp.0;
        let identity = [9u8; KEY_LEN];
        let phone = [1u8; KEY_LEN];
        let laptop = [2u8; KEY_LEN];
        store.remember_peer_identity(&identity, &[phone, laptop]).unwrap();
        remember_peer_devices(store, &[phone, laptop]);

        let mut got = recipients(store, &phone);
        got.sort();
        assert_eq!(got, vec![phone, laptop]);
        // С какого бы устройства человек ни написал, отвечаем на все.
        let mut back = recipients(store, &laptop);
        back.sort();
        assert_eq!(back, vec![phone, laptop]);
    }

    #[test]
    fn devices_with_a_conversation_come_first() {
        /*
          Своя копия сообщения ложится в базу первой отправкой. Заведение новой
          беседы — это поход за KeyPackage и ответ отдельным кадром; поставь мы
          такое устройство первым, копия ждала бы ответа сервера, а при отказе
          человек увидел бы, что его сообщение исчезло.
        */
        let temp = TempStore::new("fanout-order");
        let store = &temp.0;
        let identity = [9u8; KEY_LEN];
        let fresh = [1u8; KEY_LEN];
        let known = [2u8; KEY_LEN];
        store.remember_peer_identity(&identity, &[fresh, known]).unwrap();
        remember_peer_devices(store, &[fresh, known]);
        store.set_conversation(&known, b"group").unwrap();

        assert_eq!(recipients(store, &fresh).first(), Some(&known),
            "первым обязано идти устройство с уже заведённой беседой");
    }

    #[test]
    fn a_list_belonging_to_someone_else_is_ignored() {
        /*
          Список лежит под каждым устройством человека, но принадлежать он мог
          другому. Личность сверяется отдельно, а не берётся на веру из ключа
          настройки: иначе чужое устройство получало бы копии переписки — и это
          ровно тот исход, ради недопущения которого объявления и подписываются.
        */
        let temp = TempStore::new("fanout-foreign");
        let store = &temp.0;
        let mine = [1u8; KEY_LEN];
        let stranger = [2u8; KEY_LEN];
        // Список говорит, что это одна личность, а записи о личностях — нет.
        remember_peer_devices(store, &[mine, stranger]);
        store.remember_peer_identity(&[9u8; KEY_LEN], &[mine]).unwrap();
        store.remember_peer_identity(&[8u8; KEY_LEN], &[stranger]).unwrap();

        assert_eq!(recipients(store, &mine), vec![mine], "чужое устройство просочилось");
    }
}
