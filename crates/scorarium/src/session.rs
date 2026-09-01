use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const SESSION_LIFETIME: Duration = Duration::from_secs(3 * 24 * 60 * 60); // 3 days

/// The set of live login sessions, keyed by the token each browser holds in its session cookie.
///
/// Sessions are held in memory and get reset if the server restarts.
#[derive(Default)]
pub struct SessionStore {
    /// token -> expiry
    sessions: Mutex<HashMap<String, Instant>>,
}

impl SessionStore {
    /// Start a new session, returning its token.
    pub fn create(&self) -> String {
        let token = generate_token();
        let now = Instant::now();
        let mut sessions = self.sessions.lock().expect("session lock poisoned");
        sessions.insert(token.clone(), now + SESSION_LIFETIME);
        token
    }

    /// Is this token a live login session?
    pub fn validate(&self, token: &str) -> bool {
        let mut sessions = self.sessions.lock().expect("session lock poisoned");
        match sessions.get(token) {
            Some(expires_at) if Instant::now() < *expires_at => true,
            Some(_) => {
                sessions.remove(token);
                false
            }
            None => false,
        }
    }

    /// End the session with this token, if any (logout).
    pub fn revoke(&self, token: &str) {
        let mut sessions = self.sessions.lock().expect("session lock poisoned");
        sessions.remove(token);
    }

    /// End every session except the one with this token i.e., a password change
    pub fn revoke_all_except(&self, token: &str) {
        let mut sessions = self.sessions.lock().expect("session lock poisoned");
        sessions.retain(|t, _| t == token);
    }
}

/// A fresh 128-bit random token, hex-encoded.
fn generate_token() -> String {
    let mut buf = [0u8; 16];
    getrandom::fill(&mut buf).expect("OS randomness unavailable");
    buf.iter().fold(String::new(), |mut token, byte| {
        token.push_str(&format!("{byte:02x}"));
        token
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Force a session to have already expired.
    fn backdate(store: &SessionStore, token: &str) {
        let mut sessions = store.sessions.lock().unwrap();
        // session expired 5 seconds ago
        *sessions.get_mut(token).unwrap() = Instant::now() - Duration::from_secs(5);
    }

    #[test]
    fn created_token_validates() {
        let store = SessionStore::default();
        let token = store.create();
        assert!(store.validate(&token));
        assert!(!store.validate("no-such-token"));
    }

    #[test]
    fn tokens_are_unique() {
        let store = SessionStore::default();
        assert_ne!(store.create(), store.create());
    }

    #[test]
    fn expired_token_rejected() {
        let store = SessionStore::default();
        let token = store.create();
        backdate(&store, &token);
        assert!(!store.validate(&token));
    }

    #[test]
    fn revoke_ends_session() {
        let store = SessionStore::default();
        let token = store.create();
        store.revoke(&token);
        assert!(!store.validate(&token));
    }

    #[test]
    fn revoke_all_except_keeps_current() {
        let store = SessionStore::default();
        let other = store.create();
        let current = store.create();
        store.revoke_all_except(&current);
        assert!(store.validate(&current));
        assert!(!store.validate(&other));
    }
}
