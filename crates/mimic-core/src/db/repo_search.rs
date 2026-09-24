//! Finding what was said: every message Mimic has read, the user's and
//! everyone else's, searched through the `message_search` index (migration
//! 0013), newest first.
//!
//! What the user types is never read as the index's query language: each
//! word is looked for as it is, words in double quotes together as a phrase,
//! and the last word, from three letters, also as the start of a longer one.
//! Every word must be there. Case and accents do not matter.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::repo_messages::{count_side, map_thread, thread_cols};
use super::{Db, DbResult, ThreadMessage, Toward};

/// Whose messages a search looks through.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SearchWho {
    #[default]
    Anyone,
    /// The user's own messages.
    You,
    /// Everyone else's — every message that is not the user's, including
    /// those whose writer could not be told.
    Others,
}

/// Part of the words around a match: a word that matched, or what is
/// between.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnippetPiece {
    pub text: String,
    pub hit: bool,
}

/// One message that says what was searched for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundMessage {
    pub message: ThreadMessage,
    pub conversation_id: String,
    pub subject: Option<String>,
    /// The words around what matched, with the matches marked.
    pub snippet: Vec<SnippetPiece>,
    /// How many messages of its conversation come before it, and after.
    pub earlier: i64,
    pub later: i64,
}

/// A page of what a search found, newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    pub found: Vec<FoundMessage>,
    /// Whether more were found past the last one here.
    pub more: bool,
    /// How many messages say it in all, counted up to `COUNT_CAP`.
    pub total: i64,
    /// There are more than `COUNT_CAP`.
    pub capped: bool,
}

/// How far the matches of one search are counted.
pub const COUNT_CAP: i64 = 1000;
/// Words looked for at most; what is typed past them is not.
const MAX_TERMS: usize = 16;
/// The shortest last word also looked for as the start of a longer one: "a"
/// as a prefix would be every word beginning with it. In scripts where one
/// character is a syllable or more (`dense`), one is enough.
const MIN_PREFIX_CHARS: usize = 3;
const MAX_QUERY_CHARS: usize = 300;
/// Words of context either side of a match, in all.
const SNIPPET_WORDS: i64 = 24;
/// Where the index marks a match in a snippet: characters from the private
/// use area, which a message almost never holds.
const HIT_START: char = '\u{E000}';
const HIT_END: char = '\u{E001}';

/// A character that is a syllable or more: an ideograph, kana, a Hangul
/// syllable, or a letter of the scripts of South-East Asia. Chinese,
/// Japanese and those scripts are written without spaces, so the index keeps
/// a whole run as one word; Korean joins its particles to the word before
/// them ("학교에서"). Either way the start of a word is one or two of these.
fn dense(c: char) -> bool {
    matches!(c as u32,
        0x0E00..=0x0EFF      // Thai, Lao
        | 0x1000..=0x109F    // Myanmar
        | 0x1780..=0x17FF    // Khmer
        | 0x3040..=0x30FF    // Hiragana, Katakana
        | 0x3400..=0x4DBF    // CJK Extension A
        | 0x4E00..=0x9FFF    // CJK Unified Ideographs
        | 0xAC00..=0xD7AF    // Hangul syllables
        | 0xF900..=0xFAFF    // CJK Compatibility Ideographs
        | 0x20000..=0x3134F  // CJK Extensions B to G
    )
}

