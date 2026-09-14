use std::collections::VecDeque;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use bytes::Bytes;
use eyre::{WrapErr, bail, eyre};
use http::{HeaderMap, StatusCode, header};
use serde::de::DeserializeOwned;
use tokio::sync::{Notify, oneshot};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use url::Url;

use crate::cache::Cache;
use crate::log::{CallLog, CallRecord, Outcome};
use crate::transport::request_key;
use crate::{Priority, SourceStatus, Transport};

/// Limits one API asks of the clients that use it.
pub(crate) struct Limits {
    /// The shortest time between two requests to this source.
    pub(crate) min_interval: Duration,
    /// How long one request may take (excluding time spent queued)
    pub(crate) timeout: Duration,
}

const MAX_ATTEMPTS: u8 = 4;

/// Progressive backoff for 5xx responses
const PAUSES: [Duration; 4] = [
    Duration::from_secs(5),
    Duration::from_secs(30),
    Duration::from_secs(2 * 60),
    Duration::from_secs(5 * 60),
];

/// How long to wait after a 429 that doesn't include a Retry-After header
const DEFAULT_RETRY_AFTER: Duration = Duration::from_secs(5);

/// One request waiting its turn in the queue
struct Job {
    url: Url,
    headers: HeaderMap,
    cache_key: String,
    priority: Priority,
    retry: bool,
    attempts: u8,
    reply: oneshot::Sender<eyre::Result<http::Response<Bytes>>>,
}

enum Pending {
    Cached(http::Response<Bytes>),
    Queued(oneshot::Receiver<eyre::Result<http::Response<Bytes>>>),
}

#[derive(Default)]
struct JobDeque {
    jobs: VecDeque<Job>,
    paused_until: Option<Instant>,
    paused_by: Option<StatusCode>,
    consecutive_failures: u8,
    in_flight: bool,
}

impl JobDeque {
    /// Leaves the source alone for a while
    fn pause(&mut self, duration: Duration, by: StatusCode) {
        let until = Instant::now() + duration;
        if self.paused_until.is_none_or(|paused| until > paused) {
            self.paused_until = Some(until);
            self.paused_by = Some(by);
        }
    }

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
    name: &'static str,
    deque: Arc<Mutex<JobDeque>>,
    wake: Arc<Notify>,
    cache: Arc<Cache>,
    worker: JoinHandle<()>,
}

impl RateLimitedClient {
    pub(crate) fn spawn(
        name: &'static str,
        transport: Arc<dyn Transport>,
        limits: Limits,
        cache: Arc<Cache>,
        log: Arc<CallLog>,
    ) -> RateLimitedClient {
        let deque = Arc::new(Mutex::new(JobDeque::default()));
        let wake = Arc::new(Notify::new());
        let worker = tokio::spawn(work(
            name,
            deque.clone(),
            wake.clone(),
            transport,
            limits,
            cache.clone(),
            log,
        ));
        RateLimitedClient {
            name,
            deque,
            wake,
            cache,
            worker,
        }
    }

