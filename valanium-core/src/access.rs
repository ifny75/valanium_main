//! Пропуска: кому разрешено вам писать.
//!
//! Задача — дать серверу проверять право написать, не давая ему знать, кто с
//! кем переписывается.
//!
//! Пропуск — это 32 случайных байта. Владелец кладёт на сервер только их хеш;
//! сам секрет он раздаёт тем, кого пускает. Отправитель предъявляет секрет на
//! своём соединении, сервер сверяет хеш и запоминает допуск **в памяти, до
//! конца соединения**. На диске у сервера остаётся строка «хеш → владелец», из
//! которой нельзя узнать, кому пропуск отдан.
//!
//! Связку «отправитель → получатель» сервер и так видит транзитно в каждом
//! конверте — пропуск новых следов не добавляет.
//!
//! # Откуда пропуск берётся
//!
//! Два пути, и оба ведут к тому, что включение политики никого не отрезает:
//!
//! * **Выдача знакомым.** При каждом подключении клиент сверяет книгу
//!   отношений с [`Access::granted`] и выдаёт пропуск каждому контакту и
//!   одобренному, у кого его ещё нет. Пропуск уезжает служебным сообщением
//!   внутри шифрованного канала — сервер его не видит.
//! * **Ссылка-приглашение.** Пропуск можно выпустить отдельно, со сроком и
//!   признаком одноразовости, и передать любым способом. Так к вам пишет тот,
//!   кто ещё не знаком.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::crypto::random_bytes;
use crate::keys::{device_cert_message, verify, KEY_LEN, SIG_LEN};

const PASS_DOMAIN: &str = "valanium-pass-v1";
pub const PASS_LEN: usize = 32;

/// Кому позволено писать.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Policy {
    /// Любой, кто знает адрес. Значение по умолчанию: попасть в мессенджер
    /// можно только по приглашению, и запирать переписку ещё раз незачем.
    #[default]
    Everyone,
    /// Только предъявившие пропуск.
    Passes,
}

/// Выпущенная ссылка-приглашение.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invite {
    /// Сам секрет: без него ссылку не пересобрать и не показать повторно.
    pub pass: String,
    pub hash: String,
    #[serde(default)]
    pub label: Option<String>,
    pub one_time: bool,
    /// 0 — бессрочно.
    pub ttl_sec: u64,
    pub created_at: i64,
}

impl Invite {
    /// Ссылка в том виде, в каком её передают человеку.
    pub fn link(&self) -> String {
        format!("valanium://invite/{}", self.pass)
    }
}

/// Всё, что нужно знать о доступе. Лежит в запечатанной базе.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Access {
    pub policy: Policy,
    /// Кому мы уже выдали пропуск: устройство → хеш, чтобы можно было отозвать.
    pub granted: BTreeMap<String, String>,
    /// Пропуска, выданные нам: устройство собеседника → секрет.
    pub held: BTreeMap<String, String>,
    pub invites: Vec<Invite>,
}

/// Хеш, который уезжает на сервер. Должен совпадать с серверным до байта.
pub fn pass_hash(pass: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(PASS_DOMAIN.as_bytes());
    hasher.update(pass);
    hasher.finalize().into()
}

/// Новый пропуск: секрет и его хеш, оба в hex.
pub fn new_pass() -> (String, String) {
    let pass = random_bytes(PASS_LEN);
    (hex::encode(&pass), hex::encode(pass_hash(&pass)))
}

impl Access {
    /// Кому ещё не выдан пропуск из тех, кому он полагается.
    ///
    /// Сверка идёт при каждом подключении, поэтому включение политики не
    /// отрезает уже знакомых: они получают пропуска тем же заходом. Это же
    /// чинит и пропущенную выдачу, если в прошлый раз связь оборвалась.
    pub fn missing_grants<'a>(
        &self,
        peers: impl Iterator<Item = &'a String>,
    ) -> Vec<String> {
        peers
            .filter(|device| !self.granted.contains_key(*device))
            .cloned()
            .collect()
    }

    pub fn remember_grant(&mut self, device: &str, hash: &str) {
        self.granted.insert(device.to_owned(), hash.to_owned());
    }

    /// Забирает пропуск обратно. Возвращает хеш, который нужно отозвать.
    pub fn take_grant(&mut self, device: &str) -> Option<String> {
        self.granted.remove(device)
    }

    pub fn hold(&mut self, device: &str, pass: &str) {
        self.held.insert(device.to_owned(), pass.to_owned());
    }

    /// Что предъявлять при подключении: пары «получатель → секрет».
    pub fn to_present(&self) -> Vec<(String, String)> {
        self.held.iter().map(|(device, pass)| (device.clone(), pass.clone())).collect()
    }
}

