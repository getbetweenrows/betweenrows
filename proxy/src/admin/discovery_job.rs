use std::collections::HashMap;
use std::time::Instant;

use axum::response::sse::Event;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

// ---------- SSE event type ----------

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DiscoveryEvent {
    Progress { phase: String, detail: String },
    Result { data: serde_json::Value },
    Error { message: String },
    Cancelled,
    Done,
}

impl DiscoveryEvent {
    pub fn to_sse_event(&self) -> Result<Event, serde_json::Error> {
        let json = serde_json::to_string(self)?;
        Ok(Event::default().data(json))
    }
}

// ---------- request type ----------

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum DiscoveryRequest {
    DiscoverSchemas,
    DiscoverTables { schemas: Vec<String> },
    DiscoverColumns { tables: Vec<TableRef> },
    SaveCatalog { schemas: Vec<SaveSchemaSelection> },
    SyncCatalog,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TableRef {
    pub schema: String,
    pub table: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SaveSchemaSelection {
    pub schema_name: String,
    pub is_selected: bool,
    pub tables: Vec<SaveTableSelection>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SaveTableSelection {
    pub table_name: String,
    pub table_type: String,
    pub is_selected: bool,
    pub columns: Option<Vec<SaveColumnSelection>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SaveColumnSelection {
    pub column_name: String,
    pub is_selected: bool,
}

// ---------- job status ----------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobStatus {
    Running,
    Completed,
    Failed,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            JobStatus::Running => "running",
            JobStatus::Completed => "completed",
            JobStatus::Failed => "failed",
        }
    }
}

// ---------- job ----------

pub struct DiscoveryJob {
    pub id: String,
    pub datasource_id: Uuid,
    pub action: String,
    pub status: JobStatus,
    pub tx: broadcast::Sender<DiscoveryEvent>,
    pub cancel: CancellationToken,
    pub created_at: Instant,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

impl DiscoveryJob {
    pub fn new(datasource_id: Uuid, action: String) -> Self {
        let (tx, _) = broadcast::channel(64);
        Self {
            id: Uuid::now_v7().to_string(),
            datasource_id,
            action,
            status: JobStatus::Running,
            tx,
            cancel: CancellationToken::new(),
            created_at: Instant::now(),
            result: None,
            error: None,
        }
    }
}

// ---------- job store ----------

/// In-memory registry of all discovery jobs.
/// Enforces one-active-per-datasource.
pub struct JobStore {
    /// All jobs (running + recent completed/failed, TTL cleanup not yet implemented).
    jobs: HashMap<String, DiscoveryJob>,
    /// datasource_id → job_id for currently running jobs.
    active_by_ds: HashMap<Uuid, String>,
}

impl Default for JobStore {
    fn default() -> Self {
        Self::new()
    }
}

impl JobStore {
    pub fn new() -> Self {
        Self {
            jobs: HashMap::new(),
            active_by_ds: HashMap::new(),
        }
    }

    /// Try to register a new job for a datasource.
    /// Returns Err(existing_job_id) if a job is already running for this datasource.
    pub fn try_register(&mut self, job: DiscoveryJob) -> Result<&DiscoveryJob, String> {
        // Check for an existing running job
        if let Some(existing_id) = self.active_by_ds.get(&job.datasource_id) {
            let is_running = self
                .jobs
                .get(existing_id.as_str())
                .map(|j| j.status == JobStatus::Running)
                .unwrap_or(false);

            if is_running {
                return Err(existing_id.clone());
            }
        }

        // Remove any stale reference
        self.active_by_ds.remove(&job.datasource_id);

        let ds_id = job.datasource_id;
        let job_id = job.id.clone();
        self.jobs.insert(job_id.clone(), job);
        self.active_by_ds.insert(ds_id, job_id.clone());
        Ok(self.jobs.get(&job_id).unwrap())
    }

    pub fn get(&self, job_id: &str) -> Option<&DiscoveryJob> {
        self.jobs.get(job_id)
    }

    pub fn get_mut(&mut self, job_id: &str) -> Option<&mut DiscoveryJob> {
        self.jobs.get_mut(job_id)
    }

    /// Mark a job as completed with a result value.
    pub fn complete(&mut self, job_id: &str, result: serde_json::Value) {
        if let Some(job) = self.jobs.get_mut(job_id) {
            job.status = JobStatus::Completed;
            job.result = Some(result);
            self.active_by_ds.remove(&job.datasource_id);
        }
    }

    /// Mark a job as failed with an error message.
    pub fn fail(&mut self, job_id: &str, error: String) {
        if let Some(job) = self.jobs.get_mut(job_id) {
            job.status = JobStatus::Failed;
            job.error = Some(error);
            self.active_by_ds.remove(&job.datasource_id);
        }
    }

    /// Cancel a job (returns false if not found or not running).
    pub fn cancel(&mut self, job_id: &str) -> bool {
        if let Some(job) = self.jobs.get_mut(job_id)
            && job.status == JobStatus::Running
        {
            job.cancel.cancel();
            job.status = JobStatus::Failed;
            job.error = Some("cancelled".to_string());
            self.active_by_ds.remove(&job.datasource_id);
            return true;
        }
        false
    }

    /// Get a broadcast receiver for a job.
    pub fn subscribe(&self, job_id: &str) -> Option<broadcast::Receiver<DiscoveryEvent>> {
        self.jobs.get(job_id).map(|j| j.tx.subscribe())
    }
}
