//! Sending a reply into Telegram (0.10.0-alpha.75): the one the user
//! approved into the chat the message it answers came from, as them, the
//! way `sending::teams` sends into Teams: shown first (`envelope_for`),
//! recorded in `outbox` before Telegram is asked, as plain text, `unsure`
//! when Telegram's answer was lost, only through the account's own session
//! and only while Telegram says it is the account connected
//! (`still_theirs`); decided as sent and put in the chat under the id
//! Telegram gave it (`Db::outgoing_sent_as`), which the next check reads it
//! back by.

use serde_json::{json, Value};

use super::{answered, record_sent_draft, ReplyEnvelope, ReplyKind, SendError, SendRequest, SendingMailbox};
use super::{DRAFT_GONE, EMPTY, HELD};
use crate::db::{Db, DbError, Draft, IdentifierInput, IdentifierKind, Message, NewOutgoing, OutboxEntry, SendHow};
use crate::db::{ALREADY_DECIDED, MAY_HAVE_GONE as MAIL_MAY_HAVE_GONE};
use crate::import::Importer;
use crate::sources::telegram_account::{
    self as telegram, account_id, ChatRef, PostError, TelegramError, TelegramPost, TelegramSourceConfig,
};
use crate::sources::{AuthorRef, DiscoveredConversation, RawMessage};

pub const NOT_THIS_ACCOUNT: &str = "I only send a reply into the Telegram chat the message you're answering came from.";
pub const NO_CHAT: &str =
    "I don't know which Telegram chat this came from, so I can't send it there. Use this copies it.";
pub const ACCOUNT_GONE: &str = "That Telegram account isn't connected any more.";
pub const UNREADABLE: &str = "I can't read this account's settings; remove it and connect it again.";
pub const NOT_THE_ACCOUNT: &str =
    "This Telegram sign-in isn't the account you connected any more, so I won't send as it. Sign in again under Your mail.";
pub const MAY_HAVE_GONE: &str = "A reply to this may already have gone: Telegram's answer was lost as it was sent. Look in the chat in Telegram. If it isn't there, say so under Your mail → What I've sent, and you can send it again.";
pub const PANICKED: &str = "Something went wrong inside Mimic while sending, so it may or may not have gone. Look in the chat in Telegram before sending it again.";

fn account_of(db: &Db, message: &Message) -> Result<Option<crate::db::Source>, DbError> {
    Ok(db.get_source(&message.source_id)?.filter(|s| s.connector == telegram::CONNECTOR))
}

/// Who a reply goes to, as the user reads it: the chat's name.
fn chat_name(db: &Db, draft: &Draft, incoming: &Message) -> Result<String, DbError> {
    if let Some(subject) = db.get_conversation(&incoming.conversation_id)?.and_then(|c| c.subject) {
        if !subject.trim().is_empty() {
            return Ok(subject);
        }
    }
    let person = draft.participant_id.as_deref().or(incoming.participant_id.as_deref());
    Ok(match person {
        Some(p) => db.get_participant(p)?.map(|p| p.display_name).unwrap_or_else(|| "them".into()),
        None => "them".into(),
    })
}

/// The Send panel's envelope for a reply to a message read from Telegram,
/// or `None` for one that wasn't.
pub(super) fn envelope_for(db: &Db, draft: &Draft, incoming: &Message) -> Result<Option<ReplyEnvelope>, SendError> {
    let Some(source) = account_of(db, incoming)? else { return Ok(None) };
    let config = serde_json::from_value::<TelegramSourceConfig>(source.config.clone()).ok();
    let held = db.held_outgoing_for_draft(&draft.id)?;
    let why_not = if draft.outcome.is_some() {
        Some(ALREADY_DECIDED)
    } else if db.unsure_outgoing_for_draft(&draft.id)?.is_some() {
        Some(MAY_HAVE_GONE)
    } else if held.is_some() {
        Some(HELD)
    } else if config.is_none() {
        Some(UNREADABLE)
    } else if ChatRef::of(&incoming.metadata).is_none() {
        Some(NO_CHAT)
    } else {
        None
    };
    Ok(Some(ReplyEnvelope {
        draft_id: draft.id.clone(),
        mailboxes: vec![SendingMailbox {
            source_id: source.id.clone(),
            name: source.name,
            from_address: config.as_ref().map(|c| format!("{} on Telegram", c.name)),
            server: Some("Telegram".into()),
            why_not: config.is_none().then(|| UNREADABLE.to_string()),
            signature: None,
        }],
        mailbox: Some(source.id),
        to: vec![chat_name(db, draft, incoming)?],
        other_addresses: Vec::new(),
        others_on_it: Vec::new(),
        subject: String::new(),
        why_not: why_not.map(str::to_string),
        held,
        kind: ReplyKind::Telegram,
    }))
}

