//! Measuring the drafts: how close Mimic's replies come to what the user
//! actually wrote, next to two baselines, on conversations it was not shown
//! (docs/VOICE_ENGINE.md, "Measuring whether any of this works").
//!
//! 1. Every exchange is a candidate: a message from someone else that does
//!    not look automated, and the user's reply that came straight after it.
//! 2. The engine splits them by conversation (`eval.split`), so no thread is
//!    on both sides. From the held-out side, up to `MAX_CASES` are taken —
//!    the newest from each conversation first — so one long thread can't be
//!    most of the measurement.
//! 3. Each is answered three ways. No held-out conversation is used as an
//!    example, and each exchange's own conversation is read only up to the
//!    message being answered — the reply being predicted comes after that.
//!    * **Mimic**: the prompt a draft on the home screen gets, with no note
//!      from the user, as a draft prepared in advance has none. How the user
//!      writes is measured again for it without the held-out conversations
//!      (`voice::LeavingOut`), since the stored profiles were measured over
//!      everything, a person's own layer perhaps mostly from the very thread
//!      being tested. Habits learned from edits to earlier drafts are left
//!      out: some may come from drafts of the replies being predicted. That
//!      leans against Mimic, and the screen says so.
//!    * **A generic reply**: the same model, conversation and message, and
//!      no measurements, examples or habits of the user's.
//!    * **Your most common reply**: the reply the user sends most often,
//!      among the conversations not held out. No model.
//! 4. The engine compares each answer with the reply actually sent
//!    (`eval.compare`): length, words in common, punctuation habits, and
//!    overall wording under whatever encoder it has, which it names.
//!
//! Nothing here writes a draft. A case refers to its messages by id, and is
//! deleted with either of them (migration 0011).
//! No summary is stored: it is worked out from the cases that remain each
//! time it is read, so no figure outlives the replies it came from. And there
//! is no headline number: the four measures are not commensurable, and
//! averaging them would invent a quantity nobody defined.
//!
//! Deleting a person or a mailbox deletes every measurement (`privacy`):
//! what was written for a case came from the conversation before it and from
//! examples across the user's mail, and any of that may be theirs.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::db::{Db, DbError, Exchange, NewEvaluation, NewEvaluationCase};
use crate::engine::EngineClient;
use crate::generation::{assemble, build_context_holding_out, ComposeRequest, HeldOut};
use crate::jobs::{JobContext, JobError, JobExecutor, JobFuture};
use crate::providers::{GenerationRequest, ModelProvider};

pub const JOB_KIND: &str = "evaluate_drafts";

/// Exchanges answered per run. Each costs two model calls, so a run is
/// bounded the way assisted drafting is: enough to see a pattern, few enough
/// that it is quick on a local model and cheap on a hosted one.
pub const MAX_CASES: usize = 12;

/// Exchanges read, newest first. A mailbox of years has tens of thousands;
/// the split only needs enough conversations to hold some out.
const MAX_EXCHANGES: usize = 5000;
const SEED: u64 = 42;
const HOLDOUT_FRACTION: f64 = 0.2;
const GENERIC_MAX_TOKENS: u32 = 400;
const COMPARE_TIMEOUT: Duration = Duration::from_secs(300);

pub const MIMIC: &str = "mimic";
pub const GENERIC: &str = "generic";
pub const COMMON_REPLY: &str = "common_reply";
const SYSTEMS: [&str; 3] = [MIMIC, GENERIC, COMMON_REPLY];

/// The generic reply's instructions: what any assistant would be told.
const GENERIC_SYSTEM: &str = "You are an assistant replying to a message on someone's behalf. Write only the message body: no subject line, no preamble, no explanation, no quotation marks around it.";

const NOT_ENOUGH: &str = "There isn't enough to measure on yet. I need replies of yours in at least two conversations — messages you wrote straight after someone else's — so that I can hold some back and still have others to learn from.";

const ENGINE_DOWN: &str = "The part of Mimic that does the measuring isn't running, so this can't be measured right now. Settings → Diagnostics can restart it.";

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// How the exchanges were split (`eval.split`): indices into the list given.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Split {
    pub train: Vec<usize>,
    pub holdout: Vec<usize>,
    pub strategy: String,
    pub groups: usize,
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// The side of the measurement that numpy is better at: the split and the
/// comparison. The app uses the engine's (`EngineHarness`).
pub trait Harness: Send + Sync {
    /// Split by `group_keys`, one per exchange (its conversation), so that no
    /// group is on both sides.
    fn split(&self, group_keys: Vec<String>) -> BoxFuture<'_, Result<Split, JobError>>;
    /// Compare each `(generated, actual)`: one comparison per pair, in order.
    fn compare(&self, pairs: Vec<(String, String)>) -> BoxFuture<'_, Result<Vec<Value>, JobError>>;
}

/// `eval.split` and `eval.compare` in the Python engine.
pub struct EngineHarness(pub EngineClient);

impl Harness for EngineHarness {
    fn split(&self, group_keys: Vec<String>) -> BoxFuture<'_, Result<Split, JobError>> {
        Box::pin(async move {
            let v = self
                .0
                .call("eval.split", json!({"groupKeys": group_keys, "seed": SEED, "holdoutFraction": HOLDOUT_FRACTION}))
                .await?;
            serde_json::from_value(v)
                .map_err(|e| JobError::Failed(format!("the engine's split could not be read: {e}")))
        })
    }

    fn compare(&self, pairs: Vec<(String, String)>) -> BoxFuture<'_, Result<Vec<Value>, JobError>> {
        Box::pin(async move {
            let n = pairs.len();
            let pairs: Vec<Value> = pairs
                .into_iter()
                .map(|(generated, actual)| json!({"generated": generated, "actual": actual}))
                .collect();
            let v = self.0.call_with_timeout("eval.compare", json!({ "pairs": pairs }), COMPARE_TIMEOUT).await?;
            let cases = v.get("cases").and_then(Value::as_array).cloned().unwrap_or_default();
            if cases.len() != n {
                return Err(JobError::Failed(format!("the engine compared {} of {n} replies", cases.len())));
            }
            Ok(cases)
        })
    }
}

/// A harness that needs no engine, for Mimic's own tests. It splits by the
/// engine's rule (conversations in the order of a seeded hash, held out
/// until a fifth of the exchanges are) and makes the three comparisons that
/// need no encoder, so it reports no `embedding` — as the engine does when it
/// has none. The app never uses it.
pub struct LocalHarness;

