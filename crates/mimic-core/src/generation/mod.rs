//! Turning "reply to this, saying roughly that" into a draft.
//!
//! Three stages, deliberately separate so each can be tested on its own:
//!
//! 1. `build_context` gathers evidence — the resolved voice layers, the
//!    retrieved past exchanges, the tail of the conversation, the person.
//!    It touches the database and no model.
//! 2. `assemble` turns that evidence into a prompt. It is a pure function:
//!    same context in, same prompt out, which is what makes the prompt
//!    testable and the `prompt_hash` meaningful.
//! 3. `compose` runs the provider and records the draft.
//!
//! The intent is not a footnote. A reply assistant that only sees the incoming
//! message can do no better than guess what the user wants to say; the whole
//! point is that the user supplies the *what* and Mimic supplies the *how*.

pub mod feedback;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::db::{Db, DbError, Draft, Message, NewDraft, Participant};
use crate::providers::{GenerationRequest, ModelProvider, PromptMessage, ProviderError};
use crate::retrieval::{self, RetrievalFilter, RetrievedExchange};
use crate::voice::{self, metrics::VoiceMetrics, ResolvedVoice};

/// How many past exchanges go in the prompt. Enough to show a register, few
/// enough that the model still answers the question in front of it.
const EXAMPLES: usize = 5;
/// How much of the current conversation goes in.
const TRANSCRIPT: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Adjustment {
    Shorter,
    Longer,
    MoreCasual,
    MoreProfessional,
}

impl Adjustment {
    fn instruction(self) -> &'static str {
        match self {
            Adjustment::Shorter => "Make this noticeably shorter than you otherwise would, without dropping anything the intent asks for.",
            Adjustment::Longer => "Give this a little more room than you otherwise would — another sentence or two of substance, not padding.",
            Adjustment::MoreCasual => "Pitch this more casually than the profile suggests, while still sounding like the same person.",
            Adjustment::MoreProfessional => "Pitch this more formally than the profile suggests, while still sounding like the same person.",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ComposeRequest {
    pub participant_id: Option<String>,
    pub conversation_id: Option<String>,
    pub channel: String,
    /// What the user is replying to. Empty when starting a conversation.
    pub incoming_message: Option<String>,
    /// What the user wants to say. This is the load-bearing input.
    pub intent: Option<String>,
    pub situation_id: Option<String>,
    pub adjustment: Option<Adjustment>,
}

/// Where the situation a draft is written for came from. The two are kept
/// apart because only one of them is something the user said.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SituationSource {
    /// The user picked it.
    Chosen,
    /// Mimic read it from the user's note, by rule. A reading, not a fact.
    FromNote,
}

/// What this reply is doing, when that is known, and how it came to be known.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SituationChoice {
    pub id: String,
    pub label: String,
    pub source: SituationSource,
    /// The phrase in the note that the reading rests on, when it was read.
    pub cue: Option<String>,
}

/// Decide what the reply is doing. A situation the user chose wins; failing
/// that, their note is read by the same rules that file their messages; with
/// no note there is nothing to read, and nothing is guessed from the incoming
/// message — what they asked is not what the user has decided to answer.
pub fn choose_situation(req: &ComposeRequest) -> Result<Option<SituationChoice>, DbError> {
    if let Some(id) = req.situation_id.as_deref().filter(|s| !s.is_empty()) {
        if let Some(b) = crate::situations::builtin(id) {
            return Ok(Some(SituationChoice {
                id: b.id.into(),
                label: b.label.into(),
                source: SituationSource::Chosen,
                cue: None,
            }));
        }
        return Err(DbError::Invalid(format!("{id:?} is not a situation Mimic knows")));
    }
    let Some(note) = req.intent.as_deref().filter(|s| !s.trim().is_empty()) else { return Ok(None) };
    Ok(crate::situations::classify_note(note).and_then(|c| {
        crate::situations::builtin(&c.situation_id).map(|b| SituationChoice {
            id: b.id.into(),
            label: b.label.into(),
            source: SituationSource::FromNote,
            cue: Some(c.cue),
        })
    }))
}

/// Everything gathered before a model is involved.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationContext {
    pub participant: Option<Participant>,
    pub channel: String,
    pub voice: ResolvedVoice,
    pub effective: VoiceMetrics,
    pub examples: Vec<RetrievedExchange>,
    pub transcript: Vec<Message>,
    /// What the reply is doing, when that is known.
    pub situation: Option<SituationChoice>,
    /// Patterns in what the user changed in earlier drafts, that passed the
    /// threshold and apply to this recipient.
    pub learned: Vec<crate::learning::LearnedPattern>,
    /// Plain-language statements of what this draft is based on. Shown in the
    /// Compose evidence panel verbatim, so they are written for a person.
    pub evidence: Vec<String>,
}

