//! Sending a reply into Google Chat (0.10.0-alpha.80): the one the user
//! approved — or one sent by itself (`crate::autoreply`) — into the space
//! the message it answers came from, as them, the way `sending::teams`
//! sends into Teams: shown first (`envelope_for`), recorded in `outbox`
//! before Google is asked, as plain text, under the outbox row's own id as
//! its request id (the same request asked again makes no second message),
//! `unsure` when the answer was lost, only with the account's own sign-in
//! and only while Google says it is the account connected (`still_theirs`);
//! decided as sent and put in the chat under the name Google gave it
//! (`Db::outgoing_sent_as`), which the next check reads it back by.

use serde_json::{json, Value};

use super::{answered, record_sent_draft, ReplyEnvelope, ReplyKind, SendError, SendRequest, SendingMailbox};
use super::{DRAFT_GONE, EMPTY, HELD};
use crate::db::{Db, DbError, Draft, IdentifierInput, IdentifierKind, Message, NewOutgoing, OutboxEntry, SendHow};
use crate::db::{ALREADY_DECIDED, MAY_HAVE_GONE as MAIL_MAY_HAVE_GONE};
use crate::import::Importer;
use crate::sources::google_chat::{self, account_id, ChatApi, GoogleChatError, GoogleChatSourceConfig, PostError};
use crate::sources::{AuthorRef, DiscoveredConversation, RawMessage};

pub const NOT_THIS_ACCOUNT: &str =
    "I only send a reply into the Google Chat conversation the message you're answering came from.";
pub const NO_SPACE: &str =
    "I don't know which Google Chat conversation this came from, so I can't send it there. Use this copies it.";
pub const ACCOUNT_GONE: &str = "That Google Chat account isn't connected any more.";
pub const UNREADABLE: &str = "I can't read this account's settings; remove it and connect it again.";
pub const NOT_THE_ACCOUNT: &str = "This Google Chat sign-in isn't the account you connected any more, so I won't send as it. Sign in again under Your mail.";
pub const MAY_HAVE_GONE: &str = "A reply to this may already have gone: Google Chat's answer was lost as it was sent. Look in the chat in Google Chat. If it isn't there, say so under Your mail → What I've sent, and you can send it again.";
pub const PANICKED: &str = "Something went wrong inside Mimic while sending, so it may or may not have gone. Look in the chat in Google Chat before sending it again.";

fn account_of(db: &Db, message: &Message) -> Result<Option<crate::db::Source>, DbError> {
    Ok(db.get_source(&message.source_id)?.filter(|s| s.connector == google_chat::CONNECTOR))
}

fn space_of(message: &Message) -> Option<&str> {
    message.metadata.get("googleChatSpace").and_then(Value::as_str).filter(|s| s.starts_with("spaces/"))
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

/// The Send panel's envelope for a reply to a message read from Google
/// Chat, or `None` for one that wasn't.
pub(super) fn envelope_for(db: &Db, draft: &Draft, incoming: &Message) -> Result<Option<ReplyEnvelope>, SendError> {
    let Some(source) = account_of(db, incoming)? else { return Ok(None) };
    let config = serde_json::from_value::<GoogleChatSourceConfig>(source.config.clone()).ok();
    let held = db.held_outgoing_for_draft(&draft.id)?;
    let why_not = if draft.outcome.is_some() {
        Some(ALREADY_DECIDED)
    } else if db.unsure_outgoing_for_draft(&draft.id)?.is_some() {
        Some(MAY_HAVE_GONE)
    } else if held.is_some() {
        Some(HELD)
    } else if config.is_none() {
        Some(UNREADABLE)
    } else if space_of(incoming).is_none() {
        Some(NO_SPACE)
    } else {
        None
    };
    Ok(Some(ReplyEnvelope {
        draft_id: draft.id.clone(),
        mailboxes: vec![SendingMailbox {
            source_id: source.id.clone(),
            name: source.name,
            from_address: config.as_ref().map(|c| format!("{} in Google Chat", c.user)),
            server: Some("Google Chat".into()),
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
        kind: ReplyKind::GoogleChat,
    }))
}

fn worded(e: DbError) -> SendError {
    match e {
        DbError::Invalid(m) if m == MAIL_MAY_HAVE_GONE => SendError::Cannot(MAY_HAVE_GONE.into()),
        e => e.into(),
    }
}

/// Send a reply the user approved into the Google Chat conversation its
/// message came from, exactly as approved, through `api` (the account's
/// sign-in).
pub fn send_reply(db: &Db, api: &ChatApi, req: &SendRequest) -> Result<OutboxEntry, SendError> {
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
    let (config, space) = route(db, &incoming).map_err(|why| SendError::Cannot(why.into()))?;
    still_theirs(api, &config)?;
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
            message_id: pending_id(&space),
            how: SendHow::Approved,
            send_after: None,
        })
        .map_err(worded)?;
    let created = post(db, api, &entry, &space, &req.text)?;
    if let Err(e) = record_sent_draft(db, &draft, &req.text) {
        tracing::warn!(target: "sending", error = %e, "a sent reply's draft could not be marked as sent");
    }
    if let Err(e) = record_in_conversation(db, &entry, &config, &incoming, &req.text, created.as_deref(), false) {
        tracing::warn!(target: "sending", error = %e, "a sent reply could not be put in its conversation");
    }
    Ok(db.outgoing(&entry.id)?.unwrap_or(entry))
}

/// Whether `api`'s sign-in is still the account connected. Signed out, no
/// wait makes it go.
pub(crate) fn still_theirs(api: &ChatApi, config: &GoogleChatSourceConfig) -> Result<(), SendError> {
    match api.me() {
        Ok(me) if config.is(&me) => Ok(()),
        Ok(_) => Err(SendError::Cannot(NOT_THE_ACCOUNT.into())),
        Err(e @ GoogleChatError::SignIn(_)) => Err(SendError::Cannot(e.to_string())),
        Err(e) => Err(SendError::Service(e.to_string())),
    }
}

/// The account's settings and the space the message answered came from,
/// or why a reply can't go there.
pub(crate) fn route(db: &Db, incoming: &Message) -> Result<(GoogleChatSourceConfig, String), &'static str> {
    let source = account_of(db, incoming).ok().flatten().ok_or(ACCOUNT_GONE)?;
    let config: GoogleChatSourceConfig = serde_json::from_value(source.config).map_err(|_| UNREADABLE)?;
    let space = space_of(incoming).ok_or(NO_SPACE)?.to_string();
    Ok((config, space))
}

