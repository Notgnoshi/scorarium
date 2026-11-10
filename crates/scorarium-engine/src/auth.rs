use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{self, PasswordHasher, PasswordVerifier};

use crate::audit::{Action, AuditSource};
use crate::{Engine, Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasswordState {
    Unclaimed,
    Claimed,
    /// It's an open instance that doesn't require a password (demo mode)
    NotNeeded,
}

pub(crate) async fn hash_password(password: String) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        let hash = Argon2::default()
            .hash_password(password.as_bytes())
            .map_err(|e| Error::Generic(eyre::Report::new(e)))?;
        Ok(hash.to_string())
    })
    .await
    .map_err(eyre::Report::new)?
}

pub(crate) async fn verify_password(stored: String, password: String) -> Result<bool> {
    tokio::task::spawn_blocking(move || {
        let parsed =
            PasswordHash::new(&stored).map_err(|e| Error::Generic(eyre::Report::new(e)))?;
        match Argon2::default().verify_password(password.as_bytes(), &parsed) {
            Ok(()) => Ok(true),
            Err(password_hash::Error::PasswordInvalid) => Ok(false),
            Err(e) => Err(Error::Generic(eyre::Report::new(e))),
        }
    })
    .await
    .map_err(eyre::Report::new)?
}

const SESSION_LIFETIME: Duration = Duration::from_secs(3 * 24 * 60 * 60);
const LOGIN_ATTEMPTS: u32 = 5;
const LOGIN_WINDOW: Duration = Duration::from_secs(15 * 60);

/// The value of a session cookie
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Token(String);

impl Token {
    fn generate() -> Self {
        let mut buf = [0u8; 16];
        getrandom::fill(&mut buf).expect("OS randomness unavailable");
        Token(buf.iter().map(|byte| format!("{byte:02x}")).collect())
    }

    /// Get the token's value
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// The token is a bearer credential, so keep it out of logs
impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(..)")
    }
}

impl From<&str> for Token {
    fn from(value: &str) -> Self {
        Token(value.to_owned())
    }
}

/// Live login sessions
#[derive(Debug, Default)]
pub(crate) struct SessionStore {
    /// Map each token to its expiry.
    sessions: Mutex<HashMap<Token, Instant>>,
}

impl SessionStore {
    /// Start a session and return its token.
    pub(crate) fn create(&self) -> Token {
        let token = Token::generate();
        let mut sessions = self.sessions.lock().expect("session lock poisoned");
        sessions.insert(token.clone(), Instant::now() + SESSION_LIFETIME);
        token
    }

    /// Check whether the token is a live session, dropping it if it has expired.
    pub(crate) fn validate(&self, token: &Token) -> bool {
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

    /// End the session with this token.
    pub(crate) fn revoke(&self, token: &Token) {
        self.sessions
            .lock()
            .expect("session lock poisoned")
            .remove(token);
    }

    /// End every session.
    pub(crate) fn revoke_all(&self) {
        self.sessions.lock().expect("session lock poisoned").clear();
    }
}

/// Rate limit how fast passwords can be guessed.
#[derive(Debug, Default)]
pub(crate) struct LoginThrottle {
    /// The failures in the current window, and when that window started.
    failures: Option<(u32, Instant)>,
}

impl LoginThrottle {
    /// Get how long logins stay refused, or `None` if they are accepted now.
    pub(crate) fn locked_for(&mut self) -> Option<Duration> {
        match self.failures {
            Some((count, since)) if count >= LOGIN_ATTEMPTS => {
                let remaining = LOGIN_WINDOW.checked_sub(since.elapsed());
                if remaining.is_none() {
                    self.failures = None;
                }
                remaining
            }
            _ => None,
        }
    }

    /// Count a failed login against the current window.
    pub(crate) fn record_failure(&mut self) {
        self.failures = match self.failures {
            Some((count, since)) if since.elapsed() < LOGIN_WINDOW => Some((count + 1, since)),
            _ => Some((1, Instant::now())),
        };
    }

