//! Real WireGuard tunnel via the embeddable `wireguard.dll` (wireguard-nt) —
//! the same driver/API the official WireGuard app itself uses, loaded
//! directly rather than shelling out to a separate `wireguard.exe`.
//!
//! wireguard.dll only owns the crypto/device layer (create the virtual
//! adapter, push keys/peers, bring it up). It does **not** touch IP
//! addresses, routes, or DNS — same as upstream, that's done here with
//! `netsh`/`route`, exactly like wireguard-windows's own Go tunnel service
//! does internally.
//!
//! Struct layouts below mirror wireguard.h byte-for-byte (field order,
//! types, and the `align(8)`/`align(4)` markers), which is what lets Rust's
//! `repr(C)` layout algorithm reproduce the same padding as the C compiler
//! without needing bindgen.

use base64::{engine::general_purpose::STANDARD, Engine};
use libloading::{Library, Symbol};
use std::ffi::c_void;
use std::mem::size_of;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::str::FromStr;

// Suppresses the console window `netsh`/`route`/`powershell` would otherwise
// briefly flash open with, since this GUI app has no console of its own for
// them to attach to (https://learn.microsoft.com/windows/win32/procthread/process-creation-flags).
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const WIREGUARD_INTERFACE_HAS_PRIVATE_KEY: u32 = 1 << 1;
const WIREGUARD_INTERFACE_REPLACE_PEERS: u32 = 1 << 3;
const WIREGUARD_PEER_HAS_PUBLIC_KEY: u32 = 1 << 0;
const WIREGUARD_PEER_HAS_PERSISTENT_KEEPALIVE: u32 = 1 << 2;
const WIREGUARD_PEER_HAS_ENDPOINT: u32 = 1 << 3;
const WIREGUARD_PEER_REPLACE_ALLOWED_IPS: u32 = 1 << 5;
const ADAPTER_STATE_UP: u32 = 1;

const TUNNEL_NAME: &str = "ValaniumVPN";
const AF_INET: u16 = 2;

#[repr(C, align(4))]
#[derive(Clone, Copy)]
struct SockaddrInet {
    // Sized/aligned to match SOCKADDR_INET (sockaddr_in6 is the larger
    // union member, 28 bytes); we only ever populate the sockaddr_in
    // (IPv4) layout at the front of it.
    bytes: [u8; 28],
}

impl SockaddrInet {
    fn ipv4(ip: Ipv4Addr, port: u16) -> Self {
        let mut bytes = [0u8; 28];
        bytes[0..2].copy_from_slice(&AF_INET.to_ne_bytes());
        bytes[2..4].copy_from_slice(&port.to_be_bytes());
        bytes[4..8].copy_from_slice(&ip.octets());
        Self { bytes }
    }
}

#[repr(C, align(8))]
#[derive(Clone, Copy)]
struct WgAllowedIp {
    address: [u8; 16], // union IN_ADDR(4) / IN6_ADDR(16) — only the first 4 used here
    address_family: u16,
    cidr: u8,
}

impl WgAllowedIp {
    fn ipv4(ip: Ipv4Addr, cidr: u8) -> Self {
        let mut address = [0u8; 16];
        address[0..4].copy_from_slice(&ip.octets());
        Self {
            address,
            address_family: AF_INET,
            cidr,
        }
    }
}

#[repr(C, align(8))]
#[derive(Clone, Copy)]
struct WgPeer {
    flags: u32,
    reserved: u32,
    public_key: [u8; 32],
    preshared_key: [u8; 32],
    persistent_keepalive: u16,
    endpoint: SockaddrInet,
    tx_bytes: u64,
    rx_bytes: u64,
    last_handshake: u64,
    allowed_ips_count: u32,
}

#[repr(C, align(8))]
#[derive(Clone, Copy)]
struct WgInterfaceHeader {
    flags: u32,
    listen_port: u16,
    private_key: [u8; 32],
    public_key: [u8; 32],
    peers_count: u32,
}

type CreateAdapterFn =
    unsafe extern "system" fn(*const u16, *const u16, *const c_void) -> *mut c_void;
type CloseAdapterFn = unsafe extern "system" fn(*mut c_void);
type SetAdapterStateFn = unsafe extern "system" fn(*mut c_void, u32) -> i32;
type SetConfigurationFn = unsafe extern "system" fn(*mut c_void, *const u8, u32) -> i32;

