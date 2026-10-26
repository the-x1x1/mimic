//! Sending a reply into Slack (0.10.0-alpha.65): the one the user approved,
//! into the conversation the message it answers came from, as them.
//!
//! As a reply by mail is (see `crate::sending`): shown before it goes
//! (`envelope_for`: the workspace, who it goes to — the conversation, never
//! chosen — and why not); recorded in `outbox` before Slack is asked, in
//! one transaction with the check that its draft is undecided and has no
//! other reply on its way; and `unsure` when Slack's answer was lost
//! (`PostError::Unsure`), so its draft isn't offered again until the user
//! says it didn't go. It goes only where the message answered came from (its
//! `slackChannel`), only with the workspace's own token, and only once
//! `auth.test` says that token is still the account the workspace was
//! connected as. Slack's markup in it is escaped (`slack::escaped`), so what
//! is shown is what was written. What went is decided as sent, and put in
//! the conversation as the user's message under the id the next check reads
//! it by, so that check skips it and the thread is answered at once.

use serde_json::{json, Value};

use super::{answered, record_sent_draft, ReplyEnvelope, ReplyKind, SendError, SendRequest, SendingMailbox};
use super::{DRAFT_GONE, EMPTY, HELD};
use crate::db::{Db, DbError, Draft, IdentifierInput, IdentifierKind, Message, NewOutgoing, OutboxEntry, SendHow};
use crate::db::{ALREADY_DECIDED, MAY_HAVE_GONE as MAIL_MAY_HAVE_GONE};
use crate::import::Importer;
use crate::sources::slack::{self, account_id, PostError, SlackApi, SlackSourceConfig};
use crate::sources::{AuthorRef, DiscoveredConversation, RawMessage};

pub const NOT_THIS_WORKSPACE: &str =
    "I only send a reply into the Slack conversation the message you're answering came from.";
pub const NO_CONVERSATION: &str =
    "I don't know which Slack conversation this came from, so I can't send it there. Use this copies it.";
pub const WORKSPACE_GONE: &str = "That Slack workspace isn't connected any more.";
pub const UNREADABLE: &str = "I can't read this workspace's settings; remove it and connect it again.";
pub const NOT_THE_ACCOUNT: &str =
    "This workspace's token isn't the account you connected it as any more, so I won't send as it. Give me your own under Your mail.";
pub const MAY_HAVE_GONE: &str = "A reply to this may already have gone: Slack's answer was lost as it was sent. Look in the conversation in Slack. If it isn't there, say so under Your mail → What I've sent, and you can send it again.";
pub const PANICKED: &str = "Something went wrong inside Mimic while sending, so it may or may not have gone. Look in the conversation in Slack before sending it again.";

/// The workspace a message was read from, when it was read from Slack.
fn workspace_of(db: &Db, message: &Message) -> Result<Option<crate::db::Source>, DbError> {
    Ok(db.get_source(&message.source_id)?.filter(|s| s.connector == slack::CONNECTOR))
}

fn channel_of(message: &Message) -> Option<&str> {
    message.metadata.get("slackChannel").and_then(Value::as_str).filter(|c| !c.is_empty())
}