/// Служебное сообщение внутри шифрованного канала.
///
/// Префикс отличается от того, которым пользуется интерфейс, поэтому такое
/// сообщение до него не доходит и в переписке не появляется. Ядро разбирает его
/// само и наружу не отдаёт — человеку показывать нечего.
const CONTROL_PREFIX: &str = "\u{2063}VALCTL1:";

pub fn pass_gift(pass: &str) -> String {
    format!("{CONTROL_PREFIX}{{\"pass\":\"{pass}\"}}")
}

/// Просьба удалить сообщения и у собеседника.
///
/// Именно просьба: выполнит её клиент собеседника, а не мы. Обещать большего
/// нельзя — копия уже у него, и запретить её сохранить мы не в силах.
pub fn delete_request(ids: &[String]) -> String {
    let list = serde_json::to_string(ids).unwrap_or_else(|_| "[]".into());
    format!("{CONTROL_PREFIX}{{\"delete\":{list}}}")
}

/// Просьба заменить у собеседника тело ранее отправленного сообщения.
///
/// Как и удаление у обоих, это именно просьба: выполнит её его клиент, а
/// проверить исполнение невозможно. Старое тело у него уже было — правка не
/// отменяет того, что он прочитал.
pub fn edit_request(id: &str, body: &str) -> String {
    let payload = serde_json::json!({ "edit": { "id": id, "body": body } });
    format!("{CONTROL_PREFIX}{payload}")
}

/// Ключ, которым открывается наш аватар.
///
/// Уезжает тем же шифрованным каналом, что и пропуск, и по той же причине: это
/// то, что собеседнику нужно от нас получить, а серверу видеть незачем.
pub fn profile_key_gift(key_hex: &str) -> String {
    format!("{CONTROL_PREFIX}{{\"profileKey\":\"{key_hex}\"}}")
}

/// Больше устройств у одной личности сервер и не заводит.
///
/// Предел нужен и здесь: объявление приходит от собеседника, а каждое
/// устройство в нём — отдельный конверт при каждой отправке. Без предела
/// собеседник объявляет тысячу устройств и заставляет нас шифровать тысячу
/// копий каждого сообщения.
pub const MAX_DEVICES: usize = 8;

/// Свой список устройств — собеседнику, по уже установленному каналу.
///
/// # Почему не через сервер
///
/// Отправитель шифрует каждому устройству отдельно, поэтому список решает, кто
/// получит копию. Спроси мы его у сервера — сервер вписал бы туда своё
/// устройство и получал бы открытые копии переписки: сквозное шифрование
/// формально не нарушено, просто получателей стало на одного больше, и заметить
/// это неоткуда.
///
/// Здесь список приходит от самого человека, внутри его же шифрованного канала,
/// и каждая строка несёт подпись личности под парой (личность, устройство) — ту
/// самую, которой устройство доказывало право войти. Сервер о составе не
/// заявляет ничего, и подделывать ему нечего.
///
/// Спросить список у сервера нельзя ещё и по другой причине: чтобы проверить
/// подпись, нужен ключ личности, а он — постоянный опознаватель, переживающий и
/// смену устройств, и смену юзернейма, и блокировку. Отдавать такое любому, кто
/// набрал имя в поиске, нельзя.
pub fn devices_announce(identity_pub: &[u8], devices: &[([u8; KEY_LEN], [u8; SIG_LEN])]) -> String {
    let list: Vec<serde_json::Value> = devices
        .iter()
        .take(MAX_DEVICES)
        .map(|(device, cert)| serde_json::json!({
            "device": hex::encode(device),
            "cert": hex::encode(cert),
        }))
        .collect();
    let payload = serde_json::json!({
        "devices": { "identity": hex::encode(identity_pub), "list": list },
    });
    format!("{CONTROL_PREFIX}{payload}")
}

/// «Печатает» и «перестал печатать».
///
/// Едет тем же шифрованным каналом, что и сообщения: сервер видит очередной
/// непрозрачный конверт и о наборе текста не узнаёт ничего.
pub fn typing_signal(active: bool) -> String {
    format!("{CONTROL_PREFIX}{{\"typing\":{active}}}")
}

