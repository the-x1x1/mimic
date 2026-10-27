//! Sending a reply: a draft, to the person who wrote, through the user's own
//! mailbox.
//!
//! Mimic drafted and the user sent until 0.10.0-alpha.23; from then the user
//! can send from Mimic. The owner asked for it, and the rules that kept it
//! out (CLAUDE.md, docs/PRODUCT.md) say now what it may and may not do:
//!
//! * **It is the user's reply, sent by the user.** `send_reply` sends what
//!   they approved — the words, who it goes to, through which mailbox, under
//!   which subject — all shown before it goes (`envelope_for`), and nothing
//!   else. Replies Mimic sends on its own are a separate decision with their
//!   own rules (0.10.0-alpha.24).
//! * **It goes out the way the user's own mail does.** From their address,
//!   through their provider's server, signed in as their mailbox already
//!   signs in to read (`sources::smtp`), threaded under the message it
//!   answers (`sources::outgoing`).
//! * **It is recorded before it goes.** An `outbox` row is written first, in
//!   one transaction with the check that its draft is still undecided and has
//!   no other reply on its way, so two windows can't send one draft twice.
//!   What happened is written to it after: `sent`, or `failed` with the
//!   reason, or `unsure` for a send that may have gone (`SmtpError::Unsure`,
//!   or Mimic closing mid-send) — whose draft is not offered to send again
//!   until the user has looked and says it didn't go.
//! * **What was sent is the user's message.** The draft is decided as sent
//!   (`sent_unedited` or `sent_edited`, with what was changed as feedback,
//!   exactly as Use this records it), and the message is put in its
//!   conversation as the user's own, as the next check would read it from
//!   the Sent folder — which then skips it by its Message-ID — so the thread
//!   is answered at once.
//! * **It is signed as the mailbox signs** (0.10.0-alpha.36): the mailbox's
//!   signature, when the user set one, goes under the words after the
//!   conventional `-- ` line (`signed`), shown in the Send panel before it
//!   goes; it is part of what is recorded and sent, and not of what the
//!   draft is decided with, so it is never learned from as the user's
//!   words.
//! * **It goes to who wrote, and to the others on their message only when
//!   the user ticks them** (0.10.0-alpha.33). `envelope_for` lists who else
//!   the message was to or copied to, without the user's own addresses
//!   (`others_on_it`); `send_reply` copies in only addresses from that list
//!   (`NOT_ON_IT`). A reply sent by itself goes to who wrote and no one else.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::db::{
    Db, DbError, Draft, IdentifierInput, IdentifierKind, Message, NewEvent, NewOutgoing, OutboxEntry, SendHow,
    ALREADY_DECIDED, MAY_HAVE_GONE,
};
use crate::sources::graph_mail::{self, GraphSendError};
use crate::sources::imap::{self, Auth, Credentials, ImapSourceConfig};
use crate::sources::outgoing::{self, Mailbox, Outgoing, OutgoingError};
use crate::sources::smtp::{self, SmtpAccount, SmtpError};

pub mod google_chat;
pub mod slack;
pub mod teams;
pub mod telegram;

/// Why a reply can't be sent from here.
pub const NO_MAILBOX: &str =
    "To send from here, connect your mailbox under Your mail. Until then, Use this copies the reply for you to send.";
pub const NOT_SET_UP: &str = "I don't know how to send from this mailbox yet. Set up sending for it under Your mail.";
pub const NO_FROM: &str = "I don't know which address to send from for this mailbox. Set it under Your mail.";
pub const NOT_YOURS: &str =
    "That isn't one of your addresses, so what I sent from it wouldn't be filed as yours. Add it under Settings → You first.";
pub const NO_ADDRESS: &str = "I don't have an address to send this to.";
pub const NOT_EMAIL: &str = "This reply isn't to an email, so it can't be sent from here. Use this copies it.";
pub const MAILBOX_GONE: &str = "That mailbox isn't connected any more.";
pub const UNREADABLE: &str = "I can't read this mailbox's settings; remove it and connect it again.";
pub const DRAFT_GONE: &str = "That draft isn't here any more, so there's nothing to send.";
pub const EMPTY: &str = "There's nothing to send: the reply is empty.";
pub const HELD: &str = crate::db::HELD_BY_ITSELF;
pub const NOT_OFFERED: &str =
    "I only send a reply to the addresses the Send panel offered for it, and that isn't one of them.";
pub const NOT_ON_IT: &str = "I only copy in people who were on the message you're answering, and that address wasn't.";
pub const PANICKED: &str =
    "Something went wrong inside Mimic while sending, so it may or may not have gone. Look in your Sent folder before sending it again.";

#[derive(Debug, thiserror::Error)]
pub enum SendError {
    /// Refused before anything was said to a mail server.
    #[error("{0}")]
    Cannot(String),
    #[error(transparent)]
    Smtp(#[from] SmtpError),
    #[error(transparent)]
    Message(#[from] OutgoingError),
    #[error(transparent)]
    Db(#[from] DbError),
    /// Slack or Teams didn't take it (0.10.0-alpha.65, .68).
    #[error("{0}")]
    Service(String),
    /// Their answer was lost: it may have gone.
    #[error("{0}")]
    ServiceUnsure(String),
}

impl SendError {
    /// Whether the reply may have gone, so must not simply be sent again.
    pub fn may_have_gone(&self) -> bool {
        match self {
            SendError::Smtp(e) => e.may_have_gone(),
            SendError::ServiceUnsure(_) => true,
            SendError::Cannot(why) => why == PANICKED,
            _ => false,
        }
    }
}

/// A mailbox a reply can go out through.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendingMailbox {
    pub source_id: String,
    /// As Your mail names it.
    pub name: String,
    /// The address replies go out from; `None` when it can't send.
    pub from_address: Option<String>,
    /// The server they go out through, when it can send.
    pub server: Option<String>,
    /// Why it can't send, when it can't.
    pub why_not: Option<String>,
    /// What goes under a reply sent through it (0.10.0-alpha.36).
    pub signature: Option<String>,
}

/// Where a reply goes: out of a mailbox, or into Slack (0.10.0-alpha.65),
/// Teams (0.10.0-alpha.68), Telegram (0.10.0-alpha.75) or Google Chat
/// (0.10.0-alpha.80).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReplyKind {
    #[default]
    Mail,
    Slack,
    Teams,
    Telegram,
    GoogleChat,
}

/// Everything the Send panel shows before anything goes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplyEnvelope {
    pub draft_id: String,
    /// Every connected mailbox, the one the message came in to first.
    pub mailboxes: Vec<SendingMailbox>,
    /// The one it goes through unless the user picks another: the first
    /// that can send.
    pub mailbox: Option<String>,
    /// Who it goes to: where the sender asked for replies to go, or the
    /// address they wrote from, or their only address.
    pub to: Vec<String>,
    /// Their other addresses, to send to instead.
    pub other_addresses: Vec<String>,
    /// Everyone else the message was to or copied to, without the user's
    /// own addresses or who it goes to: the ones the user can copy in.
    pub others_on_it: Vec<String>,
    pub subject: String,
    /// Why it can't be sent from here at all, when it can't.
    pub why_not: Option<String>,
    /// A reply to it held to go by itself (`crate::autoreply`), which can be
    /// stopped or sent now.
    pub held: Option<OutboxEntry>,
    /// Mail, or Slack: for Slack, `mailboxes` is the workspace, `to` the
    /// conversation, and there is no subject (`slack::envelope_for`).
    #[serde(default)]
    pub kind: ReplyKind,
}