/// Who a reply goes to, as the user reads it: the conversation's name (a
/// direct message is named for who it's with), else the person's.
fn conversation_name(db: &Db, draft: &Draft, incoming: &Message) -> Result<String, DbError> {
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

/// The Send panel's envelope for a reply to a message read from Slack, or
/// `None` for one that wasn't. Reads; changes nothing.
pub(super) fn envelope_for(db: &Db, draft: &Draft, incoming: &Message) -> Result<Option<ReplyEnvelope>, SendError> {
    let Some(source) = workspace_of(db, incoming)? else { return Ok(None) };
    let config = serde_json::from_value::<SlackSourceConfig>(source.config.clone()).ok();
    let held = db.held_outgoing_for_draft(&draft.id)?;
    let why_not = if draft.outcome.is_some() {
        Some(ALREADY_DECIDED)
    } else if db.unsure_outgoing_for_draft(&draft.id)?.is_some() {
        Some(MAY_HAVE_GONE)
    } else if held.is_some() {
        Some(HELD)
    } else if config.is_none() {
        Some(UNREADABLE)
    } else if channel_of(incoming).is_none() {
        Some(NO_CONVERSATION)
    } else {
        None
    };
    Ok(Some(ReplyEnvelope {
        draft_id: draft.id.clone(),
        mailboxes: vec![SendingMailbox {
            source_id: source.id.clone(),
            name: source.name,
            from_address: config.as_ref().map(|c| format!("{} in {}", c.user, c.team)),
            server: Some("Slack".into()),
            why_not: config.is_none().then(|| UNREADABLE.to_string()),
            signature: None,
        }],
        mailbox: Some(source.id),
        to: vec![conversation_name(db, draft, incoming)?],
        other_addresses: Vec::new(),
        others_on_it: Vec::new(),
        subject: String::new(),
        why_not: why_not.map(str::to_string),
        held,
        kind: ReplyKind::Slack,
    }))
}

/// A refusal from the outbox, in Slack's words where mail's would mislead.
fn worded(e: DbError) -> SendError {
    match e {
        DbError::Invalid(m) if m == MAIL_MAY_HAVE_GONE => SendError::Cannot(MAY_HAVE_GONE.into()),
        e => e.into(),
    }
}

/// Send a reply the user approved into the Slack conversation its message
/// came from, exactly as approved, through `api` (the workspace's token).
pub fn send_reply(db: &Db, api: &SlackApi, req: &SendRequest) -> Result<OutboxEntry, SendError> {
    let draft = db.get_draft(&req.draft_id)?.ok_or_else(|| SendError::Cannot(DRAFT_GONE.into()))?;
    if draft.outcome.is_some() {
        return Err(SendError::Cannot(ALREADY_DECIDED.into()));
    }
    if req.text.trim().is_empty() {
        return Err(SendError::Cannot(EMPTY.into()));
    }
    let incoming = answered(db, &draft)?
        .filter(|m| m.source_id == req.source_id)
        .ok_or_else(|| SendError::Cannot(NOT_THIS_WORKSPACE.into()))?;
    let source = workspace_of(db, &incoming)?.ok_or_else(|| SendError::Cannot(WORKSPACE_GONE.into()))?;
    let config: SlackSourceConfig =
        serde_json::from_value(source.config).map_err(|_| SendError::Cannot(UNREADABLE.into()))?;
    let channel = channel_of(&incoming).ok_or_else(|| SendError::Cannot(NO_CONVERSATION.into()))?.to_string();
    still_theirs(api, &config)?;
    let entry = db
        .queue_outgoing(&NewOutgoing {
            draft_id: Some(draft.id.clone()),
            source_id: req.source_id.clone(),
            conversation_id: Some(incoming.conversation_id.clone()),
            answers_message_id: Some(incoming.id.clone()),
            participant_id: draft.participant_id.clone().or_else(|| incoming.participant_id.clone()),
            from_address: from_address(&config),
            to: vec![conversation_name(db, &draft, &incoming)?],
            cc: Vec::new(),
            subject: String::new(),
            body: req.text.clone(),
            message_id: pending_id(&config, &channel),
            how: SendHow::Approved,
            send_after: None,
        })
        .map_err(worded)?;
    post(db, api, &entry, &config, &channel, &req.text)?;
    if let Err(e) = record_sent_draft(db, &draft, &req.text) {
        tracing::warn!(target: "sending", error = %e, "a sent reply's draft could not be marked as sent");
    }
    if let Err(e) = record_in_conversation(db, &entry, &config, &channel, &req.text, false) {
        tracing::warn!(target: "sending", error = %e, "a sent reply could not be put in its conversation");
    }
    Ok(db.outgoing(&entry.id)?.unwrap_or(entry))
}

/// Whether `api`'s token is still the account the workspace was connected
/// as: Mimic sends as nobody else.
pub(crate) fn still_theirs(api: &SlackApi, config: &SlackSourceConfig) -> Result<(), SendError> {
    match api.auth_test() {
        Ok(me) if config.is(&me) => Ok(()),
        Ok(_) => Err(SendError::Cannot(NOT_THE_ACCOUNT.into())),
        Err(e) => Err(SendError::Slack(e.to_string())),
    }
}

/// What a reply goes through: its workspace's settings and the conversation
/// the message it answers came from — or why it can't go.
pub(crate) fn route(db: &Db, incoming: &Message) -> Result<(SlackSourceConfig, String), &'static str> {
    let source = workspace_of(db, incoming).ok().flatten().ok_or(WORKSPACE_GONE)?;
    let config: SlackSourceConfig = serde_json::from_value(source.config).map_err(|_| UNREADABLE)?;
    let channel = channel_of(incoming).ok_or(NO_CONVERSATION)?.to_string();
    Ok((config, channel))
}

/// The account a workspace's replies go from, as recorded on them.
pub(crate) fn from_address(config: &SlackSourceConfig) -> String {
    account_id(&config.team_id, &config.user_id)
}

