use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{self, PasswordHasher, PasswordVerifier};
use sqlx::SqliteConnection;

use crate::Result;

/// The result of verifying a password
#[derive(Debug, PartialEq, Eq)]
pub enum PasswordCheck {
    Unclaimed,
    Wrong,
    Correct,
}

pub(crate) fn hash(password: &str) -> Result<String> {
    let hash = Argon2::default().hash_password(password.as_bytes())?;
    Ok(hash.to_string())
}

pub(crate) fn verify(stored: &str, password: &str) -> Result<PasswordCheck> {
    let parsed = PasswordHash::new(stored)?;
    match Argon2::default().verify_password(password.as_bytes(), &parsed) {
        Ok(()) => Ok(PasswordCheck::Correct),
        Err(password_hash::Error::PasswordInvalid) => Ok(PasswordCheck::Wrong),
        Err(err) => Err(err.into()),
    }
}

pub(crate) async fn stored_hash(conn: &mut SqliteConnection) -> Result<Option<String>> {
    let row = sqlx::query!("SELECT password_hash FROM password WHERE id = 1")
        .fetch_optional(conn)
        .await?;
    Ok(row.map(|row| row.password_hash))
}

/// Store a hash where there is none. False when one was already stored.
pub(crate) async fn insert_hash(conn: &mut SqliteConnection, hash: &str) -> Result<bool> {
    let result = sqlx::query!(
        "INSERT INTO password (id, password_hash) VALUES (1, ?) ON CONFLICT DO NOTHING",
        hash
    )
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Replace a stored hash. Does not upsert.
pub(crate) async fn update_hash(conn: &mut SqliteConnection, hash: &str) -> Result<()> {
    sqlx::query!("UPDATE password SET password_hash = ? WHERE id = 1", hash)
        .execute(conn)
        .await?;
    Ok(())
}