pub(crate) fn from_address(config: &GoogleChatSourceConfig) -> String {
    account_id(&config.user_id)
}

/// An id for a reply until Google names it.
pub(crate) fn pending_id(space: &str) -> String {
    format!("{}:pending-{}", account_id(space), crate::ids::new_id())
}

/// Post a reply whose `outbox` row is `sending`, under the row's id as its
/// request id, and record how it went: when it went, as Google Chat says,
/// once it has.
pub(crate) fn post(
    db: &Db,
    api: &ChatApi,
    entry: &OutboxEntry,
    space: &str,
    text: &str,
) -> Result<Option<String>, SendError> {
    let posted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| api.post(space, text, &entry.id)));
    match posted {
        Ok(Ok((name, created))) => {
            db.outgoing_sent_as(&entry.id, &account_id(&name))?;
            Ok(created)
        }
        Ok(Err(PostError::NotSent(e))) => {
            db.outgoing_failed(&entry.id, &e.to_string())?;
            Err(SendError::Service(e.to_string()))
        }
        Ok(Err(e @ PostError::Unsure(_))) => {
            let why = format!("{e} Look in the chat in Google Chat before sending it again.");
            db.outgoing_unsure(&entry.id, &why)?;
            Err(SendError::ServiceUnsure(why))
        }
        Err(_) => {
            db.outgoing_unsure(&entry.id, PANICKED)?;
            Err(SendError::ServiceUnsure(PANICKED.into()))
        }
    }
}

/// What was said in a space after `incoming`, asked just before a reply
/// goes by itself: `Ok(Err(why))` when Google Chat refused to say, as it
/// would again; `Err(why)` for a failure that may pass. Anything of the
/// user's there since answers it; of the others', only people's words.
pub(crate) fn said_since(
    api: &ChatApi,
    config: &GoogleChatSourceConfig,
    space: &str,
    incoming: &Message,
) -> Result<Result<super::slack::Since, String>, String> {
    use super::slack::Since;
    let Some(created) =
        incoming.metadata.get("googleChatCreated").and_then(Value::as_str).or(incoming.sent_at.as_deref())
    else {
        return Ok(Err("I don't know when the message this answers was sent".into()));
    };
    let own = incoming.external_id.strip_prefix("google_chat:").unwrap_or("");
    match api.messages(space, Some(created), 50) {
        Ok((messages, _)) => {
            let since: Vec<&Value> =
                messages.iter().filter(|m| m.get("name").and_then(Value::as_str) != Some(own)).collect();
            let from =
                |m: &Value| m.get("sender").and_then(|s| s.get("name")).and_then(Value::as_str).map(str::to_string);
            Ok(Ok(if since.iter().any(|m| from(m).as_deref() == Some(config.user_id.as_str())) {
                Since::You
            } else if since.iter().any(|m| google_chat::kept(m)) {
                Since::Them
            } else {
                Since::Nothing
            }))
        }
        Err(e @ (GoogleChatError::Network(_) | GoogleChatError::RateLimited)) => Err(e.to_string()),
        Err(e) => Ok(Err(e.to_string())),
    }
}