impl Harness for LocalHarness {
    fn split(&self, group_keys: Vec<String>) -> BoxFuture<'_, Result<Split, JobError>> {
        Box::pin(async move { Ok(local_split(&group_keys)) })
    }

    fn compare(&self, pairs: Vec<(String, String)>) -> BoxFuture<'_, Result<Vec<Value>, JobError>> {
        Box::pin(async move { Ok(pairs.iter().map(|(g, a)| local_compare(g, a)).collect()) })
    }
}

fn local_split(keys: &[String]) -> Split {
    let mut groups: Vec<&str> = keys.iter().map(String::as_str).collect::<HashSet<_>>().into_iter().collect();
    groups.sort_by_key(|g| crate::ids::sha256_hex(format!("{SEED}:{g}").as_bytes()));
    let size = |g: &str| keys.iter().filter(|k| k.as_str() == g).count();
    let target = HOLDOUT_FRACTION * keys.len() as f64;
    let (mut held, mut acc): (HashSet<&str>, usize) = (HashSet::new(), 0);
    for &g in &groups {
        if held.is_empty() || (acc as f64) < target {
            held.insert(g);
            acc += size(g);
        }
    }
    if held.len() == groups.len() {
        // The first of the largest in alphabetical order, as the engine's
        // `max` over `sorted(set(keys))` picks it.
        let mut alphabetical = groups.clone();
        alphabetical.sort_unstable();
        let mut biggest: Option<&str> = None;
        for &g in &alphabetical {
            if biggest.is_none_or(|b| size(g) > size(b)) {
                biggest = Some(g);
            }
        }
        if let Some(b) = biggest {
            held.remove(b);
        }
    }
    let (holdout, train) = (0..keys.len()).partition(|&i| held.contains(keys[i].as_str()));
    Split { train, holdout, strategy: "conversation_grouped".into(), groups: groups.len(), warnings: Vec::new() }
}

fn local_compare(generated: &str, actual: &str) -> Value {
    let words = |t: &str| -> Vec<String> {
        t.to_lowercase()
            .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '\''))
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect()
    };
    let (g, a) = (words(generated), words(actual));
    let length = match (g.len(), a.len()) {
        (0, 0) => 1.0,
        (0, _) | (_, 0) => 0.0,
        (x, y) => x.min(y) as f64 / x.max(y) as f64,
    };
    let (gs, as_): (HashSet<&String>, HashSet<&String>) = (g.iter().collect(), a.iter().collect());
    let vocabulary = match (gs.is_empty(), as_.is_empty()) {
        (true, true) => 1.0,
        (true, _) | (_, true) => 0.0,
        _ => gs.intersection(&as_).count() as f64 / gs.union(&as_).count() as f64,
    };
    let habits = |t: &str| {
        let s = t.trim();
        [
            s.ends_with('.'),
            t.contains('?'),
            t.contains('!'),
            s.chars().next().is_some_and(char::is_lowercase),
            t.contains("\n\n"),
        ]
    };
    let (gh, ah) = (habits(generated), habits(actual));
    let punctuation = gh.iter().zip(ah).filter(|(x, y)| **x == *y).count() as f64 / gh.len() as f64;
    json!({
        "length": round4(length),
        "vocabulary": round4(vocabulary),
        "punctuation": round4(punctuation),
        "generatedWords": g.len(),
        "actualWords": a.len(),
    })
}

/// Up to `max` of the held-out exchanges: the newest from each conversation
/// first (conversations in the order of their newest), then the next newest
/// from each, and so on. `exchanges` is newest first.
pub fn choose(exchanges: &[Exchange], holdout: &[usize], max: usize) -> Vec<usize> {
    let mut held: Vec<usize> = holdout.iter().copied().filter(|&i| i < exchanges.len()).collect();
    held.sort_unstable();
    held.dedup();
    let mut order: Vec<Vec<usize>> = Vec::new();
    let mut at: HashMap<&str, usize> = HashMap::new();
    for i in held {
        let c = exchanges[i].conversation_id.as_str();
        match at.get(c) {
            Some(&k) => order[k].push(i),
            None => {
                at.insert(c, order.len());
                order.push(vec![i]);
            }
        }
    }
    let mut out = Vec::new();
    for round in 0.. {
        let before = out.len();
        for thread in &order {
            if out.len() == max {
                return out;
            }
            if let Some(&i) = thread.get(round) {
                out.push(i);
            }
        }
        if out.len() == before {
            break;
        }
    }
    out
}

/// The reply the user sends most often, as they wrote it the last time.
#[derive(Debug, Clone, PartialEq)]
pub struct CommonReply {
    pub text: String,
    pub message_id: String,
    pub times: usize,
}

/// The reply sent most often among `train` (spacing, case and a closing mark
/// aside, so "Sounds good!" and "sounds good" are one reply). The more recent
/// of two sent equally often. When no reply repeats, that is simply the
/// newest — which a voice model should have no trouble beating either.
pub fn common_reply(exchanges: &[Exchange], train: &[usize]) -> Option<CommonReply> {
    let mut seen: HashMap<String, (usize, usize)> = HashMap::new();
    for &i in train {
        let Some(ex) = exchanges.get(i) else { continue };
        let key = normalize(&ex.reply);
        if key.is_empty() {
            continue;
        }
        let entry = seen.entry(key).or_insert((0, i));
        entry.0 += 1;
        entry.1 = entry.1.min(i);
    }
    // Keys are distinct and so are their newest indices: no ties are left.
    let (times, i) = seen.into_values().max_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)))?;
    Some(CommonReply { text: exchanges[i].reply.clone(), message_id: exchanges[i].reply_id.clone(), times })
}

fn normalize(text: &str) -> String {
    let spaced = text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    spaced.trim_end_matches(['.', '!', '?', ',', '…']).trim().to_string()
}

