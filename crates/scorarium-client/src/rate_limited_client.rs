use std::collections::VecDeque;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use eyre::{WrapErr, bail, eyre};
use http::{HeaderMap, StatusCode};
use serde::de::DeserializeOwned;
use tokio::sync::{Notify, oneshot};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use url::Url;

use crate::{Priority, Transport};

/// Limits one API asks of the clients that use it.
pub(crate) struct Limits {
    /// The shortest time between two requests to this source.
    pub(crate) min_interval: Duration,
    /// How long one request may take (excluding time spent queued)
    pub(crate) timeout: Duration,
}

/// One request waiting its turn in the queue
struct Job {
    url: Url,
    headers: HeaderMap,
    priority: Priority,
    reply: oneshot::Sender<eyre::Result<http::Response<Bytes>>>,
}

#[derive(Default)]
struct JobDeque {
    // Use a deque so that we can insert higher priority jobs at the front
    jobs: VecDeque<Job>,
}

impl JobDeque {
    /// Queues a job, ahead of the background ones if it's interactive.
    ///
    /// Interactive jobs supersede each other. The assumption is that interactive jobs are
    /// progressive refinement of a query, and the caller wants responses for jobs that have already
    /// started, and the latest job, and no jobs in between.
    fn push(&mut self, job: Job) {
        match job.priority {
            Priority::Background => self.jobs.push_back(job),
            Priority::Interactive => {
                if self.interactive_is_waiting() {
                    let superseded = self.jobs.pop_front().expect("just checked");
                    let _eat_err = superseded
                        .reply
                        .send(Err(eyre!("Superseded by a newer interactive request")));
                }
                self.jobs.push_front(job);
            }
        }
    }

    /// Whether an interactive job is waiting. At most one ever is, so it's at the front.
    fn interactive_is_waiting(&self) -> bool {
        self.jobs
            .front()
            .is_some_and(|job| job.priority == Priority::Interactive)
    }

    /// The next job anyone is still waiting on.
    fn pop(&mut self) -> Option<Job> {
        loop {
            match self.jobs.pop_front() {
                Some(job) if job.reply.is_closed() => continue,
                job => return job,
            }
        }
    }
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

    pub(crate) async fn get_json<T: DeserializeOwned>(
        &self,
        url: Url,
        headers: HeaderMap,
        priority: Priority,
    ) -> eyre::Result<Option<T>> {
        let response = self.get(url.clone(), headers, priority).await?;
        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            bail!("GET {url} responded {status}");
        }
        serde_json::from_slice(response.body())
            .map(Some)
            .wrap_err_with(|| format!("Failed to parse the response to GET {url}"))
    }

    /// Queues the request and returns a future that waits for its response.
    pub(crate) fn get(
        &self,
        url: Url,
        headers: HeaderMap,
        priority: Priority,
    ) -> impl Future<Output = eyre::Result<http::Response<Bytes>>> + use<> {
        let (reply, response) = oneshot::channel();
        self.deque.lock().unwrap().push(Job {
            url,
            headers,
            priority,
            reply,
        });
        self.wake.notify_one();
        async move {
            response
                .await
                .map_err(|_| eyre!("The rate limited worker exited"))?
        }
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

        let job = deque.lock().unwrap().pop();
        let Some(job) = job else { continue };

        let url = job.url.clone();
        let attempt = tokio::time::timeout(limits.timeout, transport.get(job.url, job.headers));
        let result = match attempt.await {
            Ok(result) => result,
            Err(_elapsed) => Err(eyre!("GET {url} timed out after {:?}", limits.timeout)),
        };
        next_allowed = Instant::now() + limits.min_interval;
        // A caller that dropped its future closed the channel, and there is nobody to answer
        let _eat_err = job.reply.send(result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BoxFuture;

    /// Answers requests in order from a script, and remembers what it was asked.
    struct Scripted {
        responses: Mutex<VecDeque<http::Response<Bytes>>>,
        requests: Mutex<Vec<String>>,
    }

    impl Scripted {
        fn new(responses: usize) -> Arc<Scripted> {
            let responses = (0..responses)
                .map(|_| http::Response::new(Bytes::new()))
                .collect();
            Arc::new(Scripted {
                responses: Mutex::new(responses),
                requests: Mutex::new(Vec::new()),
            })
        }

        fn requests(&self) -> Vec<String> {
            self.requests.lock().unwrap().clone()
        }
    }

    impl Transport for Scripted {
        fn get(
            &self,
            url: Url,
            _headers: HeaderMap,
        ) -> BoxFuture<'_, eyre::Result<http::Response<Bytes>>> {
            self.requests.lock().unwrap().push(url.path().to_string());
            let response = self
                .responses
                .lock()
                .unwrap()
                .pop_front()
                .expect("the script has a response for every request the worker makes");
            Box::pin(async move { Ok(response) })
        }
    }

    fn url(path: &str) -> Url {
        Url::parse("https://example.com")
            .unwrap()
            .join(path)
            .unwrap()
    }

    #[tokio::test(start_paused = true)]
    async fn interactive_jobs_overtake_background_ones_and_dropped_callers_are_skipped() {
        let transport = Scripted::new(3);
        let client = RateLimitedClient::spawn(
            transport.clone(),
            Limits {
                min_interval: Duration::from_secs(1),
                timeout: Duration::from_secs(30),
            },
        );

        let a = client.get(url("/a"), HeaderMap::new(), Priority::Background);
        let b = client.get(url("/b"), HeaderMap::new(), Priority::Background);
        let c = client.get(url("/c"), HeaderMap::new(), Priority::Interactive);
        drop(b); // dropping the future cancels the job if it hasn't already been started

        assert!(a.await.is_ok());
        assert!(c.await.is_ok());
        // calm down, this is using tokio's start_paused feature to use simulated time
        tokio::time::sleep(Duration::from_secs(2)).await;

        assert_eq!(transport.requests(), ["/c", "/a"]);
    }
}
