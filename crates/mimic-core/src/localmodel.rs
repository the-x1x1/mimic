//! Getting a model onto the machine, without the user ever opening a terminal.
//!
//! This is the step that decides whether Mimic is usable by someone who does
//! not know what Ollama is. Until now the app assumed a model was already
//! listening on `127.0.0.1:11434` and said "not answering" when it was not,
//! which is true and useless. What it needs to do instead is work out which of
//! four situations it is in and offer the one next action that fits.
//!
//! **What this module will not do: download and run an installer by itself.**
//! Fetching an executable over the network and launching it is only safe if
//! the bytes are pinned to a hash that ships with the app, and a hash that
//! ships with the app goes stale the moment upstream publishes a new build.
//! Doing it without the hash would mean Mimic executing whatever arrived,
//! which is not a thing this app gets to do to someone's computer. So the
//! install step opens the official download page and Mimic waits, watching,
//! and takes over again the moment the model host appears. One click, no
//! terminal, and nothing executed that the user did not run themselves.
//!
//! Pulling the model afterwards *is* Mimic's job, and that is here in full:
//! several gigabytes with a real progress figure, cancellable, over the local
//! HTTP API of something already running on the machine.

use std::io::{BufRead, BufReader};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::db::{Db, DbError};
use crate::jobs::{JobContext, JobError, JobExecutor, JobFuture};

pub const JOB_KIND: &str = "pull_model";

/// Where the person is sent to get a model host. Opened in their browser; the
/// app neither downloads nor runs what is on the other end.
pub const DOWNLOAD_PAGE: &str = "https://ollama.com/download";

/// What Mimic knows about the local model right now. Every field is observed,
/// not assumed: `host_reachable` means something answered, and `models` is
/// what it said it had.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModelStatus {
    /// The OpenAI-compatible endpoint from settings, as configured.
    pub endpoint: String,
    /// Whether anything answered on it at all.
    pub host_reachable: bool,
    /// Model names the host reports. Empty when it is not reachable — never a
    /// guess at what might be installed.
    pub models: Vec<String>,
    /// The model Mimic is configured to write with.
    pub wanted: String,
    /// Whether `wanted` is among `models`.
    pub wanted_present: bool,
    /// Set when the host answered but the reply could not be read, so the UI
    /// can say what went wrong instead of implying nothing is installed.
    pub error: Option<String>,
}

/// The single next action, given what was observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NextStep {
    /// The model is there and answering. Nothing to do.
    Ready,
    /// Nothing is listening. Either it was never installed or it is not
    /// running, and from outside those look identical — so the copy on this
    /// step has to cover both, which is why there is one variant and not two.
    GetTheHost,
    /// The host is running but does not have the model Mimic wants.
    PullModel,
}

/// The whole decision, in one place, over observed facts only.
pub fn next_step(status: &LocalModelStatus) -> NextStep {
    if !status.host_reachable {
        return NextStep::GetTheHost;
    }
    if status.wanted_present {
        NextStep::Ready
    } else {
        NextStep::PullModel
    }
}

/// Ollama's own API sits at the root; the endpoint stored in settings is its
/// OpenAI-compatible surface, which lives under `/v1`. Everything here talks
/// to the root, so the suffix comes off once, here, rather than at each call
/// site where it would eventually be forgotten.
pub fn api_root(endpoint: &str) -> String {
    let trimmed = endpoint.trim_end_matches('/');
    trimmed.strip_suffix("/v1").unwrap_or(trimmed).to_string()
}

#[derive(Debug, Deserialize)]
struct TagsReply {
    #[serde(default)]
    models: Vec<TagEntry>,
}

#[derive(Debug, Deserialize)]
struct TagEntry {
    #[serde(default)]
    name: String,
}

/// Ask the host what it has. A host that is not running is not an error here:
/// it is the answer, and `host_reachable` carries it.
pub fn observe(endpoint: &str, wanted: &str) -> LocalModelStatus {
    let mut status = LocalModelStatus {
        endpoint: endpoint.to_string(),
        host_reachable: false,
        models: Vec::new(),
        wanted: wanted.to_string(),
        wanted_present: false,
        error: None,
    };
    let client = match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        // The model host is on this machine. A proxy configured in the
        // environment has no business in that path, and on a managed machine
        // it will happily swallow the request and report the host as absent.
        .no_proxy()
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            status.error = Some(one_line(&e.to_string()));
            return status;
        }
    };
    let resp = match client.get(format!("{}/api/tags", api_root(endpoint))).send() {
        Ok(r) => r,
        Err(_) => return status,
    };
    status.host_reachable = true;
    match resp.json::<TagsReply>() {
        Ok(tags) => {
            status.models = tags.models.into_iter().map(|m| m.name).filter(|n| !n.is_empty()).collect();
            status.wanted_present = status.models.iter().any(|m| same_model(m, wanted));
        }
        Err(e) => status.error = Some(one_line(&e.to_string())),
    }
    status
}

