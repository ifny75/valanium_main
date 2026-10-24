//! Optional "mask as HTTPS" relay: wraps WireGuard's own UDP traffic inside
//! a genuine TLS connection using a fake SNI (`www.microsoft.com`) so
//! passive DPI — the kind that fingerprints VPN traffic from the visible
//! ClientHello rather than doing a real man-in-the-middle — sees an
//! ordinary HTTPS connection instead of a WireGuard handshake. The server
//! presents a self-signed cert (vpn_node/tls_mask_relay.py); we never
//! validate it, same as the server never validates who's dialing in over
//! TLS — WireGuard's own handshake (inside the tunnel) is what actually
//! authenticates the peer, TLS here exists only for its outer appearance.
//!
//! WireGuard itself never knows this exists: we tell it to connect to
//! 127.0.0.1:<local-port> instead of the real server, and this relay pumps
//! datagrams between that local UDP socket and one persistent TLS
//! connection to the real server's TCP:443 (see start_relay's caller in
//! main.rs, which rewrites the parsed config's Endpoint accordingly).

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UdpSocket;
use tokio::task::JoinHandle;
use tokio_rustls::TlsConnector;

const FAKE_SNI: &str = "www.microsoft.com";

/// Release builds carry `windows_subsystem = "windows"` (see main.rs), so
/// there's never a console for eprintln! to reach — a file next to the exe
/// is the only reliable way to see what actually happened.
fn log_error(msg: &str) {
    use std::io::Write;
    if let Ok(path) = std::env::current_exe().map(|p| p.with_file_name("tls_mask.log")) {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{msg}");
        }
    }
}

#[derive(Debug)]
struct AcceptAnyCert;

impl ServerCertVerifier for AcceptAnyCert {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::ECDSA_NISTP521_SHA512,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::ED25519,
        ]
    }
}

/// Spawns the relay and returns the local address WireGuard should be told
/// to use as its peer Endpoint, plus a handle to stop it on disconnect.
pub async fn start_relay(real_server: SocketAddr) -> anyhow::Result<(SocketAddr, JoinHandle<()>)> {
    let udp = UdpSocket::bind("127.0.0.1:0").await?;
    let local_addr = udp.local_addr()?;

    let handle = tokio::spawn(async move {
        if let Err(e) = run_relay(udp, real_server).await {
            log_error(&format!("relay stopped: {e}"));
        }
    });

    Ok((local_addr, handle))
}

async fn run_relay(udp: UdpSocket, real_server: SocketAddr) -> anyhow::Result<()> {
    let config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAnyCert))
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(config));

    let tcp = tokio::net::TcpStream::connect(real_server).await?;
    let server_name = ServerName::try_from(FAKE_SNI)?.to_owned();
    let tls = connector.connect(server_name, tcp).await?;
    let (mut tls_read, mut tls_write) = tokio::io::split(tls);

    // WireGuard only ever dials this one relay once connected, so there's
    // no need to track multiple sources — just remember the last address
    // its local socket sent from and echo replies back there.
    let mut wg_peer_addr: Option<SocketAddr> = None;
    let mut buf = [0u8; 65535];

    loop {
        tokio::select! {
            recv = udp.recv_from(&mut buf) => {
                let (n, addr) = recv?;
                wg_peer_addr = Some(addr);
                tls_write.write_all(&(n as u16).to_be_bytes()).await?;
                tls_write.write_all(&buf[..n]).await?;
            }
            frame = read_frame(&mut tls_read) => {
                let payload = frame?;
                if let Some(addr) = wg_peer_addr {
                    udp.send_to(&payload, addr).await?;
                }
            }
        }
    }
}

async fn read_frame<T: tokio::io::AsyncRead + Unpin>(stream: &mut T) -> anyhow::Result<Vec<u8>> {
    let mut header = [0u8; 2];
    stream.read_exact(&mut header).await?;
    let len = u16::from_be_bytes(header) as usize;
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    Ok(buf)
}