/// What was written for one exchange.
#[derive(Debug, Clone, PartialEq)]
pub struct Written {
    /// Index into the exchanges.
    pub exchange: usize,
    pub mimic: String,
    pub generic: String,
    /// Every message the two were written from (`NewEvaluationCase::sources`).
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Writing {
    pub written: Vec<Written>,
    /// Exchanges that could not be answered both ways.
    pub failed: usize,
    pub last_error: Option<String>,
}

/// The generic reply's prompt: Mimic's conversation and message, and none of
/// the user's measurements, examples, habits or notes.
fn generic_prompt(mimic: &GenerationRequest) -> GenerationRequest {
    GenerationRequest {
        system: GENERIC_SYSTEM.to_string(),
        messages: mimic.messages.clone(),
        max_output_tokens: GENERIC_MAX_TOKENS,
        temperature: mimic.temperature,
    }
}

/// Answer each chosen exchange as Mimic and as a generic assistant, holding
/// `held` out of the examples and each exchange's own reply out of its
/// conversation. Checks `should_stop` between exchanges. An exchange the
/// model could not answer both ways is counted and skipped: one refused
/// request should not waste the rest of the run.
pub fn write_answers(
    db: &Db,
    provider: &dyn ModelProvider,
    exchanges: &[Exchange],
    chosen: &[usize],
    held: &[String],
    on_progress: &mut dyn FnMut(usize, usize),
    should_stop: &dyn Fn() -> bool,
) -> Result<Writing, DbError> {
    let mut out = Writing::default();
    // How the user writes, measured without the held-out conversations; each
    // scope once for the run.
    let fresh = crate::voice::LeavingOut::new(db, held);
    for (n, &i) in chosen.iter().enumerate() {
        if should_stop() {
            break;
        }
        on_progress(n, chosen.len());
        let ex = &exchanges[i];
        let req = ComposeRequest {
            participant_id: ex.participant_id.clone(),
            conversation_id: Some(ex.conversation_id.clone()),
            channel: ex.channel.clone(),
            incoming_message: Some(ex.incoming.clone()),
            incoming_message_id: Some(ex.incoming_id.clone()),
            ..Default::default()
        };
        let hide =
            HeldOut { conversations: held.to_vec(), before_message: Some(ex.incoming_id.clone()), voice: Some(&fresh) };
        let ctx = match build_context_holding_out(db, &req, &hide) {
            Ok(ctx) => ctx,
            // Deleted since the list was read: there is nothing to measure it on.
            Err(DbError::NotFound(_)) => {
                out.failed += 1;
                continue;
            }
            Err(e) => return Err(e),
        };
        let prompt = assemble(&ctx, &req);
        let mut sources: Vec<String> = ctx.transcript.iter().map(|m| m.id.clone()).collect();
        for e in &ctx.examples {
            sources.push(e.reply_message_id.clone());
            sources.extend(e.incoming_message_id.clone());
        }
        sources.sort_unstable();
        sources.dedup();
        let mimic = match provider.generate(&prompt) {
            Ok(r) => r.text,
            Err(e) => {
                out.failed += 1;
                out.last_error = Some(e.to_string());
                continue;
            }
        };
        let generic = match provider.generate(&generic_prompt(&prompt)) {
            Ok(r) => r.text,
            Err(e) => {
                out.failed += 1;
                out.last_error = Some(e.to_string());
                continue;
            }
        };
        out.written.push(Written { exchange: i, mimic, generic, sources });
    }
    on_progress(chosen.len(), chosen.len());
    Ok(out)
}

/// What a run did. Its figures are read with `latest`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub evaluation_id: String,
    /// Exchanges chosen, answered, and not answered.
    pub asked: usize,
    pub measured: usize,
    pub failed: usize,
}

/// Run one measurement and record it. `progress` gets (done, total, phase);
/// `should_stop` is checked between exchanges, and a stopped run records
/// nothing — part of a measurement is not a smaller one.
pub async fn measure(
    db: &Db,
    provider: &dyn ModelProvider,
    harness: &dyn Harness,
    max: usize,
    progress: &(dyn Fn(usize, usize, &str) + Send + Sync),
    should_stop: &(dyn Fn() -> bool + Send + Sync),
) -> Result<Outcome, JobError> {
    progress(0, 0, "reading your replies");
    let exchanges = db.evaluation_exchanges(MAX_EXCHANGES)?;
    let conversations: HashSet<&str> = exchanges.iter().map(|e| e.conversation_id.as_str()).collect();
    if conversations.len() < 2 {
        return Err(JobError::Failed(NOT_ENOUGH.into()));
    }
    let split = harness.split(exchanges.iter().map(|e| e.conversation_id.clone()).collect()).await?;
    let mut held: Vec<String> = Vec::new();
    for &i in &split.holdout {
        if let Some(ex) = exchanges.get(i) {
            if !held.contains(&ex.conversation_id) {
                held.push(ex.conversation_id.clone());
            }
        }
    }
    // Whatever the split says, a held-out conversation is never also
    // learned from.
    let train: Vec<usize> = split
        .train
        .iter()
        .copied()
        .filter(|&i| exchanges.get(i).is_some_and(|ex| !held.contains(&ex.conversation_id)))
        .collect();
    let chosen = choose(&exchanges, &split.holdout, max);
    if chosen.is_empty() || train.is_empty() {
        return Err(JobError::Failed(NOT_ENOUGH.into()));
    }
    let common = common_reply(&exchanges, &train);
    if should_stop() {
        return Err(JobError::Canceled);
    }

    let writing = tokio::task::block_in_place(|| {
        let mut on_progress = |done: usize, total: usize| progress(done, total, "writing replies");
        write_answers(db, provider, &exchanges, &chosen, &held, &mut on_progress, &|| should_stop())
    })?;
    if should_stop() {
        return Err(JobError::Canceled);
    }
    if writing.written.is_empty() {
        let why = writing.last_error.map(|e| format!(": {e}")).unwrap_or_default();
        return Err(JobError::Failed(format!(
            "None of the {} replies could be written, so there is nothing to measure{why}",
            chosen.len()
        )));
    }

    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut slots: Vec<(&Written, &str)> = Vec::new();
    for w in &writing.written {
        let actual = &exchanges[w.exchange].reply;
        pairs.push((w.mimic.clone(), actual.clone()));
        slots.push((w, MIMIC));
        pairs.push((w.generic.clone(), actual.clone()));
        slots.push((w, GENERIC));
        if let Some(c) = &common {
            pairs.push((c.text.clone(), actual.clone()));
            slots.push((w, COMMON_REPLY));
        }
    }
    progress(0, pairs.len(), "comparing");
    let metrics = harness.compare(pairs.clone()).await?;
    if should_stop() {
        return Err(JobError::Canceled);
    }
    let cases: Vec<NewEvaluationCase> = slots
        .into_iter()
        .zip(pairs)
        .zip(metrics)
        .map(|(((w, system), (generated, _)), metrics)| {
            let ex = &exchanges[w.exchange];
            NewEvaluationCase {
                incoming_message_id: ex.incoming_id.clone(),
                reply_message_id: ex.reply_id.clone(),
                system: system.to_string(),
                generated_text: generated,
                based_on_message_id: if system == COMMON_REPLY {
                    common.as_ref().map(|c| c.message_id.clone())
                } else {
                    None
                },
                metrics,
                // All three answers to an exchange stand or fall together, so
                // every column is measured over the same replies.
                sources: w.sources.clone(),
            }
        })
        .collect();
    let info = provider.info();
    let (id, kept) = db.record_evaluation(&NewEvaluation {
        analysis_version: crate::version::ANALYSIS_VERSION.to_string(),
        provider: info.id,
        model: Some(info.model),
        config: json!({
            "seed": SEED,
            "holdoutFraction": HOLDOUT_FRACTION,
            "asked": chosen.len(),
            "measured": writing.written.len(),
            "failed": writing.failed,
            "commonReplyTimes": common.as_ref().map(|c| c.times),
        }),
        split: json!({
            "strategy": split.strategy,
            "conversations": split.groups,
            "heldOutConversations": held.len(),
            "exchanges": exchanges.len(),
            "warnings": split.warnings,
        }),
        cases,
    })?;
    if kept == 0 {
        return Err(JobError::Failed(
            "Mail I was measuring on was deleted while I was measuring, so I've kept nothing. Measure again to see where things stand."
                .into(),
        ));
    }
    Ok(Outcome { evaluation_id: id, asked: chosen.len(), measured: writing.written.len(), failed: writing.failed })
}