/// Loaded lazily (not at process start) so the whole app doesn't fail to
/// launch on a machine without wireguard.dll — only Connect needs it.
struct WgDll {
    _lib: Library, // kept alive for as long as the function pointers are used
    create_adapter: CreateAdapterFn,
    close_adapter: CloseAdapterFn,
    set_adapter_state: SetAdapterStateFn,
    set_configuration: SetConfigurationFn,
}

impl WgDll {
    fn load() -> Result<Self, String> {
        let path = Self::find_dll()?;
        unsafe {
            let lib = Library::new(&path)
                .map_err(|e| format!("failed to load {}: {e}", path.display()))?;
            let create_adapter: Symbol<CreateAdapterFn> = lib
                .get(b"WireGuardCreateAdapter\0")
                .map_err(|e| e.to_string())?;
            let close_adapter: Symbol<CloseAdapterFn> =
                lib.get(b"WireGuardCloseAdapter\0").map_err(|e| e.to_string())?;
            let set_adapter_state: Symbol<SetAdapterStateFn> = lib
                .get(b"WireGuardSetAdapterState\0")
                .map_err(|e| e.to_string())?;
            let set_configuration: Symbol<SetConfigurationFn> = lib
                .get(b"WireGuardSetConfiguration\0")
                .map_err(|e| e.to_string())?;
            // Symbols borrow from `lib`; transmute the lifetime away since we
            // store `lib` alongside them in the same struct (it outlives them).
            let create_adapter = std::mem::transmute::<_, CreateAdapterFn>(*create_adapter);
            let close_adapter = std::mem::transmute::<_, CloseAdapterFn>(*close_adapter);
            let set_adapter_state =
                std::mem::transmute::<_, SetAdapterStateFn>(*set_adapter_state);
            let set_configuration =
                std::mem::transmute::<_, SetConfigurationFn>(*set_configuration);
            Ok(Self {
                _lib: lib,
                create_adapter,
                close_adapter,
                set_adapter_state,
                set_configuration,
            })
        }
    }

    fn find_dll() -> Result<PathBuf, String> {
        let exe_dir = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .parent()
            .ok_or("exe has no parent dir")?
            .to_path_buf();
        for candidate in [
            exe_dir.join("wireguard.dll"),
            exe_dir.join("resources").join("wireguard.dll"),
        ] {
            if candidate.exists() {
                return Ok(candidate);
            }
        }
        Err(format!(
            "wireguard.dll not found next to {}",
            exe_dir.display()
        ))
    }
}

fn widestring(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Parsed shape of the [Interface]/[Peer] wg_config text main_server hands
/// back — enough to build the wireguard.dll binary config, nothing more.
struct ParsedConfig {
    private_key: [u8; 32],
    address: Ipv4Addr,
    address_cidr: u8,
    dns: Vec<String>,
    peer_public_key: [u8; 32],
    endpoint: (Ipv4Addr, u16),
    allowed_ips: Vec<(Ipv4Addr, u8)>,
    persistent_keepalive: u16,
}

fn decode_key(b64: &str) -> Result<[u8; 32], String> {
    let bytes = STANDARD
        .decode(b64.trim())
        .map_err(|e| format!("bad base64 key: {e}"))?;
    bytes
        .try_into()
        .map_err(|_| "WireGuard key must decode to exactly 32 bytes".to_string())
}

fn parse_config(text: &str) -> Result<ParsedConfig, String> {
    let mut section = "";
    let mut private_key = None;
    let mut address = None;
    let mut dns = Vec::new();
    let mut peer_public_key = None;
    let mut endpoint = None;
    let mut allowed_ips = Vec::new();
    let mut keepalive = 0u16;

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            section = if line.eq_ignore_ascii_case("[Interface]") {
                "interface"
            } else if line.eq_ignore_ascii_case("[Peer]") {
                "peer"
            } else {
                ""
            };
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();

        match (section, key.as_str()) {
            ("interface", "privatekey") => private_key = Some(decode_key(value)?),
            ("interface", "address") => {
                let (ip, cidr) = parse_cidr(value)?;
                address = Some((ip, cidr));
            }
            ("interface", "dns") => {
                dns = value.split(',').map(|s| s.trim().to_string()).collect();
            }
            ("peer", "publickey") => peer_public_key = Some(decode_key(value)?),
            ("peer", "endpoint") => {
                let (host, port) = value
                    .rsplit_once(':')
                    .ok_or("Peer Endpoint missing :port")?;
                let ip = Ipv4Addr::from_str(host)
                    .map_err(|_| format!("Endpoint host '{host}' isn't an IPv4 address (hostnames aren't supported yet)"))?;
                let port: u16 = port.parse().map_err(|_| "bad Endpoint port")?;
                endpoint = Some((ip, port));
            }
            ("peer", "allowedips") => {
                for part in value.split(',') {
                    let part = part.trim();
                    if part.is_empty() || part.contains("::") {
                        continue; // IPv6 not supported by this MVP client
                    }
                    allowed_ips.push(parse_cidr(part)?);
                }
            }
            ("peer", "persistentkeepalive") => {
                keepalive = value.parse().map_err(|_| "bad PersistentKeepalive")?;
            }
            _ => {}
        }
    }

    let (address, address_cidr) = address.ok_or("config missing [Interface] Address")?;
    Ok(ParsedConfig {
        private_key: private_key.ok_or("config missing [Interface] PrivateKey")?,
        address,
        address_cidr,
        dns,
        peer_public_key: peer_public_key.ok_or("config missing [Peer] PublicKey")?,
        endpoint: endpoint.ok_or("config missing [Peer] Endpoint")?,
        allowed_ips,
        persistent_keepalive: keepalive,
    })
}

