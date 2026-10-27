//! Sending a reply into Teams (0.10.0-alpha.68): the one the user approved
//! — or one sent by itself (`crate::autoreply`) — into the chat the message
//! it answers came from, as them, the way `sending::slack` sends into Slack:
//! shown first (`envelope_for`), recorded in `outbox` before Graph is
//! asked, as plain text, `unsure` when Graph's answer was lost, only with
//! the account's own sign-in and only while Graph says it is the account
//! connected (`still_theirs`); decided as sent and put in the chat under
//! the id Graph gave it (`Db::outgoing_sent_as`), which the next check
//! reads it back by.

use serde_json::{json, Value};

use super::{answered, record_sent_draft, ReplyEnvelope, ReplyKind, SendError, SendRequest, SendingMailbox};
use super::{DRAFT_GONE, EMPTY, HELD};
use crate::db::{Db, DbError, Draft, IdentifierInput, IdentifierKind, Message, NewOutgoing, OutboxEntry, SendHow};
use crate::db::{ALREADY_DECIDED, MAY_HAVE_GONE as MAIL_MAY_HAVE_GONE};
use crate::import::Importer;
use crate::sources::teams::{self, account_id, GraphApi, PostError, TeamsSourceConfig};
use crate::sources::{AuthorRef, DiscoveredConversation, RawMessage};

pub const NOT_THIS_ACCOUNT: &str = "I only send a reply into the Teams chat the message you're answering came from.";
pub const NO_CHAT: &str = "I don't know which Teams chat this came from, so I can't send it there. Use this copies it.";
pub const ACCOUNT_GONE: &str = "That Teams account isn't connected any more.";
pub const UNREADABLE: &str = "I can't read this account's settings; remove it and connect it again.";
pub const NOT_THE_ACCOUNT: &str =
    "This Teams sign-in isn't the account you connected any more, so I won't send as it. Sign in again under Your mail.";
pub const MAY_HAVE_GONE: &str = "A reply to this may already have gone: Teams' answer was lost as it was sent. Look in the chat in Teams. If it isn't there, say so under Your mail → What I've sent, and you can send it again.";
pub const PANICKED: &str = "Something went wrong inside Mimic while sending, so it may or may not have gone. Look in the chat in Teams before sending it again.";

fn account_of(db: &Db, message: &Message) -> Result<Option<crate::db::Source>, DbError> {
    Ok(db.get_source(&message.source_id)?.filter(|s| s.connector == teams::CONNECTOR))
}

fn chat_of(message: &Message) -> Option<&str> {
    message.metadata.get("teamsChat").and_then(Value::as_str).filter(|c| !c.is_empty())
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

/// The Send panel's envelope for a reply to a message read from Teams, or
/// `None` for one that wasn't.
pub(super) fn envelope_for(db: &Db, draft: &Draft, incoming: &Message) -> Result<Option<ReplyEnvelope>, SendError> {
    let Some(source) = account_of(db, incoming)? else { return Ok(None) };
    let config = serde_json::from_value::<TeamsSourceConfig>(source.config.clone()).ok();
    let held = db.held_outgoing_for_draft(&draft.id)?;
    let why_not = if draft.outcome.is_some() {
        Some(ALREADY_DECIDED)
    } else if db.unsure_outgoing_for_draft(&draft.id)?.is_some() {
        Some(MAY_HAVE_GONE)
    } else if held.is_some() {
        Some(HELD)
    } else if config.is_none() {
        Some(UNREADABLE)
    } else if chat_of(incoming).is_none() {
        Some(NO_CHAT)
    } else {
        None
    };
    Ok(Some(ReplyEnvelope {
        draft_id: draft.id.clone(),
        mailboxes: vec![SendingMailbox {
            source_id: source.id.clone(),
            name: source.name,
            from_address: config.as_ref().map(|c| format!("{} in Teams", c.user)),
            server: Some("Teams".into()),
            why_not: config.is_none().then(|| UNREADABLE.to_string()),
            signature: None,
            quotes: false,
        }],
        mailbox: Some(source.id),
        to: vec![chat_name(db, draft, incoming)?],
        other_addresses: Vec::new(),
        others_on_it: Vec::new(),
        subject: String::new(),
        why_not: why_not.map(str::to_string),
        held,
        quoted: None,
        kind: ReplyKind::Teams,
    }))
}

fn worded(e: DbError) -> SendError {
    match e {
        DbError::Invalid(m) if m == MAIL_MAY_HAVE_GONE => SendError::Cannot(MAY_HAVE_GONE.into()),
        e => e.into(),
    }
}

/// Send a reply the user approved into the Teams chat its message came
/// from, exactly as approved, through `api` (the account's sign-in).
pub fn send_reply(db: &Db, api: &GraphApi, req: &SendRequest) -> Result<OutboxEntry, SendError> {
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
            message_id: pending_id(&chat),
            how: SendHow::Approved,
            send_after: None,
        })
        .map_err(worded)?;
    let created = post(db, api, &entry, &chat, &req.text)?;
    if let Err(e) = record_sent_draft(db, &draft, &req.text) {
        tracing::warn!(target: "sending", error = %e, "a sent reply's draft could not be marked as sent");
    }
    if let Err(e) = record_in_conversation(db, &entry, &config, &chat, &req.text, created.as_deref(), false) {
        tracing::warn!(target: "sending", error = %e, "a sent reply could not be put in its conversation");
    }
    Ok(db.outgoing(&entry.id)?.unwrap_or(entry))
}