/// The mean of one measure over the cases that remain, and its 10th
/// percentile — the worst tenth, so a good average can't hide a bad tail.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Measure {
    pub mean: f64,
    pub p10: f64,
}

/// How one way of answering compared, measure by measure. A measure the
/// comparison did not make (`embedding`, without an encoder) is absent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemResult {
    /// `mimic`, `generic` or `common_reply`.
    pub system: String,
    pub cases: usize,
    pub length: Option<Measure>,
    pub vocabulary: Option<Measure>,
    pub punctuation: Option<Measure>,
    pub embedding: Option<Measure>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Answer {
    pub system: String,
    pub text: String,
}

/// One held-out exchange and every answer to it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaseView {
    pub incoming: String,
    /// Who wrote the message answered, when they could be named.
    pub from: Option<String>,
    /// What the user actually wrote.
    pub reply: String,
    pub answers: Vec<Answer>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommonReplyView {
    pub text: String,
    pub times: Option<usize>,
}

/// The latest measurement, from the cases that remain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationView {
    pub id: String,
    pub created_at: String,
    pub provider: String,
    pub model: Option<String>,
    /// Exchanges answered when it ran.
    pub measured: usize,
    /// Of those, how many still count: a message answered that turns out to
    /// be the user's own (someone folded into them since) answers nothing.
    /// A deletion takes the whole measurement, so it never lowers this.
    pub remaining: usize,
    pub strategy: String,
    /// Conversations with an exchange, and how many the split held back.
    pub conversations: usize,
    pub held_out_conversations: usize,
    /// Conversations the remaining cases come from.
    pub measured_conversations: usize,
    pub warnings: Vec<String>,
    /// What the `embedding` measure used, as the engine named it.
    pub embedding_provider: Option<String>,
    pub common_reply: Option<CommonReplyView>,
    pub systems: Vec<SystemResult>,
    pub cases: Vec<CaseView>,
}

/// The most recent measurement, worked out from the cases still here.
pub fn latest(db: &Db) -> Result<Option<EvaluationView>, DbError> {
    let Some(row) = db.latest_evaluation()? else { return Ok(None) };
    let rows = db.evaluation_cases(&row.id)?;

    let mut cases: Vec<CaseView> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();
    for r in &rows {
        let k = *at.entry(r.reply_message_id.clone()).or_insert_with(|| {
            cases.push(CaseView {
                incoming: r.incoming.clone(),
                from: r.from.clone(),
                reply: r.reply.clone(),
                answers: Vec::new(),
            });
            cases.len() - 1
        });
        cases[k].answers.push(Answer { system: r.system.clone(), text: r.generated_text.clone() });
    }
    for c in &mut cases {
        c.answers.sort_by_key(|a| SYSTEMS.iter().position(|s| *s == a.system).unwrap_or(SYSTEMS.len()));
    }

    let measure_of = |system: &str, key: &str| -> Option<Measure> {
        let values: Vec<f64> =
            rows.iter().filter(|r| r.system == system).filter_map(|r| r.metrics.get(key)?.as_f64()).collect();
        summarize(&values)
    };
    let systems = SYSTEMS
        .iter()
        .filter_map(|&system| {
            let n = rows.iter().filter(|r| r.system == system).count();
            (n > 0).then(|| SystemResult {
                system: system.to_string(),
                cases: n,
                length: measure_of(system, "length"),
                vocabulary: measure_of(system, "vocabulary"),
                punctuation: measure_of(system, "punctuation"),
                embedding: measure_of(system, "embedding"),
            })
        })
        .collect();

    let number = |v: &Value, key: &str| v.get(key).and_then(Value::as_u64).map(|n| n as usize);
    let common_reply = rows
        .iter()
        .find(|r| r.system == COMMON_REPLY)
        .map(|r| CommonReplyView { text: r.generated_text.clone(), times: number(&row.config, "commonReplyTimes") });
    Ok(Some(EvaluationView {
        measured: number(&row.config, "measured").unwrap_or(0),
        remaining: cases.len(),
        measured_conversations: rows.iter().map(|r| r.conversation_id.as_str()).collect::<HashSet<_>>().len(),
        strategy: row.split.get("strategy").and_then(Value::as_str).unwrap_or("").to_string(),
        conversations: number(&row.split, "conversations").unwrap_or(0),
        held_out_conversations: number(&row.split, "heldOutConversations").unwrap_or(0),
        warnings: row
            .split
            .get("warnings")
            .and_then(Value::as_array)
            .map(|w| w.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
        embedding_provider: rows
            .iter()
            .find_map(|r| r.metrics.get("embeddingProvider").and_then(Value::as_str).map(str::to_string)),
        common_reply,
        systems,
        cases,
        id: row.id,
        created_at: row.created_at,
        provider: row.provider,
        model: row.model,
    }))
}

/// Mean and 10th percentile (interpolated between the nearest two, as numpy
/// does), rounded to four places. `None` for no values: nothing measured is
/// not a zero.
fn summarize(values: &[f64]) -> Option<Measure> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
    let pos = (sorted.len() - 1) as f64 * 0.1;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    let p10 = sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo as f64);
    Some(Measure { mean: round4(mean), p10: round4(p10) })
}