/// «Я в сети».
///
/// Отправляется при подключении тем, кому это разрешено правилом. Обратного
/// сигнала «вышел» нет и быть не может: связь рвётся без предупреждения, и
/// отправить его в этот момент уже нечем. Поэтому получатель считает
/// присутствие устаревающим — «в сети» означает «объявился недавно», а не
/// «прямо сейчас держит соединение». Обещать второе значило бы врать.
pub fn presence_signal() -> String {
    format!("{CONTROL_PREFIX}{{\"online\":true}}")
}

/// Описание группы: название, вид и владелец.
///
/// Едет обычным шифрованным сообщением внутри самой группы: в приглашении MLS
/// места под название нет, а придумать его получатель не может.
pub fn group_signal(title: &str, kind: &str, owner: &str) -> String {
    let body = serde_json::json!({ "group": { "title": title, "kind": kind, "owner": owner } });
    format!("{CONTROL_PREFIX}{body}")
}

/// Название группы приходит от собеседника, а показывается в нашем окне.
///
/// Управляющие символы и переключатели направления письма выбрасываются, длина
/// обрезается: имя в экран длиной или с U+202E внутри — это уже не название, а
/// способ выдать чужую группу за системное окно. Разметку здесь не экранируем:
/// это работа того, кто рисует, и клиенты её делают. Здесь — только то, что
/// осмысленно для любого получателя, чем бы он ни рисовал.
pub const MAX_GROUP_TITLE: usize = 64;