/// An id for a reply until Slack names it (`Db::outgoing_sent_as`).
pub(crate) fn pending_id(config: &SlackSourceConfig, channel: &str) -> String {
    format!("{}:{}", account_id(&config.team_id, channel), crate::ids::new_id())
}

/// Post a reply whose `outbox` row is `sending`, and record how it went:
/// sent under the id Slack gave it, failed with Slack's reason, or unsure.
pub(crate) fn post(
    db: &Db,
    api: &SlackApi,
    entry: &OutboxEntry,
    config: &SlackSourceConfig,
    channel: &str,
    text: &str,
) -> Result<(), SendError> {
    let posted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| api.post(channel, text)));
    match posted {
        Ok(Ok(ts)) => {
            db.outgoing_sent_as(&entry.id, &format!("{}:{ts}", account_id(&config.team_id, channel)))?;
            Ok(())
        }
        Ok(Err(PostError::NotSent(e))) => {
            db.outgoing_failed(&entry.id, &e.to_string())?;
            Err(SendError::Slack(e.to_string()))
        }
        Ok(Err(e @ PostError::Unsure(_))) => {
            let why = format!("{e} Look in the conversation in Slack before sending it again.");
            db.outgoing_unsure(&entry.id, &why)?;
            Err(SendError::SlackUnsure(why))
        }
        Err(_) => {
            db.outgoing_unsure(&entry.id, PANICKED)?;
            Err(SendError::SlackUnsure(PANICKED.into()))
        }
    }
}

/// What was said in a conversation after the message a reply answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Since {
    Nothing,
    /// The user wrote: it is answered.
    You,
    /// Someone else did: the reply would answer what is no longer the last
    /// word.
    Them,
}

/// Asked of Slack just before a reply goes by itself (0.10.0-alpha.66), as
/// a Sent folder is for mail: what people said after `incoming` in its
/// conversation. `Ok(Err(why))` when Slack refused to say, as it would
/// again (a token or a scope); `Err(why)` for a failure that may pass.
pub(crate) fn said_since(
    api: &SlackApi,
    config: &SlackSourceConfig,
    channel: &str,
    incoming: &Message,
) -> Result<Result<Since, String>, String> {
    let ts = incoming.external_id.rsplit(':').next().unwrap_or("");
    let known = |e: slack::SlackError| match e {
        slack::SlackError::Network(_) | slack::SlackError::RateLimited => Err(e.to_string()),
        e => Ok(Err(e.to_string())),
    };
    // A reply in the message's thread is not in the conversation itself:
    // the message says who replied there.
    let thread: Vec<String> = match api.message(channel, ts) {
        Ok(Some(m)) if m.get("reply_count").and_then(Value::as_u64).unwrap_or(0) > 0 => m
            .get("reply_users")
            .and_then(Value::as_array)
            .map(|u| u.iter().filter_map(Value::as_str).map(str::to_string).collect())
            // Replied to, by whom it doesn't say: someone.
            .unwrap_or_else(|| vec![String::new()]),
        Ok(_) => Vec::new(),
        Err(e) => return known(e),
    };
    match api.history(channel, Some(ts), 50) {
        Ok((messages, _)) => {
            let mut people: Vec<&str> = messages
                .iter()
                .filter(|m| m.get("bot_id").is_none() && m.get("ts").and_then(Value::as_str) != Some(ts))
                .filter(|m| {
                    m.get("subtype")
                        .and_then(Value::as_str)
                        .is_none_or(|s| s == "thread_broadcast" || s == "file_share")
                })
                .filter_map(|m| m.get("user").and_then(Value::as_str))
                .collect();
            people.extend(thread.iter().map(String::as_str));
            Ok(Ok(if people.contains(&config.user_id.as_str()) {
                Since::You
            } else if people.is_empty() {
                Since::Nothing
            } else {
                Since::Them
            }))
        }
        Err(e) => known(e),
    }
}

