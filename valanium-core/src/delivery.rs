//! Local, encrypted delivery evidence. A server ACK is never a peer receipt.
use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use crate::{Result, store::Store};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
enum Step { Queued, Accepted, Delivered }
#[derive(Default, Serialize, Deserialize)]
struct Record {
    devices: BTreeMap<String, Step>,
    #[serde(default)] queued_at: Option<i64>,
    #[serde(default)] sent_at: BTreeMap<String,i64>,
    #[serde(default)] accepted_at: BTreeMap<String,i64>,
    #[serde(default)] expires_at: Option<i64>,
}
fn key(conversation: &[u8], id: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"valanium-delivery-v1"); hash.update(conversation); hash.update(id.as_bytes());
    format!("delivery:{}", hex::encode(hash.finalize()))
}
fn load(store: &Store, conversation: &[u8], id: &str) -> Result<Record> {
    Ok(match store.load_setting(&key(conversation, id))? {
        Some(raw) => serde_json::from_slice(&raw)?, None => Record::default(),
    })
}
fn save(store: &Store, conversation: &[u8], id: &str, record: &Record) -> Result<()> {
    store.save_setting(&key(conversation, id), &serde_json::to_vec(record)?)
}
pub(crate) fn track(store: &Store, conversation: &[u8], id: &str, device: &[u8], expires_at: Option<i64>) -> Result<()> {
    let mut record = load(store, conversation, id)?;
    record.queued_at.get_or_insert_with(now);
    if record.expires_at.is_none() { record.expires_at = expires_at; }
    record.devices.entry(hex::encode(device)).or_insert(Step::Queued);
    save(store, conversation, id, &record)
}
fn advance(store: &Store, conversation: &[u8], id: &str, device: &[u8], step: Step) -> Result<()> {
    let mut record = load(store, conversation, id)?;
    // Receipts cannot manufacture evidence for an unknown message/recipient.
    if let Some(old) = record.devices.get_mut(&hex::encode(device)) {
        *old = (*old).max(step);
        save(store, conversation, id, &record)?;
    }
    Ok(())
}
pub(crate) fn accepted(store: &Store, conversation: &[u8], id: &str, device: &[u8]) -> Result<()> {
    let mut record=load(store,conversation,id)?;let device=hex::encode(device);
    if let Some(step)=record.devices.get_mut(&device){
        *step=(*step).max(Step::Accepted);
        if !record.accepted_at.contains_key(&device){
            let at=now();if let Some(sent)=record.sent_at.get(&device){crate::latency::record(store,"acknowledgement",at.saturating_sub(*sent).max(0) as u64)?;}
            record.accepted_at.insert(device,at);
        }
        save(store,conversation,id,&record)?;
    }
    Ok(())
}
fn now()->i64{std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as i64}
pub(crate) fn sending(store:&Store,conversation:&[u8],id:&str,device:&[u8])->Result<()>{
    let mut record=load(store,conversation,id)?;let device=hex::encode(device);
    if record.devices.contains_key(&device)&&!record.sent_at.contains_key(&device){let at=now();
        if let Some(queued)=record.queued_at{crate::latency::record(store,"queue",at.saturating_sub(queued).max(0) as u64)?;}
        record.sent_at.insert(device,at);save(store,conversation,id,&record)?;
    }Ok(())
}
pub(crate) fn delivered(store: &Store, conversation: &[u8], id: &str, device: &[u8]) -> Result<()> {
    advance(store, conversation, id, device, Step::Delivered)
}
pub(crate) fn state(store: &Store, conversation: &[u8], id: &str) -> Result<Option<String>> {
    let record = load(store, conversation, id)?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as i64;
    if record.devices.values().any(|step| *step == Step::Queued) && record.expires_at.is_some_and(|at| at <= now) {
        return Ok(Some("expired".into()));
    }
    Ok(record.devices.values().min().map(|step| match step {
        Step::Queued => "queued", Step::Accepted => "accepted", Step::Delivered => "delivered",
    }.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn receipts_bind_to_conversation_and_recipient_and_never_regress() {
        let store = Store::open(":memory:", b"test").unwrap();
        track(&store, &[1], "m", &[2], None).unwrap();
        track(&store, &[1], "m", &[3], None).unwrap();
        delivered(&store, &[9], "m", &[2]).unwrap();
        delivered(&store, &[1], "m", &[9]).unwrap();
        accepted(&store, &[1], "m", &[2]).unwrap();
        assert_eq!(state(&store, &[1], "m").unwrap().as_deref(), Some("queued"));
        accepted(&store, &[1], "m", &[3]).unwrap();
        assert_eq!(state(&store, &[1], "m").unwrap().as_deref(), Some("accepted"));
        delivered(&store, &[1], "m", &[2]).unwrap();
        delivered(&store, &[1], "m", &[3]).unwrap();
        accepted(&store, &[1], "m", &[2]).unwrap();
        track(&store, &[1], "m", &[2], None).unwrap();
        assert_eq!(state(&store, &[1], "m").unwrap().as_deref(), Some("delivered"));
        assert_eq!(state(&store, &[9], "m").unwrap(), None);
    }
}