/// What the user approved: sent exactly as given.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendRequest {
    pub draft_id: String,
    /// The words: the draft as written, or as the user changed it.
    pub text: String,
    /// The mailbox it goes through.
    pub source_id: String,
    pub to: Vec<String>,
    /// The others on the message the user ticked, copied in: only ever from
    /// `ReplyEnvelope::others_on_it`.
    #[serde(default)]
    pub cc: Vec<String>,
    pub subject: String,
}

/// Every connected mailbox, with whether a reply can go out through it.
pub fn sending_mailboxes(db: &Db) -> Result<Vec<SendingMailbox>, DbError> {
    let mine = db.user_identifier_set()?;
    let mut out = Vec::new();
    for s in db.list_sources()?.into_iter().filter(|s| s.connector == imap::CONNECTOR) {
        let config = serde_json::from_value::<ImapSourceConfig>(s.config.clone()).ok();
        let signature = config.as_ref().and_then(|c| c.signature()).map(str::to_string);
        let through_graph = config.as_ref().is_some_and(|c| c.through_graph && c.account.auth == Auth::Microsoft);
        let account = config
            .ok_or(UNREADABLE)
            .and_then(|c| c.sending_account().ok_or(NOT_SET_UP))
            .and_then(|a| if outgoing::is_address(&a.from_address) { Ok(a) } else { Err(NO_FROM) })
            .and_then(|a| if is_users(&mine, &a.from_address) { Ok(a) } else { Err(NOT_YOURS) });
        out.push(match account {
            Ok(a) => SendingMailbox {
                source_id: s.id,
                name: s.name,
                from_address: Some(a.from_address),
                // Sent through Microsoft Graph (0.10.0-alpha.94).
                server: Some(if through_graph { THROUGH_GRAPH.to_string() } else { a.host }),
                why_not: None,
                signature,
            },
            Err(why) => SendingMailbox {
                source_id: s.id,
                name: s.name,
                from_address: None,
                server: None,
                why_not: Some(why.to_string()),
                signature,
            },
        });
    }
    Ok(out)
}

fn is_users(mine: &std::collections::HashSet<String>, address: &str) -> bool {
    mine.contains(&IdentifierInput::new(IdentifierKind::Email, address.trim()).key())
}

/// Who a reply to this draft goes to, how, and through what — or why it
/// can't be sent from here. Reads; changes nothing.
pub fn envelope_for(db: &Db, draft_id: &str) -> Result<ReplyEnvelope, SendError> {
    let draft = db.get_draft(draft_id)?.ok_or_else(|| SendError::Cannot(DRAFT_GONE.into()))?;
    let incoming = answered(db, &draft)?;
    // A message read from Slack is answered in Slack, one from Teams in
    // Teams.
    if let Some(m) = &incoming {
        if let Some(envelope) = slack::envelope_for(db, &draft, m)? {
            return Ok(envelope);
        }
        if let Some(envelope) = teams::envelope_for(db, &draft, m)? {
            return Ok(envelope);
        }
        if let Some(envelope) = telegram::envelope_for(db, &draft, m)? {
            return Ok(envelope);
        }
        if let Some(envelope) = google_chat::envelope_for(db, &draft, m)? {
            return Ok(envelope);
        }
    }
    let held = db.held_outgoing_for_draft(&draft.id)?;
    let mut mailboxes = sending_mailboxes(db)?;
    if let Some(m) = &incoming {
        mailboxes.sort_by_key(|b| b.source_id != m.source_id);
    }
    let mailbox = mailboxes.iter().find(|b| b.from_address.is_some()).map(|b| b.source_id.clone());
    let (to, other_addresses) = addresses_for(db, &draft, incoming.as_ref())?;
    let others_on_it = others_on(db, incoming.as_ref(), &to)?;
    let subject =
        outgoing::reply_subject(incoming.as_ref().and_then(|m| m.metadata.get("subject")).and_then(Value::as_str));
    let why_not = if draft.outcome.is_some() {
        Some(ALREADY_DECIDED.to_string())
    } else if db.unsure_outgoing_for_draft(&draft.id)?.is_some() {
        Some(MAY_HAVE_GONE.to_string())
    } else if held.is_some() {
        Some(HELD.to_string())
    } else if draft.channel != "email" {
        Some(NOT_EMAIL.to_string())
    } else if mailboxes.is_empty() {
        Some(NO_MAILBOX.to_string())
    } else if mailbox.is_none() {
        // Each says why; the first is the one the message came in to.
        mailboxes.first().and_then(|b| b.why_not.clone())
    } else if to.is_empty() && other_addresses.is_empty() {
        Some(NO_ADDRESS.to_string())
    } else {
        None
    };
    Ok(ReplyEnvelope {
        draft_id: draft.id,
        mailboxes,
        mailbox,
        to,
        other_addresses,
        others_on_it,
        subject,
        why_not,
        held,
        kind: ReplyKind::Mail,
    })
}

/// The stored message a draft answers, when it answers one.
pub(crate) fn answered(db: &Db, draft: &Draft) -> Result<Option<Message>, DbError> {
    Ok(draft.incoming_message_id.as_deref().map(|id| db.get_message(id)).transpose()?.flatten())
}

/// `(to, other addresses)`: where the sender asked for replies to go
/// (`Reply-To`), else the address they wrote from — read only from mail,
/// never from a file in Mimic's own format, which could say anything — else
/// the one address Mimic has for them. Any other address of theirs is
/// offered instead.
fn addresses_for(db: &Db, draft: &Draft, incoming: Option<&Message>) -> Result<(Vec<String>, Vec<String>), DbError> {
    let mut chosen: Vec<String> = Vec::new();
    if let Some(m) = incoming {
        let from_mail =
            db.get_source(&m.source_id)?.is_some_and(|s| s.connector == imap::CONNECTOR || s.connector == "mbox");
        if from_mail {
            chosen = addresses_in(&m.metadata, "replyTo");
            if chosen.is_empty() {
                chosen = m
                    .metadata
                    .get("from")
                    .and_then(Value::as_str)
                    .filter(|a| outgoing::is_address(a))
                    .map(|a| vec![a.to_string()])
                    .unwrap_or_default();
            }
        }
    }
    let person = draft.participant_id.as_deref().or(incoming.and_then(|m| m.participant_id.as_deref()));
    let theirs: Vec<String> = match person {
        Some(p) => db
            .get_participant(p)?
            .map(|p| {
                p.identifiers
                    .into_iter()
                    .filter(|i| i.kind == IdentifierKind::Email.as_str() && outgoing::is_address(&i.value))
                    .map(|i| i.value)
                    .collect()
            })
            .unwrap_or_default(),
        None => Vec::new(),
    };
    if chosen.is_empty() && theirs.len() == 1 {
        chosen = theirs.clone();
    }
    let others = theirs.into_iter().filter(|a| !chosen.iter().any(|c| c.eq_ignore_ascii_case(a))).collect();
    Ok((chosen, others))
}

