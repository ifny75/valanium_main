//! Durable sealed-sender outbox. Retry the exact MLS ciphertext and client ref;
//! never downgrade a failed anonymous delivery to the authenticated socket.
use serde::{Deserialize, Serialize};
use crate::{error::{CoreError, Result}, store::Store};

const KEY: &str = "anonymous_outbox_v1";
const MAX_ITEMS: usize = 128;
const MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum Kind {
    #[default]
    Message,
    Essential,
    Ephemeral,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Envelope {
    #[serde(default = "normal_priority")]
    pub priority: u8,
    #[serde(default)]
    pub queued_at: i64,
    pub client_ref: [u8; 16],
    pub device: [u8; 32],
    pub conversation: Vec<u8>,
    pub logical_id: Option<String>,
    pub ciphertext: Vec<u8>,
    #[serde(default)]
    pub kind: Kind,
    #[serde(default)]
    pub expires_at: Option<i64>,
}
fn normal_priority()->u8{1}

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
    enqueue_batch(store, vec![envelope])
}

/// A group fan-out either fits in full or does not change the queue at all.
pub(crate) fn enqueue_batch(store: &Store, envelopes: Vec<Envelope>) -> Result<()> {
    let mut queue = load(store)?;
    let mut ids: std::collections::HashSet<_> = queue.iter().map(|item| item.client_ref).collect();
    for item in &envelopes {
        if !ids.insert(item.client_ref) {
            return Err(CoreError::Rejected("anonymous_queue_duplicate".into()));
        }
    }
    if queue.len().saturating_add(envelopes.len()) > MAX_ITEMS
        || queue.iter().map(|item| item.ciphertext.len()).sum::<usize>()
            .saturating_add(envelopes.iter().map(|item| item.ciphertext.len()).sum::<usize>()) > MAX_BYTES {
        return Err(CoreError::Rejected("anonymous_queue_full".into()));
    }
    store.atomic(|| {
        for item in &envelopes {
            if item.kind == Kind::Message {
                if let Some(id) = &item.logical_id {
                    crate::delivery::track(store, &item.conversation, id, &item.device, item.expires_at)?;
                }
            }
        }
        queue.extend(envelopes);
        save(store, &queue)
    })
}

pub(crate) fn acknowledge(store: &Store, client_ref: &[u8; 16]) -> Result<()> {
    let mut queue = load(store)?;
    store.atomic(|| {
        for item in queue.iter().filter(|item| &item.client_ref == client_ref) {
            if item.kind == Kind::Message {
                if let Some(id) = &item.logical_id {
                    crate::delivery::accepted(store, &item.conversation, id, &item.device)?;
                }
            }
        }
        queue.retain(|item| &item.client_ref != client_ref);
        save(store, &queue)
    })
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
    // Clearing local history must not discard an undelivered Welcome/commit.
    queue.retain(|item| item.conversation != conversation || item.kind == Kind::Essential);
    save(store, &queue)
}

pub(crate) fn forget_thread(store: &Store, conversation: &[u8]) -> Result<()> {
    let mut queue = load(store)?;
    queue.retain(|item| item.conversation != conversation);
    save(store, &queue)
}

pub(crate) fn drop_ephemeral(store: &Store) -> Result<()> {
    let mut queue = load(store)?;
    queue.retain(|item| item.kind != Kind::Ephemeral);
    save(store, &queue)
}

pub(crate) fn prune_expired(store: &Store, now: i64) -> Result<Vec<Envelope>> {
    let mut queue = load(store)?;
    let before = queue.len();
    queue.retain(|item| item.expires_at.map_or(true, |expires| expires > now));
    if before != queue.len() { save(store, &queue)?; }
    Ok(queue)
}

/// Bound legacy envelopes once on upgrade rather than retrying beyond the
/// server's deduplication retention window indefinitely.
pub(crate) fn bound_legacy_lifetimes(store: &Store, now: i64, ttl: i64) -> Result<()> {
    let mut queue = load(store)?;
    if !queue.iter().any(|item| item.expires_at.is_none()) { return Ok(()); }
    store.atomic(|| {
        for item in &mut queue {
            if item.expires_at.is_none() { item.expires_at = Some(now + ttl); }
            if item.kind == Kind::Message {
                if let Some(id) = &item.logical_id {
                    crate::delivery::track(store, &item.conversation, id, &item.device, item.expires_at)?;
                }
            }
        }
        save(store, &queue)
    })
}

