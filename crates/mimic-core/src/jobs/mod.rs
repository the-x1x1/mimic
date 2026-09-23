//! Persistent job system (spec §15).
//!
//! Jobs live in the `jobs` table. `JobRunner` drains the queue one job at a
//! time (bounded concurrency = 1 orchestrator; the engine parallelizes within
//! a job), heart-beats every few seconds, honours cancellation between work
//! items, and on startup converts stale `running` rows to `interrupted`,
//! re-queuing only kinds declared resumable.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::broadcast;

use crate::db::{Db, Job, NewEvent};

#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("{0}")]
    Failed(String),
    #[error("canceled")]
    Canceled,
    #[error("db: {0}")]
    Db(#[from] crate::db::DbError),
    #[error("engine: {0}")]
    Engine(#[from] crate::engine::EngineError),
    #[error("source: {0}")]
    Source(#[from] crate::sources::SourceError),
}

impl JobError {
    pub fn to_json(&self) -> Value {
        let code = match self {
            JobError::Failed(_) => "failed",
            JobError::Canceled => "canceled",
            JobError::Db(_) => "database",
            JobError::Engine(_) => "engine",
            JobError::Source(_) => "source",
        };
        json!({"code": code, "message": self.to_string()})
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobEvent {
    pub job_id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub status: String,
    pub phase: Option<String>,
    pub progress_current: i64,
    pub progress_total: i64,
    pub message: Option<String>,
}

/// Handed to executors; wraps progress + cancellation.
#[derive(Clone)]
pub struct JobContext {
    pub db: Db,
    pub job: Job,
    events: broadcast::Sender<JobEvent>,
}

impl JobContext {
    pub fn progress(&self, current: i64, total: i64, phase: &str) {
        let _ = self.db.update_job_progress(&self.job.id, current, total, Some(phase));
        let _ = self.events.send(JobEvent {
            job_id: self.job.id.clone(),
            kind: self.job.kind.clone(),
            status: "running".into(),
            phase: Some(phase.to_string()),
            progress_current: current,
            progress_total: total,
            message: None,
        });
    }

    pub fn message(&self, phase: &str, message: &str) {
        let _ = self.events.send(JobEvent {
            job_id: self.job.id.clone(),
            kind: self.job.kind.clone(),
            status: "running".into(),
            phase: Some(phase.to_string()),
            progress_current: 0,
            progress_total: 0,
            message: Some(message.to_string()),
        });
    }

    /// Returns `Err(Canceled)` when the user asked to stop. Call between items.
    pub fn check_cancel(&self) -> Result<(), JobError> {
        if self.db.job_cancel_requested(&self.job.id).unwrap_or(false) {
            Err(JobError::Canceled)
        } else {
            Ok(())
        }
    }
}

pub type JobFuture = Pin<Box<dyn Future<Output = Result<Value, JobError>> + Send>>;

/// Dispatches to the first executor that declares the job kind.
pub struct CompositeExecutor {
    executors: Vec<Arc<dyn JobExecutor>>,
    kinds: &'static [&'static str],
}

impl CompositeExecutor {
    pub fn new(executors: Vec<Arc<dyn JobExecutor>>) -> Self {
        let kinds: Vec<&'static str> = executors.iter().flat_map(|e| e.kinds().iter().copied()).collect();
        Self { executors, kinds: Box::leak(kinds.into_boxed_slice()) }
    }

    fn find(&self, kind: &str) -> Option<&Arc<dyn JobExecutor>> {
        self.executors.iter().find(|e| e.kinds().contains(&kind))
    }
}

impl JobExecutor for CompositeExecutor {
    fn kinds(&self) -> &'static [&'static str] {
        self.kinds
    }
    fn resumable(&self, kind: &str) -> bool {
        self.find(kind).map(|e| e.resumable(kind)).unwrap_or(false)
    }
    fn execute(&self, ctx: JobContext) -> JobFuture {
        match self.find(&ctx.job.kind) {
            Some(e) => e.execute(ctx),
            None => {
                let kind = ctx.job.kind.clone();
                Box::pin(async move { Err(JobError::Failed(format!("no executor for job kind {kind}"))) })
            }
        }
    }
}

/// Forward `job.progress` events emitted by the engine for this job into the
/// job record until the returned guard is dropped.
pub fn forward_engine_progress(engine: &crate::engine::EngineClient, ctx: &JobContext) -> ProgressForwarder {
    let mut rx = engine.subscribe();
    let ctx = ctx.clone();
    let handle = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(crate::engine::EngineEvent::JobProgress { job_id, phase, current, total, .. })
                    if job_id == ctx.job.id =>
                {
                    ctx.progress(current, total, &phase);
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });
    ProgressForwarder(handle)
}

pub struct ProgressForwarder(tokio::task::JoinHandle<()>);

impl Drop for ProgressForwarder {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Implemented by the ingest/training/apply modules. `kinds()` must list every
/// job type the executor understands; unknown kinds fail fast.
pub trait JobExecutor: Send + Sync {
    fn kinds(&self) -> &'static [&'static str];
    fn resumable(&self, kind: &str) -> bool;
    fn execute(&self, ctx: JobContext) -> JobFuture;
}

#[derive(Clone)]
pub struct JobRunner {
    db: Db,
    executor: Arc<dyn JobExecutor>,
    events: broadcast::Sender<JobEvent>,
    wake: Arc<tokio::sync::Notify>,
}

impl JobRunner {
    pub fn new(db: Db, executor: Arc<dyn JobExecutor>) -> Self {
        let (events, _) = broadcast::channel(1024);
        Self { db, executor, events, wake: Arc::new(tokio::sync::Notify::new()) }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<JobEvent> {
        self.events.subscribe()
    }

    /// Enqueue a job; returns the persisted row. Wakes the loop.
    pub fn enqueue(&self, kind: &str, payload: Value) -> Result<Job, JobError> {
        if !self.executor.kinds().contains(&kind) {
            return Err(JobError::Failed(format!("unknown job kind {kind}")));
        }
        let job = self.db.create_job(kind, &payload, self.executor.resumable(kind))?;
        let _ = self.db.log_event(&NewEvent::info("jobs", "queued", json!({"kind": kind})).entity("job", &job.id));
        self.wake.notify_one();
        Ok(job)
    }

    pub fn cancel(&self, job_id: &str) -> Result<Job, JobError> {
        let job = self.db.cancel_job(job_id)?;
        let _ = self.events.send(JobEvent {
            job_id: job.id.clone(),
            kind: job.kind.clone(),
            status: job.status.clone(),
            phase: job.phase.clone(),
            progress_current: job.progress_current,
            progress_total: job.progress_total,
            message: Some("cancel requested".into()),
        });
        Ok(job)
    }

    /// Startup recovery. Returns (interrupted, requeued).
    pub fn recover(&self) -> Result<(usize, usize), JobError> {
        let (interrupted, requeued) = self.db.recover_interrupted_jobs()?;
        if interrupted > 0 {
            let _ = self.db.log_event(&NewEvent::warn(
                "jobs",
                "recovered",
                json!({"interrupted": interrupted, "requeued": requeued}),
            ));
        }
        self.wake.notify_one();
        Ok((interrupted, requeued))
    }

    /// Run the queue forever. Spawn this on the runtime.
    pub async fn run_loop(self) {
        loop {
            match self.db.next_queued_job() {
                Ok(Some(job)) => {
                    self.run_one(job).await;
                }
                Ok(None) => {
                    let _ = tokio::time::timeout(Duration::from_secs(2), self.wake.notified()).await;
                }
                Err(e) => {
                    tracing::error!("job loop db error: {e}");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }

    /// Process a single job to completion. Public for tests.
    pub async fn run_one(&self, job: Job) {
        match self.db.mark_job_running(&job.id) {
            Ok(true) => {}
            // Canceled while it waited to start: there is nothing to run.
            Ok(false) => return,
            Err(e) => {
                tracing::error!("cannot start job {}: {e}", job.id);
                return;
            }
        }
        let _ = self.events.send(JobEvent {
            job_id: job.id.clone(),
            kind: job.kind.clone(),
            status: "running".into(),
            phase: None,
            progress_current: 0,
            progress_total: 0,
            message: None,
        });
        let ctx = JobContext { db: self.db.clone(), job: job.clone(), events: self.events.clone() };
        let hb_db = self.db.clone();
        let hb_id = job.id.clone();
        let heartbeat = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(3)).await;
                let _ = hb_db.heartbeat_job(&hb_id);
            }
        });
        // Each job runs in its own task, so a panic inside one — a bug, or
        // input nobody anticipated — fails that job instead of ending the loop
        // that runs every job after it.
        let outcome = if self.executor.kinds().contains(&job.kind.as_str()) {
            match tokio::spawn(self.executor.execute(ctx)).await {
                Ok(outcome) => outcome,
                Err(_) => {
                    // Not the panic's own message: it can quote the text being
                    // processed, and message content never reaches a log.
                    tracing::error!(job = %job.id, kind = %job.kind, "job panicked");
                    Err(JobError::Failed(
                        "Something went wrong inside Mimic and this stopped. It has been logged; trying again may work."
                            .into(),
                    ))
                }
            }
        } else {
            Err(JobError::Failed(format!("no executor for job kind {}", job.kind)))
        };
        heartbeat.abort();
        let (status, message) = match outcome {
            Ok(result) => {
                let _ = self.db.complete_job(&job.id, &result);
                ("completed", None)
            }
            Err(JobError::Canceled) => {
                let _ = self.db.finish_canceled_job(&job.id);
                ("canceled", None)
            }
            Err(e) => {
                let _ = self.db.fail_job(&job.id, &e.to_json());
                let _ = self.db.log_event(&NewEvent::error("jobs", "failed", e.to_json()).entity("job", &job.id));
                ("failed", Some(e.to_string()))
            }
        };
        let final_job = self.db.get_job(&job.id).ok().flatten();
        let _ = self.events.send(JobEvent {
            job_id: job.id.clone(),
            kind: job.kind.clone(),
            status: status.into(),
            phase: final_job.as_ref().and_then(|j| j.phase.clone()),
            progress_current: final_job.as_ref().map(|j| j.progress_current).unwrap_or(0),
            progress_total: final_job.as_ref().map(|j| j.progress_total).unwrap_or(0),
            message,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestExecutor;

    impl JobExecutor for TestExecutor {
        fn kinds(&self) -> &'static [&'static str] {
            &["count", "explode", "slow", "panic"]
        }
        fn resumable(&self, kind: &str) -> bool {
            kind == "count"
        }
        fn execute(&self, ctx: JobContext) -> JobFuture {
            Box::pin(async move {
                match ctx.job.kind.as_str() {
                    "count" => {
                        let n = ctx.job.payload["n"].as_i64().unwrap_or(3);
                        for i in 0..n {
                            ctx.check_cancel()?;
                            ctx.progress(i + 1, n, "counting");
                        }
                        Ok(json!({"counted": n}))
                    }
                    "slow" => {
                        for i in 0..50 {
                            ctx.check_cancel()?;
                            ctx.progress(i, 50, "sleeping");
                            tokio::time::sleep(Duration::from_millis(20)).await;
                        }
                        Ok(json!({}))
                    }
                    "panic" => panic!("an executor bug"),
                    _ => Err(JobError::Failed("boom".into())),
                }
            })
        }
    }

    #[tokio::test]
    async fn runs_completes_and_fails_jobs() {
        let db = Db::open_in_memory().unwrap();
        let runner = JobRunner::new(db.clone(), Arc::new(TestExecutor));
        let mut events = runner.subscribe();
        let ok = runner.enqueue("count", json!({"n": 4})).unwrap();
        assert!(ok.resumable);
        let bad = runner.enqueue("explode", json!({})).unwrap();
        assert!(!bad.resumable);
        assert!(runner.enqueue("nope", json!({})).is_err());
        runner.run_one(ok.clone()).await;
        runner.run_one(bad.clone()).await;
        let ok = db.get_job(&ok.id).unwrap().unwrap();
        assert_eq!(ok.status, "completed");
        assert_eq!(ok.result.unwrap()["counted"], 4);
        assert_eq!((ok.progress_current, ok.progress_total), (4, 4));
        let bad = db.get_job(&bad.id).unwrap().unwrap();
        assert_eq!(bad.status, "failed");
        assert_eq!(bad.error.unwrap()["message"], "boom");
        let mut saw_completed = false;
        while let Ok(ev) = events.try_recv() {
            if ev.status == "completed" {
                saw_completed = true;
            }
        }
        assert!(saw_completed);
    }

    #[tokio::test]
    async fn cancel_stops_between_items() {
        let db = Db::open_in_memory().unwrap();
        let runner = JobRunner::new(db.clone(), Arc::new(TestExecutor));
        let job = runner.enqueue("slow", json!({})).unwrap();
        let r2 = runner.clone();
        let j2 = job.clone();
        let handle = tokio::spawn(async move { r2.run_one(j2).await });
        tokio::time::sleep(Duration::from_millis(120)).await;
        runner.cancel(&job.id).unwrap();
        handle.await.unwrap();
        let job = db.get_job(&job.id).unwrap().unwrap();
        assert_eq!(job.status, "canceled");
        assert!(job.progress_current < 50);
    }

    /// A panic inside one job fails that job and leaves the runner able to
    /// run the next one.
    #[tokio::test]
    async fn a_panicking_job_fails_alone() {
        let db = Db::open_in_memory().unwrap();
        let runner = JobRunner::new(db.clone(), Arc::new(TestExecutor));
        let bad = runner.enqueue("panic", json!({})).unwrap();
        let next = runner.enqueue("count", json!({"n": 1})).unwrap();
        runner.run_one(bad.clone()).await;
        runner.run_one(next.clone()).await;
        let bad = db.get_job(&bad.id).unwrap().unwrap();
        assert_eq!(bad.status, "failed");
        assert!(bad.error.unwrap()["message"].as_str().unwrap().contains("went wrong inside Mimic"));
        assert_eq!(db.get_job(&next.id).unwrap().unwrap().status, "completed");
    }
}