pub fn build_context(db: &Db, req: &ComposeRequest) -> Result<GenerationContext, DbError> {
    let channel = if crate::db::channel_is_known(&req.channel) { req.channel.clone() } else { "other".to_string() };
    let participant = match &req.participant_id {
        Some(id) => db.get_participant(id)?,
        None => None,
    };
    let situation = choose_situation(req)?;
    let voice = voice::resolve(db, &channel, req.participant_id.as_deref(), situation.as_ref().map(|s| s.id.as_str()))?;
    let effective = voice::effective_metrics(&voice);

    let incoming = req.incoming_message.as_deref().unwrap_or_default();
    let base = RetrievalFilter {
        participant_id: req.participant_id.clone(),
        channel: Some(channel.clone()),
        ..Default::default()
    };
    let mut examples: Vec<RetrievedExchange> = Vec::new();
    if let Some(sit) = &situation {
        // Times the user did this same thing with this person on this
        // channel first; then, if there are hardly any, a couple from anyone,
        // because how someone says no carries across more than what they say.
        let done = crate::situations::builtin(&sit.id).map(|b| b.done).unwrap_or("did this");
        let narrow = RetrievalFilter { situation_id: Some(sit.id.clone()), ..base.clone() };
        let mut found = retrieval::retrieve(db, incoming, &narrow, EXAMPLES)?;
        if found.len() < 2 {
            let wide = RetrievalFilter { situation_id: Some(sit.id.clone()), ..Default::default() };
            found.extend(retrieval::retrieve(db, incoming, &wide, 2)?);
        }
        for mut e in found {
            if examples.len() < EXAMPLES && !examples.iter().any(|x| x.reply_message_id == e.reply_message_id) {
                e.reason = format!("a time you {done}; {}", e.reason);
                examples.push(e);
            }
        }
    }
    for e in retrieval::retrieve(db, incoming, &base, EXAMPLES)? {
        if examples.len() < EXAMPLES && !examples.iter().any(|x| x.reply_message_id == e.reply_message_id) {
            examples.push(e);
        }
    }
    // With nothing written to this person on this channel, widen rather than
    // hand the model nothing: the global voice is still the user's voice.
    if examples.is_empty() {
        examples = retrieval::retrieve(db, incoming, &RetrievalFilter::default(), EXAMPLES)?;
    }

    let conversation_id = match (&req.conversation_id, &req.participant_id) {
        (Some(c), _) => Some(c.clone()),
        (None, Some(p)) => db.latest_conversation_with(p)?.map(|c| c.id),
        _ => None,
    };
    let transcript = match &conversation_id {
        Some(c) => db.conversation_tail(c, TRANSCRIPT)?,
        None => Vec::new(),
    };

    let mut evidence = Vec::new();
    match voice.layers.iter().find(|l| l.layer == "relationship") {
        Some(l) if l.measurable => {
            evidence.push(format!("{} of your messages to this person", l.sample_size));
        }
        _ => {
            if participant.is_some() {
                evidence.push(
                    "No profile for this person yet — using how you write generally, not how you write to them".into(),
                );
            }
        }
    }
    match voice.layers.iter().find(|l| l.layer == "channel") {
        Some(l) if l.measurable => evidence.push(format!("{} of your {channel} messages", l.sample_size)),
        _ => {}
    }
    match voice.layers.iter().find(|l| l.layer == "global") {
        Some(l) if l.measurable => evidence.push(format!("{} of your messages overall", l.sample_size)),
        _ => evidence.push("Not enough of your own writing has been imported to describe your style yet".into()),
    }
    if let Some(sit) = &situation {
        let b = crate::situations::builtin(&sit.id);
        let done = b.map(|b| b.done).unwrap_or("did this");
        let what = sit.label.to_lowercase();
        let layer = voice.layers.iter().find(|l| l.layer == "situational");
        let why = match (sit.source, &sit.cue) {
            (SituationSource::Chosen, _) => format!("You said this reply is {what}"),
            (SituationSource::FromNote, Some(cue)) => format!("Your note reads like {what} (\"{cue}\")"),
            (SituationSource::FromNote, None) => format!("Your note reads like {what}"),
        };
        evidence.push(match layer {
            Some(l) if l.measurable => format!("{why}, so I leaned on {} times you {done}", l.sample_size),
            Some(l) => format!(
                "{why}, but I've only seen you do that {} — not enough to go on, so this uses how you write generally",
                if l.sample_size == 1 { "once".to_string() } else { format!("{} times", l.sample_size) }
            ),
            None => format!("{why}, but I haven't seen you do that yet, so this uses how you write generally"),
        });
    }
    if !examples.is_empty() {
        evidence.push(format!("{} past replies of yours, chosen by similarity", examples.len()));
    }
    let learned = crate::learning::applicable(db, req.participant_id.as_deref())?;
    if !learned.is_empty() {
        evidence.push(format!(
            "{} you've made to my drafts before: {}",
            if learned.len() == 1 { "A change".to_string() } else { format!("{} changes", learned.len()) },
            learned.iter().map(|p| crate::learning::short_label(p)).collect::<Vec<_>>().join(", ")
        ));
    }
    if !voice.overrides.is_empty() {
        evidence.push(format!(
            "{} you've told me directly",
            if voice.overrides.len() == 1 {
                "One thing".to_string()
            } else {
                format!("{} things", voice.overrides.len())
            }
        ));
    }
    if voice.layers.iter().any(|l| l.stale) {
        evidence.push("Your profile is out of date — messages have been imported since it was built".into());
    }

    Ok(GenerationContext { participant, channel, voice, effective, examples, transcript, situation, learned, evidence })
}