/// At most one envelope per device may be active. This keeps Welcome and
/// commits ahead of their dependent application messages, including retries.
pub(crate) fn ready(queue: &[Envelope], inflight: &std::collections::HashSet<[u8; 16]>, limit: usize) -> Vec<Envelope> {
    let busy: std::collections::HashSet<_> = queue.iter()
        .filter(|item| inflight.contains(&item.client_ref)).map(|item| item.device).collect();
    let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as i64;
    let score=|item:&Envelope|if item.queued_at>0&&now.saturating_sub(item.queued_at)>30_000{item.priority.min(1)}else{item.priority};
    let mut barriers=std::collections::HashSet::new();
    let mut candidates:std::collections::HashMap<_,(usize,&Envelope)>=std::collections::HashMap::new();
    for (index,item) in queue.iter().enumerate() {
        if busy.contains(&item.device)||barriers.contains(&item.device){continue;}
        if item.kind==Kind::Essential {
            barriers.insert(item.device);candidates.entry(item.device).or_insert((index,item));
        }else{
            candidates.entry(item.device).and_modify(|candidate|{if score(item)<score(candidate.1){*candidate=(index,item);}}).or_insert((index,item));
        }
    }
    let mut candidates:Vec<_>=candidates.into_values().collect();candidates.sort_by_key(|(index,item)|(score(item),*index));
    candidates.into_iter().take(limit).map(|(_,item)|item.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn text_overtakes_an_attachment_but_never_a_key_change(){
        let mut image=sample(1);image.priority=2;
        let text=sample(2);
        assert_eq!(ready(&[image.clone(),text.clone()],&Default::default(),1)[0].client_ref,text.client_ref);
        let mut commit=sample(3);commit.kind=Kind::Essential;
        let queue=vec![image.clone(),commit,text.clone()];
        assert_eq!(ready(&queue,&Default::default(),1)[0].client_ref,image.client_ref);
        assert_eq!(ready(&queue,&std::collections::HashSet::from([image.client_ref]),4).len(),0);
    }
    #[test] fn an_aged_attachment_is_not_starved(){let mut image=sample(1);image.priority=2;image.queued_at=1;assert_eq!(ready(&[image,sample(2)],&Default::default(),1)[0].client_ref,[1;16]);}
    fn sample(id: u8) -> Envelope {
        Envelope { priority:1,queued_at:0,client_ref: [id; 16], device: [2; 32], conversation: vec![4],
            logical_id: Some(format!("message-{id}")), ciphertext: vec![3; 256],
            kind: Kind::Message, expires_at: None }
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

    #[test]
    fn scheduler_keeps_welcome_commit_and_message_order_per_recipient() {
        let welcome = sample(1);
        let commit = sample(2);
        let message = sample(3);
        let mut other = sample(4);
        other.device = [5; 32];
        let queue = vec![welcome.clone(), commit.clone(), message.clone(), other.clone()];
        assert_eq!(ready(&queue, &Default::default(), 4), vec![welcome.clone(), other.clone()]);
        let inflight = [welcome.client_ref].into_iter().collect();
        assert_eq!(ready(&queue, &inflight, 4), vec![other.clone()]);
        assert_eq!(ready(&queue[1..], &Default::default(), 4), vec![commit, other]);
    }

    #[test]
    fn batch_overflow_is_atomic_and_expiry_keeps_required_commits() {
        let store = Store::open(":memory:", b"test").unwrap();
        enqueue_batch(&store, (0..127).map(sample).collect()).unwrap();
        assert!(enqueue_batch(&store, vec![sample(127), sample(128)]).is_err());
        assert_eq!(load(&store).unwrap().len(), 127);
        forget_thread(&store, &[4]).unwrap();
        let mut commit = sample(1);
        commit.kind = Kind::Essential;
        let mut typing = sample(2);
        typing.kind = Kind::Ephemeral;
        typing.expires_at = Some(100);
        enqueue_batch(&store, vec![commit.clone(), typing]).unwrap();
        assert_eq!(prune_expired(&store, 100).unwrap(), vec![commit.clone()]);
        enqueue(&store, sample(3)).unwrap();
        cancel_thread(&store, &[4]).unwrap();
        assert_eq!(load(&store).unwrap(), vec![commit]);
        forget_thread(&store, &[4]).unwrap();
        assert!(load(&store).unwrap().is_empty());
    }

    #[test]
    fn old_queue_records_remain_readable() {
        let mut old = serde_json::to_value(sample(1)).unwrap();
        old.as_object_mut().unwrap().remove("kind");
        old.as_object_mut().unwrap().remove("expires_at");
        assert_eq!(serde_json::from_value::<Envelope>(old).unwrap(), sample(1));
    }
}