/// Everyone else a message read from mail was to or copied to, in the order
/// its headers give them: without the user's own addresses, without any in
/// `to`, and each once. Nothing for a message from anywhere else.
fn others_on(db: &Db, incoming: Option<&Message>, to: &[String]) -> Result<Vec<String>, DbError> {
    let Some(m) = incoming else { return Ok(Vec::new()) };
    let from_mail =
        db.get_source(&m.source_id)?.is_some_and(|s| s.connector == imap::CONNECTOR || s.connector == "mbox");
    if !from_mail {
        return Ok(Vec::new());
    }
    let mine = db.user_identifier_set()?;
    let mut out: Vec<String> = Vec::new();
    for a in addresses_in(&m.metadata, "to").into_iter().chain(addresses_in(&m.metadata, "cc")) {
        let seen = |list: &[String]| list.iter().any(|b| b.eq_ignore_ascii_case(&a));
        if !is_users(&mine, &a) && !seen(to) && !seen(&out) {
            out.push(a);
        }
    }
    Ok(out)
}

fn addresses_in(metadata: &Value, key: &str) -> Vec<String> {
    metadata
        .get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).filter(|s| outgoing::is_address(s)).map(str::to_string).collect())
        .unwrap_or_default()
}

/// `(In-Reply-To, References)` for a reply to `incoming`: nothing when it has
/// no Message-ID a mail program wrote.
pub(crate) fn threading(incoming: Option<&Message>) -> (Option<String>, Vec<String>) {
    let Some(m) = incoming.filter(|m| outgoing::is_message_id(&m.external_id)) else {
        return (None, Vec::new());
    };
    let refs = addresses_or_ids(&m.metadata, "refs");
    let irt = m.metadata.get("inReplyTo").and_then(Value::as_str);
    (Some(m.external_id.clone()), outgoing::reply_references(&refs, irt, &m.external_id))
}

fn addresses_or_ids(metadata: &Value, key: &str) -> Vec<String> {
    metadata
        .get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

/// A mailbox that can send, and how: refused with the sentence the user is
/// shown when it can't.
pub(crate) fn sending_from(db: &Db, source_id: &str) -> Result<(ImapSourceConfig, SmtpAccount), SendError> {
    let source = db
        .get_source(source_id)?
        .filter(|s| s.connector == imap::CONNECTOR)
        .ok_or_else(|| SendError::Cannot(MAILBOX_GONE.into()))?;
    let config: ImapSourceConfig =
        serde_json::from_value(source.config).map_err(|_| SendError::Cannot(UNREADABLE.into()))?;
    let account = config.sending_account().ok_or_else(|| SendError::Cannot(NOT_SET_UP.into()))?;
    if !outgoing::is_address(&account.from_address) {
        return Err(SendError::Cannot(NO_FROM.into()));
    }
    if !is_users(&db.user_identifier_set()?, &account.from_address) {
        return Err(SendError::Cannot(NOT_YOURS.into()));
    }
    Ok((config, account))
}

/// Send a reply the user approved, exactly as approved. See the module
/// documentation for what is recorded, and when.
pub fn send_reply(db: &Db, credentials: &Credentials, req: &SendRequest) -> Result<OutboxEntry, SendError> {
    let draft = db.get_draft(&req.draft_id)?.ok_or_else(|| SendError::Cannot(DRAFT_GONE.into()))?;
    if draft.outcome.is_some() {
        return Err(SendError::Cannot(ALREADY_DECIDED.into()));
    }
    if draft.channel != "email" {
        return Err(SendError::Cannot(NOT_EMAIL.into()));
    }
    if req.text.trim().is_empty() {
        return Err(SendError::Cannot(EMPTY.into()));
    }
    let to: Vec<String> = req.to.iter().map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect();
    if to.is_empty() {
        return Err(SendError::Cannot(NO_ADDRESS.into()));
    }
    if let Some(bad) = to.iter().find(|a| !outgoing::is_address(a)) {
        return Err(OutgoingError::NotAnAddress(bad.clone()).into());
    }
    let (config, account) = sending_from(db, &req.source_id)?;
    let incoming = answered(db, &draft)?;
    // Only to what the panel offers for this draft (0.10.0-alpha.44): a
    // panel left open on another draft's envelope can't send this one there.
    let (offered, others) = addresses_for(db, &draft, incoming.as_ref())?;
    if !to.iter().all(|a| offered.iter().chain(&others).any(|o| o.eq_ignore_ascii_case(a))) {
        return Err(SendError::Cannot(NOT_OFFERED.into()));
    }
    let cc = copied_in(db, incoming.as_ref(), &to, &req.cc)?;
    let body = signed(&req.text, config.signature());
    let (in_reply_to, references) = threading(incoming.as_ref());
    let subject = match req.subject.trim() {
        "" => outgoing::reply_subject(None),
        s => s.to_string(),
    };
    let mail = Outgoing {
        from: Mailbox {
            name: db.user_identity()?.map(|u| u.display_name).filter(|n| !n.trim().is_empty()),
            address: account.from_address.clone(),
        },
        to: to.iter().map(|a| Mailbox::bare(a)).collect(),
        cc: cc.iter().map(|a| Mailbox::bare(a)).collect(),
        subject,
        body: body.clone(),
        message_id: outgoing::new_message_id(&account.from_address),
        in_reply_to,
        references,
        date: chrono::Utc::now(),
        auto_replied: false,
    };
    // Written out first: a reply that can't be written is refused before
    // anything is recorded or said.
    let bytes = mail.to_bytes()?;
    let entry = db.queue_outgoing(&NewOutgoing {
        draft_id: Some(draft.id.clone()),
        source_id: req.source_id.clone(),
        conversation_id: draft.conversation_id.clone(),
        answers_message_id: incoming.as_ref().map(|m| m.id.clone()),
        participant_id: draft.participant_id.clone(),
        from_address: account.from_address.clone(),
        to,
        cc,
        subject: mail.subject.clone(),
        body,
        message_id: mail.message_id.clone(),
        how: SendHow::Approved,
        send_after: None,
    })?;
    deliver(db, credentials, &config, &account, &entry, &bytes)?;
    file_a_copy(db, credentials, &config, &account, &entry, &bytes);
    if let Err(e) = record_sent_draft(db, &draft, &req.text) {
        // It went; what is recorded about the draft is secondary, and the
        // outbox says it went.
        tracing::warn!(target: "sending", error = %e, "a sent reply's draft could not be marked as sent");
    }
    if let Err(e) = record_in_conversation(db, &req.source_id, &bytes, false) {
        tracing::warn!(target: "sending", error = %e, "a sent reply could not be put in its conversation");
    }
    Ok(db.outgoing(&entry.id)?.unwrap_or(entry))
}

/// The words of a reply as they go: what the user approved or Mimic wrote,
/// then the mailbox's signature under the conventional `-- ` line, which
/// mail programs — and Mimic's own reading of mail
/// (`normalize::strip_signature`) — know is not part of the message.
pub fn signed(text: &str, signature: Option<&str>) -> String {
    match signature {
        Some(sig) => format!("{}\n\n-- \n{sig}", text.trim_end()),
        None => text.to_string(),
    }
}

/// The addresses to copy in, as asked: each one of the others on the
/// message being answered (`others_on`), else `NOT_ON_IT` before anything
/// is recorded; one that is also in `to`, or asked twice, once.
fn copied_in(db: &Db, incoming: Option<&Message>, to: &[String], asked: &[String]) -> Result<Vec<String>, SendError> {
    let asked: Vec<&str> = asked.iter().map(|a| a.trim()).filter(|a| !a.is_empty()).collect();
    if asked.is_empty() {
        return Ok(Vec::new());
    }
    let on_it = others_on(db, incoming, &[])?;
    let mut out: Vec<String> = Vec::new();
    for a in asked {
        let Some(known) = on_it.iter().find(|b| b.eq_ignore_ascii_case(a)) else {
            return Err(SendError::Cannot(NOT_ON_IT.into()));
        };
        let seen = |list: &[String]| list.iter().any(|b| b.eq_ignore_ascii_case(known));
        if !seen(to) && !seen(&out) {
            out.push(known.clone());
        }
    }
    Ok(out)
}

/// A recorded reply written out as it goes now: from the address and words
/// it was recorded with, under the user's name, threaded under the message
/// it answers, dated now, with the Message-ID it was given when it was
/// recorded — and, sent on its own, saying so.
pub(crate) fn written_out(db: &Db, entry: &OutboxEntry, auto_replied: bool) -> Result<Vec<u8>, SendError> {
    let incoming = entry.answers_message_id.as_deref().map(|id| db.get_message(id)).transpose()?.flatten();
    let (in_reply_to, references) = threading(incoming.as_ref());
    let mail = Outgoing {
        from: Mailbox {
            name: db.user_identity()?.map(|u| u.display_name).filter(|n| !n.trim().is_empty()),
            address: entry.from_address.clone(),
        },
        to: entry.to.iter().map(|a| Mailbox::bare(a)).collect(),
        cc: entry.cc.iter().map(|a| Mailbox::bare(a)).collect(),
        subject: entry.subject.clone(),
        body: entry.body.clone(),
        message_id: entry.message_id.clone(),
        in_reply_to,
        references,
        date: chrono::Utc::now(),
        auto_replied,
    };
    Ok(mail.to_bytes()?)
}

/// The server a mailbox sending through Microsoft Graph is said to send
/// through (0.10.0-alpha.94).
pub const THROUGH_GRAPH: &str = "Microsoft 365 itself";

/// Hand a recorded reply to the mail server and record what happened.
pub(crate) fn deliver(
    db: &Db,
    credentials: &Credentials,
    config: &ImapSourceConfig,
    account: &SmtpAccount,
    entry: &OutboxEntry,
    bytes: &[u8],
) -> Result<(), SendError> {
    // Known to send through Microsoft Graph: its organisation turned off
    // sending from apps by SMTP (0.10.0-alpha.94).
    if config.through_graph && config.account.auth == Auth::Microsoft {
        return through_graph(db, credentials, entry, bytes);
    }
    let credential = match credentials.credential(config.account.auth, &entry.source_id) {
        Ok(c) => c,
        Err(why) => {
            db.outgoing_failed(&entry.id, &why)?;
            return Err(SendError::Cannot(why));
        }
    };
    let recipients: Vec<String> = entry.to.iter().chain(&entry.cc).cloned().collect();
    let (sent, said) = crate::sources::transcript::recording(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            smtp::send(account, &credential, &entry.from_address, &recipients, bytes)
        }))
    });
    // Microsoft 365 with sending from apps by SMTP turned off — security
    // defaults turn it off — refused the sign-in, so nothing went: the same
    // message goes through Microsoft Graph instead, now and from now on
    // (0.10.0-alpha.94).
    if config.account.auth == Auth::Microsoft && matches!(&sent, Ok(Err(e)) if smtp::turned_off_for_apps(e)) {
        db.set_source_config_key(&entry.source_id, "throughGraph", &Value::Bool(true))?;
        return through_graph(db, credentials, entry, bytes);
    }
    if !matches!(sent, Ok(Ok(()))) {
        // What the server said, for working out why (never the password or
        // the message: `sources::transcript`).
        let _ = db.log_event(
            &NewEvent::warn("mail", "send_failed", json!({ "server": account.host, "transcript": said }))
                .entity("outbox", &entry.id),
        );
    }
    match sent {
        Ok(Ok(())) => {
            db.outgoing_sent(&entry.id)?;
            Ok(())
        }
        Ok(Err(e)) => {
            // A sign-in the server turned down is not offered again.
            if matches!(e, SmtpError::SignIn(_)) {
                credentials.forget(&entry.source_id);
            }
            if e.may_have_gone() {
                db.outgoing_unsure(&entry.id, &e.to_string())?;
            } else {
                db.outgoing_failed(&entry.id, &e.to_string())?;
            }
            Err(e.into())
        }
        Err(_) => {
            db.outgoing_unsure(&entry.id, PANICKED)?;
            Err(SendError::Cannot(PANICKED.into()))
        }
    }
}

