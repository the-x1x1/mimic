//! The sentence encoder: which one the app offers, downloading it when the
//! user asks, and the vectors it makes of their messages.
//!
//! The encoder is named by a manifest (`models/manifests/*.json`, bundled
//! with the app) that pins every file it needs by URL, size and SHA-256. The
//! app downloads those files into its encoders folder only when the user asks,
//! checks each digest as it downloads, and deletes anything that does not
//! match. The engine runs the encoder only while every file matches, and says
//! so when it does not (`engine/src/mimic_engine/embeddings/encoder.py`).
//!
//! What the encoder is used for is finding past replies that answered
//! something close in meaning to what is being answered now: the messages the
//! user replied to (and the user's own, where a reply answered nothing
//! stored) are turned into vectors once, in the background, and kept in
//! `message_embeddings` under the encoder's id; the message being answered is
//! turned into one when a draft is written (`retrieval`). Nothing about any of
//! it leaves the computer, except the download itself.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::db::{Db, DbError};
use crate::engine::EngineClient;
use crate::jobs::{JobContext, JobError, JobExecutor, JobFuture};
use crate::retrieval::QueryVector;

pub const DOWNLOAD_JOB_KIND: &str = "download_encoder";
pub const EMBED_JOB_KIND: &str = "embed_messages";

/// Messages turned into vectors per request to the engine.
const EMBED_PAGE: usize = 128;

/// Characters of a message sent to be turned into a vector. The encoder
/// reads 256 tokens at most — about a thousand characters of English — so
/// sending more is only more to send.
const EMBED_CHARS: usize = 2000;

type BoxFuture<'a, T> = Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub dims: usize,
    pub max_tokens: usize,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub homepage: String,
    pub files: Vec<ManifestFile>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestFile {
    pub name: String,
    pub url: String,
    pub sha256: String,
    pub bytes: u64,
}

impl Manifest {
    /// Everything the download will fetch, in bytes.
    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|f| f.bytes).sum()
    }

    fn valid(&self) -> bool {
        plain_name(&self.id)
            && !self.id.contains('.')
            && self.dims > 0
            && !self.files.is_empty()
            && self.files.iter().all(|f| {
                f.sha256.len() == 64
                    && f.sha256.chars().all(|c| c.is_ascii_hexdigit())
                    && f.url.starts_with("https://")
                    && plain_name(&f.name)
            })
    }
}

/// A name that stays in the folder it is joined to, on any platform: one
/// ordinary path component, with no separator and no drive (`C:x` leaves its
/// folder on Windows).
fn plain_name(name: &str) -> bool {
    let mut parts = Path::new(name).components();
    matches!((parts.next(), parts.next()), (Some(std::path::Component::Normal(_)), None))
        && !name.contains(['/', '\\', ':'])
}

#[derive(Debug, thiserror::Error)]
pub enum EncoderError {
    #[error("could not download {0}: {1}")]
    Download(String, String),
    #[error("{0} did not match the SHA-256 its manifest pins, so it was deleted")]
    Mismatch(String),
    #[error("could not write {0}: {1}")]
    Io(String, String),
    #[error("stopped")]
    Canceled,
}

/// Every well-formed manifest in `dir`, by file name. One that does not pin
/// every file by HTTPS URL, size and SHA-256 is left out.
pub fn read_manifests(dir: &Path) -> Vec<Manifest> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|p| std::fs::read_to_string(&p).ok())
        .filter_map(|s| serde_json::from_str::<Manifest>(&s).ok())
        .filter(Manifest::valid)
        .collect()
}

/// Where `manifest`'s files are kept.
pub fn folder(encoders_dir: &Path, manifest: &Manifest) -> PathBuf {
    encoders_dir.join(&manifest.id)
}

