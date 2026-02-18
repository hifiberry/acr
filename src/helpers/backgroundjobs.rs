use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use parking_lot::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};
use log::debug;

/// Represents a background job with its current status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundJob {
    pub id: String,
    pub name: String,
    pub start_time: u64,
    pub last_update: u64,
    pub progress: Option<String>,
    pub total_items: Option<usize>,
    pub completed_items: Option<usize>,
    pub finished: bool,
    pub finish_time: Option<u64>,
}

impl BackgroundJob {
    /// Create a new background job
    pub fn new(id: String, name: String) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        Self {
            id,
            name,
            start_time: now,
            last_update: now,
            progress: None,
            total_items: None,
            completed_items: None,
            finished: false,
            finish_time: None,
        }
    }
    
    /// Update the job with progress information
    pub fn update_progress(&mut self, progress: Option<String>, completed: Option<usize>, total: Option<usize>) {
        self.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        if let Some(prog) = progress {
            self.progress = Some(prog);
        }
        
        if let Some(comp) = completed {
            self.completed_items = Some(comp);
        }
        
        if let Some(tot) = total {
            self.total_items = Some(tot);
        }
        
        debug!("Updated background job '{}': {:?}", self.id, self);
    }
    
    /// Mark the job as finished
    pub fn mark_finished(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        self.finished = true;
        self.finish_time = Some(now);
        self.last_update = now;
        
        debug!("Marked background job '{}' as finished", self.id);
    }
    
    /// Get the duration since the job started in seconds
    pub fn duration_seconds(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now.saturating_sub(self.start_time)
    }
    
    /// Get the duration since the last update in seconds
    pub fn time_since_last_update(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now.saturating_sub(self.last_update)
    }
}

/// Singleton manager for background jobs
pub struct BackgroundJobs {
    jobs: Arc<Mutex<HashMap<String, BackgroundJob>>>,
}

impl BackgroundJobs {
    /// Create a new BackgroundJobs instance
    fn new() -> Self {
        Self {
            jobs: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    
    /// Get the global singleton instance
    pub fn instance() -> &'static BackgroundJobs {
        static INSTANCE: OnceLock<BackgroundJobs> = OnceLock::new();
        INSTANCE.get_or_init(|| BackgroundJobs::new())
    }
    
    /// Register a new background job
    pub fn register_job(&self, id: String, name: String) -> Result<(), String> {
        let job = BackgroundJob::new(id.clone(), name);

        let mut jobs = self.jobs.lock();
        if jobs.contains_key(&id) {
            debug!("Overwriting existing job with ID '{}' with new job", id);
        } else {
            debug!("Registering new background job: {}", id);
        }
        jobs.insert(id.clone(), job);
        Ok(())
    }
    
    /// Update progress for an existing job
    pub fn update_job(&self, id: &str, progress: Option<String>, completed: Option<usize>, total: Option<usize>) -> Result<(), String> {
        let mut jobs = self.jobs.lock();
        if let Some(job) = jobs.get_mut(id) {
            job.update_progress(progress, completed, total);
            Ok(())
        } else {
            Err(format!("Job with ID '{}' not found", id))
        }
    }
    
    /// Mark a job as completed/finished
    pub fn complete_job(&self, id: &str) -> Result<(), String> {
        let mut jobs = self.jobs.lock();
        if let Some(job) = jobs.get_mut(id) {
            job.mark_finished();
            debug!("Marked background job as finished: {}", id);
            Ok(())
        } else {
            Err(format!("Job with ID '{}' not found", id))
        }
    }
    
    /// Get all currently running background jobs
    pub fn get_all_jobs(&self) -> Result<Vec<BackgroundJob>, String> {
        Ok(self.jobs.lock().values().cloned().collect())
    }
    
    /// Get a specific job by ID
    pub fn get_job(&self, id: &str) -> Result<Option<BackgroundJob>, String> {
        Ok(self.jobs.lock().get(id).cloned())
    }
    
    /// Get the count of currently running jobs
    pub fn job_count(&self) -> usize {
        self.jobs.lock().len()
    }
}

/// Convenience functions for easier access to the singleton
pub fn register_job(id: String, name: String) -> Result<(), String> {
    BackgroundJobs::instance().register_job(id, name)
}

pub fn update_job(id: &str, progress: Option<String>, completed: Option<usize>, total: Option<usize>) -> Result<(), String> {
    BackgroundJobs::instance().update_job(id, progress, completed, total)
}

pub fn complete_job(id: &str) -> Result<(), String> {
    BackgroundJobs::instance().complete_job(id)
}

pub fn get_all_jobs() -> Result<Vec<BackgroundJob>, String> {
    BackgroundJobs::instance().get_all_jobs()
}

pub fn get_job(id: &str) -> Result<Option<BackgroundJob>, String> {
    BackgroundJobs::instance().get_job(id)
}

pub fn job_count() -> usize {
    BackgroundJobs::instance().job_count()
}