/// Whether `api`'s sign-in is still the account connected.
pub(crate) fn still_theirs(api: &GraphApi, config: &TeamsSourceConfig) -> Result<(), SendError> {
    match api.me() {
        Ok(me) if config.is(&me) => Ok(()),
        Ok(_) => Err(SendError::Cannot(NOT_THE_ACCOUNT.into())),
        Err(e) => Err(SendError::Service(e.to_string())),
    }
}

/// The account's settings and the chat the message answered came from, or
/// why a reply can't go there.
pub(crate) fn route(db: &Db, incoming: &Message) -> Result<(TeamsSourceConfig, String), &'static str> {
    let source = account_of(db, incoming).ok().flatten().ok_or(ACCOUNT_GONE)?;
    let config: TeamsSourceConfig = serde_json::from_value(source.config).map_err(|_| UNREADABLE)?;
    let chat = chat_of(incoming).ok_or(NO_CHAT)?.to_string();
    Ok((config, chat))
}

pub(crate) fn from_address(config: &TeamsSourceConfig) -> String {
    account_id(&config.user_id)
}

/// An id for a reply until Graph names it.
pub(crate) fn pending_id(chat: &str) -> String {
    format!("{}:pending-{}", account_id(chat), crate::ids::new_id())
}

/// Post a reply whose `outbox` row is `sending`, and record how it went:
/// when it went, as Teams says, once it has.
pub(crate) fn post(
    db: &Db,
    api: &GraphApi,
    entry: &OutboxEntry,
    chat: &str,
    text: &str,
) -> Result<Option<String>, SendError> {
    let posted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| api.post(chat, text)));
    match posted {
        Ok(Ok((id, created))) => {
            db.outgoing_sent_as(&entry.id, &format!("{}:{id}", account_id(chat)))?;
            Ok(created)
        }
        Ok(Err(PostError::NotSent(e))) => {
            db.outgoing_failed(&entry.id, &e.to_string())?;
            Err(SendError::Service(e.to_string()))
        }
        Ok(Err(e @ PostError::Unsure(_))) => {
            let why = format!("{e} Look in the chat in Teams before sending it again.");
            db.outgoing_unsure(&entry.id, &why)?;
            Err(SendError::ServiceUnsure(why))
        }
        Err(_) => {
            db.outgoing_unsure(&entry.id, PANICKED)?;
            Err(SendError::ServiceUnsure(PANICKED.into()))
        }
    }
}

/// What people said in a chat after `incoming`, asked just before a reply
/// goes by itself: `Ok(Err(why))` when Teams refused to say, as it would
/// again; `Err(why)` for a failure that may pass.
pub(crate) fn said_since(
    api: &GraphApi,
    config: &TeamsSourceConfig,
    chat: &str,
    incoming: &Message,
) -> Result<Result<super::slack::Since, String>, String> {
    use super::slack::Since;
    let created = incoming.metadata.get("teamsCreated").and_then(Value::as_str).or(incoming.sent_at.as_deref());
    let own_id = incoming.external_id.rsplit(':').next().unwrap_or("");
    match api.messages(chat, created, 50) {
        Ok((messages, _)) => {
            let people: Vec<&str> = messages
                .iter()
                .filter(|m| m.get("id").and_then(Value::as_str) != Some(own_id))
                .filter(|m| m.get("messageType").and_then(Value::as_str) == Some("message"))
                .filter(|m| m.get("deletedDateTime").is_none_or(Value::is_null))
                .filter_map(|m| {
                    m.get("from").and_then(|f| f.get("user")).and_then(|u| u.get("id")).and_then(Value::as_str)
                })
                .collect();
            Ok(Ok(if people.contains(&config.user_id.as_str()) {
                Since::You
            } else if people.is_empty() {
                Since::Nothing
            } else {
                Since::Them
            }))
        }
        Err(e @ (teams::TeamsError::Network(_) | teams::TeamsError::RateLimited)) => Err(e.to_string()),
        Err(e) => Ok(Err(e.to_string())),
    }
}