/// Whether every one of `manifest`'s files is in `encoders_dir`, at the size
/// the manifest pins. Sizes only, so it is quick enough to ask whenever mail
/// comes in; the digests were checked as the files were downloaded, and the
/// engine checks them again before it runs anything.
pub fn downloaded(encoders_dir: &Path, manifest: &Manifest) -> bool {
    let dir = folder(encoders_dir, manifest);
    manifest.files.iter().all(|f| std::fs::metadata(dir.join(&f.name)).is_ok_and(|m| m.len() == f.bytes))
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Download `manifest`'s files into `encoders_dir`, each checked against the
/// SHA-256 its manifest pins as it arrives. A file already there and
/// matching is kept; anything that does not match is deleted, and nothing is
/// ever kept under its real name until it has matched. `on_progress` is told
/// the bytes done of the bytes wanted.
pub fn download(
    manifest: &Manifest,
    encoders_dir: &Path,
    on_progress: &mut dyn FnMut(u64, u64),
    should_stop: &dyn Fn() -> bool,
) -> Result<(), EncoderError> {
    download_waiting(manifest, encoders_dir, STALL, on_progress, should_stop)
}

/// How long a download waits for the server's next bytes before giving up.
/// Each read waits this long, not the whole download: a slow connection
/// finishes, a dead one fails — and with it the job, which would otherwise
/// hold every other job behind it and not stop when asked.
const STALL: std::time::Duration = std::time::Duration::from_secs(60);

fn download_waiting(
    manifest: &Manifest,
    encoders_dir: &Path,
    stall: std::time::Duration,
    on_progress: &mut dyn FnMut(u64, u64),
    should_stop: &dyn Fn() -> bool,
) -> Result<(), EncoderError> {
    let dir = folder(encoders_dir, manifest);
    std::fs::create_dir_all(&dir).map_err(|e| EncoderError::Io(dir.display().to_string(), e.to_string()))?;
    let client = reqwest::blocking::Client::builder()
        .timeout(stall)
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| EncoderError::Download(manifest.name.clone(), one_line(&e.to_string())))?;
    let total = manifest.bytes();
    let mut done = 0u64;
    for f in &manifest.files {
        let target = dir.join(&f.name);
        let have = std::fs::metadata(&target).is_ok_and(|m| m.len() == f.bytes)
            && sha256_file(&target).is_ok_and(|h| h.eq_ignore_ascii_case(&f.sha256));
        if have {
            done += f.bytes;
            on_progress(done, total);
            continue;
        }
        let partial = dir.join(format!("{}.part", f.name));
        let result = fetch(&client, f, &partial, &mut |n| on_progress(done + n, total), should_stop);
        match result {
            Ok(()) => {
                std::fs::rename(&partial, &target)
                    .map_err(|e| EncoderError::Io(target.display().to_string(), e.to_string()))?;
                done += f.bytes;
            }
            Err(e) => {
                let _ = std::fs::remove_file(&partial);
                return Err(e);
            }
        }
    }
    Ok(())
}

