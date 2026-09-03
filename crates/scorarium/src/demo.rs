use sqlx::SqlitePool;

use crate::db::publication::{HoldingKind, NewPublication};
use crate::db::work::NewWork;
use crate::db::{self, person, publication, work};
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

    // A book with works, so pages show works without any music-specific fields
    let bierce_writings = publication::create_publication(
        pool,
        &NewPublication {
            library_id: books,
            title: "The Collected Writings of Ambrose Bierce",
            publisher: Some("Citadel Press"),
            year: Some(1979),
        },
    )
    .await?;
    publication::create_holding(pool, bierce_writings, HoldingKind::Physical, None).await?;
    add_identifier(pool, bierce_writings, Kind::Isbn, "0-8065-0180-4").await?;
    let bierce = person::create_person(pool, books, "Ambrose Bierce", "Bierce, Ambrose").await?;
    person::create_contributor(pool, books, bierce_writings, bierce, "author").await?;
    for title in [
        "In the Midst of Life",
        "The Devil's Dictionary",
        "The Parenticide Club",
    ] {
        add_work(
            pool,
            bierce_writings,
            (bierce, "author"),
            &NewWork {
                library_id: books,
                title,
                key: None,
                time_signature: None,
                instrumentation: None,
            },
            &[],
        )
        .await?;
    }

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
    let rachmaninoff = person::create_person(
        pool,
        sheet_music,
        "Sergei Rachmaninoff",
        "Rachmaninoff, Sergei",
    )
    .await?;
    // An anthology: its composers are credited on the publication, but only Rachmaninoff's
    // pieces are entered as works.
    for (name, sort_name) in [
        ("Dmitri Kabalevsky", "Kabalevsky, Dmitri"),
        ("Modest Mussorgsky", "Mussorgsky, Modest"),
        ("Sergei Prokofiev", "Prokofiev, Sergei"),
        ("Dmitri Shostakovich", "Shostakovich, Dmitri"),
        ("Pyotr Ilyich Tchaikovsky", "Tchaikovsky, Pyotr Ilyich"),
    ] {
        let composer = person::create_person(pool, sheet_music, name, sort_name).await?;
        person::create_contributor(pool, sheet_music, russian_album, composer, "composer").await?;
    }
    person::create_contributor(pool, sheet_music, russian_album, rachmaninoff, "composer").await?;
    let prelude = add_work(
        pool,
        russian_album,
        (rachmaninoff, "composer"),
        &NewWork {
            library_id: sheet_music,
            title: "Prelude in C-sharp minor",
            key: Some("C-sharp minor"),
            time_signature: None,
            instrumentation: Some("piano"),
        },
        &["Op. 3 No. 2"],
    )
    .await?;
    add_work(
        pool,
        russian_album,
        (rachmaninoff, "composer"),
        &NewWork {
            library_id: sheet_music,
            title: "Etude-Tableau",
            key: Some("A minor"),
            time_signature: None,
            instrumentation: Some("piano"),
        },
        &["Op. 39 No. 2"],
    )
    .await?;

    let masterpieces = publication::create_publication(
        pool,
        &NewPublication {
            library_id: sheet_music,
            title: "Rachmaninoff masterpieces for solo piano",
            publisher: Some("Dover"),
            year: None,
        },
    )
    .await?;
    publication::create_holding(pool, masterpieces, HoldingKind::Physical, None).await?;
    add_identifier(pool, masterpieces, Kind::Isbn, "0-486-43122-3").await?;
    person::create_contributor(pool, sheet_music, masterpieces, rachmaninoff, "composer").await?;
    // The same work in two publications, so work pages list more than one
    work::add_to_publication(pool, sheet_music, masterpieces, prelude).await?;
    add_work(
        pool,
        masterpieces,
        (rachmaninoff, "composer"),
        &NewWork {
            library_id: sheet_music,
            title: "Polichinelle",
            key: Some("F-sharp minor"),
            time_signature: None,
            instrumentation: Some("piano"),
        },
        &["Op. 3 No. 4"],
    )
    .await?;

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
    for (title, key) in [
        ("Gymnopedie No. 1", "D major"),
        ("Gymnopedie No. 2", "C major"),
        ("Gymnopedie No. 3", "A minor"),
    ] {
        add_work(
            pool,
            gymnopedies,
            (satie, "composer"),
            &NewWork {
                library_id: sheet_music,
                title,
                key: Some(key),
                time_signature: Some("3/4"),
                instrumentation: Some("piano"),
            },
            &[],
        )
        .await?;
    }

    Ok(())
}

/// Create a work in a publication with one contributor and its catalog numbers, returning its id.
async fn add_work(
    pool: &SqlitePool,
    publication_id: i64,
    (person_id, role): (i64, &str),
    work: &NewWork<'_>,
    catalog_numbers: &[&str],
) -> color_eyre::Result<i64> {
    let work_id = work::create_work(pool, work).await?;
    work::add_to_publication(pool, work.library_id, publication_id, work_id).await?;
    work::create_contributor(pool, work.library_id, work_id, person_id, role).await?;
    for value in catalog_numbers {
        work::create_catalog_number(pool, work_id, value).await?;
    }
    Ok(work_id)
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
        let orphans = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM work w \
             WHERE NOT EXISTS (SELECT 1 FROM publication_work pw WHERE pw.work_id = w.id)"
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(orphans, 0);
    }
}
