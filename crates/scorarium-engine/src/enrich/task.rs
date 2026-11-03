use std::sync::Arc;

use tokio::sync::watch;

use crate::Engine;
use crate::entity::AnyEntityId;

/// The status of an [Enrichment] task
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EnrichmentStatus {
    pub successful: usize,
    pub pending: usize,
    pub failed: usize,
    pub cancelled: bool,
}

impl EnrichmentStatus {
    /// Return the overall state of the enrichment task
    pub fn state(self) -> EnrichmentState {
        if self.cancelled {
            EnrichmentState::Cancelled
        } else if self.pending > 0 {
            EnrichmentState::InProgress
        } else if self.failed > 0 {
            EnrichmentState::Failed
        } else {
            EnrichmentState::Completed
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnrichmentState {
    InProgress,
    Failed,
    Completed,
    Cancelled,
}

#[derive(Debug)]
pub(crate) struct Task {
    status: watch::Sender<EnrichmentStatus>,
}

/// A handle of an [Enrichment] task
#[derive(Clone, Debug)]
pub struct Enrichment {
    status: watch::Receiver<EnrichmentStatus>,
    task: Arc<Task>,
}

impl Enrichment {
    fn new(task: &Arc<Task>) -> Enrichment {
        Enrichment {
            status: task.status.subscribe(),
            task: Arc::clone(task),
        }
    }

    pub fn status(&self) -> EnrichmentStatus {
        *self.status.borrow()
    }

    /// Wait for the next status change, or return at once if the task is over
    pub async fn changed(&mut self) {
        let state = self.status.borrow_and_update().state();
        if state != EnrichmentState::InProgress {
            return;
        }
        let _ = self.status.changed().await;
    }

    /// Abort the task's pending and in-flight subtasks and remove it from the task registry
    pub fn cancel(self) {
        self.task
            .status
            .send_modify(|status| status.cancelled = true);
    }
}

impl Engine {
    /// Start an enrichment task on the given `root` entity, or return the one in progress
    pub(crate) fn start_enrichment(&self, root: AnyEntityId) -> Enrichment {
        let mut enrichments = self.enrichments();
        if let Some(task) = enrichments.get(&root)
            && task.status.borrow().state() == EnrichmentState::InProgress
        {
            return Enrichment::new(task);
        }
        let task = Arc::new(Task {
            status: watch::Sender::new(EnrichmentStatus::default()),
        });
        enrichments.insert(root, Arc::clone(&task));
        Enrichment::new(&task)
    }

    /// Return the enrichment task on for `root`, unless there is none or it was cancelled
    pub(crate) fn enrichment_of(&self, root: AnyEntityId) -> Option<Enrichment> {
        let mut enrichments = self.enrichments();
        let task = enrichments.get(&root)?;
        if task.status.borrow().state() == EnrichmentState::Cancelled {
            enrichments.remove(&root);
            return None;
        }
        Some(Enrichment::new(task))
    }

    /// Wait for all enrichment tasks to finish
    ///
    /// Dropping the future is safe at any point and cancels no task.
    pub async fn wait_for_enrichment(&self) {
        loop {
            let handles: Vec<Enrichment> = {
                let mut enrichments = self.enrichments();
                enrichments
                    .retain(|_, task| task.status.borrow().state() != EnrichmentState::Cancelled);
                enrichments
                    .values()
                    .filter(|task| task.status.borrow().state() == EnrichmentState::InProgress)
                    .map(Enrichment::new)
                    .collect()
            };
            if handles.is_empty() {
                return;
            }
            for mut handle in handles {
                while handle.status().state() == EnrichmentState::InProgress {
                    handle.changed().await;
                }
            }
        }
    }
}