/// Build the prompt. Pure: the same context always produces the same request,
/// which is what makes `prompt_hash` worth recording.
pub fn assemble(ctx: &GenerationContext, req: &ComposeRequest) -> GenerationRequest {
    let mut system = String::new();
    system.push_str(
        "You are drafting a reply *as the user*, in their own voice. You are not an assistant writing on their behalf and you never mention being one. Write only the message body: no subject line, no preamble, no explanation, no quotation marks around it.\n\n",
    );

    if ctx.effective.measurable {
        system.push_str("How this person writes, measured from their own messages:\n");
        for line in describe(&ctx.effective) {
            system.push_str("- ");
            system.push_str(&line);
            system.push('\n');
        }
    } else {
        system.push_str(
            "There is not yet enough of this person's own writing to describe their style. Write plainly and briefly, and do not invent mannerisms.\n",
        );
    }

    if !ctx.learned.is_empty() {
        system.push_str(
            "\nWhat they changed in earlier drafts Mimic wrote for them, which adjusts the measurements above:\n",
        );
        for p in &ctx.learned {
            system.push_str("- ");
            system.push_str(&crate::learning::instruction(p));
            system.push('\n');
        }
    }

    if !ctx.voice.overrides.is_empty() {
        system.push_str("\nThings they have told Mimic directly, which override everything above:\n");
        for (key, value) in &ctx.voice.overrides {
            system.push_str("- ");
            system.push_str(&crate::learning::preference_text(key, value));
            system.push('\n');
        }
    }

    if !ctx.examples.is_empty() {
        system.push_str("\nMessages this person actually sent. Match their register, length and punctuation; do not reuse their content:\n");
        for ex in &ctx.examples {
            if let Some(incoming) = &ex.incoming {
                system.push_str(&format!("\n--- in reply to: {}\n", one_line(incoming, 200)));
            } else {
                system.push_str("\n---\n");
            }
            system.push_str(&one_line(&ex.reply, 600));
            system.push('\n');
        }
    }

    if let Some(p) = &ctx.participant {
        system.push_str(&format!("\nYou are writing to {}", p.display_name));
        if let Some(r) = &p.relationship {
            system.push_str(&format!(", who they describe as: {r}"));
        }
        system.push_str(&format!(". The channel is {}.\n", ctx.channel));
    } else {
        system.push_str(&format!("\nThe channel is {}.\n", ctx.channel));
    }

    if let Some((s, b)) = ctx.situation.as_ref().and_then(|s| crate::situations::builtin(&s.id).map(|b| (s, b))) {
        // Only something the user chose is stated as a fact. A reading of
        // their note is passed on as a reading, with the note itself in charge.
        match s.source {
            SituationSource::Chosen => system.push_str(&format!("\nIn this reply they are {}.", b.doing)),
            SituationSource::FromNote => system.push_str(&format!(
                "\nTheir note reads as though they are {} here. Follow the note itself if it says otherwise.",
                b.doing
            )),
        }
        let measured = ctx.voice.layers.iter().any(|l| l.layer == "situational" && l.measurable);
        if measured {
            system.push_str(" The measurements above already reflect how they write when they do that.");
        }
        system.push('\n');
    }

    if let Some(a) = req.adjustment {
        system.push('\n');
        system.push_str(a.instruction());
        system.push('\n');
    }

    let mut messages: Vec<PromptMessage> = Vec::new();
    for m in &ctx.transcript {
        let content = one_line(&m.body, 1200);
        messages.push(if m.direction == "self" {
            PromptMessage::assistant(content)
        } else {
            PromptMessage::user(content)
        });
    }

    let mut task = String::new();
    if let Some(incoming) = req.incoming_message.as_deref().filter(|s| !s.trim().is_empty()) {
        task.push_str("Reply to this message:\n\n");
        task.push_str(incoming.trim());
        task.push_str("\n\n");
    } else {
        task.push_str("Start a new message.\n\n");
    }
    match req.intent.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(intent) => {
            task.push_str("What to say, in their words:\n");
            task.push_str(intent);
        }
        None => task.push_str(
            "No intent was given, so answer the message directly and briefly, and do not commit them to anything they have not said.",
        ),
    }
    messages.push(PromptMessage::user(task));

    GenerationRequest {
        system,
        messages,
        max_output_tokens: max_tokens(&ctx.effective, req.adjustment),
        temperature: 0.7,
    }
}