fn fetch(
    client: &reqwest::blocking::Client,
    f: &ManifestFile,
    partial: &Path,
    on_progress: &mut dyn FnMut(u64),
    should_stop: &dyn Fn() -> bool,
) -> Result<(), EncoderError> {
    let failed = |e: String| EncoderError::Download(f.name.clone(), one_line(&e));
    let mut resp = client.get(&f.url).send().map_err(|e| failed(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(failed(format!("{} from the server", resp.status().as_u16())));
    }
    let mut out = std::fs::File::create(partial).map_err(|e| EncoderError::Io(f.name.clone(), e.to_string()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut got = 0u64;
    loop {
        if should_stop() {
            return Err(EncoderError::Canceled);
        }
        let n = resp.read(&mut buf).map_err(|e| failed(e.to_string()))?;
        if n == 0 {
            break;
        }
        got += n as u64;
        // More than the manifest pins is not the file it pins.
        if got > f.bytes {
            return Err(EncoderError::Mismatch(f.name.clone()));
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n]).map_err(|e| EncoderError::Io(f.name.clone(), e.to_string()))?;
        on_progress(got);
    }
    out.flush().map_err(|e| EncoderError::Io(f.name.clone(), e.to_string()))?;
    if got != f.bytes || !hex::encode(hasher.finalize()).eq_ignore_ascii_case(&f.sha256) {
        return Err(EncoderError::Mismatch(f.name.clone()));
    }
    Ok(())
}

/// Errors from a download never carry more than one short line.
fn one_line(s: &str) -> String {
    s.lines().next().unwrap_or("").chars().take(160).collect()
}

/// Where the encoder's manifests and files are, read when a job runs.
#[derive(Debug, Clone)]
pub struct Places {
    pub manifests: Option<PathBuf>,
    pub encoders: PathBuf,
}

impl Places {
    /// The encoder the app offers: the first manifest.
    pub fn offered(&self) -> Option<Manifest> {
        self.manifests.as_deref().map(read_manifests).and_then(|m| m.into_iter().next())
    }

    /// The encoder the app offers, when its files are all downloaded.
    pub fn ready(&self) -> Option<Manifest> {
        self.offered().filter(|m| downloaded(&self.encoders, m))
    }
}

/// Downloads the offered encoder as a background job.
pub struct DownloadExecutor {
    places: Places,
}

impl DownloadExecutor {
    pub fn shared(places: Places) -> Arc<dyn JobExecutor> {
        Arc::new(DownloadExecutor { places })
    }
}

impl JobExecutor for DownloadExecutor {
    fn kinds(&self) -> &'static [&'static str] {
        &[DOWNLOAD_JOB_KIND]
    }

    fn resumable(&self, _kind: &str) -> bool {
        // A download is the user's to start; after a crash it waits for them.
        false
    }

    fn execute(&self, ctx: JobContext) -> JobFuture {
        let places = self.places.clone();
        Box::pin(async move {
            let manifest = places
                .offered()
                .ok_or_else(|| JobError::Failed("There is no encoder to download in this build.".into()))?;
            let progress_ctx = ctx.clone();
            let cancel_ctx = ctx.clone();
            let name = manifest.name.clone();
            let m = manifest.clone();
            tokio::task::block_in_place(move || {
                let mut on_progress = |done: u64, total: u64| {
                    progress_ctx.progress(done as i64, total as i64, &format!("downloading {name}"))
                };
                let should_stop = || cancel_ctx.check_cancel().is_err();
                download(&m, &places.encoders, &mut on_progress, &should_stop)
            })
            .map_err(|e| match e {
                EncoderError::Canceled => JobError::Canceled,
                other => JobError::Failed(other.to_string()),
            })?;
            Ok(json!({ "encoder": manifest.id, "bytes": manifest.bytes() }))
        })
    }
}

// ------------------------------------------------------------- the vectors

/// Vectors made by the encoder in use, and which encoder that is.
#[derive(Debug, Clone, PartialEq)]
pub struct Embedded {
    pub version: String,
    pub dims: usize,
    pub vectors: Vec<Vec<f32>>,
}

/// Something that turns text into vectors: the engine, or a stand-in.
pub trait Embedder: Send + Sync {
    /// The vectors for `texts`, in order, when a sentence encoder is in use;
    /// `None` when there is none — the lexical fallback's vectors are not
    /// kept or compared.
    fn embed(&self, texts: Vec<String>) -> BoxFuture<'_, Result<Option<Embedded>, JobError>>;
}

/// The engine as an [`Embedder`].
pub struct EngineEmbedder(pub EngineClient);

impl Embedder for EngineEmbedder {
    fn embed(&self, texts: Vec<String>) -> BoxFuture<'_, Result<Option<Embedded>, JobError>> {
        Box::pin(async move {
            let v = self.0.call("text.embed", json!({ "texts": texts })).await?;
            Ok(parse_embedded(&v, texts.len()))
        })
    }
}

fn parse_embedded(v: &Value, n: usize) -> Option<Embedded> {
    if v.get("semantic").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let version = v.get("provider")?.as_str()?.to_string();
    let dims = v.get("dims")?.as_u64()? as usize;
    let vectors: Vec<Vec<f32>> = v
        .get("vectors")?
        .as_array()?
        .iter()
        .map(|row| row.as_array().map(|xs| xs.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect()))
        .collect::<Option<_>>()?;
    (vectors.len() == n && vectors.iter().all(|v: &Vec<f32>| v.len() == dims)).then_some(Embedded {
        version,
        dims,
        vectors,
    })
}

fn clip(body: &str) -> String {
    match body.char_indices().nth(EMBED_CHARS) {
        Some((cut, _)) => body[..cut].to_string(),
        None => body.to_string(),
    }
}

/// The message being answered, as a vector to find replies to messages close
/// in meaning — when the engine has a sentence encoder. `None` otherwise, or
/// when there is nothing to answer; retrieval then ranks by wording alone.
pub async fn query_vector(embedder: &dyn Embedder, text: &str) -> Option<QueryVector> {
    if text.trim().is_empty() {
        return None;
    }
    match embedder.embed(vec![clip(text)]).await {
        Ok(Some(mut e)) => e.vectors.pop().map(|vector| QueryVector { version: e.version, vector }),
        Ok(None) => None,
        Err(e) => {
            tracing::warn!(target: "encoder", error = %e, "could not turn the message into a vector");
            None
        }
    }
}