/// `llama3.2:3b` and `llama3.2:3b` are the same model; so are `llama3.2` and
/// `llama3.2:latest`, which is what a host reports for an untagged pull.
fn same_model(have: &str, want: &str) -> bool {
    let norm = |s: &str| {
        let s = s.trim();
        match s.split_once(':') {
            Some((name, "latest")) => name.to_string(),
            _ => s.to_string(),
        }
    };
    norm(have) == norm(want)
}

/// How far a pull has got. `total` is zero until the host says how big the
/// download is, which it does not do for the first few lines.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
pub struct PullProgress {
    pub completed: u64,
    pub total: u64,
}

#[derive(Debug, Deserialize)]
struct PullLine {
    #[serde(default)]
    status: String,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    total: Option<u64>,
    #[serde(default)]
    completed: Option<u64>,
}

#[derive(Debug, thiserror::Error)]
pub enum PullError {
    #[error("{0}")]
    Unreachable(String),
    #[error("{0}")]
    Refused(String),
    #[error("cancelled")]
    Cancelled,
}

/// Pull a model, reporting progress as the host streams it.
///
/// The reply is newline-delimited JSON, one object per update, and it runs for
/// minutes. It is read line by line rather than buffered to the end, both so
/// the progress bar moves and so cancelling takes effect within one line
/// instead of after several gigabytes.
pub fn pull(
    endpoint: &str,
    model: &str,
    on_progress: &mut dyn FnMut(PullProgress, &str),
    should_stop: &dyn Fn() -> bool,
) -> Result<(), PullError> {
    let client = reqwest::blocking::Client::builder()
        // No overall timeout: this is a multi-gigabyte download and any
        // deadline long enough for a slow line is no deadline at all. A host
        // that goes away mid-stream surfaces as a read error on the next line
        // instead, which is what the loop below is already shaped for.
        .timeout(None)
        .connect_timeout(std::time::Duration::from_secs(10))
        .no_proxy()
        .build()
        .map_err(|e| PullError::Unreachable(one_line(&e.to_string())))?;
    let resp = client
        .post(format!("{}/api/pull", api_root(endpoint)))
        .json(&serde_json::json!({ "model": model, "stream": true }))
        .send()
        .map_err(|e| PullError::Unreachable(one_line(&e.to_string())))?;
    if !resp.status().is_success() {
        return Err(PullError::Refused(format!("{} from the model host", resp.status().as_u16())));
    }

    let mut reader = BufReader::new(resp);
    let mut line = String::new();
    loop {
        if should_stop() {
            return Err(PullError::Cancelled);
        }
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => return Err(PullError::Unreachable(one_line(&e.to_string()))),
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parsed: PullLine = match serde_json::from_str(trimmed) {
            Ok(p) => p,
            // A line that does not parse is not a reason to abandon a download
            // that is otherwise working.
            Err(_) => continue,
        };
        if let Some(err) = parsed.error {
            return Err(PullError::Refused(one_line(&err)));
        }
        on_progress(
            PullProgress { completed: parsed.completed.unwrap_or(0), total: parsed.total.unwrap_or(0) },
            &parsed.status,
        );
    }
    Ok(())
}

/// Reads the endpoint and the model name at the moment a pull starts. Not a
/// captured pair: the user can change either in Settings between asking for a
/// download and the download beginning.
pub type ReadEndpoint = Arc<dyn Fn(&Db) -> (String, String) + Send + Sync>;

/// Runs a pull as a background job, so it survives navigating away and shows
/// up in the same place as every other piece of work.
pub struct PullExecutor {
    settings: ReadEndpoint,
}

impl PullExecutor {
    pub fn shared(settings: ReadEndpoint) -> Arc<dyn JobExecutor> {
        Arc::new(PullExecutor { settings })
    }
}