/// Turn measured numbers into instructions a model can follow. Only metrics
/// that were actually measured produce a line, and each line says what the
/// number means rather than quoting it raw.
fn describe(m: &VoiceMetrics) -> Vec<String> {
    let mut out = Vec::new();
    if let (Some(median), Some(p90)) = (m.median_words_per_message, m.p90_words_per_message) {
        out.push(format!(
            "Their typical message is about {median:.0} words, and rarely goes past {p90:.0}. Aim for the typical length unless the intent needs more."
        ));
    }
    if let Some(r) = m.terminal_period_rate {
        out.push(match r {
            r if r >= 0.8 => "They end messages with a full stop.".into(),
            r if r <= 0.2 => "They usually do not put a full stop at the end.".into(),
            _ => format!("They end about {:.0}% of messages with a full stop.", r * 100.0),
        });
    }
    if let Some(r) = m.lowercase_start_rate {
        if r >= 0.6 {
            out.push("They usually start messages in lowercase.".into());
        } else if r <= 0.15 {
            out.push("They capitalize the first word.".into());
        }
    }
    if let Some(r) = m.emoji_rate {
        out.push(match r {
            r if r <= 0.02 => "They do not use emoji. Do not add any.".into(),
            r if r >= 0.5 => "They use emoji often — about one message in two.".into(),
            r => format!("They use emoji occasionally, in roughly {:.0}% of messages.", r * 100.0),
        });
    }
    if let Some(r) = m.contractions_per_100_words {
        if r >= 4.0 {
            out.push("They use contractions freely (don't, I'll, that's).".into());
        } else if r <= 0.5 {
            out.push("They tend to write words out in full rather than contracting them.".into());
        }
    }
    if let Some(r) = m.exclamation_rate {
        if r <= 0.02 {
            out.push("They do not use exclamation marks.".into());
        } else if r >= 0.3 {
            out.push("Exclamation marks are normal for them.".into());
        }
    }
    if let Some(r) = m.multi_paragraph_rate {
        if r <= 0.05 {
            out.push("They write in a single block, not multiple paragraphs.".into());
        } else if r >= 0.5 {
            out.push("They usually break a message into paragraphs.".into());
        }
    }
    match m.greeting_rate {
        Some(r) if r <= 0.1 => out.push("They open straight into the message, with no greeting.".into()),
        Some(r) if r >= 0.5 && !m.top_greetings.is_empty() => {
            out.push(format!("They usually open with a greeting — most often \"{}\".", m.top_greetings[0].0))
        }
        _ => {}
    }
    match m.sign_off_rate {
        Some(r) if r <= 0.1 => out.push("They do not sign off.".into()),
        Some(r) if r >= 0.5 && !m.top_sign_offs.is_empty() => {
            out.push(format!("They usually close with \"{}\".", m.top_sign_offs[0].0))
        }
        _ => {}
    }
    if !m.top_phrases.is_empty() {
        let phrases: Vec<String> = m.top_phrases.iter().take(5).map(|(p, _)| format!("\"{p}\"")).collect();
        out.push(format!(
            "Turns of phrase they repeat: {}. Use them where they fit naturally, not everywhere.",
            phrases.join(", ")
        ));
    }
    out
}

/// A budget proportional to how long this person's messages actually are, so
/// a model cannot answer a two-word texter with four paragraphs.
fn max_tokens(m: &VoiceMetrics, adjustment: Option<Adjustment>) -> u32 {
    let words = m.p90_words_per_message.unwrap_or(120.0).max(20.0);
    let base = (words * 2.5).clamp(64.0, 1200.0) as u32;
    match adjustment {
        Some(Adjustment::Longer) => (base * 2).min(2000),
        Some(Adjustment::Shorter) => (base / 2).max(48),
        _ => base,
    }
}

fn one_line(text: &str, limit: usize) -> String {
    let collapsed = text.trim();
    if collapsed.chars().count() <= limit {
        return collapsed.to_string();
    }
    format!("{}…", collapsed.chars().take(limit).collect::<String>())
}

