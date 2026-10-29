//! Phase one: encrypted capability probes. No microphone, SDP, ICE or media transport.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const TTL_MS: i64 = 90_000;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Probe,
    Unavailable,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signal {
    pub v: u8,
    pub id: String,
    pub action: Action,
    pub expires_at: i64,
}
impl Signal {
    pub fn valid(&self) -> bool {
        self.v == 1
            && self.id.len() == 32
            && self
                .id
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    }
    pub fn fresh(&self, now: i64) -> bool {
        self.valid() && self.expires_at > now && self.expires_at <= now + TTL_MS + 5_000
    }
    pub fn body(&self) -> String {
        format!(
            "{}{}",
            crate::access::CONTROL_PREFIX,
            serde_json::json!({"call":self})
        )
    }
}
#[derive(Default)]
pub struct Pending {
    outgoing: HashMap<String, (String, i64)>,
    seen: HashMap<(String, String), i64>,
}
impl Pending {
    fn prune(&mut self, now: i64) {
        self.outgoing.retain(|_, (_, until)| *until > now);
        self.seen.retain(|_, until| *until > now);
    }
    pub fn start(&mut self, peer: &str, now: i64) -> Option<Signal> {
        self.prune(now);
        if self.outgoing.len() >= 8 || self.outgoing.values().any(|(p, _)| p == peer) {
            return None;
        }
        let signal = Signal {
            v: 1,
            id: hex::encode(crate::crypto::random_bytes(16)),
            action: Action::Probe,
            expires_at: now + TTL_MS,
        };
        self.outgoing
            .insert(signal.id.clone(), (peer.into(), signal.expires_at));
        Some(signal)
    }
    pub fn answer(&mut self, peer: &str, signal: &Signal, now: i64) -> bool {
        self.prune(now);
        let key = (peer.to_owned(), signal.id.clone());
        if !signal.fresh(now) || self.seen.len() >= 128 || self.seen.contains_key(&key) {
            return false;
        }
        self.seen.insert(key, signal.expires_at);
        true
    }
    pub fn complete(&mut self, peer: &str, signal: &Signal, now: i64) -> bool {
        self.prune(now);
        if signal.action != Action::Unavailable
            || !signal.fresh(now)
            || !self
                .outgoing
                .get(&signal.id)
                .is_some_and(|(p, _)| p == peer)
        {
            return false;
        }
        self.outgoing.remove(&signal.id);
        true
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parser_rejects_addresses_unknown_versions_and_unbounded_ids() {
        let signal = Signal {
            v: 1,
            id: "a".repeat(32),
            action: Action::Probe,
            expires_at: 90_000,
        };
        let mut raw = serde_json::to_value(&signal).unwrap();
        raw["ice"] = serde_json::json!("192.0.2.1");
        assert!(serde_json::from_value::<Signal>(raw).is_err());
        assert!(!Signal {
            v: 2,
            ..signal.clone()
        }
        .valid());
        assert!(!Signal {
            id: "a".repeat(4096),
            ..signal.clone()
        }
        .valid());
        assert!(!signal.fresh(90_000));
        assert!(!signal.fresh(-10_000));
    }
    #[test]
    fn replies_are_bound_to_peer_request_and_expiry() {
        let mut pending = Pending::default();
        let probe = pending.start("alice", 1).unwrap();
        assert!(pending.start("alice", 2).is_none());
        let reply = Signal {
            action: Action::Unavailable,
            ..probe
        };
        assert!(!pending.complete("mallory", &reply, 3));
        assert!(pending.complete("alice", &reply, 3));
        assert!(!pending.complete("alice", &reply, 4));
    }
    #[test]
    fn repeated_and_expired_probes_do_not_trigger_replies() {
        let mut pending = Pending::default();
        let probe = pending.start("bob", 1).unwrap();
        assert!(pending.answer("alice", &probe, 2));
        assert!(!pending.answer("alice", &probe, 3));
        assert!(!pending.answer("other", &probe, 90_001));
    }
    #[test]
    fn pending_and_replay_maps_are_bounded_and_expire() {
        let mut pending = Pending::default();
        for i in 0..8 {
            assert!(pending.start(&format!("peer-{i}"), 1).is_some());
        }
        assert!(pending.start("extra", 1).is_none());
        assert!(pending.start("extra", 90_002).is_some());
        let mut replies = Pending::default();
        for i in 0..128 {
            assert!(replies.answer(
                "peer",
                &Signal {
                    v: 1,
                    id: format!("{i:032x}"),
                    action: Action::Probe,
                    expires_at: 90_000
                },
                1
            ));
        }
        assert!(!replies.answer(
            "other",
            &Signal {
                v: 1,
                id: "a".repeat(32),
                action: Action::Probe,
                expires_at: 90_000
            },
            1
        ));
    }
}