/// What the user typed, as a query of the index every word of which must
/// match, or None when it holds nothing to look for (only punctuation, or
/// nothing at all). Each word is quoted, so nothing typed — `OR`, `NEAR(`,
/// `-`, a column name — is read as the query language; words inside double
/// quotes are one phrase; the last word outside quotes, when it has three
/// letters or more (or any `dense` character), also matches as the start of
/// a longer one, so "fri" finds "Friday" and "학교" finds "학교에서".
pub fn search_query(input: &str) -> Option<String> {
    let input: String = input.chars().take(MAX_QUERY_CHARS).collect();
    let mut terms: Vec<(String, bool)> = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let keep = |terms: &mut Vec<(String, bool)>, current: &mut String, phrase: bool| {
        if current.chars().any(char::is_alphanumeric) {
            terms.push((std::mem::take(current), phrase));
        }
        current.clear();
    };
    for c in input.chars() {
        match c {
            '"' => {
                keep(&mut terms, &mut current, quoted);
                quoted = !quoted;
            }
            c if c.is_whitespace() && !quoted => keep(&mut terms, &mut current, false),
            c => current.push(c),
        }
    }
    keep(&mut terms, &mut current, quoted);
    terms.truncate(MAX_TERMS);
    let last = terms.len().checked_sub(1)?;
    Some(
        terms
            .iter()
            .enumerate()
            .map(|(i, (text, phrase))| {
                let long_enough =
                    text.chars().any(dense) || text.chars().filter(|c| c.is_alphanumeric()).count() >= MIN_PREFIX_CHARS;
                let star = if i == last && !phrase && long_enough { "*" } else { "" };
                format!("\"{}\"{star}", text.replace('"', "\"\""))
            })
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// A snippet as the index writes it, its matches between `HIT_START` and
/// `HIT_END`, as pieces.
fn pieces(snippet: &str) -> Vec<SnippetPiece> {
    let mut out: Vec<SnippetPiece> = Vec::new();
    let mut text = String::new();
    let mut hit = false;
    let flush = |out: &mut Vec<SnippetPiece>, text: &mut String, hit: bool| {
        if !text.is_empty() {
            out.push(SnippetPiece { text: std::mem::take(text), hit });
        }
    };
    for c in snippet.chars() {
        match c {
            HIT_START => {
                flush(&mut out, &mut text, hit);
                hit = true;
            }
            HIT_END => {
                flush(&mut out, &mut text, hit);
                hit = false;
            }
            c => text.push(c),
        }
    }
    flush(&mut out, &mut text, hit);
    out
}

impl Db {
    /// Up to `limit` messages that say what `input` asks for, newest first
    /// (then by id), from after `before` — the date and id of the last one
    /// shown — so pages never share or skip a message. Each comes with its
    /// conversation, the words around the match, and how much of its
    /// conversation is either side of it. Nothing to look for finds nothing.
    pub fn search_messages(
        &self,
        input: &str,
        who: SearchWho,
        before: Option<(&str, &str)>,
        limit: usize,
    ) -> DbResult<SearchPage> {
        let Some(query) = search_query(input) else { return Ok(SearchPage::default()) };
        let whose = match who {
            SearchWho::Anyone => "1",
            SearchWho::You => "m.direction = 'self'",
            SearchWho::Others => "m.direction <> 'self'",
        };
        let conn = self.conn();
        // Which messages first, then the words around each: the index writes
        // a snippet only for the page, not for every message that matches.
        // The index's rows share their message's rowid, so the page is looked
        // up in the index by rowid; the join on the id is what shows a row is
        // its message's.
        let sql = format!(
            "WITH page AS (
               SELECT m.rowid AS r FROM message_search
               JOIN messages m ON m.id = message_search.message_id
               WHERE message_search MATCH ?1 AND {whose}
                 AND (?2 IS NULL OR (COALESCE(m.sent_at, ''), m.id) < (?2, ?3))
               ORDER BY COALESCE(m.sent_at, '') DESC, m.id DESC
               LIMIT ?4
             )
             SELECT {cols}, m.conversation_id, c.subject,
                    snippet(message_search, 0, char(57344), char(57345), char(8230), {SNIPPET_WORDS})
             FROM message_search
             JOIN messages m ON m.id = message_search.message_id
             JOIN conversations c ON c.id = m.conversation_id
             LEFT JOIN participants p ON p.id = m.participant_id
             WHERE message_search MATCH ?1 AND message_search.rowid IN (SELECT r FROM page)
             ORDER BY COALESCE(m.sent_at, '') DESC, m.id DESC",
            cols = thread_cols()
        );
        let (at, id) = before.unzip();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![query, at, id, limit as i64 + 1], |r| {
            let (message, position) = map_thread(r)?;
            let conversation_id: String = r.get(8)?;
            let subject: Option<String> = r.get(9)?;
            let snippet: String = r.get(10)?;
            Ok((message, position, conversation_id, subject, snippet))
        })?;
        let mut rows: Vec<_> = rows.collect::<Result<_, _>>()?;
        let more = rows.len() > limit;
        rows.truncate(limit);
        let mut found = Vec::with_capacity(rows.len());
        for (message, position, conversation_id, subject, snippet) in rows {
            let earlier = count_side(&conn, &conversation_id, position, &message.id, Toward::Earlier)?;
            let later = count_side(&conn, &conversation_id, position, &message.id, Toward::Later)?;
            found.push(FoundMessage { message, conversation_id, subject, snippet: pieces(&snippet), earlier, later });
        }
        let counted: i64 = conn.query_row(
            &format!(
                "SELECT COUNT(*) FROM (
                   SELECT 1 FROM message_search JOIN messages m ON m.id = message_search.message_id
                   WHERE message_search MATCH ?1 AND {whose} LIMIT ?2)"
            ),
            params![query, COUNT_CAP + 1],
            |r| r.get(0),
        )?;
        Ok(SearchPage { found, more, total: counted.min(COUNT_CAP), capped: counted > COUNT_CAP })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::repo_people::IdentifierInput;
    use crate::db::{IdentifierKind, NewMessage, NewSource};
    use serde_json::Value;

    #[test]
    fn what_is_typed_is_looked_for_word_by_word_and_never_read_as_the_query_language() {
        assert_eq!(search_query("drinks Friday").as_deref(), Some(r#""drinks" "Friday"*"#));
        // A short last word is only itself: "a" would be every word starting with it.
        assert_eq!(search_query("take a").as_deref(), Some(r#""take" "a""#));
        assert_eq!(search_query("go fr").as_deref(), Some(r#""go" "fr""#));
        assert_eq!(search_query("go fri").as_deref(), Some(r#""go" "fri"*"#));
        // A character that is a syllable or more is enough to start a word with.
        assert_eq!(search_query("中文").as_deref(), Some(r#""中文"*"#));
        assert_eq!(search_query("カ").as_deref(), Some(r#""カ"*"#));
        assert_eq!(search_query("학교").as_deref(), Some(r#""학교"*"#), "Korean joins particles to the word");
        assert_eq!(search_query(r#""中文" x"#).as_deref(), Some(r#""中文" "x""#), "only the last word, outside quotes");
        assert_eq!(search_query(r#""pub quiz" tonight"#).as_deref(), Some(r#""pub quiz" "tonight"*"#));
        assert_eq!(search_query(r#"tonight "pub quiz""#).as_deref(), Some(r#""tonight" "pub quiz""#));
        // The query language's own words and signs are only words here.
        assert_eq!(
            search_query("a OR b NOT c NEAR(d) -e body:f").as_deref(),
            Some(r#""a" "OR" "b" "NOT" "c" "NEAR(d)" "-e" "body:f"*"#)
        );
        // A quote left open is a phrase to the end.
        assert_eq!(search_query(r#"say "hi there"#).as_deref(), Some(r#""say" "hi there""#));
        for nothing in ["", "   ", "?!", "\"\"", "\" \"", "--- ..."] {
            assert_eq!(search_query(nothing), None, "{nothing:?}");
        }
        let many: Vec<String> = (0..30).map(|i| format!("w{i}")).collect();
        let q = search_query(&many.join(" ")).unwrap();
        assert_eq!(q.matches('"').count(), MAX_TERMS * 2, "only the first {MAX_TERMS} words are looked for");
    }

    #[test]
    fn a_snippet_is_read_into_the_words_around_a_match_and_the_match() {
        let s = format!("…the {HIT_START}zanzibar{HIT_END} plan and {HIT_START}Pemba{HIT_END}");
        let read = pieces(&s);
        let got: Vec<(&str, bool)> = read.iter().map(|p| (p.text.as_str(), p.hit)).collect();
        assert_eq!(got, [("…the ", false), ("zanzibar", true), (" plan and ", false), ("Pemba", true)]);
        assert!(pieces("").is_empty());
    }

    struct Mail {
        db: Db,
        source: String,
        lunch: String,
        trip: String,
        ada: String,
    }

    fn mail() -> Mail {
        let db = Db::open_in_memory().unwrap();
        let source = db
            .create_source(&NewSource {
                connector: "test".into(),
                name: "T".into(),
                channel: "email".into(),
                location: None,
                config: Value::Null,
            })
            .unwrap()
            .id;
        let lunch = db.upsert_conversation(&source, "lunch", "email", Some("Lunch")).unwrap();
        let trip = db.upsert_conversation(&source, "trip", "email", None).unwrap();
        let ada = db
            .resolve_participant("Ada", &[IdentifierInput::new(IdentifierKind::Email, "ada@example.com")], false)
            .unwrap();
        let m = |convo: &str, ext: &str, mine: bool, seq: i64, day: u32, body: &str| NewMessage {
            conversation_id: convo.into(),
            source_id: source.clone(),
            participant_id: if mine { None } else { Some(ada.clone()) },
            external_id: ext.into(),
            direction: if mine { "self" } else { "other" }.into(),
            channel: "email".into(),
            sent_at: Some(format!("2026-03-{day:02}T10:00:00Z")),
            sequence_index: seq,
            body: body.into(),
            reply_to_external_id: None,
            metadata: Value::Null,
        };
        db.insert_messages(&[
            m(&lunch, "l0", false, 0, 1, "Drinks on Friday? The café by the station."),
            m(&lunch, "l1", true, 1, 2, "friday works, the Café it is"),
            m(&lunch, "l2", false, 2, 3, "Great, see you there"),
            m(&trip, "t0", true, 0, 4, "The Zanzibar résumé is attached. Ready for Friday"),
            m(&trip, "t1", false, 1, 5, "Thanks! Reading it tonight"),
        ])
        .unwrap();
        Mail { db, source, lunch, trip, ada }
    }

    #[test]
    fn every_message_that_says_it_is_found_newest_first_a_page_at_a_time() {
        let Mail { db, lunch, trip, .. } = mail();
        let first = db.search_messages("friday", SearchWho::Anyone, None, 2).unwrap();
        let bodies: Vec<&str> = first.found.iter().map(|f| f.message.body.as_str()).collect();
        assert_eq!(bodies, ["The Zanzibar résumé is attached. Ready for Friday", "friday works, the Café it is"]);
        assert!(first.more);
        assert_eq!((first.total, first.capped), (3, false));

        // The newest: the user's own, in a conversation with no subject, the
        // match marked, and where it sits in its conversation.
        let newest = &first.found[0];
        assert_eq!(newest.conversation_id, trip);
        assert_eq!(newest.subject, None);
        assert_eq!(newest.message.direction, "self");
        assert_eq!(newest.message.author, None, "the user is not named as someone else");
        assert_eq!((newest.earlier, newest.later), (0, 1));
        let hits: Vec<&str> = newest.snippet.iter().filter(|p| p.hit).map(|p| p.text.as_str()).collect();
        assert_eq!(hits, ["Friday"]);
        let whole: String = newest.snippet.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(whole, newest.message.body, "a short message is its own snippet");

        // Read on from the last one shown.
        let last = &first.found[1];
        let next = db
            .search_messages(
                "friday",
                SearchWho::Anyone,
                Some((last.message.sent_at.as_deref().unwrap(), &last.message.id)),
                2,
            )
            .unwrap();
        assert_eq!(next.found.len(), 1);
        assert!(!next.more);
        let oldest = &next.found[0];
        assert_eq!(oldest.message.body, "Drinks on Friday? The café by the station.");
        assert_eq!(oldest.message.author.as_deref(), Some("Ada"));
        assert_eq!(oldest.subject.as_deref(), Some("Lunch"));
        assert_eq!(oldest.conversation_id, lunch);
        assert_eq!((oldest.earlier, oldest.later), (0, 2));
    }

    #[test]
    fn case_accents_and_the_start_of_a_word_are_enough_and_every_word_must_be_there() {
        let Mail { db, source, lunch, .. } = mail();
        db.insert_messages(&[NewMessage {
            conversation_id: lunch,
            source_id: source,
            participant_id: None,
            external_id: "zh".into(),
            direction: "self".into(),
            channel: "email".into(),
            sent_at: Some("2026-03-06T10:00:00Z".into()),
            sequence_index: 3,
            body: "中文句子测试".into(),
            reply_to_external_id: None,
            metadata: Value::Null,
        }])
        .unwrap();
        let bodies = |q: &str| -> Vec<String> {
            db.search_messages(q, SearchWho::Anyone, None, 10)
                .unwrap()
                .found
                .into_iter()
                .map(|f| f.message.body)
                .collect()
        };
        assert_eq!(bodies("CAFE").len(), 2, "case and accents are folded");
        assert_eq!(bodies("resume zanz"), ["The Zanzibar résumé is attached. Ready for Friday"]);
        assert!(bodies("zanzibar lunch").is_empty(), "every word must be there");
        assert_eq!(bodies(r#""see you there""#), ["Great, see you there"]);
        assert!(bodies(r#""you see there""#).is_empty(), "a phrase in its order");
        assert!(bodies("OR").is_empty(), "the query language's words are only words");
        // A run written without spaces is one word to the index: found by its start.
        assert_eq!(bodies("中文"), ["中文句子测试"]);
        assert!(bodies("句子").is_empty(), "not from inside the run");
        let nothing = db.search_messages("?!", SearchWho::Anyone, None, 10).unwrap();
        assert_eq!(nothing, SearchPage::default());
    }

    #[test]
    fn pages_run_from_the_newest_to_the_undated_without_sharing_or_skipping_one() {
        let Mail { db, source, trip, .. } = mail();
        // Two messages with no readable date: after every dated one, by id.
        for (ext, seq) in [("u0", 2), ("u1", 3)] {
            db.insert_messages(&[NewMessage {
                conversation_id: trip.clone(),
                source_id: source.clone(),
                participant_id: None,
                external_id: ext.into(),
                direction: "self".into(),
                channel: "email".into(),
                sent_at: None,
                sequence_index: seq,
                body: format!("friday, undated {ext}"),
                reply_to_external_id: None,
                metadata: Value::Null,
            }])
            .unwrap();
        }
        let mut seen: Vec<(Option<String>, String)> = Vec::new();
        let mut before: Option<(String, String)> = None;
        loop {
            let page = db
                .search_messages("friday", SearchWho::Anyone, before.as_ref().map(|(a, b)| (a.as_str(), b.as_str())), 1)
                .unwrap();
            assert_eq!(page.total, 5);
            let Some(f) = page.found.first() else { break };
            seen.push((f.message.sent_at.clone(), f.message.body.clone()));
            before = Some((f.message.sent_at.clone().unwrap_or_default(), f.message.id.clone()));
            if !page.more {
                break;
            }
        }
        assert_eq!(seen.len(), 5, "{seen:?}");
        let dated: Vec<&Option<String>> = seen.iter().map(|(at, _)| at).take(3).collect();
        assert!(dated.iter().all(|at| at.is_some()), "the dated ones first, newest first: {seen:?}");
        assert!(seen[3..].iter().all(|(at, _)| at.is_none()), "then the undated: {seen:?}");
        let mut bodies: Vec<&str> = seen.iter().map(|(_, b)| b.as_str()).collect();
        bodies.sort();
        bodies.dedup();
        assert_eq!(bodies.len(), 5, "no message twice");
    }

    #[test]
    fn whose_messages_are_looked_through_is_the_users_choice() {
        let Mail { db, .. } = mail();
        let directions = |who| -> Vec<String> {
            db.search_messages("friday", who, None, 10)
                .unwrap()
                .found
                .into_iter()
                .map(|f| f.message.direction)
                .collect()
        };
        assert_eq!(directions(SearchWho::You), ["self", "self"]);
        assert_eq!(directions(SearchWho::Others), ["other"]);
        assert_eq!(db.search_messages("friday", SearchWho::You, None, 10).unwrap().total, 2);
    }

    #[test]
    fn a_message_deleted_with_its_writer_or_its_source_is_never_found() {
        let Mail { db, ada, source, .. } = mail();
        db.conn().execute("DELETE FROM participants WHERE id = ?1", [&ada]).unwrap();
        let left = db.search_messages("friday", SearchWho::Anyone, None, 10).unwrap();
        assert_eq!(left.found.len(), 2);
        assert!(left.found.iter().all(|f| f.message.direction == "self"));
        assert_eq!(left.total, 2);
        db.conn().execute("DELETE FROM sources WHERE id = ?1", [&source]).unwrap();
        assert_eq!(db.search_messages("friday", SearchWho::Anyone, None, 10).unwrap(), SearchPage::default());
        let indexed: i64 = db.conn().query_row("SELECT COUNT(*) FROM message_search", [], |r| r.get(0)).unwrap();
        assert_eq!(indexed, 0, "nothing of them is left in the index");
    }
}