fn round4(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}

/// Runs `measure` as a background job.
pub struct EvaluateExecutor {
    engine: EngineClient,
    provider: Arc<dyn Fn() -> Option<Arc<dyn ModelProvider>> + Send + Sync>,
}

impl EvaluateExecutor {
    /// The provider is resolved when the run starts, as assisted drafting's
    /// is, because the user can change it in Settings between runs.
    pub fn shared(
        engine: EngineClient,
        provider: Arc<dyn Fn() -> Option<Arc<dyn ModelProvider>> + Send + Sync>,
    ) -> Arc<dyn JobExecutor> {
        Arc::new(EvaluateExecutor { engine, provider })
    }
}

impl JobExecutor for EvaluateExecutor {
    fn kinds(&self) -> &'static [&'static str] {
        &[JOB_KIND]
    }

    fn resumable(&self, _kind: &str) -> bool {
        // It spends model calls, and the user may not be at the machine.
        false
    }

    fn execute(&self, ctx: JobContext) -> JobFuture {
        let (engine, resolve) = (self.engine.clone(), self.provider.clone());
        Box::pin(async move {
            let provider = resolve().ok_or_else(|| JobError::Failed("no model provider is configured".to_string()))?;
            if !engine.is_ready() {
                return Err(JobError::Failed(ENGINE_DOWN.into()));
            }
            let harness = EngineHarness(engine);
            let progress = |done: usize, total: usize, phase: &str| ctx.progress(done as i64, total as i64, phase);
            let should_stop = || ctx.check_cancel().is_err();
            let outcome = measure(&ctx.db, provider.as_ref(), &harness, MAX_CASES, &progress, &should_stop).await?;
            Ok(serde_json::to_value(outcome).unwrap_or(Value::Null))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{IdentifierInput, IdentifierKind, NewMessage, NewSource};
    use crate::providers::mock::MockProvider;

    struct World {
        db: Db,
        people: Vec<String>,
        conversations: Vec<String>,
    }

    /// Six people, a conversation with each. Every conversation is three
    /// exchanges — their message, then the user's reply — and a receipt.
    /// Every reply is unique text, except "Sounds good!", sent in five: which
    /// conversations are held out depends on their (random) ids, and two
    /// held out still leave it the most common reply.
    fn world() -> World {
        let db = Db::open_in_memory().unwrap();
        db.set_user_identity("C").unwrap();
        db.add_user_identifier(IdentifierKind::Handle, "@c").unwrap();
        let source = db
            .create_source(&NewSource {
                connector: "t".into(),
                name: "chat".into(),
                channel: "chat".into(),
                location: None,
                config: Value::Null,
            })
            .unwrap();
        let mut people = Vec::new();
        let mut conversations = Vec::new();
        for (n, name) in ["Ada", "Bob", "Cy", "Dee", "Eve", "Fay"].iter().enumerate() {
            let who = db
                .resolve_participant(name, &[IdentifierInput::new(IdentifierKind::Handle, format!("@{name}"))], false)
                .unwrap();
            let convo = db.upsert_conversation(&source.id, &format!("t-{name}"), "chat", None).unwrap();
            db.link_conversation_participant(&convo, &who).unwrap();
            let mut batch = Vec::new();
            let msg = |seq: i64, dir: &str, body: String, metadata: Value| NewMessage {
                conversation_id: convo.clone(),
                source_id: source.id.clone(),
                participant_id: (dir == "other").then(|| who.clone()),
                external_id: format!("{name}-{seq}"),
                direction: dir.into(),
                channel: "chat".into(),
                sent_at: Some(format!("2026-0{}-{:02}T10:00:00Z", n + 1, seq + 1)),
                sequence_index: seq,
                body,
                reply_to_external_id: None,
                metadata,
            };
            for k in 0..3 {
                batch.push(msg(k * 2, "other", format!("question {k} from {name}"), Value::Null));
                let reply = if k == 1 && n < 5 {
                    "Sounds good!".to_string()
                } else {
                    format!("answer{k}{name} zebra{n}{k} unique reply words")
                };
                batch.push(msg(k * 2 + 1, "self", reply, Value::Null));
            }
            // A receipt, and the user's note after it: not an exchange.
            batch.push(msg(6, "other", format!("read receipt for {name}"), json!({"automated": "report"})));
            batch.push(msg(7, "self", format!("after the receipt {name}"), Value::Null));
            db.insert_messages(&batch).unwrap();
            db.link_replies(&convo).unwrap();
            db.refresh_conversation_stats(&convo).unwrap();
            people.push(who);
            conversations.push(convo);
        }
        World { db, people, conversations }
    }

    fn count(db: &Db, table: &str) -> i64 {
        db.conn().query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap()
    }

    fn no_progress(_: usize, _: usize, _: &str) {}

    #[test]
    fn an_exchange_is_someone_elses_message_and_the_reply_straight_after_it() {
        let w = world();
        let exchanges = w.db.evaluation_exchanges(1000).unwrap();
        assert_eq!(exchanges.len(), 18, "three per conversation; the receipt answers nothing");
        assert!(exchanges.iter().all(|e| e.incoming.starts_with("question")));
        assert!(exchanges.iter().all(|e| !e.reply.starts_with("after the receipt")));
        assert_eq!(exchanges[0].conversation_id, w.conversations[5], "newest first");
        assert_eq!(exchanges[0].participant_id.as_deref(), Some(w.people[5].as_str()));
        assert_eq!(w.db.evaluation_exchanges(4).unwrap().len(), 4);
    }

    fn exchange(conversation: &str, n: usize) -> Exchange {
        Exchange {
            incoming_id: format!("i{conversation}{n}"),
            incoming: "hi".into(),
            reply_id: format!("r{conversation}{n}"),
            reply: format!("reply {n}"),
            conversation_id: conversation.into(),
            participant_id: None,
            channel: "chat".into(),
        }
    }

    #[test]
    fn cases_are_spread_across_conversations_newest_first() {
        // Newest first: a, a, b, a, c, b.
        let ex = vec![
            exchange("a", 0),
            exchange("a", 1),
            exchange("b", 2),
            exchange("a", 3),
            exchange("c", 4),
            exchange("b", 5),
        ];
        let all: Vec<usize> = (0..ex.len()).collect();
        assert_eq!(choose(&ex, &all, 3), [0, 2, 4], "one from each first");
        assert_eq!(choose(&ex, &all, 5), [0, 2, 4, 1, 5], "then the next newest of each");
        assert_eq!(choose(&ex, &all, 99), [0, 2, 4, 1, 5, 3]);
        assert_eq!(choose(&ex, &[5, 1, 1, 42], 99), [1, 5], "only what was held out, once each");
        assert!(choose(&ex, &[], 5).is_empty());
    }

    #[test]
    fn the_common_reply_is_the_one_sent_most_often_among_those_learned_from() {
        let mut ex: Vec<Exchange> = (0..6).map(|n| exchange(&format!("c{n}"), n)).collect();
        ex[1].reply = "Sounds good!".into();
        ex[3].reply = "sounds  good".into();
        ex[4].reply = "Sounds good.".into();
        ex[2].reply = "ok".into();
        ex[5].reply = "ok".into();
        let all: Vec<usize> = (0..6).collect();
        let c = common_reply(&ex, &all).unwrap();
        assert_eq!((c.text.as_str(), c.times, c.message_id.as_str()), ("Sounds good!", 3, "rc11"), "as last written");
        // Held out, it is not the common reply: "ok" is, twice.
        let c = common_reply(&ex, &[0, 2, 5]).unwrap();
        assert_eq!((c.text.as_str(), c.times), ("ok", 2));
        // Nothing repeats: the newest.
        assert_eq!(common_reply(&ex, &[3, 0]).unwrap().text, "reply 0");
        assert!(common_reply(&ex, &[]).is_none());
    }

    #[test]
    fn the_tenth_percentile_is_interpolated_as_numpy_does() {
        assert_eq!(summarize(&[]), None);
        assert_eq!(summarize(&[0.5]), Some(Measure { mean: 0.5, p10: 0.5 }));
        // numpy.percentile([0, 1], 10) == 0.1; [1, 0.2, 0.4, 0.6] → 0.26.
        assert_eq!(summarize(&[1.0, 0.0]).unwrap().p10, 0.1);
        assert_eq!(summarize(&[1.0, 0.2, 0.4, 0.6]), Some(Measure { mean: 0.55, p10: 0.26 }));
    }

    #[test]
    fn the_engines_split_is_read_as_it_is_written() {
        // As `EngineService.eval_split` returns it.
        let v = json!({
            "train": [0, 2], "holdout": [1], "strategy": "conversation_grouped", "groups": 2,
            "warnings": ["only two conversations: one trains, one is held out, so the score rests on a single thread"]
        });
        let split: Split = serde_json::from_value(v).unwrap();
        assert_eq!((split.train, split.holdout, split.groups), (vec![0, 2], vec![1], 2));
        assert_eq!(split.warnings.len(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn measuring_hides_the_replies_it_predicts_and_writes_no_draft() {
        let w = world();
        let provider = MockProvider::default();
        let outcome =
            measure(&w.db, &provider, &LocalHarness, MAX_CASES, &no_progress, &|| false).await.expect("measured");
        assert!(outcome.measured > 0 && outcome.measured == outcome.asked, "{outcome:?}");
        assert_eq!(provider.call_count(), outcome.measured * 2, "two model calls per exchange");
        assert_eq!(count(&w.db, "drafts"), 0, "measuring writes no draft");

        let view = latest(&w.db).unwrap().expect("recorded");
        assert_eq!((view.measured, view.remaining), (outcome.measured, outcome.measured));
        assert!(view.held_out_conversations >= 1 && view.held_out_conversations < 6);

        // No reply that was being predicted is shown to a model as an
        // example of how to write. ("Sounds good!" is also sent in
        // conversations that were learned from, so it may be.)
        let systems: Vec<String> = provider.seen.lock().unwrap().iter().map(|r| r.system.clone()).collect();
        for case in view.cases.iter().filter(|c| c.reply != "Sounds good!") {
            assert!(
                systems.iter().all(|s| !s.contains(&case.reply)),
                "a predicted reply reached a prompt: {}",
                case.reply
            );
        }
        // Mimic's prompt carries how the user writes; the generic one doesn't.
        let seen = provider.seen.lock().unwrap();
        assert!(seen.iter().step_by(2).all(|r| r.system.contains("drafting a reply *as the user*")));
        assert!(seen.iter().skip(1).step_by(2).all(|r| r.system == GENERIC_SYSTEM));
        assert!(seen.iter().zip(seen.iter().skip(1)).step_by(2).all(|(m, g)| m.messages == g.messages));
        drop(seen);

        let names: Vec<&str> = view.systems.iter().map(|s| s.system.as_str()).collect();
        assert_eq!(names, [MIMIC, GENERIC, COMMON_REPLY]);
        for s in &view.systems {
            assert_eq!(s.cases, view.remaining);
            for m in [s.length, s.vocabulary, s.punctuation].into_iter().flatten() {
                assert!((0.0..=1.0).contains(&m.mean) && m.p10 <= m.mean, "{m:?}");
            }
            assert!(s.length.is_some() && s.embedding.is_none(), "no encoder, so no embedding measure");
        }
        let common = view.common_reply.expect("a common reply");
        assert_eq!(common.text, "Sounds good!");
        assert!(common.times.is_some_and(|t| t >= 1));
        assert!(view.cases.iter().all(|c| c.answers.len() == 3 && c.answers[0].system == MIMIC));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_transcript_stops_before_the_message_being_answered() {
        let w = world();
        let provider = MockProvider::default();
        measure(&w.db, &provider, &LocalHarness, MAX_CASES, &no_progress, &|| false).await.unwrap();
        let view = latest(&w.db).unwrap().unwrap();
        let seen = provider.seen.lock().unwrap();
        for r in seen.iter().step_by(2) {
            let task = r.messages.last().unwrap().content.clone();
            let case = view.cases.iter().find(|c| task.contains(&c.incoming)).expect("a case per prompt");
            // Everything before the task is the conversation before the
            // message, oldest first, and none of it is the message itself.
            let history: Vec<&str> = r.messages[..r.messages.len() - 1].iter().map(|m| m.content.as_str()).collect();
            assert!(!history.contains(&case.incoming.as_str()), "{history:?}");
            assert!(!history.contains(&case.reply.as_str()), "{history:?}");
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn deleting_anyone_deletes_every_measurement_and_a_fold_stops_a_case_counting() {
        let w = world();
        measure(&w.db, &MockProvider::default(), &LocalHarness, MAX_CASES, &no_progress, &|| false).await.unwrap();
        let before = latest(&w.db).unwrap().unwrap();
        assert_eq!(before.measured_conversations, 2);

        // Someone folded into the user: their message is the user's now, and
        // an exchange it answered is no exchange.
        let first = before.cases[0].clone();
        w.db.conn()
            .execute("UPDATE messages SET direction = 'self', participant_id = NULL WHERE body = ?1", [&first.incoming])
            .unwrap();
        let after = latest(&w.db).unwrap().unwrap();
        assert_eq!(after.remaining, before.remaining - 1);
        assert_eq!(after.measured, before.measured, "what ran is not rewritten");
        assert!(after.systems.iter().all(|s| s.cases == after.remaining));

        // What was written for any case may hold anyone's words — the thread
        // before it, examples from elsewhere — so a deletion takes it all,
        // whoever it was measured on, and the preview says so.
        let bystander = w.people.iter().find(|p| {
            let name = w.db.get_participant(p).unwrap().unwrap().display_name;
            before.cases.iter().all(|c| c.from.as_deref() != Some(name.as_str()))
        });
        let preview = w.db.preview_participant_deletion(bystander.unwrap()).unwrap();
        assert_eq!(preview.evaluations, 1);
        w.db.delete_participant(bystander.unwrap()).unwrap();
        assert!(latest(&w.db).unwrap().is_none());
        assert_eq!(count(&w.db, "evaluation_cases"), 0);
    }

    #[test]
    fn the_voice_is_measured_again_without_the_held_out_conversations() {
        let w = world();
        crate::voice::analyze(&w.db, &mut |_, _| {}).unwrap();
        // Four messages of the user's in each of six threads.
        let stored = crate::voice::resolve(&w.db, "chat", Some(&w.people[0]), None).unwrap();
        assert_eq!(stored.layers.iter().find(|l| l.layer == "global").unwrap().sample_size, 24);

        let fresh = crate::voice::LeavingOut::new(&w.db, std::slice::from_ref(&w.conversations[0]));
        let v = fresh.resolve("chat", Some(&w.people[0]), None).unwrap();
        let global = v.layers.iter().find(|l| l.layer == "global").unwrap();
        assert_eq!(global.sample_size, 20, "the held-out thread's four are not counted");
        assert!(global.measurable && !global.stale);
        assert!(
            v.layers.iter().all(|l| l.layer != "relationship"),
            "everything written to Ada was in the held-out thread, so there is no layer for her"
        );
        assert!(v.examples.is_empty());
        // Asked again, the same answer, from what was worked out the first time.
        let again = fresh.resolve("chat", Some(&w.people[0]), None).unwrap();
        assert_eq!(again.layers.len(), v.layers.len());
    }

    /// Reads its whole prompt back as its answer, so anything a prompt held
    /// is in what gets recorded. The first time it is asked, it deletes the
    /// person whose message the first example answered — someone the answer
    /// is certainly written from — as a user might while a measurement runs,
    /// and keeps what their thread said.
    struct Recite {
        db: Db,
        people: Vec<(String, String)>,
        deleted: std::sync::Mutex<Option<Vec<String>>>,
    }

    impl ModelProvider for Recite {
        fn info(&self) -> crate::providers::ProviderInfo {
            MockProvider::default().info()
        }

        fn generate(
            &self,
            r: &GenerationRequest,
        ) -> crate::providers::ProviderResult<crate::providers::GenerationResponse> {
            let mut deleted = self.deleted.lock().unwrap();
            if deleted.is_none() {
                let line = r.system.lines().find(|l| l.starts_with("--- in reply to: question")).expect("an example");
                let name = line.split_whitespace().last().unwrap();
                let (id, _) = self.people.iter().find(|(_, n)| n == name).unwrap();
                let thread = self.db.latest_conversation_with(id).unwrap().unwrap().id;
                let bodies: Vec<String> = self
                    .db
                    .conn()
                    .prepare("SELECT body FROM messages WHERE conversation_id = ?1")
                    .unwrap()
                    .query_map([&thread], |row| row.get(0))
                    .unwrap()
                    .collect::<Result<_, _>>()
                    .unwrap();
                self.db.delete_participant(id).unwrap();
                *deleted = Some(bodies);
            }
            let said: Vec<&str> = r.messages.iter().map(|m| m.content.as_str()).collect();
            Ok(crate::providers::GenerationResponse {
                text: format!("{}\n{}", r.system, said.join("\n")),
                provider: "mock".into(),
                model: "mock-1".into(),
                input_tokens: None,
                output_tokens: None,
            })
        }

        fn health(&self) -> crate::providers::ProviderResult<()> {
            Ok(())
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn someone_deleted_while_measuring_leaves_nothing_written_from_their_mail() {
        let w = world();
        let names = ["Ada", "Bob", "Cy", "Dee", "Eve", "Fay"];
        let provider = Recite {
            db: w.db.clone(),
            people: w.people.iter().cloned().zip(names.iter().map(|n| n.to_string())).collect(),
            deleted: std::sync::Mutex::new(None),
        };
        let outcome = measure(&w.db, &provider, &LocalHarness, MAX_CASES, &no_progress, &|| false).await;
        let theirs = provider.deleted.lock().unwrap().clone().expect("someone was deleted");
        let written: Vec<String> =
            w.db.conn()
                .prepare("SELECT generated_text FROM evaluation_cases")
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
        for body in theirs.iter().filter(|b| b.as_str() != "Sounds good!") {
            assert!(written.iter().all(|t| !t.contains(body.as_str())), "{body:?} outlived its deletion");
        }
        // The exchange answered from their mail was dropped, all three
        // answers together; whatever was kept is shown in full.
        match outcome {
            Ok(o) => {
                let view = latest(&w.db).unwrap().unwrap();
                assert!(view.remaining < o.measured, "{} of {}", view.remaining, o.measured);
                assert!(view.systems.iter().all(|s| s.cases == view.remaining));
                // Their thread may have been where the common reply was taken
                // from; then that column goes from every exchange at once.
                assert!(view.systems.len() >= 2);
                assert!(view.cases.iter().all(|c| c.answers.len() == view.systems.len()));
            }
            Err(e) => {
                assert!(e.to_string().contains("was deleted while I was measuring"), "{e}");
                assert!(latest(&w.db).unwrap().is_none());
            }
        }
    }

    /// Stops the run while it compares, as a user cancelling then would.
    struct StopsWhileComparing(std::sync::atomic::AtomicBool);

    impl Harness for StopsWhileComparing {
        fn split(&self, group_keys: Vec<String>) -> BoxFuture<'_, Result<Split, JobError>> {
            Box::pin(async move { Ok(local_split(&group_keys)) })
        }

        fn compare(&self, pairs: Vec<(String, String)>) -> BoxFuture<'_, Result<Vec<Value>, JobError>> {
            Box::pin(async move {
                self.0.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(pairs.iter().map(|(g, a)| local_compare(g, a)).collect())
            })
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_run_stopped_while_comparing_records_nothing() {
        let w = world();
        let harness = StopsWhileComparing(std::sync::atomic::AtomicBool::new(false));
        let stop = || harness.0.load(std::sync::atomic::Ordering::SeqCst);
        let err = measure(&w.db, &MockProvider::default(), &harness, MAX_CASES, &no_progress, &stop).await.unwrap_err();
        assert!(matches!(err, JobError::Canceled), "{err}");
        assert!(latest(&w.db).unwrap().is_none());
    }

    #[test]
    fn a_phrase_only_the_held_out_thread_has_never_reaches_the_prompt() {
        let db = Db::open_in_memory().unwrap();
        db.set_user_identity("C").unwrap();
        db.add_user_identifier(IdentifierKind::Handle, "@c").unwrap();
        let source = db
            .create_source(&NewSource {
                connector: "t".into(),
                name: "chat".into(),
                channel: "chat".into(),
                location: None,
                config: Value::Null,
            })
            .unwrap();
        // With Zed, the thread under test, the user keeps saying one thing.
        // With Yan, twenty-four replies: enough to measure without Zed's.
        let thread = |name: &str, n: i64, reply: &dyn Fn(i64) -> String| {
            let who = db
                .resolve_participant(name, &[IdentifierInput::new(IdentifierKind::Handle, format!("@{name}"))], false)
                .unwrap();
            let convo = db.upsert_conversation(&source.id, &format!("t-{name}"), "chat", None).unwrap();
            db.link_conversation_participant(&convo, &who).unwrap();
            let mut batch = Vec::new();
            for k in 0..n {
                for (seq, dir, body) in
                    [(k * 2, "other", format!("what now {name} {k}")), (k * 2 + 1, "self", reply(k))]
                {
                    batch.push(NewMessage {
                        conversation_id: convo.clone(),
                        source_id: source.id.clone(),
                        participant_id: (dir == "other").then(|| who.clone()),
                        external_id: format!("{name}-{seq}"),
                        direction: dir.into(),
                        channel: "chat".into(),
                        sent_at: Some(format!("2026-03-{:02}T{:02}:00:00Z", k + 1, seq % 24)),
                        sequence_index: seq,
                        body,
                        reply_to_external_id: None,
                        metadata: Value::Null,
                    });
                }
            }
            db.insert_messages(&batch).unwrap();
            db.link_replies(&convo).unwrap();
            db.refresh_conversation_stats(&convo).unwrap();
            (who, convo)
        };
        let (zed, zed_thread) = thread("Zed", 5, &|k| format!("the zanzibar protocol stands firm {k}"));
        thread("Yan", 24, &|k| format!("fine by me number {k}"));
        crate::voice::analyze(&db, &mut |_, _| {}).unwrap();

        let ex = db.evaluation_exchanges(100).unwrap().into_iter().find(|e| e.conversation_id == zed_thread).unwrap();
        let req = ComposeRequest {
            participant_id: Some(zed),
            conversation_id: Some(zed_thread.clone()),
            channel: "chat".into(),
            incoming_message: Some(ex.incoming.clone()),
            incoming_message_id: Some(ex.incoming_id.clone()),
            ..Default::default()
        };
        // An ordinary draft learns the phrase from the stored profile.
        let plain = assemble(&crate::generation::build_context(&db, &req).unwrap(), &req).system;
        assert!(plain.contains("\"zanzibar protocol stands\""), "{plain}");

        let fresh = crate::voice::LeavingOut::new(&db, std::slice::from_ref(&zed_thread));
        let hide =
            HeldOut { conversations: vec![zed_thread], before_message: Some(ex.incoming_id), voice: Some(&fresh) };
        let measured = assemble(&build_context_holding_out(&db, &req, &hide).unwrap(), &req).system;
        assert!(measured.contains("measured from their own messages"), "the voice is still measured: {measured}");
        assert!(measured.contains("fine by me"), "{measured}");
        assert!(!measured.contains("zanzibar"), "{measured}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn with_one_conversation_there_is_nothing_honest_to_measure() {
        let w = world();
        for p in &w.people[1..] {
            w.db.delete_participant(p).unwrap();
        }
        let provider = MockProvider::default();
        let err = measure(&w.db, &provider, &LocalHarness, MAX_CASES, &no_progress, &|| false).await.unwrap_err();
        assert!(matches!(&err, JobError::Failed(m) if m == NOT_ENOUGH), "{err}");
        assert_eq!(provider.call_count(), 0);
        assert!(latest(&w.db).unwrap().is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_stopped_run_records_nothing_and_a_refused_one_says_why() {
        let w = world();
        let provider = MockProvider::default();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let stop = || calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= 2;
        let err = measure(&w.db, &provider, &LocalHarness, MAX_CASES, &no_progress, &stop).await.unwrap_err();
        assert!(matches!(err, JobError::Canceled), "{err}");
        assert!(latest(&w.db).unwrap().is_none(), "part of a measurement is not recorded");
        assert_eq!(count(&w.db, "evaluation_cases"), 0);

        let refusing = MockProvider::failing("model not loaded");
        let err = measure(&w.db, &refusing, &LocalHarness, MAX_CASES, &no_progress, &|| false).await.unwrap_err();
        assert!(err.to_string().contains("model not loaded"), "{err}");
        assert!(latest(&w.db).unwrap().is_none());
    }
}
