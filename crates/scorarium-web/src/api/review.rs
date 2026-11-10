use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use scorarium_engine::Engine;
use scorarium_engine::enrich::Enrichment;
use scorarium_engine::entity::{AnyEntity, Entity, EntityKind};
use scorarium_engine::person::Person;
use scorarium_engine::publication::Publication;
use scorarium_engine::work::Work;

use crate::auth::ApiSession;
use crate::error::{Error, Result};
use crate::widget::review_queue::ReviewQueue;

pub async fn cancel(
    _session: ApiSession,
    State(engine): State<Engine>,
    Path((kind, entity)): Path<(String, String)>,
) -> Result<Response> {
    if let Some(entity) = load(&engine, &kind, &entity).await?
        && let Some(enrichment) = enrichment_of(&entity)
    {
        enrichment.cancel();
    }
    queue(&engine).await
}

pub async fn retry(
    _session: ApiSession,
    State(engine): State<Engine>,
    Path((kind, entity)): Path<(String, String)>,
) -> Result<Response> {
    match load(&engine, &kind, &entity).await? {
        Some(AnyEntity::Publication(publication)) => publication.enrich().await.map(drop)?,
        Some(AnyEntity::Work(work)) => work.enrich().await.map(drop)?,
        Some(AnyEntity::Person(person)) => person.enrich().await.map(drop)?,
        None => {}
    }
    queue(&engine).await
}

/// Return the review queue widget fragment as a response
async fn queue(engine: &Engine) -> Result<Response> {
    let page = Html(ReviewQueue::new(engine).await?.to_string());
    Ok((
        [("HX-Retarget", "#review-queue"), ("HX-Reswap", "outerHTML")],
        page,
    )
        .into_response())
}

fn enrichment_of(entity: &AnyEntity) -> Option<Enrichment> {
    match entity {
        AnyEntity::Publication(publication) => publication.enrichment(),
        AnyEntity::Work(work) => work.enrichment(),
        AnyEntity::Person(person) => person.enrichment(),
    }
}

async fn load(engine: &Engine, kind: &str, id: &str) -> Result<Option<AnyEntity>> {
    let not_found =
        |e: &dyn std::fmt::Display| Error::new(StatusCode::NOT_FOUND, eyre::eyre!("{e}"));
    let kind: EntityKind = kind.parse().map_err(|e| not_found(&e))?;
    Ok(match kind {
        EntityKind::Publication => {
            let id = id.parse().map_err(|e| not_found(&e))?;
            Publication::load(engine, id)
                .await?
                .map(AnyEntity::Publication)
        }
        EntityKind::Work => {
            let id = id.parse().map_err(|e| not_found(&e))?;
            Work::load(engine, id).await?.map(AnyEntity::Work)
        }
        EntityKind::Person => {
            let id = id.parse().map_err(|e| not_found(&e))?;
            Person::load(engine, id).await?.map(AnyEntity::Person)
        }
    })
}
