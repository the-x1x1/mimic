# Project status — 0.6.0-alpha.2

The truth table. Every row has a status and evidence naming the test, fixture or check that proves it. If this file and the code disagree, the code is right and this file is a bug.

Statuses: `IMPLEMENTED` · `PARTIAL` · `PLANNED` · `UNSUPPORTED`

## Foundation

| Item                                                               | Status      | Evidence                                                                              |
| ------------------------------------------------------------------ | ----------- | ------------------------------------------------------------------------------------- |
| SQLite, WAL, foreign keys, one connection behind a mutex           | IMPLEMENTED | `db::tests::foreign_keys_are_enforced`, `reopen_is_idempotent_and_keeps_data`         |
| Forward-only migrations with a backup before upgrading an install  | IMPLEMENTED | `db::tests::upgrade_from_older_schema_creates_backup`                                 |
| Schema v5; a v4 photography database upgrades cleanly              | IMPLEMENTED | `migrations::tests::photography_database_upgrades_to_the_communication_schema`        |
| Schema invariants (direction vocabulary, import key, cascades)     | IMPLEMENTED | `migrations::tests::communication_schema_enforces_its_invariants`                     |
| Persistent job queue: progress, cancellation, interrupted recovery | IMPLEMENTED | `jobs::tests::runs_completes_and_fails_jobs`, `cancel_stops_between_items`            |
| Python engine sidecar: NDJSON, restart budget, size caps, timeouts | IMPLEMENTED | `tests/engine_protocol.rs` (4 tests against a real child process)                     |
| Typed IPC with zod validation at the boundary                      | IMPLEMENTED | `apps/desktop/src/lib/ipc.ts`; shape mismatch throws with the offending path          |
| Cross-language contract fixtures                                   | IMPLEMENTED | `tests/pipeline_e2e.rs` writes `fixtures/contracts/`; `contracts.test.ts` parses them |
| Diagnostics bundle with no credentials and optional path redaction | IMPLEMENTED | `diagnostics::tests::bundle_has_no_credentials_and_redacts_paths`                     |

## Identity and people

| Item                                                         | Status      | Evidence                                                                                                |
| ------------------------------------------------------------ | ----------- | ------------------------------------------------------------------------------------------------------- |
| User identity with multiple identifiers, normalized          | IMPLEMENTED | `repo_identity::tests::identity_is_created_renamed_and_deduplicated`                                    |
| Phone/email/handle normalization                             | IMPLEMENTED | `models::tests::phone_numbers_normalize_to_one_form`, `emails_and_handles_fold_case_account_ids_do_not` |
| Participant resolution; the same address is the same person  | IMPLEMENTED | `repo_people::tests::the_same_address_in_any_case_is_the_same_person`                                   |
| A shared address is not silently merged into a second person | IMPLEMENTED | `repo_people::tests::a_shared_address_is_not_silently_merged_into_a_second_person`                      |
| An author with no usable identifier is refused, not pooled   | IMPLEMENTED | `repo_people::tests::unusable_identifiers_are_refused_rather_than_pooled`                               |
| User-declared relationships; never inferred                  | IMPLEMENTED | `repo_people::tests::relationship_and_name_can_be_corrected_by_hand`                                    |

## Sources and import

| Item                                                          | Status      | Evidence                                                                                                    |
| ------------------------------------------------------------- | ----------- | ----------------------------------------------------------------------------------------------------------- |
| `CommunicationSource` contract; distinct keys, known channels | IMPLEMENTED | `sources::tests::every_connector_has_a_distinct_key_and_a_known_channel`                                    |
| `mbox` connector: threading, folded headers, separator safety | IMPLEMENTED | `sources::mbox::tests` (8 tests)                                                                            |
| `mimic_json` connector: the documented generic format         | IMPLEMENTED | `sources::mimic_json::tests` (6 tests)                                                                      |
| Quoted-reply and signature stripping; sign-offs preserved     | IMPLEMENTED | `sources::normalize::tests` (7 tests)                                                                       |
| Validation before import, agreeing with what import will do   | IMPLEMENTED | `sources::validate_by_dry_run`; `mimic_json::tests::validation_reports_shape_and_the_problems_worth_naming` |
| Streaming import, batched inserts, participant cache          | IMPLEMENTED | `import::tests::an_import_attributes_every_message_and_creates_the_people`                                  |
| Re-importing is free                                          | IMPLEMENTED | `import::tests::importing_twice_changes_nothing`                                                            |
| Import refuses to run without a declared identity             | IMPLEMENTED | `import::tests::importing_without_an_identity_is_refused_rather_than_guessed`                               |
| Cancel keeps what was written; resuming finishes the job      | IMPLEMENTED | `import::tests::cancelling_keeps_what_was_written_and_leaves_the_source_resumable`                          |
| Reply linking and response latency derived per conversation   | IMPLEMENTED | `repo_messages::tests::replies_and_latency_are_derived_after_the_batch`                                     |
| Keyset pagination stable under duplicate timestamps           | IMPLEMENTED | `repo_messages::tests::self_messages_page_by_keyset_even_with_duplicate_timestamps`                         |
| Incremental sync (a watermark rather than a full re-read)     | PLANNED     | Phase 4                                                                                                     |

