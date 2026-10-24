//! Встроенный Tor для Valanium: локальный SOCKS5 поверх Arti.
//!
//! # Зачем отдельной программой
//!
//! Клиент и так умеет ходить через SOCKS5 — так он работает с Tor Browser и
//! Orbot. Значит, чтобы Tor «просто был», достаточно поднять свой SOCKS5 рядом
//! и показать на него клиенту: ни одной строки в сетевом слое менять не надо.
//!
//! Отдельная программа, а не часть приложения, — по двум причинам. Tor остаётся
//! вне основной сборки: не везде его можно везти с собой, и не всем он нужен.
//! И падение Tor не роняет мессенджер: это отдельный процесс, его перезапуск
//! ничего не рвёт.
//!
//! # Что здесь важно для приватности
//!
//! **Каталог состояния задаётся снаружи.** Arti по умолчанию кладёт его в
//! профиль пользователя, и там среди прочего живёт `guards.json` — список
//! входных узлов Tor этого человека. Это ровно та метаданная, от которой Onion
//! должен защищать: постоянный след «пользовался Tor, вот через кого». Поэтому
//! путь передаёт вызывающий, и приложение кладёт его к себе, а не в профиль.
//!
//! **Слушаем только петлю.** Привязка к `0.0.0.0` превратила бы это в открытый
//! Tor-прокси для всей сети — подарок соседям по Wi-Fi и повод для жалоб.
//!
//! **Только .onion.** Пускать наружу произвольный трафик мы не обязаны, а
//! всякий, кто нашёл порт, тут же начал бы. Снимается флагом `--allow-clearnet`,
//! если однажды понадобится.
//!
//! # Как приложение с этим общается
//!
//! Запускает процесс, читает stdout. Строка `READY <адрес>` означает, что цепь
//! построена и можно подключаться. До неё соединения принимать бессмысленно:
//! первый в жизни запуск строит цепь около минуты, и это надо показать
//! человеку честно, а не крутилкой.
//!
//! ```text
//! valanium-onionize --socks 127.0.0.1:9150 --data C:\...\valanium\tor
//! ```

use std::net::SocketAddr;
use std::path::PathBuf;

use arti_client::config::TorClientConfigBuilder;
use arti_client::TorClient;
use tor_rtcompat::PreferredRuntime;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
#[path = "../../valanium-core/src/tor_path.rs"]
mod tor_path;

/// По умолчанию порт выбирает система: `0` — «дай любой свободный».
///
/// Фиксированный номер тут вреден. 9050 занимает системный Tor, 9150 — Tor
/// Browser, и на машине, где они уже стоят, мы бы либо не поднялись, либо
/// увели чужой трафик. Проверено на живой машине: 9150 и 9151 оказались заняты
/// сразу. Настоящий адрес печатается строкой READY, и приложение берёт его
/// оттуда, а не угадывает.
const DEFAULT_SOCKS: &str = "127.0.0.1:0";

struct Options {
    socks: SocketAddr,
    data: PathBuf,
    allow_clearnet: bool,
}

fn usage() -> ! {
    eprintln!(
        "usage: valanium-onionize --data <каталог> [--socks {DEFAULT_SOCKS}] [--allow-clearnet]",
    );
    std::process::exit(2);
}

fn parse_args() -> Options {
    let mut socks = DEFAULT_SOCKS.to_owned();
    let mut data: Option<PathBuf> = None;
    let mut allow_clearnet = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--socks" => socks = args.next().unwrap_or_else(|| usage()),
            "--data" => data = Some(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
            "--allow-clearnet" => allow_clearnet = true,
            _ => usage(),
        }
    }

    let Some(data) = data else { usage() };
    let Ok(socks) = socks.parse::<SocketAddr>() else {
        eprintln!("--socks должен быть адресом вида 127.0.0.1:9150");
        std::process::exit(2);
    };
    // Слушать не на петле — значит раздавать Tor всей сети. Отказываемся сразу,
    // а не выясняем это по чужому трафику.
    if !socks.ip().is_loopback() {
        eprintln!("--socks обязан быть на петле: {socks} доступен извне");
        std::process::exit(2);
    }

    Options { socks, data, allow_clearnet }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_args();

    // Состояние и кэш — туда, куда сказало приложение. См. шапку про guards.json.
    // Разными каталогами: кэш каталога Tor можно потерять без последствий, а
    // состояние — это и есть входные узлы, и обращаться с ними надо иначе.
    let config = TorClientConfigBuilder::from_directories(
        options.data.join("state"),
        options.data.join("cache"),
    )
    .build()?;

    eprintln!("строю цепь Tor (первый запуск занимает около минуты)…");
    let client = TorClient::create_bootstrapped(config).await?;

    let listener = TcpListener::bind(options.socks).await?;
    // Спрашиваем у сокета, а не печатаем то, что просили: при порте 0 система
    // выбрала свой, и знать его приложение может только отсюда.
    let bound = listener.local_addr()?;
    // Эту строку ждёт приложение. Печатаем в stdout и сбрасываем буфер: без
    // сброса она может застрять и приложение решит, что мы не поднялись.
    println!("READY {bound}");
    use std::io::Write;
    std::io::stdout().flush().ok();

    loop {
        let (socket, _) = listener.accept().await?;
        let client = client.isolated_client();
        let allow_clearnet = options.allow_clearnet;
        tokio::spawn(async move {
            if let Err(err) = serve(socket, client, allow_clearnet).await {
                // Обычный разрыв — не отказ. Клиент, закрывший соединение
                // первым, оставляет за собой именно такую ошибку, и называть
                // её отказом значит приучить не читать этот журнал вовсе.
                let text = err.to_string();
                if text.contains("without END cell") || text.contains("early eof") {
                    eprintln!("соединение закрыто клиентом");
                } else {
                    eprintln!("соединение отклонено: {text}");
                }
            }
        });
    }
}