/// What went, put in its chat as the user's message under the id Graph
/// gave it, at the time Teams gave it (else when it went here, which this
/// computer's clock may have wrong); one sent by itself marked
/// `sentOnItsOwn`.
pub(crate) fn record_in_conversation(
    db: &Db,
    entry: &OutboxEntry,
    config: &TeamsSourceConfig,
    chat: &str,
    text: &str,
    created: Option<&str>,
    on_its_own: bool,
) -> Result<(), String> {
    let sent = db.outgoing(&entry.id).map_err(|e| e.to_string())?.ok_or("its outbox row is gone")?;
    let mut metadata = json!({ "teamsChat": chat });
    if on_its_own {
        metadata["sentOnItsOwn"] = Value::Bool(true);
    }
    let mut importer = Importer::begin(db, &entry.source_id).map_err(|e| e.to_string())?;
    let taken = importer.take(DiscoveredConversation {
        external_id: account_id(chat),
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::db::{NewDraft, SendStatus};
    use crate::sources::teams::fake::{self, TOKEN};
    use crate::sources::teams::{new_source, sync, Me};

    /// A Graph that keeps what it is told: one chat with Ada, her question
    /// in it, and what is posted added as the user's.
    fn graph(posted: Arc<Mutex<Vec<Value>>>, refuse_post: Option<(u16, &'static str)>) -> fake::Answer {
        let question = json!({
            "id": "m1", "messageType": "message", "createdDateTime": "2026-10-26T08:50:00.120Z", "deletedDateTime": null,
            "from": { "user": { "id": "u2", "displayName": "Ada Lovelace" } },
            "body": { "contentType": "text", "content": "can you send the deck?" }, "attachments": []
        });
        Box::new(move |path, body| {
            let body = match path {
                "/me" => r#"{"id":"u1","displayName":"C Haynes","mail":"c@formicaria.example"}"#.to_string(),
                "/me/chats" => r#"{"value":[{"id":"19:a1b2@unq.gbl.spaces","chatType":"oneOnOne","members":[
                    {"userId":"u1","displayName":"C Haynes"},{"userId":"u2","displayName":"Ada Lovelace","email":"ada@formicaria.example"}]}]}"#
                    .to_string(),
                "/chats/19:a1b2@unq.gbl.spaces/messages" => {
                    if let Some((status, answer)) = refuse_post {
                        return (status, answer.to_string());
                    }
                    let sent: Value = serde_json::from_str(body).unwrap();
                    let mut posted = posted.lock().unwrap();
                    let id = format!("m{}", 2 + posted.len());
                    posted.push(json!({
                        "id": id, "messageType": "message", "createdDateTime": "2026-10-26T09:20:00Z",
                        "from": { "user": { "id": "u1", "displayName": "C Haynes" } },
                        "body": { "contentType": "html", "content": format!("<p>{}</p>", sent["body"]["content"].as_str().unwrap()) },
                        "attachments": []
                    }));
                    assert_eq!(sent["body"]["contentType"], "text");
                    json!({ "id": id, "createdDateTime": "2026-10-26T09:20:00Z" }).to_string()
                }
                "/me/chats/19:a1b2@unq.gbl.spaces/messages" => {
                    let mut all: Vec<Value> = posted.lock().unwrap().clone();
                    all.reverse();
                    all.push(question.clone());
                    json!({ "value": all }).to_string()
                }
                _ => return (404, r#"{"error":{"code":"NotFound","message":"no"}}"#.into()),
            };
            (200, body)
        })
    }

    fn world(answer: fake::Answer) -> (Db, String, Draft, GraphApi) {
        let (base, _) = fake::graph(answer);
        let db = Db::open_in_memory().unwrap();
        db.set_user_identity("C Haynes").unwrap();
        db.add_user_identifier(IdentifierKind::AccountId, &account_id("u1")).unwrap();
        db.set_setting(crate::db::WITHIN_DAYS_SETTING, &0).unwrap();
        let me = Me { id: "u1".into(), name: "C Haynes".into(), email: Some("c@formicaria.example".into()) };
        let source = db.create_source(&new_source(&me).unwrap()).unwrap().id;
        let api = GraphApi::at(&base, TOKEN).unwrap();
        sync(&db, &source, &api, &|| false).unwrap();
        let (message, conversation, person): (String, String, Option<String>) = db
            .conn()
            .query_row(
                "SELECT id, conversation_id, participant_id FROM messages WHERE external_id = 'teams:19:a1b2@unq.gbl.spaces:m1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        let draft = db
            .create_draft(&NewDraft {
                participant_id: person,
                conversation_id: Some(conversation),
                channel: "chat".into(),
                situation_id: None,
                incoming_message: Some("can you send the deck?".into()),
                incoming_message_id: Some(message),
                intent: None,
                generated_text: "Sending it now.".into(),
                provider: "mock".into(),
                model: "m".into(),
                context: json!({}),
                prompt_hash: "h".into(),
                evidence: json!({}),
                alternative_to: None,
            })
            .unwrap();
        (db, source, draft, api)
    }

    fn request(draft: &Draft, source: &str, text: &str) -> SendRequest {
        SendRequest {
            draft_id: draft.id.clone(),
            text: text.into(),
            source_id: source.into(),
            to: vec!["Ada Lovelace".into()],
            cc: Vec::new(),
            subject: String::new(),
            quote: None,
        }
    }

    #[test]
    fn a_reply_goes_into_the_chat_it_answers_and_is_read_back_as_the_same_message() {
        let posted = Arc::new(Mutex::new(Vec::new()));
        let (db, source, draft, api) = world(graph(posted.clone(), None));
        let envelope = super::super::envelope_for(&db, &draft.id).unwrap();
        assert_eq!(envelope.kind, ReplyKind::Teams);
        assert_eq!(envelope.why_not, None);
        assert_eq!(envelope.to, ["Ada Lovelace"]);
        assert_eq!(envelope.mailboxes[0].from_address.as_deref(), Some("C Haynes in Teams"));

        let entry = send_reply(&db, &api, &request(&draft, &source, "Sending it <now> & then lunch")).unwrap();
        assert_eq!(entry.status, SendStatus::Sent);
        assert_eq!(entry.message_id, "teams:19:a1b2@unq.gbl.spaces:m2");
        assert_eq!(posted.lock().unwrap().len(), 1);
        assert_eq!(db.get_draft(&draft.id).unwrap().unwrap().outcome.as_deref(), Some("sent_edited"));
        let waiting: Vec<String> = db.threads_awaiting_reply(10).unwrap().into_iter().map(|t| t.last_message).collect();
        assert!(waiting.is_empty(), "{waiting:?}");
        let next = sync(&db, &source, &api, &|| false).unwrap();
        assert_eq!((next.inserted, next.duplicates), (0, 1));
    }

    #[test]
    fn a_post_teams_refuses_fails_and_a_lost_answer_is_unsure() {
        let (db, source, draft, api) = world(graph(
            Arc::new(Mutex::new(Vec::new())),
            Some((403, r#"{"error":{"code":"Forbidden","message":"Missing scope permissions on the request."}}"#)),
        ));
        let err = send_reply(&db, &api, &request(&draft, &source, "Yes")).unwrap_err();
        assert!(!err.may_have_gone(), "{err}");
        assert_eq!(db.recent_outgoing(5).unwrap()[0].status, SendStatus::Failed);

        let (db, source, draft, api) = world(graph(Arc::new(Mutex::new(Vec::new())), Some((0, ""))));
        let err = send_reply(&db, &api, &request(&draft, &source, "Yes")).unwrap_err();
        assert!(err.may_have_gone(), "{err}");
        assert_eq!(super::super::envelope_for(&db, &draft.id).unwrap().why_not.as_deref(), Some(MAY_HAVE_GONE));
    }

    #[test]
    fn a_sign_in_that_is_someone_elses_now_sends_nothing() {
        let posted = Arc::new(Mutex::new(Vec::new()));
        let switched = Arc::new(AtomicUsize::new(0));
        let later = switched.clone();
        let inner = graph(posted.clone(), None);
        let answer: fake::Answer = Box::new(move |path, body| match path {
            "/me" if later.load(Ordering::SeqCst) == 1 => (200, r#"{"id":"u9","displayName":"Someone"}"#.into()),
            _ => inner(path, body),
        });
        let (db, source, draft, api) = world(answer);
        switched.store(1, Ordering::SeqCst);
        let err = send_reply(&db, &api, &request(&draft, &source, "Yes")).unwrap_err();
        assert!(matches!(err, SendError::Cannot(ref m) if m == NOT_THE_ACCOUNT), "{err:?}");
        assert!(db.recent_outgoing(5).unwrap().is_empty());
        assert!(posted.lock().unwrap().is_empty());
    }
}
