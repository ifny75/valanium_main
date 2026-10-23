//! Durable sealed-sender outbox. Retry the exact MLS ciphertext and client ref;
//! never downgrade a failed anonymous delivery to the authenticated socket.
use serde::{Deserialize, Serialize};
use crate::{error::{CoreError, Result}, store::Store};

const KEY: &str = "anonymous_outbox_v1";
const MAX_ITEMS: usize = 128;
const MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Envelope {
    pub client_ref: [u8; 16],
    pub device: [u8; 32],
    pub conversation: Vec<u8>,
    pub logical_id: Option<String>,
    pub ciphertext: Vec<u8>,
}

pub(crate) fn load(store: &Store) -> Result<Vec<Envelope>> {
    match store.load_setting(KEY)? {
        Some(bytes) => Ok(serde_json::from_slice(&bytes)?),
        None => Ok(Vec::new()),
    }
}

fn save(store: &Store, queue: &[Envelope]) -> Result<()> {
    store.save_setting(KEY, &serde_json::to_vec(queue)?)
}

pub(crate) fn enqueue(store: &Store, envelope: Envelope) -> Result<()> {
    let mut queue = load(store)?;
    if queue.iter().any(|item| item.client_ref == envelope.client_ref) {
        return Err(CoreError::Rejected("anonymous_queue_duplicate".into()));
    }
    if queue.len() >= MAX_ITEMS
        || queue.iter().map(|item| item.ciphertext.len()).sum::<usize>()
            .saturating_add(envelope.ciphertext.len()) > MAX_BYTES {
        return Err(CoreError::Rejected("anonymous_queue_full".into()));
    }
    queue.push(envelope);
    save(store, &queue)
}

pub(crate) fn acknowledge(store: &Store, client_ref: &[u8; 16]) -> Result<()> {
    let mut queue = load(store)?;
    queue.retain(|item| &item.client_ref != client_ref);
    save(store, &queue)
}

pub(crate) fn contains_message(store: &Store, conversation: &[u8], id: &str) -> Result<bool> {
    Ok(load(store)?.iter().any(|item| item.conversation == conversation
        && (item.logical_id.as_deref() == Some(id) || hex::encode(item.client_ref) == id)))
}

pub(crate) fn cancel_message(store: &Store, conversation: &[u8], id: &str) -> Result<()> {
    let mut queue = load(store)?;
    queue.retain(|item| item.conversation != conversation
        || (item.logical_id.as_deref() != Some(id) && hex::encode(item.client_ref) != id));
    save(store, &queue)
}

pub(crate) fn cancel_thread(store: &Store, conversation: &[u8]) -> Result<()> {
    let mut queue = load(store)?;
    queue.retain(|item| item.conversation != conversation);
    save(store, &queue)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(id: u8) -> Envelope {
        Envelope { client_ref: [id; 16], device: [2; 32], conversation: vec![4],
            logical_id: Some(format!("message-{id}")), ciphertext: vec![3; 256] }
    }
    #[test]
    fn reopen_preserves_exact_ciphertext_and_ack_removes_only_one() {
        let path = std::env::temp_dir().join(format!("valanium-anon-{}.db", hex::encode(crate::crypto::random_bytes(8))));
        let path = path.to_str().unwrap();
        {
            let store = Store::open(path, b"test").unwrap();
            enqueue(&store, sample(1)).unwrap();
            enqueue(&store, sample(2)).unwrap();
        }
        {
            let store = Store::open(path, b"test").unwrap();
            assert_eq!(load(&store).unwrap(), vec![sample(1), sample(2)]);
            acknowledge(&store, &[1; 16]).unwrap();
            assert_eq!(load(&store).unwrap(), vec![sample(2)]);
            acknowledge(&store, &[1; 16]).unwrap();
            assert_eq!(load(&store).unwrap(), vec![sample(2)]);
        }
        let _ = std::fs::remove_file(path);
    }
    #[test]
    fn corrupt_queue_is_not_silently_replaced_and_oversize_does_not_drop_entries() {
        let store = Store::open(":memory:", b"test").unwrap();
        enqueue(&store, sample(1)).unwrap();
        assert!(enqueue(&store, sample(1)).is_err());
        let mut huge = sample(2);
        huge.ciphertext = vec![0; MAX_BYTES];
        assert!(enqueue(&store, huge).is_err());
        assert_eq!(load(&store).unwrap(), vec![sample(1)]);
        store.save_setting(KEY, b"broken").unwrap();
        assert!(load(&store).is_err());
        assert!(enqueue(&store, sample(3)).is_err());
    }

    #[test]
    fn deleting_pending_messages_and_threads_cancels_future_retries() {
        let store = Store::open(":memory:", b"test").unwrap();
        enqueue(&store, sample(1)).unwrap();
        enqueue(&store, sample(2)).unwrap();
        assert!(contains_message(&store, &[4], "message-1").unwrap());
        cancel_message(&store, &[4], "message-1").unwrap();
        assert_eq!(load(&store).unwrap(), vec![sample(2)]);
        cancel_thread(&store, &[4]).unwrap();
        assert!(load(&store).unwrap().is_empty());
    }
}