/// Минимальный SOCKS5: только CONNECT и только без аутентификации.
///
/// Реализуется вручную намеренно: нам нужен ровно один сценарий, а тащить
/// целую библиотеку ради него — больше кода и больше зависимостей, чем сам
/// разбор. Сервер слушает петлю, поэтому разбирает он только то, что сам же
/// клиент и прислал.
async fn serve(
    mut socket: TcpStream,
    client: TorClient<PreferredRuntime>,
    allow_clearnet: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Приветствие: версия, число методов, сами методы.
    let mut head = [0u8; 2];
    socket.read_exact(&mut head).await?;
    if head[0] != 0x05 {
        return Err("не SOCKS5".into());
    }
    let mut methods = vec![0u8; head[1] as usize];
    socket.read_exact(&mut methods).await?;
    // Отвечаем «без аутентификации»: слушаем петлю, проверять некого.
    socket.write_all(&[0x05, 0x00]).await?;

    // Запрос: версия, команда, резерв, тип адреса.
    let mut request = [0u8; 4];
    socket.read_exact(&mut request).await?;
    if request[1] != 0x01 {
        // 0x07 — команда не поддерживается.
        socket.write_all(&[0x05, 0x07, 0x00, 0x01, 0, 0, 0, 0, 0, 0]).await?;
        return Err("поддерживается только CONNECT".into());
    }

    let host = match request[3] {
        0x01 => {
            let mut raw = [0u8; 4];
            socket.read_exact(&mut raw).await?;
            std::net::Ipv4Addr::from(raw).to_string()
        }
        0x03 => {
            let mut len = [0u8; 1];
            socket.read_exact(&mut len).await?;
            let mut raw = vec![0u8; len[0] as usize];
            socket.read_exact(&mut raw).await?;
            String::from_utf8(raw)?
        }
        0x04 => {
            let mut raw = [0u8; 16];
            socket.read_exact(&mut raw).await?;
            std::net::Ipv6Addr::from(raw).to_string()
        }
        _ => {
            socket.write_all(&[0x05, 0x08, 0x00, 0x01, 0, 0, 0, 0, 0, 0]).await?;
            return Err("неизвестный тип адреса".into());
        }
    };

    let mut port = [0u8; 2];
    socket.read_exact(&mut port).await?;
    let port = u16::from_be_bytes(port);

    // Только скрытые сервисы, если не разрешено обратное. Иначе всякий, кто
    // нашёл порт, получил бы бесплатный выход в интернет через Tor от нашего
    // имени — и жалобы прилетели бы нам.
    if !allow_clearnet && !host.ends_with(".onion") {
        // 0x02 — соединение запрещено правилами.
        socket.write_all(&[0x05, 0x02, 0x00, 0x01, 0, 0, 0, 0, 0, 0]).await?;
        return Err(format!("не onion-адрес: {host}").into());
    }

    let mut tunnel = match client.connect((host.as_str(), port)).await {
        Ok(stream) => stream,
        Err(err) => {
            // 0x04 — хост недоступен.
            socket.write_all(&[0x05, 0x04, 0x00, 0x01, 0, 0, 0, 0, 0, 0]).await?;
            return Err(format!("{host}: {err}").into());
        }
    };

    // Успех. Адрес привязки не сообщаем — клиенту он не нужен, а выдумывать
    // настоящий незачем.
    let circuit = tor_path::observe(&tunnel, &host);
    println!("CIRCUIT {}", tor_path::snapshot());
    socket.write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0]).await?;

    let result = tokio::io::copy_bidirectional(&mut socket, &mut tunnel).await;
    drop(circuit);
    println!("CIRCUIT {}", tor_path::snapshot());
    result?;
    Ok(())
}