impl JobExecutor for PullExecutor {
    fn kinds(&self) -> &'static [&'static str] {
        &[JOB_KIND]
    }

    fn resumable(&self, _kind: &str) -> bool {
        // Re-running is cheap — the host keeps what it already fetched — but
        // starting a multi-gigabyte download on its own after a crash, with
        // nobody at the machine, is not something to do unasked.
        false
    }

    fn execute(&self, ctx: JobContext) -> JobFuture {
        let read = self.settings.clone();
        Box::pin(async move {
            let db = ctx.db.clone();
            let progress_ctx = ctx.clone();
            let cancel_ctx = ctx.clone();
            tokio::task::block_in_place(move || {
                let (endpoint, model) = read(&db);
                let mut on_progress = |p: PullProgress, phase: &str| {
                    progress_ctx.progress(p.completed as i64, p.total as i64, phase);
                };
                let should_stop = || cancel_ctx.check_cancel().is_err();
                pull(&endpoint, &model, &mut on_progress, &should_stop)
            })
            .map_err(|e| match e {
                PullError::Cancelled => JobError::Canceled,
                other => JobError::Failed(other.to_string()),
            })?;
            Ok(serde_json::json!({ "pulled": true }))
        })
    }
}

/// Provider errors must never carry a body; the same rule applies to anything
/// from the model host, which sees message text.
fn one_line(s: &str) -> String {
    s.lines().next().unwrap_or("").chars().take(160).collect()
}

