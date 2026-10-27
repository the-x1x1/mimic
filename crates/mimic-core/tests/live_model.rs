//! Drafts from a real model on this computer, over the checked-in sample
//! export (0.10.0-alpha.85). Ignored unless asked for, since it needs a model
//! running: `cargo test -p mimic-core --test live_model -- --ignored --nocapture`,
//! with `MIMIC_LIVE_ENDPOINT` (Ollama's `http://127.0.0.1:11434/v1` unless
//! set) and `MIMIC_LIVE_MODELS` (`llama3.2:3b` unless set; several separated
//! by commas). Everything sent is the sample export's made-up people.
//!
//! What a model writes differs from run to run, so only what Mimic promises
//! is asserted — a draft is written and kept, it is only the reply, and no
//! thinking is in it — and each draft is printed for a person to read.

use std::path::PathBuf;

use mimic_core::db::{Db, IdentifierKind, NewSource};
use mimic_core::generation::{self, ComposeRequest};
use mimic_core::providers::http::LocalHttpProvider;
use mimic_core::{evaluation, import, voice};
use serde_json::Value;

fn sample() -> (Db, String, String) {
    let db = Db::open_in_memory().unwrap();
    db.set_user_identity("C").unwrap();
    db.add_user_identifier(IdentifierKind::Email, "c@example.com").unwrap();
    db.add_user_identifier(IdentifierKind::Phone, "+1 (555) 010-1111").unwrap();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/import/sample_export.json");
    let source = db
        .create_source(&NewSource {
            connector: "mimic_json".into(),
            name: "Sample export".into(),
            channel: "chat".into(),
            location: Some(path.to_string_lossy().into()),
            config: Value::Null,
        })
        .unwrap();
    import::import_source(&db, &source.id, &mut |_, _| {}, &|| false).unwrap();
    voice::analyze(&db, &mut |_, _| {}).unwrap();
    let people = db.list_participants(10).unwrap();
    let id = |name: &str| {
        people.iter().find(|p| p.participant.display_name.starts_with(name)).unwrap().participant.id.clone()
    };
    let (ada, bob) = (id("Ada"), id("Bob"));
    (db, ada, bob)
}

#[test]
#[ignore = "needs a model running on this computer; see the top of the file"]
fn a_real_model_writes_drafts_that_are_only_the_reply() {
    let endpoint = std::env::var("MIMIC_LIVE_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:11434/v1".into());
    let models = std::env::var("MIMIC_LIVE_MODELS").unwrap_or_else(|_| "llama3.2:3b".into());
    let (db, ada, bob) = sample();
    for model in models.split(',').map(str::trim).filter(|m| !m.is_empty()) {
        let provider = LocalHttpProvider { base_url: endpoint.clone(), model: model.to_string() };
        for (who, channel, incoming, intent) in [
            (&ada, "chat", "can you send the deck again before friday?", Some("yes, tomorrow morning")),
            (&bob, "sms", "you around later?", Some("yes from 6")),
            (&ada, "email", "Could you review the appendix before the board meeting?", None),
        ] {
            let req = ComposeRequest {
                participant_id: Some(who.clone()),
                channel: channel.into(),
                incoming_message: Some(incoming.into()),
                intent: intent.map(str::to_string),
                ..Default::default()
            };
            let ctx = generation::build_context(&db, &req).unwrap();
            let started = std::time::Instant::now();
            let draft = generation::compose(&db, &provider, &req)
                .unwrap_or_else(|e| panic!("{model}, {channel}: no draft: {e}"));
            println!(
                "--- {model}, {channel}{} ({:.1}s)\n{}\n",
                if ctx.form_from_elsewhere { ", its own voice unmeasured" } else { "" },
                started.elapsed().as_secs_f64(),
                draft.generated_text
            );
            let text = &draft.generated_text;
            assert!(!text.trim().is_empty(), "{model}, {channel}: an empty draft was kept");
            assert!(!text.to_lowercase().contains("<think"), "{model}, {channel}: thinking in the draft");
            assert_eq!(generation::tidy(text), *text, "{model}, {channel}: kept with its wrapping");
            // And by its look, apart from what tidy knows: no line above it
            // ending in a colon, no subject, no fence.
            let first = text.lines().next().unwrap_or_default().trim();
            assert!(!first.ends_with(':'), "{model}, {channel}: begins with an introduction? {first:?}");
            assert!(!first.to_lowercase().starts_with("subject:"), "{model}, {channel}: a subject line");
            assert!(!text.contains("```"), "{model}, {channel}: a code fence");
            assert!(db.get_draft(&draft.id).unwrap().is_some(), "kept");
        }
    }
}

