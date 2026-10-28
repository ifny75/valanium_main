//! Read-only transport check: opens WebSocket, validates HELLO, then closes.
//! No account is created or opened. Pass URL and optional local SOCKS address.
use futures_util::StreamExt;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};
use valanium_core::{edge, proto};

async fn hello<S: AsyncRead + AsyncWrite + Unpin>(mut socket: WebSocketStream<S>) -> Result<(), Box<dyn std::error::Error>> {
    let message = socket.next().await.ok_or("no HELLO")??;
    let Message::Binary(bytes) = message else { return Err("HELLO must be binary".into()) };
    let (opcode, body) = proto::split(&bytes)?;
    if opcode != proto::op::HELLO { return Err("unexpected opcode".into()); }
    let hello: proto::Hello = proto::parse_json(body)?;
    if hello.v != 1 || hello.nonce.is_empty() || hello.heartbeat_sec == 0 { return Err("invalid HELLO".into()); }
    socket.close(None).await?;
    println!("WebSocket upgrade and protocol HELLO verified");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let url = args.get(1).ok_or("URL required")?;
    tokio::time::timeout(std::time::Duration::from_secs(50), async {
        let request = edge::ws_request(url)?;
        if let Some(proxy) = args.get(2) {
            let uri = request.uri();
            let host = uri.host().ok_or("host missing")?;
            if uri.scheme_str() != Some("ws") || !host.ends_with(".onion") {
                return Err("SOCKS probe is restricted to ws onion endpoints".into());
            }
            let stream = tokio_socks::tcp::Socks5Stream::connect(proxy.as_str(), (host, uri.port_u16().unwrap_or(80))).await?;
            let (socket, _) = tokio_tungstenite::client_async(request, stream).await?;
            hello(socket).await
        } else {
            let (socket, _) = tokio_tungstenite::connect_async(request).await?;
            hello(socket).await
        }
    }).await?
}
