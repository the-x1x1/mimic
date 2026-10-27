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
use mimic_core::{import, voice};
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
                if generation::chat_unmeasured(&ctx.channel, &ctx.voice) { ", its own voice unmeasured" } else { "" },
                started.elapsed().as_secs_f64(),
                draft.generated_text
            );
            let text = &draft.generated_text;
            assert!(!text.trim().is_empty(), "{model}, {channel}: an empty draft was kept");
            assert!(!text.to_lowercase().contains("<think"), "{model}, {channel}: thinking in the draft");
            assert_eq!(generation::tidy(text), *text, "{model}, {channel}: kept with its wrapping");
            assert!(db.get_draft(&draft.id).unwrap().is_some(), "kept");
        }
    }
}