/// What went, put in its conversation as the next check would read it: the
/// user's message, under the id Slack gave it — the one its `outbox` row
/// now carries, so one sent on its own (`on_its_own`) is marked as such
/// however it is read (`import::SENDS_CONNECTORS`), and never learned from
/// as the user's writing.
pub(crate) fn record_in_conversation(
    db: &Db,
    entry: &OutboxEntry,
    config: &SlackSourceConfig,
    channel: &str,
    text: &str,
    on_its_own: bool,
) -> Result<(), String> {
    let conversation = account_id(&config.team_id, channel);
    let sent = db.outgoing(&entry.id).map_err(|e| e.to_string())?.ok_or("its outbox row is gone")?;
    let ts = sent.message_id.rsplit(':').next().unwrap_or("").to_string();
    let mut metadata = json!({ "slackChannel": channel });
    if on_its_own {
        metadata["sentOnItsOwn"] = Value::Bool(true);
    }
    let mut importer = Importer::begin(db, &entry.source_id).map_err(|e| e.to_string())?;
    let taken = importer.take(DiscoveredConversation {
        external_id: conversation.clone(),
        subject: None,
        channel: "chat".into(),
        messages: vec![RawMessage {
            external_id: sent.message_id.clone(),
            author: AuthorRef {
                display_name: config.user.clone(),
                identifiers: vec![IdentifierInput::new(
                    IdentifierKind::AccountId,
                    account_id(&config.team_id, &config.user_id),
                )],
            },
            sent_at: slack::ts_time(&ts),
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
    use std::sync::Arc;

    use super::*;
    use crate::db::{NewDraft, SendStatus};
    use crate::sources::slack::fake::{self, TOKEN};
    use crate::sources::slack::sync;

    /// Acme read once, and a draft for Ada's question.
    fn world(round: Arc<AtomicUsize>, answer: fake::Answer) -> (Db, String, Draft, SlackApi, fake::Seen) {
        let (base, seen) = fake::slack(answer);
        let db = Db::open_in_memory().unwrap();
        let source = fake::workspace(&db);
        let api = SlackApi::at(&base, TOKEN).unwrap();
        sync(&db, &source, &api, &|| false).unwrap();
        round.store(2, Ordering::SeqCst);
        let (message, conversation, participant): (String, String, Option<String>) = db
            .conn()
            .query_row(
                "SELECT id, conversation_id, participant_id FROM messages WHERE external_id = 'slack:T1:D1:1770110400.000200'",
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
                incoming_message: Some("lunch thursday, @C?".into()),
                incoming_message_id: Some(message),
                intent: None,
                generated_text: "Yes, noon works.".into(),
                provider: "mock".into(),
                model: "m".into(),
                context: json!({}),
                prompt_hash: "h".into(),
                evidence: json!({}),
                alternative_to: None,
            })
            .unwrap();
        (db, source, draft, api, seen)
    }

    fn request(draft: &Draft, source: &str, text: &str) -> SendRequest {
        SendRequest {
            draft_id: draft.id.clone(),
            text: text.into(),
            source_id: source.into(),
            to: vec!["Ada Lovelace".into()],
            cc: Vec::new(),
            subject: String::new(),
        }
    }

    #[test]
    fn a_reply_goes_into_the_conversation_it_answers_as_written_and_is_the_users() {
        let round = Arc::new(AtomicUsize::new(0));
        let (db, source, draft, api, seen) = world(round.clone(), fake::acme(round.clone()));

        let envelope = super::super::envelope_for(&db, &draft.id).unwrap();
        assert_eq!(envelope.kind, ReplyKind::Slack);
        assert_eq!(envelope.why_not, None);
        assert_eq!(envelope.mailbox.as_deref(), Some(source.as_str()));
        assert_eq!(envelope.to, ["Ada Lovelace"]);
        assert_eq!(envelope.mailboxes[0].from_address.as_deref(), Some("c in Acme"));

        let entry = send_reply(&db, &api, &request(&draft, &source, "Yes <noon> & thanks")).unwrap();
        assert_eq!(entry.status, SendStatus::Sent);
        assert_eq!(entry.from_address, "slack:T1:U1");
        assert_eq!(entry.to, ["Ada Lovelace"]);
        let posted: Vec<String> =
            seen.lock().unwrap().iter().filter(|r| r.starts_with("chat.postMessage?")).cloned().collect();
        assert_eq!(posted.len(), 1);
        let body: Value = serde_json::from_str(posted[0].trim_start_matches("chat.postMessage?")).unwrap();
        assert_eq!(body, json!({ "channel": "D1", "text": "Yes &lt;noon&gt; &amp; thanks" }));

        // Decided as sent, and the thread answered at once.
        assert_eq!(db.get_draft(&draft.id).unwrap().unwrap().outcome.as_deref(), Some("sent_edited"));
        let waiting: Vec<String> = db.threads_awaiting_reply(10).unwrap().into_iter().map(|t| t.last_message).collect();
        assert!(!waiting.iter().any(|m| m.starts_with("lunch thursday")), "{waiting:?}");
        assert!(super::super::envelope_for(&db, &draft.id).unwrap().why_not.is_some());

        // The next check reads it back as the copy already here.
        let next = sync(&db, &source, &api, &|| false).unwrap();
        assert_eq!((next.inserted, next.duplicates), (0, 1));
        let again = send_reply(&db, &api, &request(&draft, &source, "twice"));
        assert!(matches!(again, Err(SendError::Cannot(ref m)) if m == ALREADY_DECIDED), "{again:?}");
    }

    #[test]
    fn a_post_slack_refuses_fails_and_one_whose_answer_is_lost_is_unsure() {
        let round = Arc::new(AtomicUsize::new(0));
        let acme = fake::acme(round.clone());
        let refusing: fake::Answer = Box::new(move |method, query| match method {
            "chat.postMessage" => (200, r#"{"ok":false,"error":"missing_scope","needed":"chat:write"}"#.into()),
            _ => acme(method, query),
        });
        let (db, source, draft, api, _) = world(round, refusing);
        let err = send_reply(&db, &api, &request(&draft, &source, "Yes")).unwrap_err();
        assert!(err.to_string().contains("add the chat:write scope"), "{err}");
        assert!(!err.may_have_gone());
        assert_eq!(db.recent_outgoing(5).unwrap()[0].status, SendStatus::Failed);
        assert_eq!(super::super::envelope_for(&db, &draft.id).unwrap().why_not, None, "it can be sent again");

        let round = Arc::new(AtomicUsize::new(0));
        let acme = fake::acme(round.clone());
        let losing: fake::Answer = Box::new(move |method, query| match method {
            "chat.postMessage" => (0, String::new()),
            _ => acme(method, query),
        });
        let (db, source, draft, api, _) = world(round, losing);
        let err = send_reply(&db, &api, &request(&draft, &source, "Yes")).unwrap_err();
        assert!(err.may_have_gone(), "{err}");

        // Slack's own failure may have come after it took it.
        let round = Arc::new(AtomicUsize::new(0));
        let acme = fake::acme(round.clone());
        let failing: fake::Answer = Box::new(move |method, query| match method {
            "chat.postMessage" => (200, r#"{"ok":false,"error":"fatal_error"}"#.into()),
            _ => acme(method, query),
        });
        let (failed_db, failed_source, failed_draft, failed_api, _) = world(round, failing);
        let err = send_reply(&failed_db, &failed_api, &request(&failed_draft, &failed_source, "Yes")).unwrap_err();
        assert!(err.may_have_gone(), "{err}");
        assert_eq!(failed_db.recent_outgoing(5).unwrap()[0].status, SendStatus::Unsure);
        assert_eq!(db.recent_outgoing(5).unwrap()[0].status, SendStatus::Unsure);
        assert_eq!(super::super::envelope_for(&db, &draft.id).unwrap().why_not.as_deref(), Some(MAY_HAVE_GONE));
        let again = send_reply(&db, &api, &request(&draft, &source, "Yes"));
        assert!(matches!(again, Err(SendError::Cannot(ref m)) if m == MAY_HAVE_GONE), "{again:?}");
        assert_eq!(db.get_draft(&draft.id).unwrap().unwrap().outcome, None);
    }

    #[test]
    fn a_token_that_is_someone_elses_now_sends_nothing() {
        let round = Arc::new(AtomicUsize::new(0));
        let acme = fake::acme(round.clone());
        let switched = Arc::new(AtomicUsize::new(0));
        let later = switched.clone();
        let answer: fake::Answer = Box::new(move |method, query| match method {
            "auth.test" if later.load(Ordering::SeqCst) == 1 => {
                (200, r#"{"ok":true,"team_id":"T1","team":"Acme","user_id":"U9","user":"someone"}"#.into())
            }
            _ => acme(method, query),
        });
        let (db, source, draft, api, seen) = world(round, answer);
        switched.store(1, Ordering::SeqCst);
        let err = send_reply(&db, &api, &request(&draft, &source, "Yes")).unwrap_err();
        assert!(matches!(err, SendError::Cannot(ref m) if m == NOT_THE_ACCOUNT), "{err:?}");
        assert!(db.recent_outgoing(5).unwrap().is_empty());
        assert!(!seen.lock().unwrap().iter().any(|r| r.starts_with("chat.postMessage")));
    }

    #[test]
    fn only_into_the_workspace_the_message_came_from() {
        let round = Arc::new(AtomicUsize::new(0));
        let (db, _, draft, api, _) = world(round.clone(), fake::acme(round));
        let err = send_reply(&db, &api, &request(&draft, "another-source", "Yes")).unwrap_err();
        assert!(matches!(err, SendError::Cannot(ref m) if m == NOT_THIS_WORKSPACE), "{err:?}");
        assert!(db.recent_outgoing(5).unwrap().is_empty());
    }
}