/// Pulls just the real server Endpoint out of a wg_config, for main.rs to
/// hand to tls_mask::start_relay before it ever calls WgConnection::connect
/// (which is the point at which the endpoint gets overridden to loopback).
pub fn parse_endpoint(config_text: &str) -> Option<std::net::SocketAddr> {
    let cfg = parse_config(config_text).ok()?;
    let (ip, port) = cfg.endpoint;
    Some(std::net::SocketAddr::new(std::net::IpAddr::V4(ip), port))
}

fn cidr_to_mask(cidr: u8) -> Ipv4Addr {
    let bits: u32 = if cidr == 0 { 0 } else { u32::MAX << (32 - cidr as u32) };
    Ipv4Addr::from(bits)
}

fn parse_cidr(s: &str) -> Result<(Ipv4Addr, u8), String> {
    let (ip, cidr) = s.split_once('/').unwrap_or((s, "32"));
    let ip = Ipv4Addr::from_str(ip.trim()).map_err(|_| format!("bad IPv4 address '{ip}'"))?;
    let cidr: u8 = cidr.trim().parse().map_err(|_| "bad CIDR")?;
    Ok((ip, cidr))
}

fn build_config_blob(cfg: &ParsedConfig) -> Vec<u8> {
    let mut buf = Vec::new();

    let iface = WgInterfaceHeader {
        flags: WIREGUARD_INTERFACE_HAS_PRIVATE_KEY | WIREGUARD_INTERFACE_REPLACE_PEERS,
        listen_port: 0,
        private_key: cfg.private_key,
        public_key: [0u8; 32],
        peers_count: 1,
    };
    buf.extend_from_slice(struct_bytes(&iface));

    let (ep_ip, ep_port) = cfg.endpoint;
    let peer = WgPeer {
        flags: WIREGUARD_PEER_HAS_PUBLIC_KEY
            | WIREGUARD_PEER_HAS_ENDPOINT
            | WIREGUARD_PEER_REPLACE_ALLOWED_IPS
            | if cfg.persistent_keepalive > 0 {
                WIREGUARD_PEER_HAS_PERSISTENT_KEEPALIVE
            } else {
                0
            },
        reserved: 0,
        public_key: cfg.peer_public_key,
        preshared_key: [0u8; 32],
        persistent_keepalive: cfg.persistent_keepalive,
        endpoint: SockaddrInet::ipv4(ep_ip, ep_port),
        tx_bytes: 0,
        rx_bytes: 0,
        last_handshake: 0,
        allowed_ips_count: cfg.allowed_ips.len() as u32,
    };
    buf.extend_from_slice(struct_bytes(&peer));

    for (ip, cidr) in &cfg.allowed_ips {
        let allowed = WgAllowedIp::ipv4(*ip, *cidr);
        buf.extend_from_slice(struct_bytes(&allowed));
    }

    buf
}