    pub(crate) fn status(&self) -> SourceStatus {
        let deque = self.deque.lock().unwrap();
        // A pause whose instant has passed is over, even if the instant is still recorded.
        let paused_for = deque
            .paused_until
            .map(|until| until.saturating_duration_since(Instant::now()))
            .filter(|remaining| !remaining.is_zero());
        SourceStatus {
            source: self.name,
            queued: deque.jobs.len(),
            in_flight: deque.in_flight,
            paused_for,
            paused_by: paused_for.and(deque.paused_by),
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
    ///
    /// A response already in the cache is returned without queuing the job.
    pub(crate) fn get(
        &self,
        url: Url,
        headers: HeaderMap,
        priority: Priority,
    ) -> impl Future<Output = eyre::Result<http::Response<Bytes>>> + use<> {
        let key = request_key(&url, &headers);
        let pending = match self.cache.get(&key) {
            Some(cached) => Pending::Cached(cached),
            None => {
                let (reply, response) = oneshot::channel();
                self.deque.lock().unwrap().push(Job {
                    url,
                    headers,
                    cache_key: key,
                    priority,
                    retry: priority == Priority::Background,
                    attempts: 0,
                    reply,
                });
                self.wake.notify_one();
                Pending::Queued(response)
            }
        };
        async move {
            match pending {
                Pending::Cached(cached) => Ok(cached),
                Pending::Queued(response) => response
                    .await
                    .map_err(|_| eyre!("The rate limited worker exited"))?,
            }
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
    name: &'static str,
    deque: Arc<Mutex<JobDeque>>,
    wake: Arc<Notify>,
    transport: Arc<dyn Transport>,
    limits: Limits,
    cache: Arc<Cache>,
    log: Arc<CallLog>,
) {
    let mut next_allowed = Instant::now();
    loop {
        if deque.lock().unwrap().jobs.is_empty() {
            wake.notified().await;
            continue;
        }
        let paused_until = deque.lock().unwrap().paused_until;
        let start_at = paused_until.map_or(next_allowed, |paused| next_allowed.max(paused));
        tokio::time::sleep_until(start_at).await;

        let job = deque.lock().unwrap().pop();
        let Some(mut job) = job else { continue };

        // We check the cache again when we start a job so that if there were duplicate requests
        // queued together, only one of them hits the API.
        if let Some(cached) = cache.get(&job.cache_key) {
            let _eat_err = job.reply.send(Ok(cached));
            continue;
        }
        job.attempts += 1;

        let url = job.url.clone();
        let attempt = tokio::time::timeout(
            limits.timeout,
            transport.get(job.url.clone(), job.headers.clone()),
        );
        let at = SystemTime::now();
        let started = Instant::now();
        deque.lock().unwrap().in_flight = true;
        let attempt = attempt.await;
        deque.lock().unwrap().in_flight = false;
        next_allowed = Instant::now() + limits.min_interval;

        let (result, outcome) = match attempt {
            Ok(Ok(response)) => {
                let outcome = Outcome::Status(response.status());
                (Ok(response), outcome)
            }
            Ok(Err(error)) => {
                let outcome = Outcome::Error(format!("{error:#}"));
                (Err(error), outcome)
            }
            Err(_elapsed) => (
                Err(eyre!("GET {url} timed out after {:?}", limits.timeout)),
                Outcome::Timeout,
            ),
        };
        log.record(CallRecord {
            source: name,
            url: url.clone(),
            attempt: job.attempts,
            outcome,
            duration: started.elapsed(),
            at,
        });

        if let Ok(response) = &result
            && (response.status().is_success() || response.status() == StatusCode::NOT_FOUND)
        {
            cache.insert(job.cache_key.clone(), response);
        }

        // A 429 and a gateway failure are both the source asking us (explicitly, or implicitly) to
        // stop for a while.
        let asked_to_wait = result.as_ref().ok().and_then(|r| retry_after(r.headers()));
        let backed_off = match result.as_ref().map(|response| response.status()) {
            Ok(status) if status == StatusCode::TOO_MANY_REQUESTS => {
                deque
                    .lock()
                    .unwrap()
                    .pause(asked_to_wait.unwrap_or(DEFAULT_RETRY_AFTER), status);
                Some(status)
            }
            Ok(status) if is_gateway_failure(status) => {
                let mut deque = deque.lock().unwrap();
                deque.consecutive_failures = deque.consecutive_failures.saturating_add(1);
                let escalated =
                    PAUSES[usize::from(deque.consecutive_failures).min(PAUSES.len()) - 1];
                // A gateway failure may carry Retry-After too, and we should follow it if it does
                deque.pause(asked_to_wait.unwrap_or(escalated), status);
                Some(status)
            }
            Ok(status) if status.is_success() || status == StatusCode::NOT_FOUND => {
                deque.lock().unwrap().consecutive_failures = 0;
                None
            }
            _ => None,
        };

        let Some(status) = backed_off else {
            let _eat_err = job.reply.send(result);
            continue;
        };
        if job.retry && job.attempts < MAX_ATTEMPTS {
            deque.lock().unwrap().push(job);
            continue;
        }
        let attempts = job.attempts;
        let _eat_err = job.reply.send(Err(if job.retry {
            eyre!("Gave up on GET {url} after {attempts} attempts, last status {status}")
        } else {
            eyre!("GET {url} responded {status}")
        }));
    }
}

fn is_gateway_failure(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::BAD_GATEWAY | StatusCode::SERVICE_UNAVAILABLE | StatusCode::GATEWAY_TIMEOUT
    )
}

/// The `Retry-After` header could be a delay or a datetime
fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    let value = headers.get(header::RETRY_AFTER)?.to_str().ok()?.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let at = httpdate::parse_http_date(value).ok()?;
    Some(
        at.duration_since(SystemTime::now())
            .unwrap_or(Duration::ZERO),
    )
}

#[cfg(test)]
mod tests {
    use http::HeaderValue;

    use super::*;
    use crate::BoxFuture;

    /// Answers requests in order from a script, and remembers what it was asked.
    struct Scripted {
        responses: Mutex<VecDeque<http::Response<Bytes>>>,
        requests: Mutex<Vec<String>>,
    }

    impl Scripted {
        fn new(statuses: impl IntoIterator<Item = u16>) -> Arc<Scripted> {
            let responses = statuses
                .into_iter()
                .map(|status| {
                    let mut response = http::Response::new(Bytes::new());
                    *response.status_mut() = StatusCode::from_u16(status).unwrap();
                    response
                })
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
        let transport = Scripted::new([200, 200, 200]);
        let log = Arc::new(CallLog::default());
        let client = RateLimitedClient::spawn(
            "Test",
            transport.clone(),
            Limits {
                min_interval: Duration::from_secs(1),
                timeout: Duration::from_secs(30),
            },
            Arc::new(Cache::new(log.clone())),
            log.clone(),
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
        // The skipped job leaves nothing behind it
        assert_eq!(client.status().queued, 0);
        assert!(!client.status().in_flight);
    }

    #[tokio::test(start_paused = true)]
    async fn a_gateway_failure_pauses_the_source_and_the_job_is_retried() {
        let transport = Scripted::new([503, 200, 200]);
        let log = Arc::new(CallLog::default());
        let client = RateLimitedClient::spawn(
            "Test",
            transport.clone(),
            Limits {
                min_interval: Duration::from_secs(1),
                timeout: Duration::from_secs(30),
            },
            Arc::new(Cache::new(log.clone())),
            log.clone(),
        );
        let start = Instant::now();

        let a = client.get(url("/a"), HeaderMap::new(), Priority::Background);
        let b = client.get(url("/b"), HeaderMap::new(), Priority::Background);

        assert_eq!(a.await.unwrap().status(), StatusCode::OK);
        assert_eq!(b.await.unwrap().status(), StatusCode::OK);

        assert_eq!(transport.requests(), ["/a", "/b", "/a"]);
        // we did actually suspend for the backoff period
        assert!(start.elapsed() >= PAUSES[0], "{:?}", start.elapsed());
    }

    #[tokio::test(start_paused = true)]
    async fn a_repeated_request_never_reaches_the_transport() {
        let transport = Scripted::new([200]);
        let log = Arc::new(CallLog::default());
        let cache = Arc::new(Cache::new(log.clone()));
        let client = RateLimitedClient::spawn(
            "Test",
            transport.clone(),
            Limits {
                min_interval: Duration::from_secs(1),
                timeout: Duration::from_secs(30),
            },
            cache.clone(),
            log.clone(),
        );

        let first = client.get(url("/a"), HeaderMap::new(), Priority::Background);
        assert!(first.await.is_ok());
        let again = client.get(url("/a"), HeaderMap::new(), Priority::Background);
        assert!(again.await.is_ok());

        assert_eq!(transport.requests(), ["/a"]);
        assert_eq!(log.history().calls.len(), 1);
        assert_eq!(log.history().cache_hits, 1);
    }

    #[test]
    fn retry_after_reads_seconds_and_http_dates() {
        let delay = |value: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(header::RETRY_AFTER, HeaderValue::from_str(value).unwrap());
            retry_after(&headers)
        };

        assert_eq!(retry_after(&HeaderMap::new()), None);
        assert_eq!(delay("120"), Some(Duration::from_secs(120)));
        assert_eq!(delay("the day after tomorrow"), None);

        let ahead = delay(&httpdate::fmt_http_date(
            SystemTime::now() + Duration::from_secs(60),
        ));
        assert!(
            ahead.is_some_and(
                |ahead| ahead > Duration::from_secs(58) && ahead <= Duration::from_secs(60)
            ),
            "{ahead:?}"
        );
        // a datetime in the past results in no waiting
        let behind = delay(&httpdate::fmt_http_date(
            SystemTime::now() - Duration::from_secs(60),
        ));
        assert_eq!(behind, Some(Duration::ZERO));
    }
}