/// What else a model on this computer is asked to do, over the same sample:
/// file the user's messages by what they were doing, put the measurements
/// into words, and be measured against the replies the user actually sent.
/// Printed for a person to read; asserted only that each ran.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a model running on this computer; see the top of the file"]
async fn a_real_model_files_describes_and_is_measured() {
    let endpoint = std::env::var("MIMIC_LIVE_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:11434/v1".into());
    let models = std::env::var("MIMIC_LIVE_MODELS").unwrap_or_else(|_| "llama3.2:3b".into());
    for model in models.split(',').map(str::trim).filter(|m| !m.is_empty()) {
        let (db, _, _) = sample();
        let provider = LocalHttpProvider { base_url: endpoint.clone(), model: model.to_string() };

        // Blocking calls, out of the runtime as the app makes them.
        let started = std::time::Instant::now();
        let filed = tokio::task::block_in_place(|| {
            mimic_core::situations::read_with_model(&db, &provider, 40, &mut |_, _| {}, &|| false)
        })
        .unwrap_or_else(|e| panic!("{model}: filing failed: {e}"));
        println!(
            "--- {model}, filing ({:.1}s): read {}, not understood {}, situations changed {:?}",
            started.elapsed().as_secs_f64(),
            filed.read,
            filed.not_understood,
            filed.changed
        );
        assert!(filed.read + filed.not_understood > 0, "{model}: nothing was read");

        let started = std::time::Instant::now();
        let described = tokio::task::block_in_place(|| {
            mimic_core::voice::describe::describe_layers(&db, &provider, 3, &mut |_, _| {}, &|| false)
        })
        .unwrap();
        println!(
            "--- {model}, in words ({:.1}s): {} described, {} failed{}",
            started.elapsed().as_secs_f64(),
            described.described,
            described.failed,
            described.last_error.as_deref().map(|e| format!(" ({e})")).unwrap_or_default()
        );
        for row in db.list_voice_profiles(mimic_core::version::ANALYSIS_VERSION).unwrap() {
            let metrics: voice::metrics::VoiceMetrics = serde_json::from_value(row.metrics.clone()).unwrap_or_default();
            if let Some(r) = voice::describe::reading_of(&row.qualitative, &metrics) {
                println!("    {} {}: {}", row.layer, row.scope_key, r.text);
            }
        }
        assert!(described.described > 0, "{model}: no layer was put into words");

        let started = std::time::Instant::now();
        match evaluation::measure(&db, &provider, &evaluation::LocalHarness, 6, &|_, _, _| {}, &|| false).await {
            Ok(outcome) => {
                println!(
                    "--- {model}, measured ({:.1}s): asked {}, measured {}, failed {}",
                    started.elapsed().as_secs_f64(),
                    outcome.asked,
                    outcome.measured,
                    outcome.failed
                );
                let view = evaluation::latest(&db).unwrap().expect("recorded");
                for system in &view.systems {
                    println!("    {}", serde_json::to_string(system).unwrap());
                }
                for case in &view.cases {
                    println!("    {}", serde_json::to_string(case).unwrap());
                }
                assert!(outcome.measured > 0, "{model}: nothing was measured");
            }
            Err(e) => panic!("{model}: measuring failed: {e}"),
        }
    }
}
