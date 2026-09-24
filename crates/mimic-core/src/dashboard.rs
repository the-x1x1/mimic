//! The dashboard read model: what is waiting on the user, and what Mimic has
//! already written for it.
//!
//! This is the home screen's single query. It exists as one read model rather
//! than five commands so the screen cannot show a half-consistent picture —
//! threads from one moment and drafts from another.
//!
//! Two honesty rules shape it. Nothing here is a live inbox unless a mailbox is
//! connected: every message it reports came from an import or a mailbox check,
//! `last_import_at` says when, and `mail_checking` says whether anything
//! arrives by itself, so the screen never implies mail is arriving when it is
//! not. And a thread is "awaiting a reply" because the last message in it came
//! from someone else and was never answered — no heuristic about urgency, no
//! inferred intent — unless the user said it needs none, its headers say a
//! machine sent it, or it is older than the waiting window the user chose
//! (`db::repo_waiting` has the order). What was left out is counted and can
//! be shown, so leaving something out never hides it.

use serde::Serialize;

use crate::db::{AwaitingReply, Db, DbError, Draft, DraftOutcomes, LeftOut, Participant, ThreadMark};

/// One row of the feed: a conversation waiting on the user, the message it is
/// waiting on, and the draft Mimic has for it, if any.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardThread {
    pub conversation_id: String,
    pub channel: String,
    pub subject: Option<String>,
    pub is_group: bool,
    pub message_count: i64,
    /// The unanswered message, verbatim.
    pub last_message: String,
    pub last_message_at: Option<String>,
    pub last_message_id: String,
    /// Messages of the conversation before the one on screen, and after it.
    /// Those after it are ones that do not decide whether it is waiting —
    /// something that looks automated, or whose writer could not be told —
    /// and they are counted so the screen can show them, never dropped.
    pub earlier: i64,
    pub later: i64,
    /// Who it is from. `None` when the import could not attribute it; the row
    /// still appears, named as unattributed, rather than being hidden.
    pub participant: Option<Participant>,
    /// True when a relationship-layer voice profile exists for this person, so
    /// the screen can say whether a draft would be shaped by how the user
    /// writes *to them* or only by how they write in general.
    pub has_relationship_profile: bool,
    /// An unresolved draft written for `last_message`. Present only because
    /// one was generated — never a placeholder — and never a draft written for
    /// an earlier message in the thread.
    pub draft: Option<Draft>,
    /// Other ways of saying `draft` still on offer beside it, oldest first
    /// (`Db::alternatives_of`). Empty without a draft.
    pub alternatives: Vec<Draft>,
    /// Why the last message looks automated, from its headers, if it does. A
    /// reading, and the screen words it as one.
    pub automated: Option<String>,
    /// What the user said about this thread, while it still applies.
    pub mark: Option<ThreadMark>,
    /// Its last message is older than the waiting window. On a thread that is
    /// waiting, only because the user said it needs a reply.
    pub quiet: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dashboard {
    pub people: i64,
    pub conversations: i64,
    pub messages: i64,
    pub own_messages: i64,
    /// Threads that need a reply, newest first: unanswered, and not marked by
    /// the user as needing none, looking automated, or older than the waiting
    /// window — unless the user said they need one (`db::repo_waiting`).
    pub awaiting: Vec<DashboardThread>,
    /// How many such threads exist in total, which may exceed `awaiting.len()`.
    pub awaiting_total: i64,
    /// Unanswered threads that are not in `awaiting`, counted by reason.
    pub left_out: LeftOut,
    /// Those threads themselves, up to `limit` for each reason, newest first
    /// within it — only when asked for, so the home screen does not load a
    /// thousand newsletters to show a count.
    pub left_out_threads: Vec<DashboardThread>,
    /// Whether `left_out_threads` was asked for. Empty and not asked for are
    /// different answers.
    pub showing_left_out: bool,
    /// How many days back a thread can be waiting; `None` for any age. The
    /// screen names it wherever it says a thread has gone quiet.
    pub waiting_within_days: Option<i64>,
    /// Unresolved drafts, including any not attached to a conversation.
    pub pending_drafts: Vec<Draft>,
    pub outcomes: DraftOutcomes,
    /// When the most recent import finished. `None` before the first one.
    pub last_import_at: Option<String>,
    /// Whether Mimic is allowed to draft replies without being asked each
    /// time. Off unless the user turned it on.
    pub auto_draft: bool,
    /// Whether mail arrives by itself: `None` unless a mailbox is connected,
    /// so the screen can keep saying "nothing arrives on its own" until that
    /// stops being true.
    pub mail_checking: Option<crate::sources::imap::MailChecking>,
}