/// What an embedding run did.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbedSummary {
    /// The encoder whose vectors were made; none when no sentence encoder
    /// is in use, and nothing was done.
    pub encoder: Option<String>,
    pub embedded: usize,
    /// Vectors of any other encoder, removed.
    pub removed: usize,
}

/// Turn every message retrieval compares by meaning — each one the user
/// replied to, and each of the user's own — into a vector, where it has none
/// from the encoder in use, a page at a time. Vectors from another encoder are
/// removed: they cannot be compared with this one's. What is done is kept as
/// it goes, so a stop keeps it and the next run starts where this one ended.
pub async fn embed_messages(
    db: &Db,
    embedder: &dyn Embedder,
    on_progress: &(dyn Fn(usize, usize) + Sync),
    should_stop: &(dyn Fn() -> bool + Sync),
) -> Result<EmbedSummary, JobError> {
    let mut summary = EmbedSummary::default();
    // Which encoder is in use: ask with the first page.
    let mut cursor = String::new();
    let mut version: Option<String> = None;
    let mut total: Option<usize> = None;
    loop {
        if should_stop() {
            return Err(JobError::Canceled);
        }
        let page = db.page_unembedded(version.as_deref(), &cursor, EMBED_PAGE)?;
        let Some((last, _)) = page.last() else { break };
        let texts: Vec<String> = page.iter().map(|(_, body)| clip(body)).collect();
        let Some(made) = embedder.embed(texts).await? else { return Ok(summary) };
        // The first page says which encoder is in use (and, should the engine
        // change encoders part way, a later one does). Any other encoder's
        // vectors go — they cannot be compared with this one's — and paging
        // starts again from the first message this one has not read.
        let learned = version.as_deref() != Some(made.version.as_str());
        if learned {
            summary.removed += db.forget_embeddings_except(&made.version)?;
            summary.encoder = Some(made.version.clone());
            version = Some(made.version.clone());
            total = Some(summary.embedded + db.count_unembedded(&made.version)?);
        }
        let rows: Vec<(String, Vec<f32>)> = page.iter().map(|(id, _)| id.clone()).zip(made.vectors).collect();
        summary.embedded += db.put_embeddings(&made.version, made.dims, &rows)?;
        if learned {
            cursor.clear();
        } else {
            cursor = last.clone();
        }
        on_progress(summary.embedded, total.unwrap_or(0));
    }
    Ok(summary)
}

/// Runs `embed_messages` as a background job, with the engine.
pub struct EmbedExecutor {
    engine: EngineClient,
}

impl EmbedExecutor {
    pub fn shared(engine: EngineClient) -> Arc<dyn JobExecutor> {
        Arc::new(EmbedExecutor { engine })
    }
}

impl JobExecutor for EmbedExecutor {
    fn kinds(&self) -> &'static [&'static str] {
        &[EMBED_JOB_KIND]
    }

    fn resumable(&self, _kind: &str) -> bool {
        // Local, and what was done is kept: a run cut short goes on.
        true
    }

    fn execute(&self, ctx: JobContext) -> JobFuture {
        let engine = self.engine.clone();
        Box::pin(async move {
            if !engine.is_ready() {
                return Err(JobError::Failed(
                    "The text engine isn't running, so nothing could be read for meaning.".into(),
                ));
            }
            let embedder = EngineEmbedder(engine);
            let progress = |done: usize, total: usize| {
                ctx.progress(done as i64, total as i64, "reading your messages for meaning")
            };
            let should_stop = || ctx.check_cancel().is_err();
            let summary = embed_messages(&ctx.db, &embedder, &progress, &should_stop).await?;
            Ok(serde_json::to_value(summary).unwrap_or(Value::Null))
        })
    }
}

/// How many messages retrieval compares by meaning, and how many of them
/// have a vector from `version`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Coverage {
    pub wanted: i64,
    pub done: i64,
}