/// Build the context, run the provider, record the draft.
pub fn compose(db: &Db, provider: &dyn ModelProvider, req: &ComposeRequest) -> Result<Draft, GenerationError> {
    let ctx = build_context(db, req)?;
    let prompt = assemble(&ctx, req);
    let prompt_hash = crate::ids::stable_json_hash(&serde_json::to_value(&prompt).unwrap_or(json!({})));
    let response = provider.generate(&prompt)?;
    let info = provider.info();
    let draft = db.create_draft(&NewDraft {
        participant_id: req.participant_id.clone(),
        conversation_id: req.conversation_id.clone(),
        channel: ctx.channel.clone(),
        situation_id: ctx.situation.as_ref().map(|s| s.id.clone()),
        incoming_message: req.incoming_message.clone(),
        intent: req.intent.clone(),
        generated_text: response.text.clone(),
        provider: info.id,
        model: response.model.clone(),
        context: json!({
            "layers": ctx.voice.layers.iter().map(|l| json!({"layer": l.layer, "sampleSize": l.sample_size, "measurable": l.measurable})).collect::<Vec<_>>(),
            "adjustment": req.adjustment,
            "examples": ctx.examples.len(),
            "situation": ctx.situation,
            "learned": ctx.learned.iter().map(|p| json!({"habit": p.habit, "direction": p.direction, "agreeing": p.agreeing})).collect::<Vec<_>>(),
        }),
        prompt_hash,
        evidence: json!({
            "statements": ctx.evidence,
            "examples": ctx.examples.iter().map(|e| json!({"messageId": e.reply_message_id, "reason": e.reason, "score": e.score})).collect::<Vec<_>>(),
        }),
    })?;
    Ok(draft)
}