fn struct_bytes<T>(value: &T) -> &[u8] {
    unsafe { std::slice::from_raw_parts((value as *const T) as *const u8, size_of::<T>()) }
}

fn run(cmd: &str, args: &[&str]) -> Result<(), String> {
    let output = Command::new(cmd)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("{cmd} failed to launch: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "{cmd} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

/// Returns (gateway IP, interface index) of the current lowest-metric
/// default route — i.e. the real physical NIC's route, found *before* the
/// tunnel adapter existed to compete with it.
fn default_route_info() -> Result<(String, u32), String> {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "(Get-NetRoute -DestinationPrefix '0.0.0.0/0' | Sort-Object RouteMetric | Select-Object -First 1 | ForEach-Object { \"$($_.NextHop)|$($_.ifIndex)\" })",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| e.to_string())?;
    let line = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let (gw, idx) = line
        .split_once('|')
        .ok_or("couldn't determine the current default gateway")?;
    let idx: u32 = idx.trim().parse().map_err(|_| "bad default route ifIndex")?;
    if gw.is_empty() {
        Err("couldn't determine the current default gateway".to_string())
    } else {
        Ok((gw.to_string(), idx))
    }
}

/// The current default route's physical adapter's own IPv4 address — used to
/// scope the DNS-leak-blocking firewall rules to just that interface.
fn physical_adapter_ip(ifindex: u32) -> Result<String, String> {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!(
                "(Get-NetIPAddress -InterfaceIndex {ifindex} -AddressFamily IPv4 | Select-Object -First 1 -ExpandProperty IPAddress)"
            ),
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| e.to_string())?;
    let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if ip.is_empty() {
        Err(format!("couldn't determine IPv4 address for ifIndex {ifindex}"))
    } else {
        Ok(ip)
    }
}

/// The tunnel adapter's own interface index — needed because plain `route
/// add <dest> mask <mask> <local-tunnel-ip>` lets Windows *guess* the
/// outgoing interface from the gateway address, and for a /32 point-to-point
/// address (no local subnet to match against) it guesses wrong, silently
/// binding the route to the physical NIC instead of the tunnel. Passing an
/// explicit `if <index>` removes the guesswork.
fn adapter_index(name: &str) -> Result<u32, String> {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!("(Get-NetAdapter -Name '{name}').ifIndex"),
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| e.to_string())?;
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .map_err(|_| format!("couldn't determine ifIndex for adapter '{name}'"))
}

pub struct WgConnection {
    adapter: Option<*mut c_void>,
    dll: Option<WgDll>,
    excluded_server_ip: Option<String>,
    dns_leak_block_active: bool,
}

// The adapter handle is just an opaque driver handle — safe to move between
// threads as long as we don't call into it concurrently, which our single
// Mutex<VpnManager>-guarded usage already guarantees.
unsafe impl Send for WgConnection {}

impl WgConnection {
    pub fn new() -> Self {
        Self {
            adapter: None,
            dll: None,
            excluded_server_ip: None,
            dns_leak_block_active: false,
        }
    }

    pub fn is_connected(&self) -> bool {
        self.adapter.is_some()
    }