/// What went, put in its chat as the user's message under the name Google
/// gave it, at the time Google Chat gave it (else when it went here); one
/// sent by itself marked `sentOnItsOwn`.
pub(crate) fn record_in_conversation(
    db: &Db,
    entry: &OutboxEntry,
    config: &GoogleChatSourceConfig,
    incoming: &Message,
    text: &str,
    created: Option<&str>,
    on_its_own: bool,
) -> Result<(), String> {
    let sent = db.outgoing(&entry.id).map_err(|e| e.to_string())?.ok_or("its outbox row is gone")?;
    let space = space_of(incoming).ok_or(NO_SPACE)?.to_string();
    let mut metadata = json!({
        "googleChatSpace": space,
        "googleChatDirect": incoming.metadata.get("googleChatDirect").cloned().unwrap_or(Value::Bool(false)),
        "attachments": false,
    });
    if let Some(created) = created {
        metadata["googleChatCreated"] = json!(created);
    }
    if on_its_own {
        metadata["sentOnItsOwn"] = Value::Bool(true);
    }
    let mut importer = Importer::begin(db, &entry.source_id).map_err(|e| e.to_string())?;
    let taken = importer.take(DiscoveredConversation {
        external_id: account_id(&space),
        subject: None,
        channel: "chat".into(),
        messages: vec![RawMessage {
            external_id: sent.message_id.clone(),
            author: AuthorRef {
                display_name: config.user.clone(),
                identifiers: vec![IdentifierInput::new(IdentifierKind::AccountId, account_id(&config.user_id))],
            },
            sent_at: created
                .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
                .map(|t| t.with_timezone(&chrono::Utc).to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
                .or(sent.sent_at.clone()),
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
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::db::{NewDraft, SendStatus};
    use crate::sources::google_chat::live::{self, said, Space};
    use crate::sources::google_chat::{new_source, sync, Me};
    use crate::sources::teams::fake::{graph as server, TOKEN};

    struct World {
        db: Db,
        source: String,
        api: ChatApi,
        spaces: live::Spaces,
        posts: Arc<Mutex<Vec<String>>>,
        draft: Draft,
    }

    fn me() -> Me {
        Me { id: "users/7".into(), name: "C Haynes".into(), email: Some("c@formicaria.example".into()) }
    }

    /// An account read once: Ada's question waits in a direct message, and
    /// a draft is ready.
    fn world() -> World {
        let db = Db::open_in_memory().unwrap();
        db.set_setting(crate::db::WITHIN_DAYS_SETTING, &0).unwrap();
        db.set_user_identity("C").unwrap();
        db.add_user_identifier(IdentifierKind::AccountId, &account_id("users/7")).unwrap();
        let source = db.create_source(&new_source(&me()).unwrap()).unwrap().id;
        let spaces: live::Spaces = Arc::new(Mutex::new(vec![Space {
            name: "spaces/DM1".into(),
            other: Some(("users/42".into(), "Ada Lovelace".into())),
            messages: vec![said(
                "spaces/DM1",
                "m1",
                "users/42",
                "Ada Lovelace",
                "2026-10-26T09:01:00.000000Z",
                "lunch on thursday?",
            )],
        }]));
        let posts = Arc::new(Mutex::new(Vec::new()));
        let (base, _) = server(live::chat(spaces.clone(), posts.clone()));
        let api = ChatApi::at(&base, &format!("{base}/userinfo"), TOKEN).unwrap();
        sync(&db, &source, &api, &|| false).unwrap();
        let (message, conversation, participant): (String, String, Option<String>) = db
            .conn()
            .query_row(
                "SELECT id, conversation_id, participant_id FROM messages WHERE external_id = 'google_chat:spaces/DM1/messages/m1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        let draft = db
            .create_draft(&NewDraft {
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
        World { db, source, api, spaces, posts, draft }
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
        assert_eq!(envelope.kind, ReplyKind::GoogleChat);
        assert_eq!(envelope.to, vec!["Ada Lovelace".to_string()]);
        assert_eq!(envelope.why_not, None);

        let sent = send_reply(&w.db, &w.api, &request(&w, "yes! the place by the river?")).unwrap();
        assert_eq!((sent.status, sent.how), (SendStatus::Sent, SendHow::Approved));
        assert_eq!(sent.message_id, "google_chat:spaces/DM1/messages/p1");
        assert_eq!(sent.from_address, "google_chat:users/7");
        assert_eq!(w.posts.lock().unwrap().len(), 1);
        assert!(w.db.get_draft(&w.draft.id).unwrap().unwrap().outcome.is_some());

        // The next check reads it back: the same message, the user's, once.
        let summary = sync(&w.db, &w.source, &w.api, &|| false).unwrap();
        assert_eq!((summary.inserted, summary.duplicates), (0, 1), "{summary:?}");
        let mine: i64 =
            w.db.conn().query_row("SELECT count(*) FROM messages WHERE direction = 'self'", [], |r| r.get(0)).unwrap();
        assert_eq!(mine, 1);
    }

    #[test]
    fn a_post_google_refuses_fails_and_a_signed_out_one_sends_nothing() {
        let w = world();
        let (base, _) = server(Box::new(|path, body| match path {
            "/userinfo" => (200, r#"{"sub":"7","name":"C Haynes"}"#.into()),
            _ if body.starts_with('{') => {
                (403, r#"{"error":{"code":403,"status":"PERMISSION_DENIED","message":"no posting here"}}"#.into())
            }
            _ => (404, "{}".into()),
        }));
        let refusing = ChatApi::at(&base, &format!("{base}/userinfo"), TOKEN).unwrap();
        let err = send_reply(&w.db, &refusing, &request(&w, "yes")).unwrap_err();
        assert!(matches!(err, SendError::Service(_)), "{err:?}");
        assert_eq!(w.db.recent_outgoing(5).unwrap()[0].status, SendStatus::Failed);

        let w = world();
        let (base, _) = server(Box::new(|_, _| (401, r#"{"error":{"code":401,"status":"UNAUTHENTICATED"}}"#.into())));
        let signed_out = ChatApi::at(&base, &format!("{base}/userinfo"), TOKEN).unwrap();
        let err = send_reply(&w.db, &signed_out, &request(&w, "yes")).unwrap_err();
        assert!(matches!(&err, SendError::Cannot(why) if why.contains("sign in to Google Chat again")), "{err:?}");
        assert!(w.db.recent_outgoing(5).unwrap().is_empty(), "nothing recorded");
    }

    #[test]
    fn a_server_failure_after_asking_is_unsure() {
        let w = world();
        let (base, _) = server(Box::new(|path, body| match path {
            "/userinfo" => (200, r#"{"sub":"7","name":"C Haynes"}"#.into()),
            _ if body.starts_with('{') => (503, r#"{"error":{"code":503,"status":"UNAVAILABLE"}}"#.into()),
            _ => (404, "{}".into()),
        }));
        let failing = ChatApi::at(&base, &format!("{base}/userinfo"), TOKEN).unwrap();
        let err = send_reply(&w.db, &failing, &request(&w, "yes")).unwrap_err();
        assert!(matches!(err, SendError::ServiceUnsure(_)), "{err:?}");
        assert_eq!(w.db.recent_outgoing(5).unwrap()[0].status, SendStatus::Unsure);
        let envelope = super::super::envelope_for(&w.db, &w.draft.id).unwrap();
        assert_eq!(envelope.why_not.as_deref(), Some(MAY_HAVE_GONE));
    }

    #[test]
    fn what_was_said_since_is_the_users_anything_or_someone_elses_words() {
        use crate::sending::slack::Since;
        let w = world();
        let incoming = answered(&w.db, &w.draft).unwrap().unwrap();
        let config: GoogleChatSourceConfig =
            serde_json::from_value(w.db.get_source(&w.source).unwrap().unwrap().config).unwrap();
        let since = || said_since(&w.api, &config, "spaces/DM1", &incoming).unwrap().unwrap();
        assert!(matches!(since(), Since::Nothing));
        let push = |m: Value| w.spaces.lock().unwrap()[0].messages.push(m);
        push(json!({ "name": "spaces/DM1/messages/bot", "sender": { "name": "users/app", "type": "BOT" },
                     "createTime": "2026-10-26T09:02:00.000000Z", "text": "Reminder" }));
        assert!(matches!(since(), Since::Nothing), "an app isn't anyone");
        push(said("spaces/DM1", "m2", "users/42", "Ada Lovelace", "2026-10-26T09:03:00.000000Z", "or friday"));
        assert!(matches!(since(), Since::Them));
        push(json!({ "name": "spaces/DM1/messages/m3", "sender": { "name": "users/7", "type": "HUMAN" },
                     "createTime": "2026-10-26T09:04:00.000000Z", "attachment": [{ "name": "a" }] }));
        assert!(matches!(since(), Since::You), "anything of the user's answers it");
    }
}