    /// End the current window.
    pub(crate) fn reset(&mut self) {
        self.failures = None;
    }
}

impl Engine {
    /// Log in with the password and start a session.
    pub async fn login(&self, password: &str) -> Result<Token> {
        if self.is_demo() {
            tracing::info!("logged in");
            return Ok(self.sessions().create());
        }
        // The lock spans the whole sequence so that concurrent guesses are counted before they are
        // checked, and so that a password change cannot revoke sessions while a login is in flight.
        let mut throttle = self.login_throttle().await;
        if let Some(retry_after) = throttle.locked_for() {
            return Err(Error::Throttled { retry_after });
        }
        let Some(stored) = self.stored_password_hash().await? else {
            throttle.record_failure();
            return Err(Error::WrongPassword);
        };
        if !verify_password(stored, password.to_owned()).await? {
            tracing::warn!("rejected a login with the wrong password");
            throttle.record_failure();
            return Err(Error::WrongPassword);
        }
        throttle.reset();
        tracing::info!("logged in");
        Ok(self.sessions().create())
    }

    /// Get the stored password hash, or `None` if the instance is unclaimed.
    async fn stored_password_hash(&self) -> Result<Option<String>> {
        let mut conn = self.begin_read().await?;
        Ok(
            sqlx::query_scalar!("SELECT password_hash FROM password WHERE id = 1")
                .fetch_optional(&mut *conn)
                .await?,
        )
    }

    /// Check whether the token is a live session.
    pub fn is_authenticated(&self, token: &Token) -> bool {
        self.sessions().validate(token)
    }

    /// End the session with this token.
    pub fn logout(&self, token: &Token) {
        self.sessions().revoke(token);
        tracing::info!("logged out");
    }

    pub async fn password_state(&self) -> Result<PasswordState> {
        if self.is_demo() {
            return Ok(PasswordState::NotNeeded);
        }
        let mut conn = self.begin_read().await?;
        let row = sqlx::query!("SELECT 1 AS \"present!: i64\" FROM password WHERE id = 1")
            .fetch_optional(&mut *conn)
            .await?;
        Ok(match row {
            Some(_) => PasswordState::Claimed,
            None => PasswordState::Unclaimed,
        })
    }

    /// Set the password, claiming the instance if it is unclaimed
    pub async fn replace_password(&self, password: &str) -> Result<()> {
        if self.is_demo() {
            return Err(Error::Generic(eyre::eyre!("the demo has no password")));
        }
        if password.is_empty() {
            return Err(Error::InvalidValue(
                "The password must not be empty.".into(),
            ));
        }
        // Hash before taking the write lock so argon2 does not block other writers.
        let hash = hash_password(password.to_owned()).await?;

        // Hold the login lock through the revocation so that a login in flight cannot issue a
        // session after it.
        let _throttle = self.login_throttle().await;
        let mut tx = self.begin_write().await?;
        let existing = sqlx::query!("SELECT 1 AS \"present!: i64\" FROM password WHERE id = 1")
            .fetch_optional(&mut *tx)
            .await?;
        sqlx::query!(
            "INSERT INTO password (id, password_hash) VALUES (1, ?) \
             ON CONFLICT (id) DO UPDATE SET password_hash = excluded.password_hash",
            hash
        )
        .execute(&mut *tx)
        .await?;
        let action = if existing.is_some() {
            Action::PasswordReset
        } else {
            Action::PasswordClaimed
        };
        tx.record(AuditSource::User, action, None, None)
            .await?
            .commit()
            .await?;
        self.sessions().revoke_all();
        Ok(())
    }

