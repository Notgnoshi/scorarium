use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use eyre::eyre;
use http::HeaderMap;
use tokio::sync::{Notify, oneshot};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use url::Url;

use crate::Transport;

/// Limits one API asks of the clients that use it.
pub(crate) struct Limits {
    /// The shortest time between two requests to this source.
    pub(crate) min_interval: Duration,
}

/// One request waiting its turn in the queue
struct Job {
    url: Url,
    headers: HeaderMap,
    reply: oneshot::Sender<eyre::Result<http::Response<Bytes>>>,
}

#[derive(Default)]
struct JobDeque {
    jobs: VecDeque<Job>,
}

/// An HTTP client that respects API rate limits.
///
/// Each API will get their own [RateLimitedClient] with rate [Limits] tuned for that API according
/// to their developer documentation. Note that some APIs enforce different limits based on your
/// User-Agent, particularly if you provide contact information in the User-Agent.
pub(crate) struct RateLimitedClient {
    deque: Arc<Mutex<JobDeque>>,
    wake: Arc<Notify>,
    worker: JoinHandle<()>,
}

impl RateLimitedClient {
    pub(crate) fn spawn(transport: Arc<dyn Transport>, limits: Limits) -> RateLimitedClient {
        let deque = Arc::new(Mutex::new(JobDeque::default()));
        let wake = Arc::new(Notify::new());
        let worker = tokio::spawn(work(deque.clone(), wake.clone(), transport, limits));
        RateLimitedClient {
            deque,
            wake,
            worker,
        }
    }

    /// Waits the request's turn, performs it, and answers with whatever the transport said.
    pub(crate) async fn get(
        &self,
        url: Url,
        headers: HeaderMap,
    ) -> eyre::Result<http::Response<Bytes>> {
        let (reply, response) = oneshot::channel();
        self.deque.lock().unwrap().jobs.push_back(Job {
            url,
            headers,
            reply,
        });
        self.wake.notify_one();
        response
            .await
            .map_err(|_| eyre!("The rate limited worker exited"))?
    }
}

impl Drop for RateLimitedClient {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

/// Performs jobs one at a time from the front of the deque, one at a time, one per `min_interval`.
///
/// The interval between job completions is technically stricter than what the APIs ask for, but
/// it's easier to implement this way: the interval is measured from job end to the next job's start
/// rather than from start to start.
async fn work(
    deque: Arc<Mutex<JobDeque>>,
    wake: Arc<Notify>,
    transport: Arc<dyn Transport>,
    limits: Limits,
) {
    let mut next_allowed = Instant::now();
    loop {
        if deque.lock().unwrap().jobs.is_empty() {
            wake.notified().await;
            continue;
        }
        tokio::time::sleep_until(next_allowed).await;

        let job = deque.lock().unwrap().jobs.pop_front();
        let Some(job) = job else { continue };

        let result = transport.get(job.url, job.headers).await;
        next_allowed = Instant::now() + limits.min_interval;
        // A caller that dropped its future closed the channel, and there is nobody to answer
        let _eat_err = job.reply.send(result);
    }
}