fn worded(e: DbError) -> SendError {
    match e {
        DbError::Invalid(m) if m == MAIL_MAY_HAVE_GONE => SendError::Cannot(MAY_HAVE_GONE.into()),
        e => e.into(),
    }
}

/// Send a reply the user approved into the Telegram chat its message came
/// from, exactly as approved, through `telegram` (the account's session).
pub fn send_reply(db: &Db, telegram: &dyn TelegramPost, req: &SendRequest) -> Result<OutboxEntry, SendError> {
    let draft = db.get_draft(&req.draft_id)?.ok_or_else(|| SendError::Cannot(DRAFT_GONE.into()))?;
    if draft.outcome.is_some() {
        return Err(SendError::Cannot(ALREADY_DECIDED.into()));
    }
    if req.text.trim().is_empty() {
        return Err(SendError::Cannot(EMPTY.into()));
    }
    let incoming = answered(db, &draft)?
        .filter(|m| m.source_id == req.source_id)
        .ok_or_else(|| SendError::Cannot(NOT_THIS_ACCOUNT.into()))?;
    let (config, chat) = route(db, &incoming).map_err(|why| SendError::Cannot(why.into()))?;
    still_theirs(telegram, &config)?;
    let entry = db
        .queue_outgoing(&NewOutgoing {
            draft_id: Some(draft.id.clone()),
            source_id: req.source_id.clone(),
            conversation_id: Some(incoming.conversation_id.clone()),
            answers_message_id: Some(incoming.id.clone()),
            participant_id: draft.participant_id.clone().or_else(|| incoming.participant_id.clone()),
            from_address: from_address(&config),
            to: vec![chat_name(db, &draft, &incoming)?],
            cc: Vec::new(),
            subject: String::new(),
            body: req.text.clone(),
            message_id: pending_id(&chat),
            how: SendHow::Approved,
            send_after: None,
        })
        .map_err(worded)?;
    let at = post(db, telegram, &entry, &chat, &req.text)?;
    if let Err(e) = record_sent_draft(db, &draft, &req.text) {
        tracing::warn!(target: "sending", error = %e, "a sent reply's draft could not be marked as sent");
    }
    if let Err(e) = record_in_conversation(db, &entry, &config, &incoming, &req.text, at, false) {
        tracing::warn!(target: "sending", error = %e, "a sent reply could not be put in its conversation");
    }
    Ok(db.outgoing(&entry.id)?.unwrap_or(entry))
}

/// Whether the session is still the account connected.
pub(crate) fn still_theirs(telegram: &dyn TelegramPost, config: &TelegramSourceConfig) -> Result<(), SendError> {
    match telegram.me() {
        Ok(me) if me.id == config.user_id => Ok(()),
        Ok(_) => Err(SendError::Cannot(NOT_THE_ACCOUNT.into())),
        Err(e) => Err(SendError::Service(e.to_string())),
    }
}

/// The account's settings and the chat the message answered came from, or
/// why a reply can't go there.
pub(crate) fn route(db: &Db, incoming: &Message) -> Result<(TelegramSourceConfig, ChatRef), &'static str> {
    let source = account_of(db, incoming).ok().flatten().ok_or(ACCOUNT_GONE)?;
    let config: TelegramSourceConfig = serde_json::from_value(source.config).map_err(|_| UNREADABLE)?;
    let chat = ChatRef::of(&incoming.metadata).ok_or(NO_CHAT)?;
    Ok((config, chat))
}

pub(crate) fn from_address(config: &TelegramSourceConfig) -> String {
    account_id(config.user_id)
}

/// An id for a reply until Telegram names it.
pub(crate) fn pending_id(chat: &ChatRef) -> String {
    format!("telegram:{}:pending-{}", chat.id, crate::ids::new_id())
}

/// Post a reply whose `outbox` row is `sending`, and record how it went:
/// when it went, by Telegram's clock, once it has.
pub(crate) fn post(
    db: &Db,
    telegram: &dyn TelegramPost,
    entry: &OutboxEntry,
    chat: &ChatRef,
    text: &str,
) -> Result<Option<i64>, SendError> {
    let posted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| telegram.post(chat, text)));
    match posted {
        Ok(Ok((id, at))) => {
            db.outgoing_sent_as(&entry.id, &format!("telegram:{}:{id}", chat.id))?;
            Ok(Some(at))
        }
        Ok(Err(PostError::NotSent(e))) => {
            db.outgoing_failed(&entry.id, &e.to_string())?;
            Err(SendError::Service(e.to_string()))
        }
        Ok(Err(e @ PostError::Unsure(_))) => {
            let why = format!("{e} Look in the chat in Telegram before sending it again.");
            db.outgoing_unsure(&entry.id, &why)?;
            Err(SendError::ServiceUnsure(why))
        }
        Err(_) => {
            db.outgoing_unsure(&entry.id, PANICKED)?;
            Err(SendError::ServiceUnsure(PANICKED.into()))
        }
    }
}

