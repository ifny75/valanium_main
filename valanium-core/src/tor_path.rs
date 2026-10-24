//! Local-only snapshot of the most recently connected onion stream, not all Tor circuits.
use std::sync::{Mutex, atomic::{AtomicU64, Ordering}};
use tor_linkspec::HasAddrs;
use tor_proto::stream::{ClientStreamCtrl, DataStream};

static LAST: Mutex<Option<serde_json::Value>> = Mutex::new(None);
static SEQUENCE: AtomicU64 = AtomicU64::new(1);

pub fn snapshot() -> String {
    LAST.lock().ok().and_then(|v| v.clone()).unwrap_or(serde_json::Value::Null).to_string()
}

pub struct CircuitLease(u64);

pub fn observe(stream: &DataStream, destination: &str) -> CircuitLease {
    let id = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let circuit = stream.client_stream_ctrl().and_then(|ctrl| ctrl.circuit());
    let hops: Vec<Vec<String>> = circuit.map(|circuit| circuit.path_ref().hops().iter()
        .filter_map(|hop| hop.as_chan_target().map(|relay|
            relay.addrs().iter().map(|addr| addr.ip().to_string()).collect()))
        .collect()).unwrap_or_default();
    if let Ok(mut last) = LAST.lock() {
        *last = Some(serde_json::json!({"id":id, "active":true, "destination":destination, "hops":hops}));
    }
    CircuitLease(id)
}

impl Drop for CircuitLease {
    fn drop(&mut self) {
        if let Ok(mut last) = LAST.lock() {
            if let Some(value) = last.as_mut() {
                if value["id"].as_u64() == Some(self.0) { value["active"] = false.into(); }
            }
        }
    }
}