pub fn dashboard(db: &Db, limit: usize, auto_draft: bool, show_left_out: bool) -> Result<Dashboard, DbError> {
    // One window for the whole screen, so its lists and counts are judged
    // against the same moment and the same setting.
    let window = db.waiting_window()?;
    let awaiting = db
        .threads_awaiting_reply_in(&window, limit)?
        .into_iter()
        .map(|row| thread(db, row))
        .collect::<Result<Vec<_>, _>>()?;
    let left_out_threads = if show_left_out {
        db.threads_left_out_in(&window, limit)?.into_iter().map(|row| thread(db, row)).collect::<Result<Vec<_>, _>>()?
    } else {
        Vec::new()
    };

    let (awaiting_total, left_out) = db.waiting_counts_in(&window)?;
    Ok(Dashboard {
        people: db.count_participants()?,
        conversations: db.count_conversations()?,
        messages: db.count_messages()?,
        own_messages: db.count_self_messages(None, None)?,
        awaiting_total,
        awaiting,
        left_out,
        left_out_threads,
        showing_left_out: show_left_out,
        waiting_within_days: window.within_days,
        pending_drafts: db.pending_drafts(limit)?,
        outcomes: db.measured_draft_outcomes()?,
        last_import_at: db.last_import_at()?,
        auto_draft,
        mail_checking: crate::sources::imap::checking(db)?,
    })
}