/// Send `bytes` — the message SMTP would have carried — through Microsoft
/// Graph as the mailbox's user (0.10.0-alpha.94), recorded as SMTP's is,
/// with `via` `graph` said first: `sent`, `failed`, or `unsure` when Graph's
/// answer was lost or was its own failure; what Graph said is kept when it
/// fails.
fn through_graph(db: &Db, credentials: &Credentials, entry: &OutboxEntry, bytes: &[u8]) -> Result<(), SendError> {
    let token = match credentials.graph_send_token(&entry.source_id) {
        Ok(t) => t,
        Err(why) => {
            db.outgoing_failed(&entry.id, &why)?;
            return Err(SendError::Cannot(why));
        }
    };
    db.outgoing_via(&entry.id, graph_mail::VIA)?;
    let url = credentials.graph_send_url().to_string();
    let (sent, said) = crate::sources::transcript::recording(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| graph_mail::send(&url, &token, bytes)))
    });
    if !matches!(sent, Ok(Ok(()))) {
        let _ = db.log_event(
            &NewEvent::warn("mail", "send_failed", json!({ "server": "graph.microsoft.com", "transcript": said }))
                .entity("outbox", &entry.id),
        );
    }
    match sent {
        Ok(Ok(())) => {
            db.outgoing_sent(&entry.id)?;
            Ok(())
        }
        Ok(Err(e)) => {
            if matches!(e, GraphSendError::SignIn(_)) {
                credentials.forget(&entry.source_id);
            }
            if e.may_have_gone() {
                db.outgoing_unsure(&entry.id, &e.to_string())?;
                Err(SendError::ServiceUnsure(e.to_string()))
            } else {
                db.outgoing_failed(&entry.id, &e.to_string())?;
                Err(SendError::Service(e.to_string()))
            }
        }
        Err(_) => {
            db.outgoing_unsure(&entry.id, PANICKED)?;
            Err(SendError::Cannot(PANICKED.into()))
        }
    }
}

/// Put a copy of a reply that went in the mailbox's Sent folder, when its
/// provider keeps none (`SmtpAccount::copies_to_sent`). A copy that can't be
/// made costs the copy, not the reply: it is noted on the reply's record
/// (`Db::outgoing_note`), which still says it went.
pub(crate) fn file_a_copy(
    db: &Db,
    credentials: &Credentials,
    config: &ImapSourceConfig,
    account: &SmtpAccount,
    entry: &OutboxEntry,
    bytes: &[u8],
) {
    if !account.copies_to_sent() {
        return;
    }
    // Through Graph: Exchange keeps the copy itself (0.10.0-alpha.94).
    if db.outgoing(&entry.id).ok().flatten().is_some_and(|e| e.via.is_some()) {
        return;
    }
    let copied = credentials.credential(config.account.auth, &entry.source_id).and_then(|c| {
        crate::sources::transcript::kept_if_it_fails(db, "sent_copy_failed", &config.account.host, || {
            imap::copy_to_sent(config, &c, bytes)
        })
        .map_err(|e| e.to_string())
    });
    if let Err(why) = copied {
        tracing::warn!(target: "sending", "a sent reply could not be copied to the Sent folder");
        let note = format!("It went, but I couldn't put a copy in your Sent folder: {why}");
        if let Err(e) = db.outgoing_note(&entry.id, &note) {
            tracing::warn!(target: "sending", error = %e, "could not note that the copy failed");
        }
    }
}