    /// Change the password of a claimed instance and start a session.
    ///
    /// End every other session, so the returned token is the only one that remains.
    pub async fn change_password(&self, new: &str) -> Result<Token> {
        if self.is_demo() {
            return Err(Error::Generic(eyre::eyre!("the demo has no password")));
        }
        if new.is_empty() {
            return Err(Error::InvalidValue(
                "The password must not be empty.".into(),
            ));
        }
        // Hash before taking the write lock so argon2 does not block other writers.
        let hash = hash_password(new.to_owned()).await?;

        // Hold the login lock through the revocation so that a login in flight cannot issue a
        // session after it.
        let mut throttle = self.login_throttle().await;
        let mut tx = self.begin_write().await?;
        let updated = sqlx::query!("UPDATE password SET password_hash = ? WHERE id = 1", hash)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if updated == 0 {
            return Err(Error::Generic(eyre::eyre!("no password is set")));
        }
        tx.record(AuditSource::User, Action::PasswordChanged, None, None)
            .await?
            .commit()
            .await?;
        self.sessions().revoke_all();
        // Failed guesses at the old password say nothing about the new one.
        throttle.reset();
        Ok(self.sessions().create())
    }
}

#[cfg(test)]
mod tests {
    use crate::audit::Action;
    use crate::auth::PasswordState;
    use crate::{Engine, Error};

    #[tokio::test]
    async fn replace_password_records_a_claim_then_a_reset() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), crate::test::options())
            .await
            .unwrap();
        assert_eq!(
            engine.password_state().await.unwrap(),
            PasswordState::Unclaimed
        );

        engine.replace_password("first").await.unwrap();
        assert_eq!(
            engine.password_state().await.unwrap(),
            PasswordState::Claimed
        );
        engine.replace_password("second").await.unwrap();
        assert_eq!(
            engine.password_state().await.unwrap(),
            PasswordState::Claimed
        );

        let actions: Vec<_> = engine
            .audit_log(0, 10)
            .await
            .unwrap()
            .events
            .iter()
            .map(|e| e.action)
            .collect();
        assert_eq!(actions, [Action::PasswordReset, Action::PasswordClaimed]);
    }

    #[tokio::test]
    async fn the_throttle_refuses_a_correct_password() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), crate::test::options())
            .await
            .unwrap();
        for _ in 0..5 {
            assert!(matches!(engine.login("x").await, Err(Error::WrongPassword)));
        }
        engine.replace_password("secret").await.unwrap();
        assert!(matches!(
            engine.login("secret").await,
            Err(Error::Throttled { .. })
        ));
    }

    #[tokio::test]
    async fn replacing_the_password_revokes_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), crate::test::options())
            .await
            .unwrap();
        engine.replace_password("secret").await.unwrap();
        let token = engine.login("secret").await.unwrap();
        assert!(engine.is_authenticated(&token));
        engine.replace_password("other").await.unwrap();
        assert!(!engine.is_authenticated(&token));
    }

    #[tokio::test]
    async fn changing_the_password_keeps_only_the_new_token() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), crate::test::options())
            .await
            .unwrap();
        engine.replace_password("secret").await.unwrap();
        let old = engine.login("secret").await.unwrap();

        let new = engine.change_password("next").await.unwrap();
        assert!(!engine.is_authenticated(&old));
        assert!(engine.is_authenticated(&new));
        assert!(matches!(
            engine.login("secret").await,
            Err(Error::WrongPassword)
        ));
        engine.login("next").await.unwrap();
    }

    #[tokio::test]
    async fn the_throttle_does_not_block_a_password_change() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), crate::test::options())
            .await
            .unwrap();
        engine.replace_password("secret").await.unwrap();
        for _ in 0..5 {
            assert!(matches!(engine.login("x").await, Err(Error::WrongPassword)));
        }
        assert!(matches!(
            engine.login("secret").await,
            Err(Error::Throttled { .. })
        ));

        engine.change_password("next").await.unwrap();
        engine.login("next").await.unwrap();
    }

    #[tokio::test]
    async fn concurrent_logins_are_throttled() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), crate::test::options())
            .await
            .unwrap();
        engine.replace_password("secret").await.unwrap();

        let mut attempts = tokio::task::JoinSet::new();
        for _ in 0..10 {
            let engine = engine.clone();
            attempts.spawn(async move { engine.login("wrong").await });
        }
        let (mut wrong, mut throttled) = (0, 0);
        while let Some(result) = attempts.join_next().await {
            match result.unwrap() {
                Err(Error::WrongPassword) => wrong += 1,
                Err(Error::Throttled { .. }) => throttled += 1,
                other => panic!("unexpected login result: {other:?}"),
            }
        }
        assert_eq!((wrong, throttled), (5, 5));
    }
}