/// The id Telegram gave a message read from it.
fn message_number(incoming: &Message) -> Option<i32> {
    let id = incoming
        .metadata
        .get("telegramMessage")
        .and_then(Value::as_i64)
        .or_else(|| incoming.external_id.rsplit(':').next().and_then(|id| id.parse().ok()))?;
    i32::try_from(id).ok()
}

/// What people said in a chat after `incoming`, asked just before a reply
/// goes by itself (0.10.0-alpha.76): `Ok(Err(why))` when Telegram refused
/// to say, as it would again; `Err(why)` for a failure that may pass.
/// Telegram's own lines and bots aren't anyone saying anything.
pub(crate) fn said_since(
    telegram: &dyn TelegramPost,
    chat: &ChatRef,
    incoming: &Message,
) -> Result<Result<super::slack::Since, String>, String> {
    use super::slack::Since;
    let Some(after) = message_number(incoming) else {
        return Ok(Err("I don't know which message this answers in Telegram".into()));
    };
    match telegram.after(chat, after) {
        Ok(said) => {
            let people: Vec<_> = said.iter().filter(|m| !m.notice && !m.bot).collect();
            Ok(Ok(if people.iter().any(|m| m.outgoing) {
                Since::You
            } else if people.is_empty() {
                Since::Nothing
            } else {
                Since::Them
            }))
        }
        Err(e @ (TelegramError::Network(_) | TelegramError::RateLimited)) => Err(e.to_string()),
        Err(e) => Ok(Err(e.to_string())),
    }
}