## Voice

| Item                                                        | Status      | Evidence                                                                                                                                                              |
| ----------------------------------------------------------- | ----------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Deterministic metrics (length, punctuation, case, emoji, …) | IMPLEMENTED | `voice::metrics::tests` (10 tests)                                                                                                                                    |
| A sample under 20 reports its size and no rates             | IMPLEMENTED | `voice::metrics::tests::a_small_sample_reports_its_size_and_refuses_to_guess`                                                                                         |
| `null` (unmeasured) and `0` (measured zero) stay distinct   | IMPLEMENTED | `metrics::tests::punctuation_habits_are_counted_from_the_last_real_character`; `contracts.test.ts` "rates are rendered honestly"                                      |
| Global, channel and relationship layers                     | IMPLEMENTED | `voice::tests::analysis_writes_a_layer_per_scope_and_says_what_it_could_not_measure`                                                                                  |
| Situational layer                                           | PARTIAL     | Tables, resolution and prompt slot exist; nothing classifies. Phase 2                                                                                                 |
| Layer resolution, innermost measurable wins                 | IMPLEMENTED | `voice::tests::effective_metrics_prefer_the_innermost_measurable_layer`                                                                                               |
| Manual preferences override the statistics                  | IMPLEMENTED | `repo_voice::tests::manual_preferences_are_upserted_and_scoped`; `generation::tests::manual_preferences_appear_after_the_measurements_and_are_labelled_as_overriding` |
| Representative examples: deterministic, de-duplicated       | IMPLEMENTED | `voice::tests::example_selection_is_deterministic_and_avoids_duplicates`, `near_duplicate_messages_are_not_all_chosen`                                                |
| Staleness propagates from a person to the aggregates        | IMPLEMENTED | `repo_voice::tests::staleness_spreads_from_a_person_to_the_aggregates`                                                                                                |
| Analysis streams rather than materializing a scope          | PARTIAL     | Paged reads, in-memory accumulation. Phase 2                                                                                                                          |

## Retrieval and generation

| Item                                                            | Status      | Evidence                                                                                 |
| --------------------------------------------------------------- | ----------- | ---------------------------------------------------------------------------------------- |
| Metadata filter applied before ranking                          | IMPLEMENTED | `retrieval::tests::the_filter_runs_before_the_ranking`                                   |
| Every filter dimension narrows                                  | IMPLEMENTED | `retrieval::tests::every_filter_dimension_narrows`                                       |
| Lexical ranking with inverse document frequency                 | IMPLEMENTED | `retrieval::tests::similar_wording_outranks_recency`                                     |
| Embedding-backed ranking                                        | PARTIAL     | `lexical_v1` in the engine, reporting `semantic: false`. Phase 2                         |
| Generation context with human-readable evidence                 | IMPLEMENTED | `generation::tests::the_prompt_carries_the_intent_the_incoming_message_and_the_examples` |
| Prompt assembly is a pure function                              | IMPLEMENTED | `generation::tests::the_prompt_is_a_pure_function_of_the_context`                        |
| Measured habits become instructions, unmeasured ones are silent | IMPLEMENTED | `generation::tests::measured_habits_become_instructions_not_raw_numbers`                 |
| Output budget derived from the user's own message length        | IMPLEMENTED | `generation::tests::the_output_budget_follows_how_long_this_person_actually_writes`      |
| Adjustments (shorter, longer, casual, professional)             | IMPLEMENTED | `generation::tests::an_adjustment_changes_the_prompt_and_the_hash`                       |
| Falling back to the global voice for an unknown recipient       | IMPLEMENTED | `generation::tests::writing_to_someone_new_falls_back_to_the_global_voice_and_says_so`   |

## Providers

| Item                                                 | Status      | Evidence                                                                                                                            |
| ---------------------------------------------------- | ----------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| `ModelProvider` trait; registry; local-first default | IMPLEMENTED | `providers::tests::the_default_provider_is_a_local_one_when_there_is_one`                                                           |
| Local OpenAI-compatible provider                     | IMPLEMENTED | `providers::http::tests::a_local_provider_only_claims_to_be_local_when_it_is`                                                       |
| The locality claim is computed, not asserted         | IMPLEMENTED | same test; `providers_config::tests::settings_change_the_endpoint_and_the_locality_claim_follows`                                   |
| Anthropic provider                                   | IMPLEMENTED | `providers::http::tests::anthropic_asks_for_a_key_before_it_asks_for_anything_else`. **Never exercised against the live API in CI** |
| Errors carry no request body                         | IMPLEMENTED | `providers::http::tests::provider_errors_stay_one_line_and_carry_no_body`                                                           |
| Credentials in the OS credential store               | PARTIAL     | Owner-only file; `secrets::tests::the_file_is_owner_only`. Phase 2                                                                  |

