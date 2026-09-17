use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use http::{HeaderMap, StatusCode};

use crate::log::CallLog;

pub(crate) struct Cache {
    entries: Mutex<HashMap<String, CachedResponse>>,
    log: Arc<CallLog>,
}

/// [http::Response] isn't [Clone], so the parts are kept and a response is rebuilt for each hit.
struct CachedResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
}

impl Cache {
    pub(crate) fn new(log: Arc<CallLog>) -> Cache {
        Cache {
            entries: Mutex::new(HashMap::new()),
            log,
        }
    }

    pub(crate) fn get(&self, key: &str) -> Option<http::Response<Bytes>> {
        let entries = self.entries.lock().unwrap();
        let entry = entries.get(key)?;
        self.log.cache_hit();

        let mut response = http::Response::new(entry.body.clone());
        *response.status_mut() = entry.status;
        *response.headers_mut() = entry.headers.clone();
        Some(response)
    }

    pub(crate) fn insert(&self, key: String, response: &http::Response<Bytes>) {
        let entry = CachedResponse {
            status: response.status(),
            headers: response.headers().clone(),
            body: response.body().clone(),
        };
        self.entries.lock().unwrap().insert(key, entry);
    }

    pub(crate) fn remove(&self, key: &str) {
        self.entries.lock().unwrap().remove(key);
    }

    pub(crate) fn clear(&self) {
        self.entries.lock().unwrap().clear();
    }
}

#[cfg(test)]
mod tests {
    use http::HeaderValue;
    use http::header::ACCEPT;
    use url::Url;

    use super::*;
    use crate::transport::request_key;

    fn accepting(kind: &'static str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static(kind));
        headers
    }

    #[test]
    fn entries_are_keyed_by_the_url_and_the_request_headers() {
        let log = Arc::new(CallLog::default());
        let cache = Cache::new(log.clone());
        let url = Url::parse("https://openlibrary.org/search.json?title=rondos").unwrap();
        let json = request_key(&url, &accepting("application/json"));
        let xml = request_key(&url, &accepting("application/xml"));
        let mut response = http::Response::new(Bytes::from_static(b"{}"));
        *response.status_mut() = StatusCode::NOT_FOUND;

        cache.insert(json.clone(), &response);

        let hit = cache.get(&json).expect("just inserted");
        assert_eq!(hit.status(), StatusCode::NOT_FOUND);
        assert_eq!(hit.body(), &Bytes::from_static(b"{}"));
        assert!(cache.get(&xml).is_none());
        assert_eq!(log.history().cache_hits, 1);

        cache.clear();
        assert!(cache.get(&json).is_none());
        // Clearing forgets the responses, not the calls they already saved
        assert_eq!(log.history().cache_hits, 1);
    }
}