/// What went, put in its chat as the user's message under the id Telegram
/// gave it, at the time Telegram gave it (else when it went here); one sent
/// by itself marked `sentOnItsOwn`.
pub(crate) fn record_in_conversation(
    db: &Db,
    entry: &OutboxEntry,
    config: &TelegramSourceConfig,
    incoming: &Message,
    text: &str,
    at: Option<i64>,
    on_its_own: bool,
) -> Result<(), String> {
    let sent = db.outgoing(&entry.id).map_err(|e| e.to_string())?.ok_or("its outbox row is gone")?;
    let chat = ChatRef::of(&incoming.metadata).ok_or(NO_CHAT)?;
    let mut metadata = json!({
        "telegramChat": chat.id.to_string(),
        "telegramDirect": incoming.metadata.get("telegramDirect").cloned().unwrap_or(Value::Bool(chat.kind == "user")),
        "telegramKind": chat.kind,
        "attachments": false,
    });
    if let Some(id) = sent.message_id.rsplit(':').next().and_then(|id| id.parse::<i64>().ok()) {
        metadata["telegramMessage"] = json!(id);
    }
    if on_its_own {
        metadata["sentOnItsOwn"] = Value::Bool(true);
    }
    let sent_at = at
        .and_then(|secs| chrono::TimeZone::timestamp_opt(&chrono::Utc, secs, 0).single())
        .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .or(sent.sent_at.clone());
    let mut importer = Importer::begin(db, &entry.source_id).map_err(|e| e.to_string())?;
    let taken = importer.take(DiscoveredConversation {
        external_id: format!("telegram:chat:{}", chat.id),
        subject: None,
        channel: "chat".into(),
        messages: vec![RawMessage {
            external_id: sent.message_id.clone(),
            author: AuthorRef {
                display_name: config.name.clone(),
                identifiers: vec![IdentifierInput::new(IdentifierKind::AccountId, account_id(config.user_id))],
            },
            sent_at,
            body: text.trim().to_string(),
            metadata,
        }],
    });
    if let Err(e) = taken {
        importer.abandon();
        return Err(e.to_string());
    }
    importer.finish().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
pub(crate) mod fake {
    //! A Telegram that keeps what it is told: chats of `Said`, and whose
    //! session it is.
    use std::sync::{Arc, Mutex};

    use crate::sources::telegram_account::{ChatRef, Me, PostError, Said, TelegramError, TelegramPost};

    /// Each chat's messages, by chat id.
    pub type Chats = Arc<Mutex<Vec<(i64, Vec<Said>)>>>;
    /// What the next post does instead of going: `Some(Ok(()))` takes it and
    /// loses the answer; `Some(Err(why))` refuses it.
    pub type NextPost = Arc<Mutex<Option<Result<(), String>>>>;

    #[derive(Clone)]
    pub struct Telegram {
        pub me: Me,
        pub chats: Chats,
        pub next_post: NextPost,
        /// Asking what was said since fails, as Telegram can.
        pub after_fails: Arc<Mutex<Option<TelegramError>>>,
    }

    impl Telegram {
        pub fn new(me: Me, chats: Vec<(i64, Vec<Said>)>) -> Self {
            Telegram {
                me,
                chats: Arc::new(Mutex::new(chats)),
                next_post: Arc::new(Mutex::new(None)),
                after_fails: Arc::new(Mutex::new(None)),
            }
        }

        pub fn said_in(&self, chat: i64) -> Vec<Said> {
            self.chats.lock().unwrap().iter().find(|(id, _)| *id == chat).map(|(_, m)| m.clone()).unwrap_or_default()
        }
    }

    impl TelegramPost for Telegram {
        fn me(&self) -> Result<Me, TelegramError> {
            Ok(self.me.clone())
        }
        fn post(&self, chat: &ChatRef, text: &str) -> Result<(i32, i64), PostError> {
            let outcome = self.next_post.lock().unwrap().take();
            if let Some(Err(why)) = &outcome {
                return Err(PostError::NotSent(TelegramError::Refused(why.clone())));
            }
            let mut chats = self.chats.lock().unwrap();
            let Some((_, messages)) = chats.iter_mut().find(|(id, _)| *id == chat.id) else {
                return Err(PostError::NotSent(TelegramError::Refused("PEER_ID_INVALID".into())));
            };
            let last = messages.last().map(|m| (m.id, m.at)).unwrap_or((0, 1_770_110_000));
            let id = last.0 + 1;
            let at = last.1 + 60;
            messages.push(Said { id, at, text: text.to_string(), outgoing: true, ..Default::default() });
            match outcome {
                Some(Ok(())) => Err(PostError::Unsure("its answer didn't come in time".into())),
                _ => Ok((id, at)),
            }
        }
        fn after(&self, chat: &ChatRef, after: i32) -> Result<Vec<Said>, TelegramError> {
            if let Some(e) = self.after_fails.lock().unwrap().take() {
                return Err(e);
            }
            Ok(self.said_in(chat.id).into_iter().filter(|m| m.id > after).collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{SendHow, SendStatus};
    use crate::sources::telegram_account::{check, new_source, Chat, Fetched, Me, Said};

    struct World {
        db: Db,
        source: String,
        telegram: fake::Telegram,
        draft: Draft,
    }

    fn me() -> Me {
        Me { id: 7, name: "C Haynes".into(), username: Some("c".into()) }
    }

    fn ada_said(id: i32, minutes: i64, text: &str) -> Said {
        Said {
            id,
            at: 1_770_110_000 + minutes * 60,
            text: text.into(),
            from: Some((4242, "Ada Lovelace".into())),
            ..Default::default()
        }
    }

    /// An account read once: Ada's question waits, and a draft is ready.
    fn world() -> World {
        let db = Db::open_in_memory().unwrap();
        db.set_setting(crate::db::WITHIN_DAYS_SETTING, &0).unwrap();
        db.set_user_identity("C").unwrap();
        db.add_user_identifier(IdentifierKind::AccountId, &account_id(7)).unwrap();
        let source = db.create_source(&new_source(&me()).unwrap()).unwrap().id;
        let question = ada_said(1, 0, "lunch on thursday?");
        let telegram = fake::Telegram::new(me(), vec![(4242, vec![question.clone()])]);
        let chat = Chat {
            id: 4242,
            title: "Ada Lovelace".into(),
            direct: true,
            kind: "user".into(),
            newest: 1,
            messages: vec![question],
            complete: true,
        };
        check(&db, &source, Ok(Fetched { me: Some(me()), chats: vec![chat], ..Default::default() })).unwrap();
        let (message, conversation, participant): (String, String, Option<String>) = db
            .conn()
            .query_row(
                "SELECT id, conversation_id, participant_id FROM messages WHERE external_id = 'telegram:4242:1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        let draft = db
            .create_draft(&crate::db::NewDraft {
                participant_id: participant,
                conversation_id: Some(conversation),
                channel: "chat".into(),
                situation_id: None,
                incoming_message: Some("lunch on thursday?".into()),
                incoming_message_id: Some(message),
                intent: None,
                generated_text: "yes! the place by the river?".into(),
                provider: "mock".into(),
                model: "m".into(),
                context: json!({}),
                prompt_hash: "h".into(),
                evidence: json!({}),
                alternative_to: None,
            })
            .unwrap();
        World { db, source, telegram, draft }
    }

    fn request(w: &World, text: &str) -> SendRequest {
        SendRequest {
            draft_id: w.draft.id.clone(),
            source_id: w.source.clone(),
            to: vec!["Ada Lovelace".into()],
            cc: Vec::new(),
            subject: String::new(),
            text: text.into(),
        }
    }

    #[test]
    fn a_reply_goes_into_the_chat_it_answers_and_is_read_back_as_the_same_message() {
        let w = world();
        let envelope = super::super::envelope_for(&w.db, &w.draft.id).unwrap();
        assert_eq!(envelope.kind, ReplyKind::Telegram);
        assert_eq!(envelope.to, vec!["Ada Lovelace".to_string()]);
        assert_eq!(envelope.why_not, None);

        let sent = send_reply(&w.db, &w.telegram, &request(&w, "yes! the place by the river?")).unwrap();
        assert_eq!((sent.status, sent.how), (SendStatus::Sent, SendHow::Approved));
        assert_eq!(sent.message_id, "telegram:4242:2");
        assert_eq!(sent.from_address, "telegram:user7");
        let said = w.telegram.said_in(4242);
        assert_eq!((said[1].text.as_str(), said[1].outgoing), ("yes! the place by the river?", true));
        let draft = w.db.get_draft(&w.draft.id).unwrap().unwrap();
        assert!(draft.outcome.is_some());

        // The next check reads it back: the same message, the user's.
        let back = Chat {
            id: 4242,
            title: "Ada Lovelace".into(),
            direct: true,
            kind: "user".into(),
            newest: 2,
            messages: vec![Said {
                id: 2,
                at: said[1].at,
                text: said[1].text.clone(),
                outgoing: true,
                ..Default::default()
            }],
            complete: true,
        };
        let summary =
            check(&w.db, &w.source, Ok(Fetched { me: Some(me()), chats: vec![back], ..Default::default() })).unwrap();
        assert_eq!((summary.inserted, summary.duplicates), (0, 1), "{summary:?}");
        let mine: i64 =
            w.db.conn().query_row("SELECT count(*) FROM messages WHERE direction = 'self'", [], |r| r.get(0)).unwrap();
        assert_eq!(mine, 1);
    }

    #[test]
    fn a_post_telegram_refuses_fails_and_a_lost_answer_is_unsure() {
        let w = world();
        *w.telegram.next_post.lock().unwrap() = Some(Err("CHAT_WRITE_FORBIDDEN".into()));
        let err = send_reply(&w.db, &w.telegram, &request(&w, "yes")).unwrap_err();
        assert!(matches!(err, SendError::Service(_)), "{err:?}");
        assert_eq!(w.db.recent_outgoing(5).unwrap()[0].status, SendStatus::Failed);

        let w = world();
        *w.telegram.next_post.lock().unwrap() = Some(Ok(()));
        let err = send_reply(&w.db, &w.telegram, &request(&w, "yes")).unwrap_err();
        assert!(matches!(err, SendError::ServiceUnsure(_)), "{err:?}");
        assert_eq!(w.db.recent_outgoing(5).unwrap()[0].status, SendStatus::Unsure);
        let envelope = super::super::envelope_for(&w.db, &w.draft.id).unwrap();
        assert_eq!(envelope.why_not.as_deref(), Some(MAY_HAVE_GONE));
    }

    #[test]
    fn a_session_that_is_someone_elses_now_sends_nothing() {
        let w = world();
        let someone = fake::Telegram { me: Me { id: 8, name: "Bob".into(), username: None }, ..w.telegram.clone() };
        let err = send_reply(&w.db, &someone, &request(&w, "yes")).unwrap_err();
        assert!(err.to_string().contains("isn't the account you connected"), "{err}");
        assert!(w.db.recent_outgoing(5).unwrap().is_empty(), "nothing recorded");
        assert_eq!(w.telegram.said_in(4242).len(), 1, "nothing posted");
    }

    #[test]
    fn only_into_the_account_the_message_came_from() {
        let w = world();
        let other =
            w.db.create_source(&new_source(&Me { id: 9, name: "Other".into(), username: None }).unwrap()).unwrap().id;
        let req = SendRequest { source_id: other, ..request(&w, "yes") };
        let err = send_reply(&w.db, &w.telegram, &req).unwrap_err();
        assert!(err.to_string().contains("came from"), "{err}");
    }
}
