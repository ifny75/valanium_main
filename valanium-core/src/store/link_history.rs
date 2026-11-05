//! Snapshot streaming for a NEW device. Only history/public conversation
//! metadata is exported. Never export MLS state, old device keys or outboxes.
use super::*;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;
#[derive(Serialize, Deserialize)]
#[serde(tag = "record", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Record {
    Meta {
        conversations: Vec<(String, String)>,
        threads: Vec<(String, String)>,
        peers: Vec<(String, String)>,
        groups: Vec<crate::migrate::ArchiveGroup>,
        messages: u64,
    },
    Message(crate::migrate::ArchiveMessage),
}
const LIMIT: u64 = 8 * 1024 * 1024 * 1024;
impl Store {
    pub(crate) fn path(&self) -> String {
        self.path.clone()
    }
    pub(crate) fn clear_link_stage(&self, session: Option<&str>) -> Result<()> {
        self.connection.execute_batch("CREATE TABLE IF NOT EXISTS link_stage (session TEXT NOT NULL, seq INTEGER NOT NULL, sealed BLOB NOT NULL, PRIMARY KEY(session,seq)); CREATE TABLE IF NOT EXISTS link_stage_state(session TEXT PRIMARY KEY,next_seq INTEGER NOT NULL,total_bytes INTEGER NOT NULL)")?;
        if let Some(session) = session {
            self.connection
                .execute("DELETE FROM link_stage WHERE session=?1", params![session])?;
            self.connection.execute(
                "DELETE FROM link_stage_state WHERE session=?1",
                params![session],
            )?;
        } else {
            self.connection.execute("DELETE FROM link_stage", [])?;
            self.connection
                .execute("DELETE FROM link_stage_state", [])?;
        }
        Ok(())
    }
    pub(crate) fn stage_link_record(&self, session: &str, seq: u64, raw: &[u8]) -> Result<()> {
        if seq >= 1_000_000 || raw.len() > 16 * 1024 * 1024 {
            return Err(CoreError::BadFrame);
        }
        let (next, size): (i64, i64) = self
            .connection
            .query_row(
                "SELECT next_seq,total_bytes FROM link_stage_state WHERE session=?1",
                params![session],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .unwrap_or((0, 0));
        if next as u64 != seq || size as u64 + raw.len() as u64 > LIMIT {
            return Err(CoreError::BadFrame);
        }
        let aad = format!("pairing-history-v2:{session}:{seq}");
        let sealed = self.key.seal(aad.as_bytes(), raw)?;
        self.atomic(|| {
   self.connection.execute("INSERT INTO link_stage(session,seq,sealed) VALUES(?1,?2,?3)",params![session,seq as i64,sealed])?;
   self.connection.execute("INSERT INTO link_stage_state(session,next_seq,total_bytes) VALUES(?1,?2,?3) ON CONFLICT(session) DO UPDATE SET next_seq=excluded.next_seq,total_bytes=excluded.total_bytes",params![session,seq as i64+1,size+raw.len()as i64])?;Ok(())
  })
    }
    /// Runs on a bounded blocking producer with its own connection/read snapshot.
    pub(crate) fn stream_link_snapshot(
        &self,
        output: &tokio::sync::mpsc::Sender<Result<Zeroizing<Vec<u8>>>>,
    ) -> Result<()> {
        self.connection.execute_batch("BEGIN DEFERRED")?;
        let result = (|| {
            let pairs = |sql: &str| -> Result<Vec<(String, String)>> {
                let mut q = self.connection.prepare(sql)?;
                let rows = q.query_map([], |r| {
                    Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Vec<u8>>(1)?))
                })?;
                let mut out = Vec::new();
                for row in rows {
                    let (a, b) = row?;
                    out.push((hex::encode(a), hex::encode(b)));
                }
                Ok(out)
            };
            let conversations =
                pairs("SELECT device_pub,group_id FROM conversations ORDER BY rowid")?;
            let threads = pairs("SELECT group_id,thread FROM threads")?;
            let peers = pairs("SELECT device_pub,identity FROM peer_devices")?;
            let groups = self
                .list_groups()?
                .into_iter()
                .map(|(id, kind, meta)| crate::migrate::ArchiveGroup {
                    group_id: hex::encode(id),
                    kind,
                    meta: hex::encode(meta),
                    created_at: 0,
                })
                .collect();
            let count: i64 =
                self.connection
                    .query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))?;
            let emit = |record: Record| -> Result<()> {
                let bytes = Zeroizing::new(serde_json::to_vec(&record)?);
                if bytes.len() > 16 * 1024 * 1024 {
                    return Err(CoreError::BadFrame);
                }
                output
                    .blocking_send(Ok(bytes))
                    .map_err(|_| CoreError::Rejected("pairing_cancelled".into()))
            };
            emit(Record::Meta {
                conversations,
                threads,
                peers,
                groups,
                messages: count as u64,
            })?;
            let mut q=self.connection.prepare("SELECT id,conversation,outgoing,created_at,sealed FROM messages ORDER BY created_at,rowid")?;
            let rows = q.query_map([], |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, Vec<u8>>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, Vec<u8>>(4)?,
                ))
            })?;
            for row in rows {
                let (id, conversation, outgoing, created_at, sealed) = row?;
                let body = Zeroizing::new(self.key.open(&id, &sealed)?);
                emit(Record::Message(crate::migrate::ArchiveMessage {
                    id: hex::encode(id),
                    conversation: hex::encode(conversation),
                    outgoing: outgoing != 0,
                    created_at,
                    body: hex::encode(&*body),
                }))?;
            }
            Ok(())
        })();
        let _ = self.connection.execute_batch("ROLLBACK");
        result
    }
    pub(crate) fn install_link_history(&self, session: &str) -> Result<u64> {
        let decode = |value: &str| -> Result<Vec<u8>> {
            if value.is_empty() || value.len() > 512 {
                return Err(CoreError::BadFrame);
            }
            hex::decode(value).map_err(|_| CoreError::BadFrame)
        };
        let mut q = self
            .connection
            .prepare("SELECT seq,sealed FROM link_stage WHERE session=?1 ORDER BY seq")?;
        let rows = q.query_map(params![session], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?))
        })?;
        let (mut meta, mut expected, mut count) = (false, 0, 0);
        for row in rows {
            let (seq, sealed) = row?;
            let aad = format!("pairing-history-v2:{session}:{seq}");
            let bytes = Zeroizing::new(self.key.open(aad.as_bytes(), &sealed)?);
            match serde_json::from_slice::<Record>(&bytes)? {
                Record::Meta {
                    conversations,
                    threads,
                    peers,
                    groups,
                    messages,
                } => {
                    if meta || seq != 0 {
                        return Err(CoreError::BadFrame);
                    }
                    meta = true;
                    expected = messages;
                    for (device, group) in conversations {
                        let device = decode(&device)?;
                        if device.len() != 32 {
                            return Err(CoreError::BadKeyLength);
                        }
                        self.set_conversation(&device, &decode(&group)?)?;
                    }
                    for (group, thread) in threads {
                        self.set_thread(&decode(&group)?, &decode(&thread)?)?;
                    }
                    for (device, identity) in peers {
                        let device = decode(&device)?;
                        let identity = decode(&identity)?;
                        let device: [u8; 32] =
                            device.try_into().map_err(|_| CoreError::BadKeyLength)?;
                        if identity.len() != 32 {
                            return Err(CoreError::BadKeyLength);
                        }
                        self.remember_peer_identity(&identity, &[device])?;
                    }
                    for group in groups {
                        let id = decode(&group.group_id)?;
                        let body = Zeroizing::new(
                            hex::decode(&group.meta).map_err(|_| CoreError::BadFrame)?,
                        );
                        self.save_group(&id, &group.kind, &body, group.created_at)?;
                        self.save_setting(
                            &format!("history-only-group:{}", group.group_id),
                            b"true",
                        )?;
                    }
                }
                Record::Message(message) => {
                    if !meta || count >= expected {
                        return Err(CoreError::BadFrame);
                    }
                    let id = decode(&message.id)?;
                    let conversation = decode(&message.conversation)?;
                    let body = Zeroizing::new(
                        hex::decode(&message.body).map_err(|_| CoreError::BadFrame)?,
                    );
                    self.insert_message(
                        &id,
                        &conversation,
                        message.outgoing,
                        message.created_at,
                        &body,
                    )?;
                    count += 1;
                }
            }
        }
        if !meta || expected != count {
            return Err(CoreError::BadFrame);
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn meta(messages: u64) -> Record {
        Record::Meta {
            conversations: vec![(hex::encode([7; 32]), hex::encode(b"thread"))],
            threads: vec![],
            peers: vec![],
            groups: vec![],
            messages,
        }
    }
    fn message() -> Record {
        Record::Message(crate::migrate::ArchiveMessage {
            id: hex::encode([1; 16]),
            conversation: hex::encode(b"thread"),
            outgoing: false,
            created_at: 123,
            body: hex::encode(b"secret-history-content"),
        })
    }
    #[test]
    fn staged_history_is_encrypted_ordered_and_truncation_rolls_back_all_account_data() {
        let store = Store::open(":memory:", b"destination-key").unwrap();
        store.clear_link_stage(None).unwrap();
        let header = serde_json::to_vec(&meta(2)).unwrap();
        let record = serde_json::to_vec(&message()).unwrap();
        assert!(store.stage_link_record("session", 2, &record).is_err());
        store.stage_link_record("session", 0, &header).unwrap();
        store.stage_link_record("session", 1, &record).unwrap();
        let raw: Vec<u8> = store
            .connection
            .query_row("SELECT sealed FROM link_stage WHERE seq=1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(!raw.windows(8).any(|w| w == b"secret-h"));
        let result = store.atomic(|| {
            store.save_credentials(&Credentials::generate())?;
            store.install_link_history("session")
        });
        assert!(result.is_err());
        assert!(!store.has_credentials().unwrap());
        assert!(store.list_conversations().unwrap().is_empty());
        assert!(store.list_messages(b"thread", 10, None).unwrap().is_empty());
        store.clear_link_stage(Some("session")).unwrap();
        store
            .stage_link_record("session", 0, &serde_json::to_vec(&meta(1)).unwrap())
            .unwrap();
        store.stage_link_record("session", 1, &record).unwrap();
        store
            .atomic(|| {
                store.save_credentials(&Credentials::generate())?;
                assert_eq!(store.install_link_history("session")?, 1);
                store.clear_link_stage(Some("session"))
            })
            .unwrap();
        let messages = store.list_messages(b"thread", 10, None).unwrap();
        assert_eq!(messages[0].body, b"secret-history-content");
        assert_eq!(messages[0].created_at, 123);
        assert!(!messages[0].outgoing);
        assert!(store.has_credentials().unwrap());
        let left: i64 = store
            .connection
            .query_row("SELECT COUNT(*) FROM link_stage", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 0);
    }
}
