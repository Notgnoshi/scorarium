use sqlx::SqlitePool;

use crate::db::publication::{HoldingKind, NewPublication};
use crate::db::{self, person, publication};
use crate::identifier::{self, Kind};

/// Fill an empty database with the demo libraries.
pub async fn populate(pool: &SqlitePool) -> color_eyre::Result<()> {
    let books = db::create_library(pool, "Books").await?;
    let sheet_music = db::create_library(pool, "Sheet music").await?;

    let practical_vim = publication::create_publication(
        pool,
        &NewPublication {
            library_id: books,
            title: "Practical Vim",
            publisher: Some("Pragmatic Bookshelf"),
            year: Some(2015),
        },
    )
    .await?;
    publication::create_holding(pool, practical_vim, HoldingKind::Physical, Some("Desk")).await?;
    add_identifier(pool, practical_vim, Kind::Isbn, "978-1-68050-127-8").await?;
    let drew_neil = person::create_person(pool, books, "Drew Neil", "Neil, Drew").await?;
    person::create_contributor(pool, books, practical_vim, drew_neil, "author").await?;

    let pro_git = publication::create_publication(
        pool,
        &NewPublication {
            library_id: books,
            title: "Pro Git",
            publisher: Some("Apress"),
            year: Some(2014),
        },
    )
    .await?;
    publication::create_holding(pool, pro_git, HoldingKind::Physical, None).await?;
    publication::create_holding(pool, pro_git, HoldingKind::Digital, Some("pro-git.pdf")).await?;
    add_identifier(pool, pro_git, Kind::Isbn, "978-1-4842-0077-3").await?;
    let scott_chacon = person::create_person(pool, books, "Scott Chacon", "Chacon, Scott").await?;
    person::create_contributor(pool, books, pro_git, scott_chacon, "author").await?;
    let ben_straub = person::create_person(pool, books, "Ben Straub", "Straub, Ben").await?;
    person::create_contributor(pool, books, pro_git, ben_straub, "author").await?;

    let russian_album = publication::create_publication(
        pool,
        &NewPublication {
            library_id: sheet_music,
            title: "Russian piano album",
            publisher: Some("Schirmer"),
            year: None,
        },
    )
    .await?;
    publication::create_holding(pool, russian_album, HoldingKind::Physical, None).await?;
    add_identifier(pool, russian_album, Kind::Isbn, "978-1-4950-0871-9").await?;
    add_identifier(pool, russian_album, Kind::PublisherNumber, "Vol 2115").await?;

    let gymnopedies = publication::create_publication(
        pool,
        &NewPublication {
            library_id: sheet_music,
            title: "Three gymnopedies for the piano",
            publisher: Some("Schirmer"),
            year: None,
        },
    )
    .await?;
    publication::create_holding(
        pool,
        gymnopedies,
        HoldingKind::Physical,
        Some("Piano bench"),
    )
    .await?;
    add_identifier(pool, gymnopedies, Kind::Isbn, "978-0-7935-2590-4").await?;
    add_identifier(pool, gymnopedies, Kind::PublisherNumber, "Vol 1869").await?;
    let satie = person::create_person(pool, sheet_music, "Erik Satie", "Satie, Erik").await?;
    person::create_contributor(pool, sheet_music, gymnopedies, satie, "composer").await?;

    Ok(())
}

async fn add_identifier(
    pool: &SqlitePool,
    publication_id: i64,
    kind: Kind,
    value: &str,
) -> color_eyre::Result<()> {
    let normalized = identifier::normalize(kind, value)?;
    publication::create_identifier(pool, publication_id, kind, &normalized).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[sqlx::test]
    async fn demo_populates(pool: SqlitePool) {
        populate(&pool).await.unwrap();

        let orphans = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM publication p \
             WHERE NOT EXISTS (SELECT 1 FROM holding h WHERE h.publication_id = p.id)"
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(orphans, 0);
    }
}