    /// `relay_endpoint`, when set, replaces the config's real server
    /// Endpoint with a local address instead — used for the HTTPS-masking
    /// relay (tls_mask.rs), which WireGuard talks to on loopback without
    /// ever knowing its real traffic is being relayed over TLS elsewhere.
    pub fn connect(
        &mut self,
        config_text: &str,
        dns_override: Option<&[String]>,
        relay_endpoint: Option<std::net::SocketAddr>,
    ) -> Result<(), String> {
        if self.is_connected() {
            self.disconnect();
        }

        let mut cfg = parse_config(config_text)?;
        if let Some(dns) = dns_override {
            if !dns.is_empty() {
                cfg.dns = dns.to_vec();
            }
        }
        if let Some(addr) = relay_endpoint {
            if let std::net::IpAddr::V4(ip) = addr.ip() {
                cfg.endpoint = (ip, addr.port());
            }
        }
        let dll = WgDll::load()?;

        let name = widestring(TUNNEL_NAME);
        let tunnel_type = widestring("WireGuard");
        let adapter = unsafe {
            (dll.create_adapter)(name.as_ptr(), tunnel_type.as_ptr(), std::ptr::null())
        };
        if adapter.is_null() {
            // Без кода ошибки это сообщение — гадание: «запустите от
            // администратора» одинаково выглядит и при отказе в правах, и при
            // не установившемся драйвере, и при чужом адаптере с тем же
            // именем. GetLastError отвечает, что именно произошло.
            let err = std::io::Error::last_os_error();
            let hint = match err.raw_os_error() {
                Some(5) => " — нет прав администратора",
                Some(1058) | Some(1060) => " — драйвер WireGuard не установлен или отключён",
                Some(87) => " — адаптер с таким именем уже занят другим процессом",
                _ => "",
            };
            return Err(format!("WireGuardCreateAdapter failed{hint}: {err}"));
        }

        let blob = build_config_blob(&cfg);
        let ok = unsafe { (dll.set_configuration)(adapter, blob.as_ptr(), blob.len() as u32) };
        if ok == 0 {
            unsafe { (dll.close_adapter)(adapter) };
            return Err("WireGuardSetConfiguration failed".to_string());
        }

        let ok = unsafe { (dll.set_adapter_state)(adapter, ADAPTER_STATE_UP) };
        if ok == 0 {
            unsafe { (dll.close_adapter)(adapter) };
            return Err("WireGuardSetAdapterState(UP) failed".to_string());
        }

        // wireguard.dll only sets up the crypto device; IP/routes/DNS are
        // ours to configure, same as upstream's own Go tunnel service does.
        if let Err(e) = self.configure_networking(&cfg) {
            unsafe {
                (dll.set_adapter_state)(adapter, 0);
                (dll.close_adapter)(adapter);
            }
            return Err(e);
        }

        self.adapter = Some(adapter);
        self.dll = Some(dll);
        Ok(())
    }