/// Decide a draft as sent with `text`, as Use this does: unedited, or
/// edited with what was changed kept as feedback.
pub fn record_sent_draft(db: &Db, draft: &Draft, text: &str) -> Result<Draft, DbError> {
    let edited = text != draft.generated_text;
    let decided = db.resolve_draft(&draft.id, if edited { "sent_edited" } else { "sent_unedited" }, Some(text))?;
    if edited {
        let diff = crate::generation::feedback::diff_draft(&draft.generated_text, text);
        db.add_draft_feedback(
            &draft.id,
            "edit",
            crate::generation::feedback::EDIT_WEIGHT,
            &serde_json::to_value(&diff)?,
            None,
        )?;
    }
    Ok(decided)
}

/// Put a sent message in its conversation as the user's own, read through
/// the mailbox it went out of exactly as the next check reads it from the
/// Sent folder: joined to the thread by the message it answers, and kept
/// once, by its Message-ID, when that check comes to it.
pub(crate) fn record_in_conversation(db: &Db, source_id: &str, bytes: &[u8], on_its_own: bool) -> Result<(), String> {
    let mut mail = crate::sources::mbox::parse_raw(bytes).ok_or("the sent message has no words in it")?;
    mail.sent = true;
    mail.sent_on_its_own = on_its_own;
    let mut importer = crate::import::Importer::begin(db, source_id).map_err(|e| e.to_string())?;
    for convo in crate::sources::mbox::thread(vec![mail]) {
        if let Err(e) = importer.take(convo) {
            importer.abandon();
            return Err(e.to_string());
        }
    }
    importer.finish().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{NewDraft, NewSource, SendStatus};
    use crate::sources::smtp::test_server::{serve, Behaviour};
    use crate::sources::smtp::SmtpSecurity;
    use serde_json::json;
    use std::sync::Arc;

    fn credentials() -> Credentials {
        Credentials::new(
            Arc::new(|_: &str| Some("app-pass".to_string())),
            Arc::new(|_: &str, _: &str| Ok(())),
            crate::sources::oauth::Provider::microsoft(),
            None,
        )
    }

    struct World {
        db: Db,
        mailbox: String,
        draft: Draft,
        conversation: String,
    }

    const QUESTION: &str = "From: Ada Lovelace <ada@example.com>\r\nReply-To: Ada at work <ada@work.example>\r\nTo: C <c@example.com>, Bob <bob@example.com>\r\nSubject: Offsite\r\nDate: Mon, 2 Mar 2026 09:00:00 +0000\r\nMessage-ID: <q1@x.example>\r\nReferences: <q0@x.example>\r\nIn-Reply-To: <q0@x.example>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\ncan you make the offsite on friday?\r\n";

    /// A mailbox that sends through `port`, with Ada's question read
    /// through it and a draft written for it.
    fn world(port: u16) -> World {
        let db = Db::open_in_memory().unwrap();
        db.set_setting(crate::db::WITHIN_DAYS_SETTING, &0).unwrap();
        db.set_user_identity("C Haynes").unwrap();
        db.add_user_identifier(IdentifierKind::Email, "c@example.com").unwrap();
        let config = ImapSourceConfig {
            account: imap::ImapAccount {
                host: "127.0.0.1".into(),
                port: 1,
                username: "c@example.com".into(),
                security: imap::Security::Plain,
                auth: imap::Auth::Password,
            },
            folders: vec![],
            sent_folder: None,
            marks: Default::default(),
            last_checked_at: None,
            last_read_at: None,
            sending: Some(SmtpAccount {
                host: "127.0.0.1".into(),
                port,
                security: SmtpSecurity::Plain,
                username: "c@example.com".into(),
                from_address: "c@example.com".into(),
                copy_to_sent: Some(false),
            }),
            signature: None,
            also_read: Vec::new(),
            passed_over: Vec::new(),
            through_graph: false,
        };
        let mailbox = db
            .create_source(&NewSource {
                connector: imap::CONNECTOR.into(),
                name: "c@example.com".into(),
                channel: "email".into(),
                location: None,
                config: serde_json::to_value(&config).unwrap(),
            })
            .unwrap()
            .id;
        read_in(&db, &mailbox, QUESTION.as_bytes());
        let (message, conversation, participant): (String, String, Option<String>) = db
            .conn()
            .query_row(
                "SELECT id, conversation_id, participant_id FROM messages WHERE external_id = 'q1@x.example'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        let draft = db
            .create_draft(&NewDraft {
                participant_id: participant,
                conversation_id: Some(conversation.clone()),
                channel: "email".into(),
                situation_id: None,
                incoming_message: Some("can you make the offsite on friday?".into()),
                incoming_message_id: Some(message),
                intent: None,
                generated_text: "Yes, I'll be there.".into(),
                provider: "mock".into(),
                model: "m".into(),
                context: json!({}),
                prompt_hash: "h".into(),
                evidence: json!({}),
                alternative_to: None,
            })
            .unwrap();
        World { db, mailbox, draft, conversation }
    }

    /// Mail read from the inbox, as a check reads it.
    fn read_in(db: &Db, source: &str, raw: &[u8]) {
        let mail = crate::sources::mbox::parse_raw(raw).unwrap();
        let mut importer = crate::import::Importer::begin(db, source).unwrap();
        for convo in crate::sources::mbox::thread(vec![mail]) {
            importer.take(convo).unwrap();
        }
        importer.finish().unwrap();
    }

    fn approved(w: &World, envelope: &ReplyEnvelope, text: &str) -> SendRequest {
        SendRequest {
            draft_id: w.draft.id.clone(),
            text: text.into(),
            source_id: envelope.mailbox.clone().unwrap_or_else(|| w.mailbox.clone()),
            to: envelope.to.clone(),
            cc: vec![],
            subject: envelope.subject.clone(),
        }
    }

    #[test]
    fn a_reply_goes_where_they_asked_threaded_under_their_message_and_is_their_answer_at_once() {
        let (port, seen) = serve(Behaviour::default());
        let w = world(port);
        assert_eq!(w.db.threads_awaiting_reply(10).unwrap().len(), 1, "Ada is waiting");

        let envelope = envelope_for(&w.db, &w.draft.id).unwrap();
        assert_eq!(envelope.why_not, None);
        assert_eq!(envelope.mailbox.as_deref(), Some(w.mailbox.as_str()));
        assert_eq!(envelope.to, vec!["ada@work.example".to_string()], "where she asked for replies to go");
        assert_eq!(envelope.other_addresses, vec!["ada@example.com".to_string()]);
        assert_eq!(envelope.subject, "Re: Offsite");
        assert_eq!(envelope.mailboxes[0].server.as_deref(), Some("127.0.0.1"));

        let sent = send_reply(&w.db, &credentials(), &approved(&w, &envelope, "Yes, I'll be there at nine.")).unwrap();
        assert_eq!(sent.status, SendStatus::Sent);
        assert_eq!(sent.how, SendHow::Approved);
        assert_eq!(sent.to, vec!["ada@work.example".to_string()]);

        let t = seen.recv().unwrap();
        assert!(t.commands.contains(&"RCPT TO:<ada@work.example>".to_string()), "{:?}", t.commands);
        assert!(!t.commands.iter().any(|c| c.contains("bob@")), "a reply goes to who wrote, not to everyone");
        let data = String::from_utf8(t.data).unwrap();
        assert!(data.contains("\r\nIn-Reply-To: <q1@x.example>\r\n"), "{data}");
        assert!(data.contains("\r\nReferences: <q0@x.example> <q1@x.example>\r\n"), "{data}");
        assert!(data.contains("From: C Haynes <c@example.com>\r\n"), "{data}");
        assert!(!data.contains("Auto-Submitted"), "the user sent it");

        let draft = w.db.get_draft(&w.draft.id).unwrap().unwrap();
        assert_eq!(draft.outcome.as_deref(), Some("sent_edited"));
        assert_eq!(draft.final_text.as_deref(), Some("Yes, I'll be there at nine."));
        assert_eq!(w.db.draft_feedback(&w.draft.id).unwrap().len(), 1, "what was changed is learned, as Use this does");

        let own: i64 =
            w.db.conn()
                .query_row(
                    "SELECT COUNT(*) FROM messages WHERE conversation_id = ?1 AND direction = 'self'",
                    [&w.conversation],
                    |r| r.get(0),
                )
                .unwrap();
        assert_eq!(own, 1, "the reply is in her conversation, as the user's");
        assert!(w.db.threads_awaiting_reply(10).unwrap().is_empty(), "and she isn't waiting any more");

        // The Sent folder's copy, read later, is the same message.
        record_in_conversation(
            &w.db,
            &w.mailbox,
            &Outgoing {
                from: Mailbox::bare("c@example.com"),
                to: vec![Mailbox::bare("ada@work.example")],
                cc: vec![],
                subject: "Re: Offsite".into(),
                body: "Yes, I'll be there at nine.".into(),
                message_id: sent.message_id.clone(),
                in_reply_to: Some("q1@x.example".into()),
                references: vec![],
                date: chrono::Utc::now(),
                auto_replied: false,
            }
            .to_bytes()
            .unwrap(),
            false,
        )
        .unwrap();
        let copies: i64 =
            w.db.conn()
                .query_row("SELECT COUNT(*) FROM messages WHERE external_id = ?1", [&sent.message_id], |r| r.get(0))
                .unwrap();
        assert_eq!(copies, 1);

        // A second Send of the same draft sends nothing.
        let again = send_reply(&w.db, &credentials(), &approved(&w, &envelope, "Yes."));
        assert!(matches!(again, Err(SendError::Cannot(ref m)) if m == ALREADY_DECIDED), "{again:?}");
        assert_eq!(w.db.recent_outgoing(10).unwrap().len(), 1);
    }

    #[test]
    fn a_reply_copies_in_only_the_others_on_their_message_the_user_ticked() {
        let (port, seen) = serve(Behaviour::default());
        let w = world(port);
        let envelope = envelope_for(&w.db, &w.draft.id).unwrap();
        assert_eq!(
            envelope.others_on_it,
            vec!["bob@example.com".to_string()],
            "Bob was on it; the user's own address and where it goes aren't offered"
        );

        // Someone who wasn't on the message is refused before anything is
        // recorded or said.
        let stranger = SendRequest { cc: vec!["eve@example.com".into()], ..approved(&w, &envelope, "Yes.") };
        let e = send_reply(&w.db, &credentials(), &stranger).unwrap_err();
        assert!(matches!(e, SendError::Cannot(ref m) if m == NOT_ON_IT), "{e}");
        assert!(w.db.recent_outgoing(10).unwrap().is_empty());

        let everyone = SendRequest {
            cc: vec!["Bob@Example.com".into(), " bob@example.com ".into(), "ada@work.example".into()],
            ..approved(&w, &envelope, "Yes, all of us.")
        };
        let e = send_reply(&w.db, &credentials(), &everyone).unwrap_err();
        assert!(
            matches!(e, SendError::Cannot(ref m) if m == NOT_ON_IT),
            "where she asked for replies wasn't on the message: {e}"
        );
        let everyone = SendRequest { cc: vec!["Bob@Example.com".into(), " bob@example.com ".into()], ..everyone };
        let sent = send_reply(&w.db, &credentials(), &everyone).unwrap();
        assert_eq!(sent.to, vec!["ada@work.example".to_string()]);
        assert_eq!(sent.cc, vec!["bob@example.com".to_string()], "once, as the message had it");

        let t = seen.recv().unwrap();
        assert!(t.commands.contains(&"RCPT TO:<ada@work.example>".to_string()), "{:?}", t.commands);
        assert!(t.commands.contains(&"RCPT TO:<bob@example.com>".to_string()), "{:?}", t.commands);
        assert_eq!(t.commands.iter().filter(|c| c.starts_with("RCPT")).count(), 2, "{:?}", t.commands);
        let data = String::from_utf8(t.data).unwrap();
        assert!(data.contains("\r\nTo: ada@work.example\r\nCc: bob@example.com\r\n"), "{data}");
    }

    #[test]
    fn a_reply_goes_signed_as_its_mailbox_signs_and_its_draft_keeps_only_the_words() {
        let (port, seen) = serve(Behaviour::default());
        let w = world(port);
        let mut config: ImapSourceConfig =
            serde_json::from_value(w.db.get_source(&w.mailbox).unwrap().unwrap().config).unwrap();
        config.signature = Some("  C Haynes\nFormicaria  \n".into());
        w.db.set_source_config(&w.mailbox, &serde_json::to_value(&config).unwrap()).unwrap();

        let envelope = envelope_for(&w.db, &w.draft.id).unwrap();
        assert_eq!(envelope.mailboxes[0].signature.as_deref(), Some("C Haynes\nFormicaria"), "shown before it goes");
        let sent = send_reply(&w.db, &credentials(), &approved(&w, &envelope, "Yes, I'll be there.")).unwrap();
        assert_eq!(sent.body, "Yes, I'll be there.\n\n-- \nC Haynes\nFormicaria", "recorded as it went");

        let data = String::from_utf8(seen.recv().unwrap().data).unwrap();
        assert!(data.contains("Yes, I'll be there.\r\n\r\n--=20\r\nC Haynes\r\nFormicaria\r\n"), "{data}");

        let draft = w.db.get_draft(&w.draft.id).unwrap().unwrap();
        assert_eq!(draft.final_text.as_deref(), Some("Yes, I'll be there."), "the signature isn't the user's words");
        assert_eq!(draft.outcome.as_deref(), Some("sent_unedited"));
        let own: String =
            w.db.conn()
                .query_row(
                    "SELECT body FROM messages WHERE conversation_id = ?1 AND direction = 'self'",
                    [&w.conversation],
                    |r| r.get(0),
                )
                .unwrap();
        assert_eq!(own, "Yes, I'll be there.", "read back as mail is read, without its signature");
        assert_eq!(signed("Hi", None), "Hi");
    }

    #[test]
    fn a_reply_the_server_refuses_is_kept_as_failed_and_its_draft_waits() {
        let (port, _seen) = serve(Behaviour { refuse_rcpt: Some("ada@work.example"), ..Behaviour::default() });
        let w = world(port);
        let envelope = envelope_for(&w.db, &w.draft.id).unwrap();
        let e = send_reply(&w.db, &credentials(), &approved(&w, &envelope, "Yes, I'll be there.")).unwrap_err();
        assert!(matches!(e, SendError::Smtp(SmtpError::Recipient(..))), "{e}");
        assert!(!e.may_have_gone());
        let entry = &w.db.recent_outgoing(10).unwrap()[0];
        assert_eq!(entry.status, SendStatus::Failed);
        assert!(entry.error.as_deref().is_some_and(|m| m.contains("ada@work.example")));
        assert_eq!(w.db.get_draft(&w.draft.id).unwrap().unwrap().outcome, None, "still the user's to send");
        assert_eq!(w.db.threads_awaiting_reply(10).unwrap().len(), 1, "and Ada is still waiting");

        // What the server said is kept for working out why, without the
        // password or the reply.
        let event = w.db.recent_events(10, Some("warn")).unwrap().into_iter().find(|e| e.event_type == "send_failed");
        let payload = event.expect("the failure is recorded").payload_json;
        assert!(payload.contains("smtp > RCPT TO:<ada@work.example>"), "{payload}");
        assert!(payload.contains("smtp < 550"), "{payload}");
        assert!(!payload.contains("app-pass") && !payload.contains("be there"), "{payload}");

        // Sent again once the address is fixed, unedited.
        let (port, _seen) = serve(Behaviour::default());
        let mut config: ImapSourceConfig =
            serde_json::from_value(w.db.get_source(&w.mailbox).unwrap().unwrap().config).unwrap();
        config.sending.as_mut().unwrap().port = port;
        w.db.set_source_config(&w.mailbox, &serde_json::to_value(&config).unwrap()).unwrap();
        let retry =
            SendRequest { to: vec!["ada@example.com".into()], ..approved(&w, &envelope, "Yes, I'll be there.") };
        send_reply(&w.db, &credentials(), &retry).unwrap();
        assert_eq!(w.db.get_draft(&w.draft.id).unwrap().unwrap().outcome.as_deref(), Some("sent_unedited"));
    }

    #[test]
    fn what_a_failed_send_said_goes_with_the_person_it_was_to_and_with_everything() {
        let failed = |db: &Db| {
            db.recent_events(10, Some("warn")).unwrap().into_iter().filter(|e| e.event_type == "send_failed").count()
        };
        for everything in [false, true] {
            let (port, _seen) = serve(Behaviour { refuse_rcpt: Some("ada@work.example"), ..Behaviour::default() });
            let w = world(port);
            let envelope = envelope_for(&w.db, &w.draft.id).unwrap();
            send_reply(&w.db, &credentials(), &approved(&w, &envelope, "Yes.")).unwrap_err();
            assert_eq!(failed(&w.db), 1);
            if everything {
                w.db.delete_all_communication_data().unwrap();
            } else {
                w.db.delete_participant(w.draft.participant_id.as_deref().unwrap()).unwrap();
            }
            assert_eq!(failed(&w.db), 0, "her address was in it");
        }
    }

    #[test]
    fn a_reply_that_may_have_gone_is_not_offered_again_until_the_user_says_it_didnt() {
        let (port, _seen) = serve(Behaviour { drop_after_data: true, ..Behaviour::default() });
        let w = world(port);
        let envelope = envelope_for(&w.db, &w.draft.id).unwrap();
        let e = send_reply(&w.db, &credentials(), &approved(&w, &envelope, "Yes, I'll be there.")).unwrap_err();
        assert!(e.may_have_gone(), "{e}");
        let entry = w.db.recent_outgoing(10).unwrap().remove(0);
        assert_eq!(entry.status, SendStatus::Unsure);
        assert_eq!(w.db.get_draft(&w.draft.id).unwrap().unwrap().outcome, None);
        assert_eq!(envelope_for(&w.db, &w.draft.id).unwrap().why_not.as_deref(), Some(MAY_HAVE_GONE));
        let again = send_reply(&w.db, &credentials(), &approved(&w, &envelope, "Yes, I'll be there."));
        assert!(matches!(again, Err(SendError::Db(DbError::Invalid(ref m))) if m == MAY_HAVE_GONE), "{again:?}");

        // The user looked in their Sent folder: it didn't go.
        w.db.outgoing_did_not_go(&entry.id).unwrap();
        assert_eq!(envelope_for(&w.db, &w.draft.id).unwrap().why_not, None);
    }

    #[test]
    fn a_reply_goes_only_to_an_address_offered_for_its_draft() {
        let (port, _seen) = serve(Behaviour::default());
        let w = world(port);
        let envelope = envelope_for(&w.db, &w.draft.id).unwrap();
        for to in ["bob@example.com", "someone@else.example"] {
            let req = SendRequest { to: vec![to.into()], ..approved(&w, &envelope, "Yes.") };
            let e = send_reply(&w.db, &credentials(), &req).unwrap_err();
            assert!(matches!(e, SendError::Cannot(ref m) if m == NOT_OFFERED), "{to}: {e}");
        }
        assert!(w.db.recent_outgoing(10).unwrap().is_empty(), "nothing was recorded, nothing was said");
        // Either address the panel offers, in any case.
        let req = SendRequest { to: vec!["ADA@example.com".into()], ..approved(&w, &envelope, "Yes.") };
        assert_eq!(send_reply(&w.db, &credentials(), &req).unwrap().status, SendStatus::Sent);
    }

    #[test]
    fn a_reply_is_only_sent_from_an_address_of_the_users_own() {
        let (port, _seen) = serve(Behaviour::default());
        let w = world(port);
        let mut config: ImapSourceConfig =
            serde_json::from_value(w.db.get_source(&w.mailbox).unwrap().unwrap().config).unwrap();
        config.sending.as_mut().unwrap().from_address = "team@example.com".into();
        w.db.set_source_config(&w.mailbox, &serde_json::to_value(&config).unwrap()).unwrap();
        let envelope = envelope_for(&w.db, &w.draft.id).unwrap();
        assert_eq!(envelope.mailbox, None);
        assert_eq!(envelope.why_not.as_deref(), Some(NOT_YOURS));
        let req = SendRequest { source_id: w.mailbox.clone(), ..approved(&w, &envelope, "Yes.") };
        let e =
            send_reply(&w.db, &credentials(), &SendRequest { to: vec!["ada@example.com".into()], ..req }).unwrap_err();
        assert!(matches!(e, SendError::Cannot(ref m) if m == NOT_YOURS), "{e}");
        assert!(w.db.recent_outgoing(10).unwrap().is_empty(), "nothing was recorded, nothing was said");
    }

    #[test]
    fn without_a_mailbox_a_reply_says_how_to_send_one() {
        let (port, _seen) = serve(Behaviour::default());
        let w = world(port);
        // An mbox export rather than a mailbox: nowhere to send from.
        w.db.conn().execute("UPDATE sources SET connector = 'mbox'", []).unwrap();
        let envelope = envelope_for(&w.db, &w.draft.id).unwrap();
        assert!(envelope.mailboxes.is_empty());
        assert_eq!(envelope.why_not.as_deref(), Some(NO_MAILBOX));
        assert_eq!(envelope.to, vec!["ada@work.example".to_string()], "an export's mail is mail all the same");
    }

    /// A Microsoft mailbox whose sign-in comes from `token_url` and whose
    /// replies go through Graph at `graph`.
    fn microsoft(w: &World, token_url: String, graph: &str) -> Credentials {
        let mut config: ImapSourceConfig =
            serde_json::from_value(w.db.get_source(&w.mailbox).unwrap().unwrap().config).unwrap();
        config.account.auth = imap::Auth::Microsoft;
        w.db.set_source_config(&w.mailbox, &serde_json::to_value(&config).unwrap()).unwrap();
        Credentials::new(
            Arc::new(|_: &str| Some("refresh-1".to_string())),
            Arc::new(|_: &str, _: &str| Ok(())),
            crate::sources::oauth::Provider { token_url, ..crate::sources::oauth::Provider::microsoft() },
            Some("m-1".into()),
        )
        .sending_through_graph_at(graph)
    }

    #[test]
    fn microsoft_365_with_sending_from_apps_off_sends_through_graph_and_its_copy_is_kept_once() {
        use crate::sources::teams::fake::{graph as graph_server, TOKEN};
        let (port, seen) = serve(Behaviour { secret: "token-1", turned_off: true, ..Behaviour::default() });
        let w = world(port);
        let (token_url, _forms) = crate::sources::oauth::fake::token_endpoint(vec![
            (200, r#"{"access_token":"token-1","expires_in":3599}"#.into()),
            (200, format!(r#"{{"access_token":"{TOKEN}","expires_in":3599}}"#)),
        ]);
        let handed: Arc<std::sync::Mutex<Vec<u8>>> = Arc::default();
        let kept = handed.clone();
        let (base, _) = graph_server(Box::new(move |path, body| match path {
            "/me/sendMail" => {
                *kept.lock().unwrap() = crate::sources::mime::decode_base64(body);
                (202, String::new())
            }
            _ => (404, "{}".into()),
        }));
        let credentials = microsoft(&w, token_url, &format!("{base}/me/sendMail"));

        let envelope = envelope_for(&w.db, &w.draft.id).unwrap();
        let sent = send_reply(&w.db, &credentials, &approved(&w, &envelope, "Yes, I'll be there at nine.")).unwrap();
        assert_eq!((sent.status, sent.via.as_deref()), (SendStatus::Sent, Some("graph")));
        let t = seen.recv().unwrap();
        assert!(!t.commands.iter().any(|c| c.starts_with("MAIL")), "SMTP sent nothing: {:?}", t.commands);
        let message = String::from_utf8(handed.lock().unwrap().clone()).unwrap();
        assert!(
            message.contains("\r\nIn-Reply-To: <q1@x.example>\r\n"),
            "the same message SMTP would carry: {message}"
        );
        assert!(message.contains(&format!("Message-ID: <{}>", sent.message_id)), "{message}");
        let config: ImapSourceConfig =
            serde_json::from_value(w.db.get_source(&w.mailbox).unwrap().unwrap().config).unwrap();
        assert!(config.through_graph, "from now on, straight through Graph");

        // Its copy in the Sent folder, under an id Exchange gave it: known by
        // its words, and not kept a second time.
        let own = |db: &Db| -> i64 {
            db.conn()
                .query_row(
                    "SELECT COUNT(*) FROM messages WHERE conversation_id = ?1 AND direction = 'self'",
                    [&w.conversation],
                    |r| r.get(0),
                )
                .unwrap()
        };
        assert_eq!(own(&w.db), 1);
        let copy = |id: &str| {
            let date = chrono::Utc::now().to_rfc2822();
            format!(
                "From: C Haynes <c@example.com>\r\nTo: <ada@work.example>\r\nSubject: Re: Offsite\r\nDate: {date}\r\nMessage-ID: <{id}>\r\nIn-Reply-To: <q1@x.example>\r\nReferences: <q0@x.example> <q1@x.example>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nYes, I'll be there at nine.\r\n"
            )
        };
        read_in(&w.db, &w.mailbox, copy("AM0PR01MB0001@eur.prod.outlook.com").as_bytes());
        assert_eq!(own(&w.db), 1, "the copy is the reply already kept");
        let read_back: Option<String> =
            w.db.conn().query_row("SELECT read_back FROM outbox WHERE id = ?1", [&sent.id], |r| r.get(0)).unwrap();
        assert_eq!(read_back.as_deref(), Some("AM0PR01MB0001@eur.prod.outlook.com"));
        // Once: the same words from the user later are theirs.
        read_in(&w.db, &w.mailbox, copy("later@example.com").as_bytes());
        assert_eq!(own(&w.db), 2);
    }

    #[test]
    fn a_reply_whose_answer_from_graph_was_lost_is_found_by_its_copy_and_kept_once() {
        use crate::sources::teams::fake::{graph as graph_server, TOKEN};
        let w = world(1);
        let (token_url, _forms) = crate::sources::oauth::fake::token_endpoint(vec![(
            200,
            format!(r#"{{"access_token":"{TOKEN}","expires_in":3599}}"#),
        )]);
        // No answer: it may have gone.
        let (base, _) = graph_server(Box::new(|_, _| (0, String::new())));
        let credentials = microsoft(&w, token_url, &format!("{base}/me/sendMail"));
        let mut config: ImapSourceConfig =
            serde_json::from_value(w.db.get_source(&w.mailbox).unwrap().unwrap().config).unwrap();
        config.through_graph = true;
        w.db.set_source_config(&w.mailbox, &serde_json::to_value(&config).unwrap()).unwrap();

        let envelope = envelope_for(&w.db, &w.draft.id).unwrap();
        let e = send_reply(&w.db, &credentials, &approved(&w, &envelope, "Yes, see you then.")).unwrap_err();
        assert!(e.may_have_gone(), "{e}");
        let entry = w.db.recent_outgoing(10).unwrap().remove(0);
        assert_eq!((entry.status, entry.via.as_deref()), (SendStatus::Unsure, Some("graph")));

        // It went after all, and Exchange gave it its own id: its copy is
        // the reply, kept once, and the reply is known to have gone.
        let date = chrono::Utc::now().to_rfc2822();
        let copy = format!(
            "From: C Haynes <c@example.com>\r\nTo: <ada@work.example>\r\nSubject: Re: Offsite\r\nDate: {date}\r\nMessage-ID: <AM0PR01MB0002@eur.prod.outlook.com>\r\nIn-Reply-To: <q1@x.example>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nYes, see you then.\r\n"
        );
        read_in(&w.db, &w.mailbox, copy.as_bytes());
        let entry = w.db.outgoing(&entry.id).unwrap().unwrap();
        assert_eq!((entry.status, entry.message_id.as_str()), (SendStatus::Sent, "AM0PR01MB0002@eur.prod.outlook.com"));
        let own: i64 =
            w.db.conn()
                .query_row(
                    "SELECT COUNT(*) FROM messages WHERE conversation_id = ?1 AND direction = 'self'",
                    [&w.conversation],
                    |r| r.get(0),
                )
                .unwrap();
        assert_eq!(own, 1);
    }

    #[test]
    fn a_microsoft_sign_in_from_before_graph_was_asked_for_is_asked_again_and_nothing_goes() {
        let (port, _seen) = serve(Behaviour { secret: "token-1", turned_off: true, ..Behaviour::default() });
        let w = world(port);
        let (token_url, _forms) = crate::sources::oauth::fake::token_endpoint(vec![
            (200, r#"{"access_token":"token-1","expires_in":3599}"#.into()),
            (400, r#"{"error":"invalid_grant","error_description":"AADSTS65001: The user or administrator has not consented to use the application."}"#.into()),
        ]);
        let credentials = microsoft(&w, token_url, "http://127.0.0.1:9/me/sendMail");
        let envelope = envelope_for(&w.db, &w.draft.id).unwrap();
        let e = send_reply(&w.db, &credentials, &approved(&w, &envelope, "Yes.")).unwrap_err();
        assert!(matches!(e, SendError::Cannot(ref why) if why == imap::GRAPH_SIGN_IN_AGAIN), "{e}");
        assert!(!e.may_have_gone());
        let entry = &w.db.recent_outgoing(10).unwrap()[0];
        assert_eq!(entry.status, SendStatus::Failed, "it didn't go, and can be sent again once signed in");
    }
}
