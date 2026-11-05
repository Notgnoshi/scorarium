use argon2::Argon2;
use argon2::password_hash::PasswordHasher;

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

impl Engine {
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
            .await
    }
}

#[cfg(test)]
mod tests {
    use crate::Engine;
    use crate::audit::Action;
    use crate::auth::PasswordState;

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
            .audit_log(None, 10)
            .await
            .unwrap()
            .iter()
            .map(|e| e.action)
            .collect();
        assert_eq!(actions, [Action::PasswordReset, Action::PasswordClaimed]);
    }
}