    fn configure_networking(&mut self, cfg: &ParsedConfig) -> Result<(), String> {
        run(
            "netsh",
            &[
                "interface",
                "ip",
                "set",
                "address",
                &format!("name={TUNNEL_NAME}"),
                "static",
                &cfg.address.to_string(),
                &cidr_to_mask(cfg.address_cidr).to_string(),
            ],
        )?;

        // Clamp MTU to the standard WireGuard value. A fresh Windows virtual
        // adapter defaults to MTU 1500, but every packet already carries the
        // tunnel's own IP/UDP/WireGuard header overhead (~60 bytes) — so any
        // packet near 1500 bytes needs to fragment or gets silently dropped
        // once it's re-encapsulated. Small requests (DNS, handshakes) still
        // get through fine, which is exactly the "connects, but pages/IP
        // checks never actually load" symptom this fixes.
        run(
            "netsh",
            &[
                "interface",
                "ipv4",
                "set",
                "subinterface",
                TUNNEL_NAME,
                "mtu=1420",
                "store=persistent",
            ],
        )?;

        // Force a low interface metric. Windows' effective route metric is
        // (interface metric + route metric), and a brand-new virtual adapter
        // can get an "automatic" metric that's actually *higher* than the
        // physical NIC's — in that case the split-default routes below lose
        // the race silently: the adapter comes up, `route print` looks right,
        // but the OS keeps sending traffic out the real NIC, so internet
        // never actually drops (which is exactly what looked broken).
        run(
            "netsh",
            &[
                "interface",
                "ipv4",
                "set",
                "interface",
                &format!("interface={TUNNEL_NAME}"),
                "metric=1",
            ],
        )?;

        if !cfg.dns.is_empty() {
            run(
                "netsh",
                &[
                    "interface",
                    "ip",
                    "set",
                    "dnsservers",
                    &format!("name={TUNNEL_NAME}"),
                    "static",
                    &cfg.dns[0],
                    "primary",
                ],
            )?;
            for extra in cfg.dns.iter().skip(1) {
                run(
                    "netsh",
                    &[
                        "interface",
                        "ip",
                        "add",
                        "dnsservers",
                        &format!("name={TUNNEL_NAME}"),
                        extra,
                    ],
                )?;
            }
        }

        let (gateway, physical_ifindex) = default_route_info()?;

        // Block DNS (port 53) from leaving via the physical adapter. Setting
        // the tunnel's own DNS servers above isn't enough on its own: Windows'
        // Smart Multi-Homed Name Resolution races the query across every
        // active interface and keeps whichever answer comes back first, so a
        // chatty physical NIC can still win and leak the queried domain to
        // the real ISP/DHCP resolver even though the tunnel has the lower
        // metric. Scoping a block rule to just the physical adapter's own
        // local IP starves that race down to the tunnel adapter without
        // touching any other traffic on that NIC. Undone in disconnect().
        if !cfg.dns.is_empty() {
            if let Ok(physical_ip) = physical_adapter_ip(physical_ifindex) {
                for proto in ["UDP", "TCP"] {
                    run(
                        "netsh",
                        &[
                            "advfirewall",
                            "firewall",
                            "add",
                            "rule",
                            &format!("name=ValaniumVPN-BlockDNSLeak-{proto}"),
                            "dir=out",
                            "action=block",
                            &format!("protocol={proto}"),
                            "remoteport=53",
                            &format!("localip={physical_ip}"),
                        ],
                    )?;
                }
                self.dns_leak_block_active = true;
            }
        }

        // Exclude the VPN server's own IP from the tunnel via the ORIGINAL
        // default gateway/interface — otherwise handshake/keepalive packets
        // to the server would try to route through the tunnel that carries
        // them, a routing loop. This is the one change that lives outside
        // the tunnel adapter, so it's the one thing disconnect() must undo
        // by hand (everything else disappears when the adapter is destroyed).
        // Skip when the endpoint is loopback (the HTTPS-masking relay,
        // tls_mask.rs, puts WireGuard's Endpoint at 127.0.0.1) — Windows
        // always routes 127.0.0.0/8 via the loopback interface regardless
        // of the split-default routes below, so there's no loop to break
        // out of, and routing 127.0.0.1 via the physical gateway would be
        // actively wrong.
        let (server_ip, _) = cfg.endpoint;
        if !server_ip.is_loopback() {
            run(
                "route",
                &[
                    "add",
                    &server_ip.to_string(),
                    "mask",
                    "255.255.255.255",
                    &gateway,
                    "metric",
                    "5",
                    "if",
                    &physical_ifindex.to_string(),
                ],
            )?;
            self.excluded_server_ip = Some(server_ip.to_string());
        }

        // Split-default route (0.0.0.0/1 + 128.0.0.0/1) through the tunnel —
        // the standard trick to send all traffic through the VPN without
        // literally replacing/deleting the existing default route. Bound
        // explicitly to the tunnel's ifIndex (see adapter_index's doc
        // comment) rather than trusting `route`'s gateway-based interface
        // guess, which picks the wrong adapter for a /32 tunnel address.
        let tunnel_ifindex = adapter_index(TUNNEL_NAME)?;
        for (dest, mask) in [("0.0.0.0", "128.0.0.0"), ("128.0.0.0", "128.0.0.0")] {
            run(
                "route",
                &[
                    "add",
                    dest,
                    "mask",
                    mask,
                    &cfg.address.to_string(),
                    "metric",
                    "3",
                    "if",
                    &tunnel_ifindex.to_string(),
                ],
            )?;
        }

        Ok(())
    }

    pub fn disconnect(&mut self) {
        if self.dns_leak_block_active {
            for proto in ["UDP", "TCP"] {
                let _ = run(
                    "netsh",
                    &[
                        "advfirewall",
                        "firewall",
                        "delete",
                        "rule",
                        &format!("name=ValaniumVPN-BlockDNSLeak-{proto}"),
                    ],
                );
            }
            self.dns_leak_block_active = false;
        }
        if let Some(server_ip) = self.excluded_server_ip.take() {
            let _ = run("route", &["delete", &server_ip]);
        }
        // The split-default routes and the tunnel's own address are owned
        // by the adapter and vanish with it below.
        if let (Some(adapter), Some(dll)) = (self.adapter.take(), self.dll.take()) {
            unsafe {
                (dll.set_adapter_state)(adapter, 0);
                (dll.close_adapter)(adapter);
            }
        }
    }
}

impl Drop for WgConnection {
    fn drop(&mut self) {
        self.disconnect();
    }
}
