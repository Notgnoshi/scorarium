use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{self, PasswordHasher, PasswordVerifier};
use sqlx::SqlitePool;

use crate::db;

#[derive(Debug, PartialEq, Eq)]
pub enum PasswordCheck {
    Unclaimed,
    Wrong,
    Correct,
}

/// Claim the password.
///
/// Does not modify the stored password if it has already been claimed.
pub async fn claim_password(pool: &SqlitePool, password: &str) -> color_eyre::Result<bool> {
    let hash = hash_password(password)?;
    Ok(db::insert_password_hash(pool, &hash).await?)
}

/// Replace a stored password.
///
/// The caller is responsible for verifying the current password first; a no-op when the password is
/// unclaimed.
pub async fn change_password(pool: &SqlitePool, password: &str) -> color_eyre::Result<()> {
    let hash = hash_password(password)?;
    db::update_password_hash(pool, &hash).await?;
    Ok(())
}

/// Check a login attempt against the stored password.
pub async fn verify_password(
    pool: &SqlitePool,
    password: &str,
) -> color_eyre::Result<PasswordCheck> {
    let Some(stored) = db::get_password_hash(pool).await? else {
        return Ok(PasswordCheck::Unclaimed);
    };
    let parsed = PasswordHash::new(&stored)?;
    match Argon2::default().verify_password(password.as_bytes(), &parsed) {
        Ok(()) => Ok(PasswordCheck::Correct),
        Err(password_hash::Error::PasswordInvalid) => Ok(PasswordCheck::Wrong),
        Err(err) => Err(err.into()),
    }
}

fn hash_password(password: &str) -> color_eyre::Result<String> {
    let hash = Argon2::default().hash_password(password.as_bytes())?;
    Ok(hash.to_string())
}

#[cfg(test)]
mod tests {
    use sqlx::SqlitePool;

    use super::*;

    #[sqlx::test]
    async fn test_first_claim_wins(pool: SqlitePool) {
        assert!(claim_password(&pool, "hunter2").await.unwrap());
        assert!(!claim_password(&pool, "usurper").await.unwrap());

        let check = verify_password(&pool, "hunter2").await.unwrap();
        assert_eq!(check, PasswordCheck::Correct);
    }

    #[sqlx::test]
    async fn test_wrong_password(pool: SqlitePool) {
        let check = verify_password(&pool, "hunter2").await.unwrap();
        assert_eq!(check, PasswordCheck::Unclaimed);

        claim_password(&pool, "hunter2").await.unwrap();
        let check = verify_password(&pool, "wrong").await.unwrap();
        assert_eq!(check, PasswordCheck::Wrong);
        let check = verify_password(&pool, "hunter2").await.unwrap();
        assert_eq!(check, PasswordCheck::Correct);
    }

    #[sqlx::test]
    async fn test_change_password(pool: SqlitePool) {
        claim_password(&pool, "hunter2").await.unwrap();
        change_password(&pool, "hunter3").await.unwrap();

        let check = verify_password(&pool, "hunter2").await.unwrap();
        assert_eq!(check, PasswordCheck::Wrong);
        let check = verify_password(&pool, "hunter3").await.unwrap();
        assert_eq!(check, PasswordCheck::Correct);
    }
}