fn clean_title(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(*c,
                    '\u{200e}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .collect();
    cleaned.trim().chars().take(MAX_GROUP_TITLE).collect()
}

/// Что было в служебном сообщении.
#[derive(Debug, PartialEq, Eq)]
pub enum Control {
    Pass(String),
    /// Ключ от аватара собеседника.
    ProfileKey(String),
    Delete(Vec<String>),
    /// Правка уже отправленного: тот же логический идентификатор, новое тело.
    Edit { id: String, body: String },
    Typing(bool),
    Online,
    Group { title: String, kind: String, owner: String },
    /// Список устройств собеседника — **ещё не проверенный**.
    ///
    /// Проверка требует знать, от какого устройства сообщение пришло, а разбор
    /// служебных сообщений этого не знает. Поэтому разбор и доверие разведены:
    /// здесь лежит только разобранное, а годным его делает [`Announcement::accept`].
    Devices(Announcement),
}

/// Объявление о своих устройствах до проверки подписей.
///
/// Отдельный тип, а не готовый список, именно затем, чтобы непроверенное
/// нельзя было взять по ошибке: разобранные байты сами по себе не значат
/// ничего, пока не сойдутся подписи.
#[derive(Debug, PartialEq, Eq)]
pub struct Announcement {
    identity: [u8; KEY_LEN],
    entries: Vec<([u8; KEY_LEN], [u8; SIG_LEN])>,
}

impl Announcement {
    /// Проверяет объявление, пришедшее от устройства `from`.
    ///
    /// Возвращает список устройств, которым можно писать, или `None`, если
    /// верить объявлению нельзя.
    ///
    /// Проверяется двоё.
    ///
    /// **Подпись каждой строки.** Без неё список — просто чужие слова: кто
    /// угодно приписал бы к нему своё устройство и получал копии. Строки, чья
    /// подпись не сходится, выбрасываются молча — жаловаться человеку не на
    /// что, починить он это не может.
    ///
    /// **Присутствие отправителя в списке.** Иначе собеседник объявляет список,
    /// в котором его самого нет, и переписка целиком уезжает к тому, кто в нём
    /// перечислен. Список, не содержащий того, кто его прислал, — не «мои
    /// устройства», а перенаправление.
    pub fn accept(self, from: &[u8; KEY_LEN]) -> Option<Vec<[u8; KEY_LEN]>> {
        let identity = self.identity;
        let devices: Vec<[u8; KEY_LEN]> = self
            .entries
            .into_iter()
            .filter(|(device, cert)| verify(cert, &device_cert_message(&identity, device), &identity))
            .map(|(device, _)| device)
            .collect();

        if !devices.contains(from) {
            return None;
        }
        Some(devices)
    }
}

pub fn parse_signal(body: &str) -> Option<Control> {
    let payload = body.strip_prefix(CONTROL_PREFIX)?;
    let value: serde_json::Value = serde_json::from_str(payload).ok()?;

    if let Some(pass) = value.get("pass").and_then(|v| v.as_str()) {
        // Длину проверяем здесь: мусор в этом поле стоил бы отказа при каждом
        // подключении, а починить его человек не смог бы — он его не видит.
        if pass.len() != PASS_LEN * 2 || hex::decode(pass).is_err() {
            return None;
        }
        return Some(Control::Pass(pass.to_owned()));
    }
    if let Some(key) = value.get("profileKey").and_then(|v| v.as_str()) {
        // Длина проверяется здесь по той же причине, что и у пропуска: мусор в
        // этом поле человек не видит и починить не может.
        if key.len() != 64 || hex::decode(key).is_err() {
            return None;
        }
        return Some(Control::ProfileKey(key.to_owned()));
    }
    if let Some(edit) = value.get("edit") {
        // Обе половины обязательны: правка без тела или без адресата — это
        // мусор, а не команда, и применять из него нечего.
        let (Some(id), Some(body)) = (
            edit.get("id").and_then(|v| v.as_str()),
            edit.get("body").and_then(|v| v.as_str()),
        ) else {
            return None;
        };
        if id.is_empty() || body.is_empty() {
            return None;
        }
        return Some(Control::Edit { id: id.to_owned(), body: body.to_owned() });
    }
    if let Some(ids) = value.get("delete").and_then(|v| v.as_array()) {
        return Some(Control::Delete(
            ids.iter().filter_map(|id| id.as_str().map(str::to_owned)).collect(),
        ));
    }
    if let Some(active) = value.get("typing").and_then(|v| v.as_bool()) {
        return Some(Control::Typing(active));
    }
    if value.get("online").and_then(|v| v.as_bool()) == Some(true) {
        return Some(Control::Online);
    }
    if let Some(devices) = value.get("devices") {
        let identity = fixed::<KEY_LEN>(devices.get("identity")?.as_str()?)?;
        let list = devices.get("list")?.as_array()?;
        // Длиннее предела — не разбираем вовсе: объявление на тысячу устройств
        // это не «много устройств», а попытка нагрузить нас шифрованием.
        if list.len() > MAX_DEVICES {
            return None;
        }
        let mut entries = Vec::with_capacity(list.len());
        for item in list {
            entries.push((
                fixed::<KEY_LEN>(item.get("device")?.as_str()?)?,
                fixed::<SIG_LEN>(item.get("cert")?.as_str()?)?,
            ));
        }
        return Some(Control::Devices(Announcement { identity, entries }));
    }
    if let Some(group) = value.get("group") {
        return Some(Control::Group {
            title: clean_title(group.get("title")?.as_str()?),
            kind: group.get("kind")?.as_str()?.to_string(),
            owner: group.get("owner")?.as_str()?.to_string(),
        });
    }
    None
}

/// Hex ровно нужной длины — или ничего.
///
/// Длина проверяется до разбора: `hex::decode` принял бы и три байта, и
/// тридцать, а ключ неверной длины дальше по коду означал бы либо панику, либо
/// молчаливое сравнение с чем-то не тем.
fn fixed<const N: usize>(raw: &str) -> Option<[u8; N]> {
    if raw.len() != N * 2 {
        return None;
    }
    hex::decode(raw).ok()?.try_into().ok()
}

pub fn is_control(body: &str) -> bool {
    body.starts_with(CONTROL_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_group_title_from_a_peer_cannot_forge_the_window() {
        // Название рисуют в заголовке рядом с нашими собственными надписями.
        // Перевод строки, U+202E и длина в экран — не имя, а подделка окна.
        let hostile = format!("Совет\u{202e}\n\u{7}безопасности {}", "и".repeat(200));
        let signal = group_signal(&hostile, "group", "me");
        let Some(Control::Group { title, .. }) = parse_signal(&signal) else {
            panic!("сигнал о группе обязан разбираться");
        };
        assert!(title.chars().count() <= MAX_GROUP_TITLE, "длина не обрезана: {title:?}");
        assert!(!title.contains('\u{202e}'), "переключатель направления уцелел");
        assert!(!title.chars().any(char::is_control), "управляющие символы уцелели");
    }

    #[test]
    fn an_ordinary_group_title_survives_untouched() {
        let signal = group_signal("  Разработка  ", "group", "me");
        let Some(Control::Group { title, .. }) = parse_signal(&signal) else {
            panic!("сигнал о группе обязан разбираться");
        };
        assert_eq!(title, "Разработка");
    }

    #[test]
    fn a_profile_key_travels_and_comes_back_whole() {
        let key = "ab".repeat(32);
        let signal = profile_key_gift(&key);
        assert!(is_control(&signal), "ключ обязан ехать служебным сообщением");
        assert_eq!(parse_signal(&signal), Some(Control::ProfileKey(key.clone())));
    }

    #[test]
    fn a_profile_key_of_the_wrong_shape_is_ignored() {
        // Мусор в этом поле человек не видит и починить не может, поэтому
        // разбирается он здесь и молча.
        for bad in ["", "короткий", &"zz".repeat(32), &"ab".repeat(31)] {
            let signal = format!("{CONTROL_PREFIX}{{\"profileKey\":\"{bad}\"}}");
            assert_eq!(parse_signal(&signal), None, "принят негодный ключ: {bad}");
        }
    }

    #[test]
    fn a_pass_is_random_and_its_hash_is_stable() {
        let (first, first_hash) = new_pass();
        let (second, _) = new_pass();
        assert_ne!(first, second, "пропуска обязаны быть разными");
        assert_eq!(first.len(), PASS_LEN * 2);
        assert_eq!(hex::encode(pass_hash(&hex::decode(&first).unwrap())), first_hash);
    }

    /// Хеш не должен быть пропуском в другом виде: сервер хранит именно его.
    #[test]
    fn the_hash_hides_the_pass() {
        let (pass, hash) = new_pass();
        assert_ne!(pass, hash);
    }

    #[test]
    fn grants_are_tracked_so_they_are_not_issued_twice() {
        let mut access = Access::default();
        let peers = vec!["aa".to_string(), "bb".to_string()];

        assert_eq!(access.missing_grants(peers.iter()).len(), 2);
        access.remember_grant("aa", "хеш");
        assert_eq!(access.missing_grants(peers.iter()), vec!["bb".to_string()]);

        access.remember_grant("bb", "хеш2");
        assert!(access.missing_grants(peers.iter()).is_empty());
    }

    /// Отзыв возвращает хеш: без него серверу нечего было бы сказать.
    #[test]
    fn taking_a_grant_back_yields_the_hash_to_revoke() {
        let mut access = Access::default();
        access.remember_grant("aa", "хеш");
        assert_eq!(access.take_grant("aa").as_deref(), Some("хеш"));
        assert_eq!(access.take_grant("aa"), None);
    }

    #[test]
    fn held_passes_are_presented_per_recipient() {
        let mut access = Access::default();
        access.hold("aa", "секрет-а");
        access.hold("bb", "секрет-б");

        let mut presented = access.to_present();
        presented.sort();
        assert_eq!(presented, vec![
            ("aa".to_string(), "секрет-а".to_string()),
            ("bb".to_string(), "секрет-б".to_string()),
        ]);
    }

    /// Служебное сообщение не должно доходить до интерфейса.
    #[test]
    fn a_gift_round_trips_and_is_recognisable() {
        let (pass, _) = new_pass();
        let body = pass_gift(&pass);

        assert!(is_control(&body));
        assert_eq!(parse_signal(&body), Some(Control::Pass(pass)));
    }

    #[test]
    fn a_delete_request_round_trips() {
        let ids = vec!["один".to_string(), "два".to_string()];
        assert_eq!(parse_signal(&delete_request(&ids)), Some(Control::Delete(ids)));
    }

    #[test]
    fn typing_round_trips_both_ways() {
        assert_eq!(parse_signal(&typing_signal(true)), Some(Control::Typing(true)));
        assert_eq!(parse_signal(&typing_signal(false)), Some(Control::Typing(false)));
    }

    #[test]
    fn presence_round_trips() {
        assert_eq!(parse_signal(&presence_signal()), Some(Control::Online));
    }

    /// Служебные сообщения не должны путаться между собой.
    #[test]
    fn signals_do_not_collide() {
        let (pass, _) = new_pass();
        assert!(matches!(parse_signal(&pass_gift(&pass)), Some(Control::Pass(_))));
        assert!(matches!(parse_signal(&typing_signal(true)), Some(Control::Typing(_))));
        assert!(matches!(parse_signal(&delete_request(&[])), Some(Control::Delete(_))));
        assert!(matches!(parse_signal(&presence_signal()), Some(Control::Online)));
    }

    #[test]
    fn ordinary_text_is_not_control() {
        for body in ["привет", "", "VALCTL1:{}", "\u{2063}OBS1:{\"type\":\"text\"}"] {
            assert!(!is_control(body), "принято за служебное: {body}");
            assert!(parse_signal(body).is_none());
        }
    }

    /// Мусор в служебном поле человек не увидит и не починит — отвергаем молча.
    #[test]
    fn a_malformed_gift_is_refused() {
        for bad in [
            "\u{2063}VALCTL1:не json",
            "\u{2063}VALCTL1:{\"pass\":\"коротко\"}",
            "\u{2063}VALCTL1:{\"pass\":\"ZZ\"}",
            "\u{2063}VALCTL1:{}",
        ] {
            assert!(is_control(bad), "префикс обязан распознаваться: {bad}");
            assert!(parse_signal(bad).is_none(), "принят мусор: {bad}");
        }
    }

    #[test]
    fn an_invite_becomes_a_link() {
        let (pass, hash) = new_pass();
        let invite = Invite {
            pass: pass.clone(),
            hash,
            label: Some("для Миры".into()),
            one_time: true,
            ttl_sec: 3600,
            created_at: 0,
        };
        assert_eq!(invite.link(), format!("valanium://invite/{pass}"));
    }

    /// Запись прошлой версии обязана подниматься с безопасным значением.
    #[test]
    fn an_older_record_loads_as_everyone() {
        let access: Access = serde_json::from_str("{}").unwrap();
        assert_eq!(access.policy, Policy::Everyone);
        assert!(access.granted.is_empty());
        assert!(access.held.is_empty());
    }

    #[test]
    fn access_round_trips() {
        let mut access = Access::default();
        access.policy = Policy::Passes;
        access.remember_grant("aa", "хеш");
        access.hold("bb", "секрет");

        let text = serde_json::to_string(&access).unwrap();
        assert_eq!(serde_json::from_str::<Access>(&text).unwrap(), access);
    }

    #[test]
    fn an_edit_request_survives_a_round_trip() {
        let body = edit_request("m-1", "исправленный текст");
        assert_eq!(
            parse_signal(&body),
            Some(Control::Edit { id: "m-1".into(), body: "исправленный текст".into() })
        );
    }

    #[test]
    fn half_an_edit_is_not_a_command() {
        // Без тела или без адресата применять нечего — это мусор, а не правка.
        for payload in [
            r#"{"edit":{"id":"m-1"}}"#,
            r#"{"edit":{"body":"текст"}}"#,
            r#"{"edit":{"id":"","body":"текст"}}"#,
            r#"{"edit":{"id":"m-1","body":""}}"#,
            r#"{"edit":"строка"}"#,
        ] {
            let body = format!("{CONTROL_PREFIX}{payload}");
            assert_eq!(parse_signal(&body), None, "принято: {payload}");
        }
    }

    // --- список устройств -----------------------------------------------------

    use crate::keys::Credentials;

    /// Пара «устройство + подпись личности под ним», как её видит объявление.
    fn signed(identity: &Credentials, device: &Credentials) -> ([u8; KEY_LEN], [u8; SIG_LEN]) {
        let device_pub = device.device_pub();
        let cert = identity
            .identity
            .sign(&device_cert_message(&identity.identity_pub(), &device_pub));
        (device_pub, cert)
    }

    fn announced(identity: &Credentials, devices: &[([u8; KEY_LEN], [u8; SIG_LEN])]) -> Announcement {
        let body = devices_announce(&identity.identity_pub(), devices);
        match parse_signal(&body) {
            Some(Control::Devices(announcement)) => announcement,
            other => panic!("объявление не разобралось: {other:?}"),
        }
    }

    #[test]
    fn a_catalogue_survives_the_round_trip() {
        let alice = Credentials::generate();
        let phone = signed(&alice, &alice);
        let laptop = signed(&alice, &Credentials::generate());

        let accepted = announced(&alice, &[phone, laptop])
            .accept(&phone.0)
            .expect("свой же список обязан приниматься");
        assert_eq!(accepted, vec![phone.0, laptop.0]);
    }

    #[test]
    fn a_forged_entry_is_dropped_and_the_rest_survives() {
        /*
          Ровно то, ради чего подписи здесь и нужны. Тот, кто пересылает
          объявление, дописывает в него своё устройство — и начал бы получать
          копии всей переписки. Подпись он подделать не может: приватного ключа
          личности у него нет.

          Остальные строки при этом обязаны уцелеть: иначе одна испорченная
          строка отключала бы доставку целиком.
        */
        let alice = Credentials::generate();
        let mallory = Credentials::generate();
        let phone = signed(&alice, &alice);
        let laptop = signed(&alice, &Credentials::generate());
        // Подпись своя, но под чужой личностью — то есть не годится.
        let intruder = signed(&mallory, &mallory);

        let accepted = announced(&alice, &[phone, laptop, intruder])
            .accept(&phone.0)
            .expect("испорченная строка не должна ронять весь список");
        assert_eq!(accepted, vec![phone.0, laptop.0], "чужое устройство просочилось");
    }

    #[test]
    fn a_list_without_its_own_sender_is_refused() {
        /*
          Список, не содержащий того, кто его прислал, — это не «мои
          устройства», а перенаправление: приняв его, мы перестали бы писать
          самому собеседнику и слали бы всё тем, кого он перечислил.

          Подписи тут не спасают: перечисленные устройства могут быть вполне
          настоящими устройствами другого человека.
        */
        let alice = Credentials::generate();
        let laptop = signed(&alice, &Credentials::generate());
        let stranger = Credentials::generate().device_pub();

        assert_eq!(announced(&alice, &[laptop]).accept(&stranger), None);
    }

    #[test]
    fn a_cert_does_not_travel_between_devices() {
        // Подпись считается по паре (личность, устройство). Взять настоящую
        // подпись одного устройства и приложить её к другому не выйдет.
        let alice = Credentials::generate();
        let phone = signed(&alice, &alice);
        let other = Credentials::generate().device_pub();

        let accepted = announced(&alice, &[phone, (other, phone.1)])
            .accept(&phone.0)
            .expect("своё устройство на месте");
        assert_eq!(accepted, vec![phone.0], "переставленная подпись прошла");
    }

    #[test]
    fn an_oversized_list_is_not_parsed_at_all() {
        // Каждое устройство — отдельный конверт при каждой отправке. Объявление
        // на сотню устройств это не «много устройств», а попытка заставить нас
        // шифровать сотню копий каждого сообщения.
        let alice = Credentials::generate();
        let mut devices = Vec::new();
        for _ in 0..MAX_DEVICES + 1 {
            devices.push(signed(&alice, &Credentials::generate()));
        }
        // Собираем объявление в обход `devices_announce`: он сам обрезает по
        // пределу, а проверить надо приёмную сторону.
        let list: Vec<serde_json::Value> = devices
            .iter()
            .map(|(device, cert)| serde_json::json!({
                "device": hex::encode(device),
                "cert": hex::encode(cert),
            }))
            .collect();
        let payload = serde_json::json!({
            "devices": { "identity": hex::encode(alice.identity_pub()), "list": list },
        });
        assert_eq!(parse_signal(&format!("{CONTROL_PREFIX}{payload}")), None);
    }

    #[test]
    fn a_malformed_catalogue_is_refused() {
        let alice = hex::encode(Credentials::generate().identity_pub());
        let key = hex::encode([7u8; KEY_LEN]);
        let cert = hex::encode([7u8; SIG_LEN]);
        for payload in [
            // Ключ не той длины: дальше по коду он означал бы сравнение не с тем.
            format!(r#"{{"devices":{{"identity":"{alice}","list":[{{"device":"aa","cert":"{cert}"}}]}}}}"#),
            // Подпись не той длины.
            format!(r#"{{"devices":{{"identity":"{alice}","list":[{{"device":"{key}","cert":"bb"}}]}}}}"#),
            // Не hex вовсе.
            format!(r#"{{"devices":{{"identity":"{alice}","list":[{{"device":"{key}","cert":"{}"}}]}}}}"#, "z".repeat(SIG_LEN * 2)),
            // Личность не той длины.
            format!(r#"{{"devices":{{"identity":"ff","list":[]}}}}"#),
            // Нет списка вовсе.
            format!(r#"{{"devices":{{"identity":"{alice}"}}}}"#),
            // Строка без подписи.
            format!(r#"{{"devices":{{"identity":"{alice}","list":[{{"device":"{key}"}}]}}}}"#),
        ] {
            assert_eq!(
                parse_signal(&format!("{CONTROL_PREFIX}{payload}")),
                None,
                "принят мусор: {payload}",
            );
        }
    }
}
