use std::io::Write;
use std::time::SystemTime;

use scorarium_engine::entity::{AnyEntity, Entity, ProposalId, Timestamp};
use scorarium_engine::{Engine, Error};

use crate::enrichment;
use crate::library::write_table;
use crate::search::describe;

/// List the review queue
pub(crate) async fn run(engine: &Engine, out: &mut impl Write) -> eyre::Result<()> {
    let now = Timestamp::from(SystemTime::now());
    let rows: Vec<[String; 8]> = engine
        .review_queue()
        .await?
        .into_iter()
        .map(|item| {
            let (kind, id, label) = describe(&item.entity);
            let seconds = now.signed_duration_since(item.queued_at).num_seconds();
            let draft = if is_draft(&item.entity) { "draft" } else { "" };
            let enrichment = item
                .enrichment
                .map(|e| enrichment::status(e.status()))
                .unwrap_or_default();
            let proposals = match item.proposals {
                0 => String::new(),
                n => n.to_string(),
            };
            [
                item.library_name,
                kind.to_string(),
                id,
                label,
                draft.to_string(),
                enrichment,
                proposals,
                age(seconds),
            ]
        })
        .collect();
    write_table(
        out,
        [
            "library",
            "kind",
            "id",
            "title",
            "draft",
            "enrichment",
            "proposals",
            "age",
        ],
        &rows,
    )
}

/// Remove an entity from the review queue
pub(crate) async fn discard_changes<E: Entity>(
    entity: &E,
    kind: &str,
    reference: &str,
) -> eyre::Result<()> {
    let Some(item) = entity.review_item().await? else {
        eyre::bail!("{kind} '{reference}' is not in the review queue");
    };
    item.discard().await?;
    Ok(())
}

pub(crate) async fn accept<E: Entity>(
    entity: &mut E,
    proposals: &[ProposalId],
) -> eyre::Result<()> {
    for &id in proposals {
        entity
            .accept(id)
            .await
            .map_err(|e| unknown_proposal(e, id))?;
    }
    Ok(())
}

pub(crate) async fn reject<E: Entity>(
    entity: &mut E,
    proposals: &[ProposalId],
) -> eyre::Result<()> {
    for &id in proposals {
        entity
            .reject(id)
            .await
            .map_err(|e| unknown_proposal(e, id))?;
    }
    Ok(())
}

fn unknown_proposal(error: Error, id: ProposalId) -> eyre::Report {
    match error {
        Error::NotFound => eyre::eyre!("no proposal '{id}'"),
        e => e.into(),
    }
}

fn is_draft(entity: &AnyEntity) -> bool {
    match entity {
        AnyEntity::Publication(e) => e.is_draft(),
        AnyEntity::Work(e) => e.is_draft(),
        AnyEntity::Person(e) => e.is_draft(),
    }
}

fn age(seconds: i64) -> String {
    match seconds {
        ..=0 => "0s".to_string(),
        1..60 => format!("{seconds}s"),
        60..3600 => format!("{}m", seconds / 60),
        3600..86400 => format!("{}h", seconds / 3600),
        _ => format!("{}d", seconds / 86400),
    }
}