fn thread(db: &Db, row: AwaitingReply) -> Result<DashboardThread, DbError> {
    let participant = match row.participant_id.as_deref() {
        Some(id) => db.get_participant(id)?,
        None => None,
    };
    let has_relationship_profile = match participant.as_ref() {
        Some(p) => db.has_relationship_profile(&p.id)?,
        None => false,
    };
    let (earlier, later) = match db.place_in_conversation(&row.conversation.id, &row.last_message_id) {
        Ok(place) => place,
        // Deleted since the list was read — the user removed its person or
        // its mail. The card goes on the next read; the screen doesn't fail.
        Err(DbError::NotFound(_)) => (0, 0),
        Err(e) => return Err(e),
    };
    let draft = db.pending_draft_for_message(&row.conversation.id, &row.last_message_id, &row.last_message)?;
    let alternatives = match &draft {
        Some(d) => db.alternatives_of(&d.id)?,
        None => Vec::new(),
    };
    Ok(DashboardThread {
        draft,
        alternatives,
        conversation_id: row.conversation.id,
        channel: row.conversation.channel,
        subject: row.conversation.subject,
        is_group: row.conversation.is_group,
        message_count: row.conversation.message_count,
        last_message: row.last_message,
        last_message_at: row.last_message_at,
        last_message_id: row.last_message_id,
        earlier,
        later,
        participant,
        has_relationship_profile,
        automated: row.automated,
        mark: row.mark,
        quiet: row.quiet,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::repo_people::IdentifierInput;
    use crate::db::{IdentifierKind, NewDraft, NewMessage, NewSource};
    use serde_json::{json, Value};

    fn db_with_thread() -> (Db, String, String, String) {
        let db = Db::open_in_memory().unwrap();
        let source = db
            .create_source(&NewSource {
                connector: "test".into(),
                name: "T".into(),
                channel: "chat".into(),
                location: None,
                config: Value::Null,
            })
            .unwrap();
        // Dated September 2026; the window is measured against the clock, so
        // these rules are tested at any age (`repo_waiting` tests the window).
        db.set_setting(crate::db::WITHIN_DAYS_SETTING, &0).unwrap();
        let convo = db.upsert_conversation(&source.id, "t1", "chat", Some("Lunch")).unwrap();
        let ada =
            db.resolve_participant("Ada", &[IdentifierInput::new(IdentifierKind::Handle, "@ada")], false).unwrap();
        db.link_conversation_participant(&convo, &ada).unwrap();
        (db, source.id, convo, ada)
    }

    fn msg(source: &str, convo: &str, who: Option<&str>, ext: &str, dir: &str, seq: i64, body: &str) -> NewMessage {
        NewMessage {
            conversation_id: convo.into(),
            source_id: source.into(),
            participant_id: who.map(str::to_string),
            external_id: ext.into(),
            direction: dir.into(),
            channel: "chat".into(),
            sent_at: Some(format!("2026-09-{:02}T10:00:00Z", seq + 1)),
            sequence_index: seq,
            body: body.into(),
            reply_to_external_id: None,
            metadata: Value::Null,
        }
    }

    #[test]
    fn a_thread_waits_only_while_its_last_message_is_someone_elses() {
        let (db, source, convo, ada) = db_with_thread();
        db.insert_messages(&[
            msg(&source, &convo, None, "m1", "self", 0, "are we still on for lunch"),
            msg(&source, &convo, Some(&ada), "m2", "other", 1, "can we move it to thursday?"),
        ])
        .unwrap();
        db.refresh_conversation_stats(&convo).unwrap();

        let waiting = db.threads_awaiting_reply(10).unwrap();
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].last_message, "can we move it to thursday?");
        assert_eq!(waiting[0].participant_id.as_deref(), Some(ada.as_str()));

        // Answering it takes the thread off the list; nothing else changed.
        db.insert_messages(&[msg(&source, &convo, None, "m3", "self", 2, "thursday works")]).unwrap();
        assert!(db.threads_awaiting_reply(10).unwrap().is_empty());
        assert_eq!(db.count_threads_awaiting_reply().unwrap(), 0);
    }

    #[test]
    fn a_card_offers_its_draft_with_the_other_ways_of_saying_it_until_one_is_used() {
        let (db, source, convo, ada) = db_with_thread();
        db.insert_messages(&[msg(&source, &convo, Some(&ada), "m1", "other", 0, "lunch on thursday?")]).unwrap();
        db.refresh_conversation_stats(&convo).unwrap();
        let waiting = db.threads_awaiting_reply(10).unwrap().remove(0);
        let new = |text: &str, beside: Option<&str>| NewDraft {
            participant_id: Some(ada.clone()),
            conversation_id: Some(convo.clone()),
            channel: "chat".into(),
            situation_id: None,
            incoming_message: Some(waiting.last_message.clone()),
            incoming_message_id: Some(waiting.last_message_id.clone()),
            intent: None,
            generated_text: text.into(),
            provider: "mock".into(),
            model: "m".into(),
            context: json!({}),
            prompt_hash: "h".into(),
            evidence: json!({}),
            alternative_to: beside.map(str::to_string),
        };
        let first = db.create_draft(&new("yes, thursday works", None)).unwrap();
        let shorter = db.create_draft(&new("thursday!", Some(&first.id))).unwrap();
        let longer = db.create_draft(&new("yes, thursday works well for me. same place?", Some(&first.id))).unwrap();
        let card = || dashboard(&db, 10, false, false).unwrap().awaiting.remove(0);
        let ids = |ds: &[Draft]| ds.iter().map(|d| d.id.clone()).collect::<Vec<_>>();

        let c = card();
        assert_eq!(
            c.draft.as_ref().map(|d| d.id.clone()),
            Some(first.id.clone()),
            "the first, never another way of it"
        );
        assert_eq!(ids(&c.alternatives), [shorter.id.clone(), longer.id.clone()]);
        assert_eq!(dashboard(&db, 10, false, false).unwrap().pending_drafts.len(), 1);

        // One other way put aside: the rest are still on offer, and the
        // message still waits for a choice.
        db.resolve_draft(&shorter.id, "discarded", None).unwrap();
        let c = card();
        assert_eq!(c.draft.as_ref().map(|d| d.id.clone()), Some(first.id.clone()));
        assert_eq!(ids(&c.alternatives), std::slice::from_ref(&longer.id));

        // Another way used: the message is dealt with.
        db.resolve_draft(&longer.id, "sent_unedited", Some(&longer.generated_text)).unwrap();
        let c = card();
        assert!(c.draft.is_none() && c.alternatives.is_empty());
    }

    #[test]
    fn a_message_of_unknown_direction_does_not_decide_either_way() {
        let (db, source, convo, ada) = db_with_thread();
        db.insert_messages(&[
            msg(&source, &convo, Some(&ada), "m1", "other", 0, "you around?"),
            // An unattributable message arriving last must not make the thread
            // look answered — direction was never established.
            msg(&source, &convo, None, "m2", "unknown", 1, "(unattributed)"),
        ])
        .unwrap();
        db.refresh_conversation_stats(&convo).unwrap();
        let waiting = db.threads_awaiting_reply(10).unwrap();
        assert_eq!(waiting.len(), 1, "the last message with a known direction is theirs");
        assert_eq!(waiting[0].last_message, "you around?");

        // What came after it is counted on the card, so the screen can show
        // it rather than lose it.
        let card = &dashboard(&db, 10, false, false).unwrap().awaiting[0];
        assert_eq!((card.earlier, card.later), (0, 1));
    }

    #[test]
    fn the_dashboard_counts_what_is_there_and_claims_nothing_else() {
        let (db, source, convo, ada) = db_with_thread();
        let empty = dashboard(&db, 10, false, false).unwrap();
        assert_eq!(empty.messages, 0);
        assert!(empty.awaiting.is_empty());
        assert!(empty.pending_drafts.is_empty());
        assert_eq!(empty.last_import_at, None, "nothing has been imported yet and the screen must say so");
        assert_eq!(empty.waiting_within_days, None, "any age, as set above");
        assert_eq!(empty.outcomes.unedited_rate, None, "unmeasured, not zero");
        assert!(!empty.auto_draft);

        db.insert_messages(&[
            msg(&source, &convo, None, "m1", "self", 0, "hello"),
            msg(&source, &convo, Some(&ada), "m2", "other", 1, "hello back"),
        ])
        .unwrap();
        db.refresh_conversation_stats(&convo).unwrap();

        let view = dashboard(&db, 10, true, false).unwrap();
        assert_eq!(view.messages, 2);
        assert_eq!(view.own_messages, 1);
        assert_eq!(view.conversations, 1);
        assert_eq!(view.people, 1);
        assert_eq!(view.awaiting.len(), 1);
        assert_eq!(view.awaiting_total, 1);
        let thread = &view.awaiting[0];
        assert_eq!(thread.participant.as_ref().unwrap().display_name, "Ada");
        assert!(!thread.has_relationship_profile, "one message is not a profile");
        assert!(thread.draft.is_none(), "no draft exists until one is generated");
        assert!(view.auto_draft);
    }

    #[test]
    fn a_thread_that_has_gone_quiet_is_counted_and_named_with_its_window() {
        let (db, source, convo, ada) = db_with_thread();
        db.set_setting(crate::db::WITHIN_DAYS_SETTING, &30).unwrap();
        let mut old = msg(&source, &convo, Some(&ada), "m1", "other", 0, "any news on the lease?");
        old.sent_at = Some(crate::ids::fmt_rfc3339(chrono::Utc::now() - chrono::Duration::days(60)));
        db.insert_messages(&[old]).unwrap();
        db.refresh_conversation_stats(&convo).unwrap();

        let view = dashboard(&db, 10, false, true).unwrap();
        assert!(view.awaiting.is_empty());
        assert_eq!(view.awaiting_total, 0);
        assert_eq!(view.left_out.quiet, 1);
        assert_eq!(view.waiting_within_days, Some(30));
        assert_eq!(view.left_out_threads.len(), 1);
        assert!(view.left_out_threads[0].quiet);
        assert_eq!(view.left_out_threads[0].participant.as_ref().unwrap().display_name, "Ada");
    }
}