/// The endpoint and model as configured, with the defaults the app ships.
pub fn configured(db: &Db) -> Result<(String, String), DbError> {
    let endpoint =
        db.get_setting::<String>("generation.localUrl")?.unwrap_or_else(|| "http://127.0.0.1:11434/v1".to_string());
    let model = db.get_setting::<String>("generation.localModel")?.unwrap_or_else(|| "llama3.2:3b".to_string());
    Ok((endpoint, model))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn status(reachable: bool, models: &[&str], wanted: &str) -> LocalModelStatus {
        LocalModelStatus {
            endpoint: "http://127.0.0.1:11434/v1".into(),
            host_reachable: reachable,
            models: models.iter().map(|s| s.to_string()).collect(),
            wanted: wanted.into(),
            wanted_present: models.iter().any(|m| same_model(m, wanted)),
            error: None,
        }
    }

    #[test]
    fn the_next_step_follows_from_what_was_observed() {
        assert_eq!(next_step(&status(false, &[], "llama3.2:3b")), NextStep::GetTheHost);
        assert_eq!(next_step(&status(true, &["mistral:7b"], "llama3.2:3b")), NextStep::PullModel);
        assert_eq!(next_step(&status(true, &["llama3.2:3b"], "llama3.2:3b")), NextStep::Ready);
    }

    #[test]
    fn an_untagged_model_is_the_same_model_as_its_latest_tag() {
        // What a host reports after an untagged pull, which would otherwise
        // send someone round the pull step a second time for a model they
        // already have.
        assert!(same_model("llama3.2:latest", "llama3.2"));
        assert!(same_model("llama3.2", "llama3.2:latest"));
        assert!(!same_model("llama3.2:3b", "llama3.2:1b"));
        assert_eq!(next_step(&status(true, &["llama3.2:latest"], "llama3.2")), NextStep::Ready);
    }

    #[test]
    fn the_openai_suffix_comes_off_exactly_once() {
        assert_eq!(api_root("http://127.0.0.1:11434/v1"), "http://127.0.0.1:11434");
        assert_eq!(api_root("http://127.0.0.1:11434/v1/"), "http://127.0.0.1:11434");
        assert_eq!(api_root("http://127.0.0.1:11434"), "http://127.0.0.1:11434");
        // A host whose own path happens to contain v1 keeps it.
        assert_eq!(api_root("http://box/v1/api"), "http://box/v1/api");
    }

    /// Serves one canned HTTP response and hands back its address.
    ///
    /// It reads the whole request first, headers and body. Replying and
    /// closing while the client is still writing its POST body resets the
    /// connection, which the client reports as "error sending request" — a
    /// race that shows up only when the machine is busy, which is to say only
    /// when the suite runs in parallel.
    fn serve_once(body: &'static str) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else { return };
            let mut reader = BufReader::new(stream);
            let mut length = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    return;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap_or(0);
                }
                if line.trim().is_empty() {
                    break;
                }
            }
            if length > 0 {
                let mut sink = vec![0u8; length];
                let _ = std::io::Read::read_exact(&mut reader, &mut sink);
            }
            let mut stream = reader.into_inner();
            let _ = stream.write_all(
                format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body)
                    .as_bytes(),
            );
            let _ = stream.flush();
        });
        (addr, handle)
    }

    #[test]
    fn a_host_that_is_not_running_is_an_answer_rather_than_an_error() {
        // Nothing is listening on this port: the result says so plainly and
        // claims nothing about what might be installed.
        let s = observe("http://127.0.0.1:1/v1", "llama3.2:3b");
        assert!(!s.host_reachable);
        assert!(s.models.is_empty(), "an unreachable host must not produce a guess at its models");
        assert!(!s.wanted_present);
        assert_eq!(next_step(&s), NextStep::GetTheHost);
    }

    #[test]
    fn what_the_host_reports_is_what_is_believed() {
        let (addr, handle) = serve_once(r#"{"models":[{"name":"llama3.2:3b"},{"name":"mistral:7b"}]}"#);
        let s = observe(&addr, "llama3.2:3b");
        handle.join().unwrap();
        assert!(s.host_reachable);
        assert_eq!(s.models, vec!["llama3.2:3b", "mistral:7b"]);
        assert!(s.wanted_present);
        assert_eq!(next_step(&s), NextStep::Ready);
    }

    #[test]
    fn progress_follows_the_stream_and_the_last_line_wins() {
        let (addr, handle) = serve_once(concat!(
            "{\"status\":\"pulling manifest\"}\n",
            "{\"status\":\"pulling 8eeb52dfb3bb\",\"total\":2019377376,\"completed\":0}\n",
            "{\"status\":\"pulling 8eeb52dfb3bb\",\"total\":2019377376,\"completed\":1009688688}\n",
            "{\"status\":\"success\"}\n",
        ));
        let mut seen: Vec<(u64, u64, String)> = Vec::new();
        pull(&addr, "llama3.2:3b", &mut |p, phase| seen.push((p.completed, p.total, phase.to_string())), &|| false)
            .unwrap();
        handle.join().unwrap();
        assert_eq!(seen.len(), 4, "every line should be reported, including the ones with no numbers on them");
        assert_eq!(seen[0], (0, 0, "pulling manifest".to_string()));
        assert_eq!(seen[2], (1009688688, 2019377376, "pulling 8eeb52dfb3bb".to_string()));
        assert_eq!(seen[3].2, "success");
    }

    #[test]
    fn an_error_in_the_stream_stops_the_pull_and_carries_one_line() {
        let (addr, handle) = serve_once("{\"status\":\"pulling\"}\n{\"error\":\"model 'nope' not found\\nstack\"}\n");
        let err = pull(&addr, "nope", &mut |_, _| {}, &|| false).unwrap_err();
        handle.join().unwrap();
        match err {
            PullError::Refused(msg) => {
                assert!(msg.contains("not found"));
                assert!(!msg.contains("stack"), "nothing multi-line reaches the user");
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn cancelling_stops_before_the_next_line_rather_than_after_the_download() {
        let (addr, handle) = serve_once(concat!(
            "{\"status\":\"pulling\",\"total\":100,\"completed\":10}\n",
            "{\"status\":\"pulling\",\"total\":100,\"completed\":20}\n",
        ));
        let stop = AtomicBool::new(false);
        let mut lines = 0;
        let err = pull(
            &addr,
            "llama3.2:3b",
            &mut |_, _| {
                lines += 1;
                stop.store(true, Ordering::SeqCst);
            },
            &|| stop.load(Ordering::SeqCst),
        )
        .unwrap_err();
        handle.join().unwrap();
        assert!(matches!(err, PullError::Cancelled));
        assert_eq!(lines, 1, "cancelling should take effect after the line that set it, not at the end");
    }

    #[test]
    fn a_line_that_does_not_parse_does_not_abandon_a_working_download() {
        let (addr, handle) = serve_once(concat!(
            "{\"status\":\"pulling\",\"total\":100,\"completed\":10}\n",
            "not json at all\n",
            "{\"status\":\"success\"}\n",
        ));
        let mut seen = 0;
        pull(&addr, "llama3.2:3b", &mut |_, _| seen += 1, &|| false).unwrap();
        handle.join().unwrap();
        assert_eq!(seen, 2);
    }
}