impl Db {
    /// Messages retrieval compares by meaning — each the user replied to,
    /// and each of the user's own — as `(id, body)`, by id after `after`,
    /// that have no vector from `version` (or, with no version, any).
    pub fn page_unembedded(
        &self,
        version: Option<&str>,
        after: &str,
        limit: usize,
    ) -> Result<Vec<(String, String)>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "WITH wanted(id) AS (
               SELECT id FROM messages WHERE direction = 'self'
               UNION
               SELECT reply_to_message_id FROM messages
               WHERE direction = 'self' AND reply_to_message_id IS NOT NULL
             )
             SELECT m.id, m.body FROM wanted w JOIN messages m ON m.id = w.id
             WHERE m.id > ?1 AND TRIM(m.body) <> ''
               AND NOT EXISTS (SELECT 1 FROM message_embeddings e
                               WHERE e.message_id = m.id AND (?2 IS NULL OR e.embedding_version = ?2))
             ORDER BY m.id LIMIT {limit}"
        ))?;
        let rows = stmt.query_map(rusqlite::params![after, version], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// How many of those have no vector from `version`.
    pub fn count_unembedded(&self, version: &str) -> Result<usize, DbError> {
        let c = self.embedding_coverage(version)?;
        Ok((c.wanted - c.done).max(0) as usize)
    }

    /// How many messages retrieval compares by meaning, and how many have a
    /// vector from `version`.
    pub fn embedding_coverage(&self, version: &str) -> Result<Coverage, DbError> {
        Ok(self.conn().query_row(
            "WITH wanted(id) AS (
               SELECT id FROM messages WHERE direction = 'self'
               UNION
               SELECT reply_to_message_id FROM messages
               WHERE direction = 'self' AND reply_to_message_id IS NOT NULL
             )
             SELECT COUNT(*),
                    COUNT(*) FILTER (WHERE EXISTS (SELECT 1 FROM message_embeddings e
                                                   WHERE e.message_id = m.id AND e.embedding_version = ?1))
             FROM wanted w JOIN messages m ON m.id = w.id
             WHERE TRIM(m.body) <> ''",
            [version],
            |r| Ok(Coverage { wanted: r.get(0)?, done: r.get(1)? }),
        )?)
    }

    /// Keep these vectors, made by `version`, `dims` numbers each. A message
    /// deleted since it was read is skipped, not an error.
    pub fn put_embeddings(&self, version: &str, dims: usize, rows: &[(String, Vec<f32>)]) -> Result<usize, DbError> {
        self.transaction(|tx| {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO message_embeddings(message_id, embedding_version, dims, vector, computed_at)
                 SELECT ?1, ?2, ?3, ?4, ?5 WHERE EXISTS (SELECT 1 FROM messages WHERE id = ?1)
                 ON CONFLICT(message_id, embedding_version) DO UPDATE SET
                   dims = excluded.dims, vector = excluded.vector, computed_at = excluded.computed_at",
            )?;
            let now = crate::ids::now_rfc3339();
            let mut n = 0;
            for (id, vector) in rows {
                if vector.len() != dims {
                    continue;
                }
                n += stmt.execute(rusqlite::params![id, version, dims as i64, to_blob(vector), now])?;
            }
            Ok(n)
        })
    }

    /// Remove every vector not made by `version`.
    pub fn forget_embeddings_except(&self, version: &str) -> Result<usize, DbError> {
        Ok(self.conn().execute("DELETE FROM message_embeddings WHERE embedding_version <> ?1", [version])?)
    }

    /// The vectors `version` made of these messages, where there are any.
    pub fn vectors_for(
        &self,
        ids: &[String],
        version: &str,
    ) -> Result<std::collections::HashMap<String, Vec<f32>>, DbError> {
        if ids.is_empty() {
            return Ok(Default::default());
        }
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT message_id, vector FROM message_embeddings
             WHERE embedding_version = ?1 AND message_id IN (SELECT value FROM json_each(?2))",
        )?;
        let json = serde_json::to_string(ids).unwrap_or_else(|_| "[]".into());
        let rows = stmt.query_map(rusqlite::params![version, json], |r| {
            Ok((r.get::<_, String>(0)?, from_blob(&r.get::<_, Vec<u8>>(1)?)))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

/// A vector as stored: its numbers as little-endian 32-bit floats.
pub fn to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

pub fn from_blob(b: &[u8]) -> Vec<f32> {
    let (whole, _) = b.as_chunks::<4>();
    whole.iter().map(|c| f32::from_le_bytes(*c)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{IdentifierKind, NewMessage, NewSource};
    use std::net::TcpListener;
    use std::sync::Mutex;

    fn manifest_for(files: &[(&str, &[u8])], base: &str) -> Manifest {
        Manifest {
            id: "tiny".into(),
            name: "Tiny".into(),
            description: String::new(),
            dims: 2,
            max_tokens: 8,
            license: String::new(),
            homepage: String::new(),
            files: files
                .iter()
                .map(|(name, content)| ManifestFile {
                    name: name.to_string(),
                    url: format!("{base}/{name}"),
                    sha256: hex::encode(Sha256::digest(content)),
                    bytes: content.len() as u64,
                })
                .collect(),
        }
    }

    /// A server that answers every request with `body`, `times` times.
    fn serve(body: Vec<u8>, times: usize) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().take(times) {
                let mut stream = stream.unwrap();
                let mut req = [0u8; 2048];
                let _ = stream.read(&mut req);
                let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        format!("http://{addr}")
    }

    #[test]
    fn a_download_is_kept_only_when_it_matches_what_its_manifest_pins() {
        let dir = tempfile::tempdir().unwrap();
        let base = serve(b"model bytes".to_vec(), 1);
        let m = manifest_for(&[("model.onnx", b"model bytes")], &base);
        let mut seen = Vec::new();
        download(&m, dir.path(), &mut |done, total| seen.push((done, total)), &|| false).unwrap();
        assert_eq!(std::fs::read(dir.path().join("tiny/model.onnx")).unwrap(), b"model bytes");
        assert_eq!(seen.last(), Some(&(11, 11)));
        assert!(downloaded(dir.path(), &m));

        // Asked again, a file that matches is not fetched again: the server
        // above answered once and is gone.
        download(&m, dir.path(), &mut |_, _| {}, &|| false).unwrap();
    }

    #[test]
    fn a_file_that_does_not_match_is_deleted_and_never_kept() {
        let dir = tempfile::tempdir().unwrap();
        let base = serve(b"model byteZ".to_vec(), 1);
        let m = manifest_for(&[("model.onnx", b"model bytes")], &base);
        let err = download(&m, dir.path(), &mut |_, _| {}, &|| false).unwrap_err();
        assert!(matches!(err, EncoderError::Mismatch(ref name) if name == "model.onnx"), "{err}");
        assert!(!dir.path().join("tiny/model.onnx").exists());
        assert!(!dir.path().join("tiny/model.onnx.part").exists());
        assert!(!downloaded(dir.path(), &m));

        // Nor one longer than it should be.
        let base = serve(b"model bytes, and more".to_vec(), 1);
        let m = manifest_for(&[("model.onnx", b"model bytes")], &base);
        assert!(matches!(download(&m, dir.path(), &mut |_, _| {}, &|| false), Err(EncoderError::Mismatch(_))));
        assert!(!dir.path().join("tiny/model.onnx.part").exists());
    }

    /// A server that says it will send `claimed` bytes, sends `body` in
    /// `pieces` with `gap` between them, then holds the connection open for
    /// `hold` before closing it.
    fn serve_slowly(
        body: Vec<u8>,
        claimed: usize,
        pieces: usize,
        gap: std::time::Duration,
        hold: std::time::Duration,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut req = [0u8; 2048];
            let _ = stream.read(&mut req);
            let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {claimed}\r\nConnection: close\r\n\r\n");
            let _ = stream.write_all(head.as_bytes());
            let size = body.len().div_ceil(pieces);
            for piece in body.chunks(size) {
                let _ = stream.write_all(piece);
                let _ = stream.flush();
                std::thread::sleep(gap);
            }
            std::thread::sleep(hold);
        });
        format!("http://{addr}")
    }

    #[test]
    fn a_download_that_stalls_fails_and_a_slow_one_finishes() {
        let dir = tempfile::tempdir().unwrap();
        let body = b"model bytes, sent slowly".to_vec();
        let wait = std::time::Duration::from_millis(800);

        // Slow but moving: every read waits less than the limit, though the
        // whole takes longer than it.
        let base = serve_slowly(body.clone(), body.len(), 6, std::time::Duration::from_millis(300), Default::default());
        let m = manifest_for(&[("model.onnx", &body)], &base);
        download_waiting(&m, dir.path(), wait, &mut |_, _| {}, &|| false).unwrap();
        assert!(downloaded(dir.path(), &m));

        // Half the file, then nothing.
        let other = tempfile::tempdir().unwrap();
        let base =
            serve_slowly(body[..10].to_vec(), body.len(), 1, Default::default(), std::time::Duration::from_secs(5));
        let head_says_more = manifest_for(&[("model.onnx", &body)], &base);
        let started = std::time::Instant::now();
        let err = download_waiting(&head_says_more, other.path(), wait, &mut |_, _| {}, &|| false).unwrap_err();
        assert!(started.elapsed() < std::time::Duration::from_secs(4), "{:?}", started.elapsed());
        assert!(matches!(err, EncoderError::Download(..)), "{err}");
        assert!(!other.path().join("tiny/model.onnx.part").exists());
    }

    #[test]
    fn a_name_that_would_leave_its_folder_is_not_a_manifest() {
        for name in ["model.onnx", "tokenizer.json"] {
            assert!(plain_name(name), "{name}");
        }
        for name in ["", ".", "..", "../x", "a/b", "a\\b", "C:x", "C:\\x", "/etc/passwd"] {
            assert!(!plain_name(name), "{name:?}");
        }
    }

    #[test]
    fn a_stopped_download_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let base = serve(b"model bytes".to_vec(), 1);
        let m = manifest_for(&[("model.onnx", b"model bytes")], &base);
        assert!(matches!(download(&m, dir.path(), &mut |_, _| {}, &|| true), Err(EncoderError::Canceled)));
        assert!(!dir.path().join("tiny/model.onnx").exists());
        assert!(!dir.path().join("tiny/model.onnx.part").exists());
    }

    #[test]
    fn only_a_manifest_that_pins_every_file_is_offered() {
        let dir = tempfile::tempdir().unwrap();
        let good = manifest_for(&[("model.onnx", b"x")], "https://example.invalid");
        let mut plain = good.clone();
        plain.files[0].url = "http://example.invalid/model.onnx".into();
        let mut escaping = good.clone();
        escaping.id = "../elsewhere".into();
        let mut short = good.clone();
        short.files[0].sha256 = "abc".into();
        for (name, m) in [("a", &good), ("b", &plain), ("c", &escaping), ("d", &short)] {
            std::fs::write(dir.path().join(format!("{name}.json")), serde_json::to_string(m).unwrap()).unwrap();
        }
        std::fs::write(dir.path().join("e.json"), "not json").unwrap();
        std::fs::write(dir.path().join("README.md"), "# not a manifest").unwrap();
        assert_eq!(read_manifests(dir.path()), vec![good]);
    }

    #[test]
    fn the_bundled_manifest_is_offered() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../models/manifests");
        let offered = read_manifests(&dir);
        assert_eq!(offered.len(), 1, "one encoder is offered");
        assert_eq!(offered[0].id, "all-minilm-l6-v2");
        assert_eq!(offered[0].dims, 384);
        assert!(offered[0].files.iter().any(|f| f.name == "model.onnx"));
        assert!(offered[0].files.iter().any(|f| f.name == "tokenizer.json"));
    }

    /// An encoder that makes a vector of each text's length and its count of
    /// "e"s, and records what it was asked.
    struct Lengths {
        semantic: bool,
        asked: Mutex<Vec<usize>>,
    }

    impl Embedder for Lengths {
        fn embed(&self, texts: Vec<String>) -> BoxFuture<'_, Result<Option<Embedded>, JobError>> {
            Box::pin(async move {
                self.asked.lock().unwrap().push(texts.len());
                Ok(self.semantic.then(|| Embedded {
                    version: "lengths".into(),
                    dims: 2,
                    vectors: texts.iter().map(|t| vec![t.len() as f32, t.matches('e').count() as f32]).collect(),
                }))
            })
        }
    }

    fn corpus() -> (Db, Vec<String>) {
        let db = Db::open_in_memory().unwrap();
        db.set_user_identity("C").unwrap();
        db.add_user_identifier(IdentifierKind::Handle, "@c").unwrap();
        let source = db
            .create_source(&NewSource {
                connector: "t".into(),
                name: "T".into(),
                channel: "chat".into(),
                location: None,
                config: Value::Null,
            })
            .unwrap();
        let convo = db.upsert_conversation(&source.id, "t", "chat", None).unwrap();
        let bodies = [("other", "lunch friday?"), ("self", "yes please"), ("other", "great"), ("other", "unanswered")];
        let batch: Vec<NewMessage> = bodies
            .iter()
            .enumerate()
            .map(|(i, (dir, body))| NewMessage {
                conversation_id: convo.clone(),
                source_id: source.id.clone(),
                participant_id: None,
                external_id: format!("m{i}"),
                direction: (*dir).into(),
                channel: "chat".into(),
                sent_at: Some(format!("2026-02-01T09:0{i}:00Z")),
                sequence_index: i as i64,
                body: (*body).into(),
                reply_to_external_id: None,
                metadata: Value::Null,
            })
            .collect();
        db.insert_messages(&batch).unwrap();
        db.link_replies(&convo).unwrap();
        let ids = (0..bodies.len())
            .map(|i| {
                db.conn()
                    .query_row("SELECT id FROM messages WHERE external_id = ?1", [format!("m{i}")], |r| r.get(0))
                    .unwrap()
            })
            .collect();
        (db, ids)
    }

    #[tokio::test]
    async fn the_messages_retrieval_compares_are_read_for_meaning_once() {
        let (db, ids) = corpus();
        // A vector from an encoder no longer in use goes.
        db.put_embeddings("old", 1, &[(ids[0].clone(), vec![1.0])]).unwrap();
        let encoder = Lengths { semantic: true, asked: Mutex::new(vec![]) };
        let summary = embed_messages(&db, &encoder, &|_, _| {}, &|| false).await.unwrap();
        assert_eq!(summary.encoder.as_deref(), Some("lengths"));
        assert_eq!(summary.embedded, 2, "the message answered and the answer; not what nobody answered");
        assert_eq!(summary.removed, 1);
        let got = db.vectors_for(&ids, "lengths").unwrap();
        assert_eq!(got.get(&ids[0]), Some(&vec![13.0, 0.0]), "lunch friday?");
        assert_eq!(got.get(&ids[1]), Some(&vec![10.0, 3.0]), "yes please");
        assert!(!got.contains_key(&ids[3]));
        assert_eq!(db.embedding_coverage("lengths").unwrap(), Coverage { wanted: 2, done: 2 });

        // Nothing left to do.
        let again = embed_messages(&db, &encoder, &|_, _| {}, &|| false).await.unwrap();
        assert_eq!(again.embedded, 0);
        assert_eq!(again.encoder, None, "nothing was asked, so nothing was learned");
    }

    #[tokio::test]
    async fn without_a_sentence_encoder_nothing_is_kept() {
        let (db, ids) = corpus();
        let lexical = Lengths { semantic: false, asked: Mutex::new(vec![]) };
        let summary = embed_messages(&db, &lexical, &|_, _| {}, &|| false).await.unwrap();
        assert_eq!(summary, EmbedSummary::default());
        assert_eq!(lexical.asked.lock().unwrap().len(), 1, "asked once, and stopped at no");
        assert!(db.vectors_for(&ids, "lexical_v1").unwrap().is_empty());
        assert_eq!(query_vector(&lexical, "lunch?").await, None);
    }

    #[tokio::test]
    async fn the_message_being_answered_becomes_a_vector_of_the_same_encoder() {
        let encoder = Lengths { semantic: true, asked: Mutex::new(vec![]) };
        let q = query_vector(&encoder, "see you there").await.unwrap();
        assert_eq!(q, QueryVector { version: "lengths".into(), vector: vec![13.0, 4.0] });
        assert_eq!(query_vector(&encoder, "   ").await, None, "nothing to answer, nothing asked");
        assert_eq!(encoder.asked.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_vector_for_a_message_deleted_meanwhile_is_skipped() {
        let (db, ids) = corpus();
        db.conn().execute("DELETE FROM messages WHERE id = ?1", [&ids[0]]).unwrap();
        let kept = db.put_embeddings("v", 1, &[(ids[0].clone(), vec![1.0]), (ids[1].clone(), vec![2.0])]).unwrap();
        assert_eq!(kept, 1);
        assert_eq!(db.vectors_for(&ids, "v").unwrap().len(), 1);
    }

    #[test]
    fn vectors_are_stored_as_they_were_made() {
        let v = vec![0.5f32, -1.25, 3.0e-7];
        assert_eq!(from_blob(&to_blob(&v)), v);
        let engine_answer = json!({"provider": "p", "dims": 2, "semantic": true, "vectors": [[0.5, 1.0]]});
        assert_eq!(
            parse_embedded(&engine_answer, 1),
            Some(Embedded { version: "p".into(), dims: 2, vectors: vec![vec![0.5, 1.0]] })
        );
        assert_eq!(parse_embedded(&engine_answer, 2), None, "one vector for two texts is not an answer");
        let lexical = json!({"provider": "lexical_v1", "dims": 2, "semantic": false, "vectors": [[0.5, 1.0]]});
        assert_eq!(parse_embedded(&lexical, 1), None);
    }
}
