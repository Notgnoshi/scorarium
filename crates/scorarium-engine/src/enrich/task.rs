use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::watch;
use tokio::task::AbortHandle;

use crate::entity::{AnyEntityId, Timestamp};
use crate::{Engine, Result};

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
    root: AnyEntityId,
    queued_at: Timestamp,
    status: watch::Sender<EnrichmentStatus>,
    subtasks: Mutex<Vec<AbortHandle>>,
}

impl Task {
    fn subtasks(&self) -> MutexGuard<'_, Vec<AbortHandle>> {
        self.subtasks
            .lock()
            .expect("enrichment subtasks lock poisoned")
    }

    fn cancel(&self) {
        // Holding the lock across the flag and the drain means a concurrent enqueue either pushes
        // its subtask before the drain or sees the flag and spawns nothing.
        let mut subtasks = self.subtasks();
        self.status.send_modify(|status| status.cancelled = true);
        for subtask in subtasks.drain(..) {
            subtask.abort();
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SubTaskQueue {
    task: Arc<Task>,
}

impl SubTaskQueue {
    pub(crate) fn enqueue(&self, subtask: impl Future<Output = Result<()>> + Send + 'static) {
        let mut subtasks = self.task.subtasks();
        if self.is_cancelled() {
            return;
        }
        self.task.status.send_modify(|status| status.pending += 1);
        let outcome = Outcome {
            task: Arc::clone(&self.task),
            succeeded: false,
        };
        let root = self.task.root;
        let handle = tokio::spawn(async move {
            let mut outcome = outcome;
            match subtask.await {
                Ok(()) => outcome.succeeded = true,
                Err(e) => log::warn!("Enrichment subtask of {root:?} failed: {e}"),
            }
        });
        subtasks.push(handle.abort_handle());
    }

    pub(crate) fn cancel(&self) {
        self.task.cancel();
    }

    /// Return whether the task was cancelled
    pub(crate) fn is_cancelled(&self) -> bool {
        self.task.status.borrow().cancelled
    }
}

struct Outcome {
    task: Arc<Task>,
    succeeded: bool,
}

impl Drop for Outcome {
    fn drop(&mut self) {
        self.task.status.send_modify(|status| {
            status.pending -= 1;
            if self.succeeded {
                status.successful += 1;
            } else {
                status.failed += 1;
            }
        });
    }
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

    /// Return the entity the task enriches
    pub fn entity(&self) -> AnyEntityId {
        self.task.root
    }

    /// Return when the task started
    pub(crate) fn queued_at(&self) -> Timestamp {
        self.task.queued_at
    }

    /// Return whether the task puts its entity in the review queue
    pub(crate) fn needs_review(&self) -> bool {
        matches!(
            self.status().state(),
            EnrichmentState::InProgress | EnrichmentState::Failed
        )
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
        self.task.cancel();
    }
}

impl Engine {
    /// Start an enrichment task on the given `root` entity, or return the one in progress
    pub(crate) fn start_enrichment(
        &self,
        root: AnyEntityId,
        enqueue_subtasks: impl FnOnce(&SubTaskQueue),
    ) -> Enrichment {
        let mut enrichments = self.enrichments();
        if let Some(task) = enrichments.get(&root)
            && task.status.borrow().state() == EnrichmentState::InProgress
        {
            return Enrichment::new(task);
        }
        let task = Arc::new(Task {
            root,
            queued_at: chrono::Utc::now(),
            status: watch::Sender::new(EnrichmentStatus::default()),
            subtasks: Mutex::default(),
        });
        enrichments.insert(root, Arc::clone(&task));
        let enrichment = Enrichment::new(&task);
        // The registry stays locked while the seeds are enqueued, so that no one can find the new
        // task before its seeds are counted and mistake it for a finished one.
        enqueue_subtasks(&SubTaskQueue { task });
        enrichment
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

    /// Return every task whose entity belongs in the review queue
    pub(crate) fn enrichments_to_review(&self) -> Vec<Enrichment> {
        self.enrichments()
            .values()
            .map(Enrichment::new)
            .filter(Enrichment::needs_review)
            .collect()
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::Error;
    use crate::publication::PublicationId;

    #[tokio::test]
    async fn subtasks_are_counted_as_they_finish() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), crate::test::options())
            .await
            .unwrap();
        let root = AnyEntityId::Publication(PublicationId::new(1));
        engine.start_enrichment(root, |queue| {
            let children = queue.clone();
            queue.enqueue(async move {
                children.enqueue(async { Err(Error::NotFound) });
                Ok(())
            });
        });
        tokio::time::timeout(Duration::from_secs(5), engine.wait_for_enrichment())
            .await
            .unwrap();
        let status = engine.enrichment_of(root).unwrap().status();
        assert_eq!(
            status,
            EnrichmentStatus {
                successful: 1,
                pending: 0,
                failed: 1,
                cancelled: false
            }
        );
        assert_eq!(status.state(), EnrichmentState::Failed);
    }

    #[tokio::test]
    async fn cancel_aborts_a_blocked_subtask() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), crate::test::options())
            .await
            .unwrap();
        let root = AnyEntityId::Publication(PublicationId::new(1));
        let (mut never_sent, blocked) = tokio::sync::oneshot::channel::<()>();
        let enrichment = engine.start_enrichment(root, |queue| {
            queue.enqueue(async move {
                let _ = blocked.await;
                Ok(())
            });
        });
        assert_eq!(enrichment.status().state(), EnrichmentState::InProgress);

        let kept = enrichment.clone();
        enrichment.cancel();
        assert_eq!(kept.status().state(), EnrichmentState::Cancelled);
        assert!(engine.enrichment_of(root).is_none());
        tokio::time::timeout(Duration::from_secs(5), engine.wait_for_enrichment())
            .await
            .unwrap();

        tokio::time::timeout(Duration::from_secs(5), never_sent.closed())
            .await
            .unwrap();
        assert_eq!(
            kept.status(),
            EnrichmentStatus {
                successful: 0,
                pending: 0,
                failed: 1,
                cancelled: true
            }
        );
    }
    #[tokio::test]
    async fn a_subtask_can_end_its_task() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), crate::test::options())
            .await
            .unwrap();
        let root = AnyEntityId::Publication(PublicationId::new(1));
        engine.start_enrichment(root, |queue| {
            let task = queue.clone();
            queue.enqueue(async move {
                task.cancel();
                Ok(())
            });
        });
        tokio::time::timeout(Duration::from_secs(5), engine.wait_for_enrichment())
            .await
            .unwrap();
        assert!(engine.enrichment_of(root).is_none());
    }
}