## Learning loop

| Item                                                   | Status      | Evidence                                                                                |
| ------------------------------------------------------ | ----------- | --------------------------------------------------------------------------------------- |
| Draft → sent → diff recorded                           | IMPLEMENTED | `pipeline_e2e::the_learning_loop_records_what_the_user_actually_sent`                   |
| Diff describes length, greeting, sign-off, emoji, case | IMPLEMENTED | `generation::feedback::tests` (8 tests)                                                 |
| Stated preferences outweigh inferred edits             | IMPLEMENTED | `repo_drafts::tests::explicit_preferences_outweigh_inferred_edits_and_replace_in_place` |
| Measured draft outcomes; unmeasured stays null         | IMPLEMENTED | `repo_drafts::tests::outcomes_are_unmeasured_until_a_draft_is_resolved`                 |
| Feedback changes a profile                             | PLANNED     | Phase 3. Nothing consumes `draft_feedback` yet                                          |

## Privacy

| Item                                                              | Status      | Evidence                                                                                   |
| ----------------------------------------------------------------- | ----------- | ------------------------------------------------------------------------------------------ |
| Deletion preview matches the deletion exactly                     | IMPLEMENTED | `privacy::tests::a_preview_changes_nothing_and_matches_what_deletion_does`                 |
| Deleting a person removes the user's half of the conversation     | IMPLEMENTED | `privacy::tests::deleting_a_person_removes_their_words_and_the_users_half_of_the_exchange` |
| Everything derived from them goes too                             | IMPLEMENTED | `privacy::tests::deleting_a_person_removes_everything_derived_from_them`                   |
| Aggregates that included them are invalidated                     | IMPLEMENTED | `privacy::tests::the_aggregates_that_included_them_are_invalidated_not_left_standing`      |
| Group conversations survive minus that person                     | IMPLEMENTED | `privacy::tests::a_group_conversation_survives_minus_the_deleted_person`                   |
| Deleting a source removes its import and the people it introduced | IMPLEMENTED | `privacy::tests::deleting_a_source_takes_its_import_and_the_people_it_introduced`          |
| Delete-everything keeps settings and identity                     | IMPLEMENTED | `privacy::tests::deleting_everything_keeps_the_settings_and_the_identity`                  |
| No message content in logs by default                             | IMPLEMENTED | By construction; provider errors truncated and body-free                                   |

## Evaluation

| Item                                                    | Status      | Evidence                                                                             |
| ------------------------------------------------------- | ----------- | ------------------------------------------------------------------------------------ |
| Conversation-grouped split                              | IMPLEMENTED | `test_text.py::test_split_is_reproducible_and_honest_about_thin_data`                |
| Comparison components (length, vocab, punct, embedding) | IMPLEMENTED | `test_text.py::test_comparison_components_are_each_meaningful`                       |
| Summary refuses a headline score                        | IMPLEMENTED | `test_service.py::test_eval_compare_reports_components_and_refuses_a_headline_score` |
| The loop that runs it over a real corpus                | PLANNED     | Phase 3. `evaluations` rows are never written                                        |
| Baselines (generic assistant, most common phrasing)     | PLANNED     | Phase 3                                                                              |
| An accuracy figure in the UI                            | UNSUPPORTED | Nothing has earned one. Deliberate                                                   |

## Interface

| Item                                             | Status      | Evidence                                                    |
| ------------------------------------------------ | ----------- | ----------------------------------------------------------- |
| Compose / People / Voice / Sources / Settings    | IMPLEMENTED | `apps/desktop/src/features/`                                |
| Evidence panel with measured habits and examples | IMPLEMENTED | `EvidencePanel.test.tsx` (5 tests)                          |
| The provider's locality is shown in the top bar  | IMPLEMENTED | `StatusBadges.test.tsx`                                     |
| Deletion dialog states consequences, not counts  | IMPLEMENTED | `contracts.test.ts` "deletion is described in consequences" |
| Onboarding advances on facts, not checkboxes     | IMPLEMENTED | `contracts.test.ts` "onboarding advances on facts"          |
| Conversation view; Compose opened from a thread  | PLANNED     | Phase 5                                                     |

## Packaging

| Item                                            | Status      | Evidence                                                                                  |
| ----------------------------------------------- | ----------- | ----------------------------------------------------------------------------------------- |
| Version consistency across every manifest       | IMPLEMENTED | `node scripts/sync-version.mjs --check`, in CI                                            |
| CI: frontend, Rust, Python, security            | IMPLEMENTED | `.github/workflows/ci.yml`                                                                |
| Signed updater, `latest.json`, release workflow | PARTIAL     | Plumbing unchanged from 0.5.0 and never observed producing a Windows installer end to end |
| Windows installer                               | PARTIAL     | `tauri build` has not been run in this environment                                        |
| macOS                                           | PLANNED     | Nothing is Windows-specific; nothing has been tested                                      |
| Nightly smoke workflow                          | PARTIAL     | Still exercises the photography path; needs rewriting                                     |