#[derive(Debug, thiserror::Error)]
pub enum GenerationError {
    #[error("db: {0}")]
    Db(#[from] DbError),
    #[error("{0}")]
    Provider(#[from] ProviderError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{IdentifierInput, IdentifierKind, NewMessage, NewSource, VoiceLayer};
    use crate::providers::mock::MockProvider;

    fn seeded(n: usize, body: &str) -> (Db, String) {
        let db = Db::open_in_memory().unwrap();
        db.set_user_identity("C").unwrap();
        db.add_user_identifier(IdentifierKind::Handle, "@c").unwrap();
        let source = db
            .create_source(&NewSource {
                connector: "t".into(),
                name: "chat".into(),
                channel: "chat".into(),
                location: None,
                config: serde_json::Value::Null,
            })
            .unwrap();
        let convo = db.upsert_conversation(&source.id, "t1", "chat", None).unwrap();
        let ada =
            db.resolve_participant("Ada", &[IdentifierInput::new(IdentifierKind::Handle, "@ada")], false).unwrap();
        db.set_participant_relationship(&ada, Some("colleague")).unwrap();
        db.link_conversation_participant(&convo, &ada).unwrap();
        let mut batch = Vec::new();
        for i in 0..n {
            batch.push(NewMessage {
                conversation_id: convo.clone(),
                source_id: source.id.clone(),
                participant_id: Some(ada.clone()),
                external_id: format!("in{i}"),
                direction: "other".into(),
                channel: "chat".into(),
                sent_at: Some(format!("2026-02-{:02}T09:00:00Z", (i % 27) + 1)),
                sequence_index: (i * 2) as i64,
                body: "can you send the deck".into(),
                reply_to_external_id: None,
                metadata: serde_json::Value::Null,
            });
            batch.push(NewMessage {
                conversation_id: convo.clone(),
                source_id: source.id.clone(),
                participant_id: None,
                external_id: format!("out{i}"),
                direction: "self".into(),
                channel: "chat".into(),
                sent_at: Some(format!("2026-02-{:02}T09:05:00Z", (i % 27) + 1)),
                sequence_index: (i * 2 + 1) as i64,
                body: format!("{body} {i}"),
                reply_to_external_id: None,
                metadata: serde_json::Value::Null,
            });
        }
        db.insert_messages(&batch).unwrap();
        db.link_replies(&convo).unwrap();
        db.refresh_conversation_stats(&convo).unwrap();
        voice::analyze(&db, &mut |_, _| {}).unwrap();
        (db, ada)
    }

    fn request(ada: &str) -> ComposeRequest {
        ComposeRequest {
            participant_id: Some(ada.to_string()),
            channel: "chat".into(),
            incoming_message: Some("can you send the deck?".into()),
            intent: Some("yes, tomorrow morning".into()),
            ..Default::default()
        }
    }

    #[test]
    fn the_prompt_carries_the_intent_the_incoming_message_and_the_examples() {
        let (db, ada) = seeded(25, "yeah sending it over");
        let ctx = build_context(&db, &request(&ada)).unwrap();
        let prompt = assemble(&ctx, &request(&ada));
        let last = prompt.messages.last().unwrap();
        assert_eq!(last.role, "user");
        assert!(last.content.contains("can you send the deck?"), "{}", last.content);
        assert!(last.content.contains("yes, tomorrow morning"));
        assert!(prompt.system.contains("Messages this person actually sent"));
        assert!(prompt.system.contains("writing to Ada"));
        assert!(prompt.system.contains("colleague"));
        assert!(prompt.system.contains("The channel is chat"));
    }

    #[test]
    fn the_transcript_is_rendered_with_the_right_speaker_on_each_turn() {
        let (db, ada) = seeded(25, "yeah sending it over");
        let ctx = build_context(&db, &request(&ada)).unwrap();
        let prompt = assemble(&ctx, &request(&ada));
        // Everything except the final task message is transcript.
        let turns = &prompt.messages[..prompt.messages.len() - 1];
        assert!(!turns.is_empty());
        assert!(turns.iter().any(|m| m.role == "assistant" && m.content.starts_with("yeah sending")));
        assert!(turns.iter().any(|m| m.role == "user" && m.content == "can you send the deck"));
    }

    #[test]
    fn a_style_with_no_data_says_so_instead_of_inventing_one() {
        let db = Db::open_in_memory().unwrap();
        let req = ComposeRequest { channel: "email".into(), intent: Some("say no".into()), ..Default::default() };
        let ctx = build_context(&db, &req).unwrap();
        assert!(!ctx.effective.measurable);
        let prompt = assemble(&ctx, &req);
        assert!(prompt.system.contains("not yet enough"), "{}", prompt.system);
        assert!(prompt.system.contains("do not invent mannerisms"));
        assert!(ctx.evidence.iter().any(|e| e.contains("Not enough of your own writing")), "{:?}", ctx.evidence);
    }

    #[test]
    fn measured_habits_become_instructions_not_raw_numbers() {
        let m = VoiceMetrics {
            measurable: true,
            median_words_per_message: Some(6.0),
            p90_words_per_message: Some(14.0),
            terminal_period_rate: Some(0.05),
            lowercase_start_rate: Some(0.9),
            emoji_rate: Some(0.0),
            exclamation_rate: Some(0.0),
            greeting_rate: Some(0.02),
            sign_off_rate: Some(0.0),
            contractions_per_100_words: Some(8.0),
            top_phrases: vec![("let me know".into(), 40)],
            ..Default::default()
        };
        let lines = describe(&m).join("\n");
        assert!(lines.contains("about 6 words"));
        assert!(lines.contains("usually do not put a full stop"));
        assert!(lines.contains("start messages in lowercase"));
        assert!(lines.contains("Do not add any"), "an emoji rate of zero is an instruction");
        assert!(lines.contains("no greeting"));
        assert!(lines.contains("do not sign off"));
        assert!(lines.contains("\"let me know\""));
        // Nothing claims a habit that was not measured.
        assert!(!lines.contains("paragraph"));
    }

    #[test]
    fn the_output_budget_follows_how_long_this_person_actually_writes() {
        let terse = VoiceMetrics { measurable: true, p90_words_per_message: Some(8.0), ..Default::default() };
        let verbose = VoiceMetrics { measurable: true, p90_words_per_message: Some(400.0), ..Default::default() };
        assert!(max_tokens(&terse, None) < max_tokens(&verbose, None));
        assert!(max_tokens(&terse, Some(Adjustment::Shorter)) < max_tokens(&terse, None));
        assert!(max_tokens(&terse, Some(Adjustment::Longer)) > max_tokens(&terse, None));
        assert!(max_tokens(&verbose, Some(Adjustment::Longer)) <= 2000);
    }

    #[test]
    fn manual_preferences_appear_after_the_measurements_and_are_labelled_as_overriding() {
        let (db, ada) = seeded(25, "yeah sending it over");
        db.set_voice_preference(VoiceLayer::Relationship, &ada, "signOff", &json!("— C"), None).unwrap();
        let ctx = build_context(&db, &request(&ada)).unwrap();
        let prompt = assemble(&ctx, &request(&ada));
        let measured = prompt.system.find("How this person writes").unwrap();
        let overrides = prompt.system.find("override everything above").unwrap();
        assert!(overrides > measured, "overrides come last so they win");
        assert!(prompt.system.contains("signOff: — C"));
    }

    #[test]
    fn an_adjustment_changes_the_prompt_and_the_hash() {
        let (db, ada) = seeded(25, "yeah sending it over");
        let plain = request(&ada);
        let shorter = ComposeRequest { adjustment: Some(Adjustment::Shorter), ..request(&ada) };
        let ctx = build_context(&db, &plain).unwrap();
        let a = assemble(&ctx, &plain);
        let b = assemble(&ctx, &shorter);
        assert_ne!(a, b);
        assert!(b.system.contains("noticeably shorter"));
        assert!(b.max_output_tokens < a.max_output_tokens);
    }

    #[test]
    fn the_prompt_is_a_pure_function_of_the_context() {
        let (db, ada) = seeded(25, "yeah sending it over");
        let req = request(&ada);
        let ctx = build_context(&db, &req).unwrap();
        assert_eq!(assemble(&ctx, &req), assemble(&ctx, &req));
    }

    #[test]
    fn composing_records_the_draft_with_its_evidence() {
        let (db, ada) = seeded(25, "yeah sending it over");
        let provider = MockProvider::answering("yeah, tomorrow morning");
        let draft = compose(&db, &provider, &request(&ada)).unwrap();
        assert_eq!(draft.generated_text, "yeah, tomorrow morning");
        assert_eq!(draft.provider, "mock");
        assert_eq!(draft.participant_id.as_deref(), Some(ada.as_str()));
        assert!(!draft.prompt_hash.is_empty());
        let statements = draft.evidence["statements"].as_array().unwrap();
        assert!(statements.iter().any(|s| s.as_str().unwrap().contains("messages to this person")), "{statements:?}");
        assert!(!draft.evidence["examples"].as_array().unwrap().is_empty());
        assert_eq!(db.recent_drafts(5).unwrap().len(), 1);
    }

    #[test]
    fn a_provider_failure_records_no_draft() {
        let (db, ada) = seeded(25, "yeah sending it over");
        let provider = MockProvider::failing("connection refused");
        assert!(compose(&db, &provider, &request(&ada)).is_err());
        assert!(db.recent_drafts(5).unwrap().is_empty(), "a failed generation is not a draft");
    }

    #[test]
    fn writing_to_someone_new_falls_back_to_the_global_voice_and_says_so() {
        let (db, _ada) = seeded(25, "yeah sending it over");
        let stranger =
            db.resolve_participant("Zed", &[IdentifierInput::new(IdentifierKind::Handle, "@zed")], false).unwrap();
        let req = ComposeRequest {
            participant_id: Some(stranger),
            channel: "chat".into(),
            intent: Some("decline politely".into()),
            ..Default::default()
        };
        let ctx = build_context(&db, &req).unwrap();
        assert!(ctx.evidence.iter().any(|e| e.contains("No profile for this person yet")), "{:?}", ctx.evidence);
        assert!(!ctx.examples.is_empty(), "the global voice still has examples to offer");
        assert!(ctx.effective.measurable);
    }

    /// A corpus where the user says no to Ada twenty-five times, in their own
    /// clipped way, and writes ordinary messages otherwise.
    fn seeded_with_refusals() -> (Db, String) {
        let (db, ada) = seeded(25, "yeah sending it over");
        let source = db.list_sources().unwrap()[0].id.clone();
        let convo = db.upsert_conversation(&source, "t2", "chat", None).unwrap();
        db.link_conversation_participant(&convo, &ada).unwrap();
        let mut batch = Vec::new();
        for i in 0..25 {
            batch.push(NewMessage {
                conversation_id: convo.clone(),
                source_id: source.clone(),
                participant_id: Some(ada.clone()),
                external_id: format!("ask{i}"),
                direction: "other".into(),
                channel: "chat".into(),
                sent_at: Some(format!("2026-03-{:02}T09:00:00Z", (i % 27) + 1)),
                sequence_index: (i * 2) as i64,
                body: format!("want to come to the thing on friday {i}?"),
                reply_to_external_id: None,
                metadata: serde_json::Value::Null,
            });
            batch.push(NewMessage {
                conversation_id: convo.clone(),
                source_id: source.clone(),
                participant_id: None,
                external_id: format!("no{i}"),
                direction: "self".into(),
                channel: "chat".into(),
                sent_at: Some(format!("2026-03-{:02}T09:05:00Z", (i % 27) + 1)),
                sequence_index: (i * 2 + 1) as i64,
                body: format!("ah can't make it this time, sorry {i}"),
                reply_to_external_id: None,
                metadata: serde_json::Value::Null,
            });
        }
        db.insert_messages(&batch).unwrap();
        db.link_replies(&convo).unwrap();
        db.refresh_conversation_stats(&convo).unwrap();
        voice::analyze(&db, &mut |_, _| {}).unwrap();
        (db, ada)
    }

    #[test]
    fn a_note_that_reads_like_a_no_uses_the_times_the_user_said_no() {
        let (db, ada) = seeded_with_refusals();
        let req = ComposeRequest { intent: Some("say no, busy".into()), ..request(&ada) };
        let ctx = build_context(&db, &req).unwrap();
        let sit = ctx.situation.clone().expect("the note reads like a no");
        assert_eq!(sit.id, "declining");
        assert_eq!(sit.source, SituationSource::FromNote, "read from the note, not stated by the user");
        assert!(ctx.voice.layers.iter().any(|l| l.layer == "situational" && l.measurable));
        assert!(ctx.examples.iter().take(2).all(|e| e.reply.starts_with("ah can't make it")), "{:?}", ctx.examples);
        assert!(ctx.examples[0].reason.starts_with("a time you said no"));
        assert!(
            ctx.evidence
                .iter()
                .any(|e| e.starts_with("Your note reads like saying no") && e.contains("times you said no")),
            "{:?}",
            ctx.evidence
        );
        let prompt = assemble(&ctx, &req);
        assert!(prompt.system.contains("Their note reads as though they are saying no to something here."));
        assert!(!prompt.system.contains("In this reply they are"), "a reading is not stated as a fact");
    }

    #[test]
    fn a_situation_the_user_chose_is_labelled_as_theirs() {
        let (db, ada) = seeded_with_refusals();
        let req = ComposeRequest { situation_id: Some("declining".into()), intent: None, ..request(&ada) };
        let ctx = build_context(&db, &req).unwrap();
        assert_eq!(ctx.situation.as_ref().unwrap().source, SituationSource::Chosen);
        assert!(ctx.evidence.iter().any(|e| e.starts_with("You said this reply is saying no")), "{:?}", ctx.evidence);
        assert!(assemble(&ctx, &req).system.contains("In this reply they are saying no to something."));
    }

    #[test]
    fn a_situation_with_too_little_behind_it_says_so_and_changes_nothing_measured() {
        let (db, ada) = seeded(25, "yeah sending it over");
        let req = ComposeRequest { intent: Some("thank them for the intro".into()), ..request(&ada) };
        let ctx = build_context(&db, &req).unwrap();
        assert_eq!(ctx.situation.as_ref().unwrap().id, "thanking");
        assert!(!ctx.voice.layers.iter().any(|l| l.layer == "situational"));
        assert!(ctx.evidence.iter().any(|e| e.contains("haven't seen you do that yet")), "{:?}", ctx.evidence);
        let plain = build_context(&db, &request(&ada)).unwrap();
        assert_eq!(ctx.effective, plain.effective, "no situational layer, no change to the measurements");
    }

    #[test]
    fn an_unknown_situation_is_refused_rather_than_ignored() {
        let (db, ada) = seeded(25, "yeah sending it over");
        let req = ComposeRequest { situation_id: Some("gloating".into()), ..request(&ada) };
        assert!(build_context(&db, &req).is_err());
    }

    #[test]
    fn with_no_note_nothing_is_guessed_from_their_message() {
        let (db, ada) = seeded_with_refusals();
        let req = ComposeRequest {
            intent: None,
            incoming_message: Some("sorry, can you make it friday?".into()),
            ..request(&ada)
        };
        assert!(build_context(&db, &req).unwrap().situation.is_none());
    }

    #[test]
    fn the_draft_records_the_situation_it_was_written_for() {
        let (db, ada) = seeded_with_refusals();
        let provider = MockProvider::answering("ah can't this time");
        let req = ComposeRequest { intent: Some("say no".into()), ..request(&ada) };
        let draft = compose(&db, &provider, &req).unwrap();
        assert_eq!(draft.situation_id.as_deref(), Some("declining"));
        assert_eq!(draft.context["situation"]["source"], "fromNote");
    }

    fn send(db: &Db, provider: &MockProvider, req: &ComposeRequest, final_text: &str) {
        let d = compose(db, provider, req).unwrap();
        let outcome = if d.generated_text == final_text { "sent_unedited" } else { "sent_edited" };
        db.resolve_draft(&d.id, outcome, Some(final_text)).unwrap();
    }

    #[test]
    fn three_edits_the_same_way_change_the_next_prompt_and_two_do_not() {
        let (db, ada) = seeded(25, "yeah sending it over");
        let provider = MockProvider::answering("Hi Ada,\n\nyeah, tomorrow morning works for me");
        send(&db, &provider, &request(&ada), "yeah, tomorrow morning works for me then");
        send(&db, &provider, &request(&ada), "yeah, tomorrow morning works for me then");
        let ctx = build_context(&db, &request(&ada)).unwrap();
        assert!(ctx.learned.is_empty(), "two edits are an anecdote");
        assert!(!assemble(&ctx, &request(&ada)).system.contains("changed in earlier drafts"));

        send(&db, &provider, &request(&ada), "yeah, tomorrow morning works for me then");
        let ctx = build_context(&db, &request(&ada)).unwrap();
        assert_eq!(ctx.learned.len(), 1);
        assert!(
            ctx.evidence.iter().any(|e| e.starts_with("A change you've made to my drafts before: no greeting")),
            "{:?}",
            ctx.evidence
        );
        let prompt = assemble(&ctx, &request(&ada));
        let learned = prompt.system.find("changed in earlier drafts").unwrap();
        assert!(prompt.system[learned..].contains("- Do not open with a greeting."));
        assert!(learned > prompt.system.find("How this person writes").unwrap(), "after the measurements it adjusts");
    }

    #[test]
    fn a_note_the_user_typed_reaches_the_prompt_in_their_words_and_outranks_the_rest() {
        let (db, ada) = seeded(25, "yeah sending it over");
        crate::learning::remember_note(&db, Some(&ada), "she hates being called Mrs Lovelace").unwrap();
        let ctx = build_context(&db, &request(&ada)).unwrap();
        assert!(ctx.evidence.iter().any(|e| e == "One thing you've told me directly"), "{:?}", ctx.evidence);
        let prompt = assemble(&ctx, &request(&ada));
        assert!(prompt.system.contains("- she hates being called Mrs Lovelace\n"), "{}", prompt.system);
        assert!(!prompt.system.contains("said:"), "the storage key never reaches the model");
        // Someone else is not told about Ada.
        let other = ComposeRequest { participant_id: None, ..request(&ada) };
        assert!(!assemble(&build_context(&db, &other).unwrap(), &other).system.contains("Lovelace"));
    }
}
