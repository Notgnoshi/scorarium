use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use scorarium_engine::Engine;
use scorarium_engine::enrich::Enrichment;
use scorarium_engine::entity::{AnyEntity, Entity, EntityKind};
use scorarium_engine::person::Person;
use scorarium_engine::publication::Publication;
use scorarium_engine::review::ReviewItem;
use scorarium_engine::work::Work;
use serde::Deserialize;

use crate::auth::ApiSession;
use crate::error::{Error, Result};
use crate::widget::confirm::{Confirm, count};
use crate::widget::review_queue::{self, ReviewQueue, Sort};

#[derive(Default, Deserialize)]
pub(crate) struct ReviewQuery {
    pub(crate) sort: Option<Sort>,
}

#[derive(Deserialize)]
pub struct DiscardForm {
    confirmed: Option<String>,
}

pub async fn cancel(
    _session: ApiSession,
    State(engine): State<Engine>,
    Path((kind, entity)): Path<(String, String)>,
    Query(query): Query<ReviewQuery>,
) -> Result<Response> {
    if let Some(entity) = load(&engine, &kind, &entity).await?
        && let Some(enrichment) = enrichment_of(&entity)
    {
        enrichment.cancel();
    }
    queue(&engine, query).await
}

pub async fn retry(
    _session: ApiSession,
    State(engine): State<Engine>,
    Path((kind, entity)): Path<(String, String)>,
    Query(query): Query<ReviewQuery>,
) -> Result<Response> {
    match load(&engine, &kind, &entity).await? {
        Some(AnyEntity::Publication(publication)) => publication.enrich().await.map(drop)?,
        Some(AnyEntity::Work(work)) => work.enrich().await.map(drop)?,
        Some(AnyEntity::Person(person)) => person.enrich().await.map(drop)?,
        None => {}
    }
    queue(&engine, query).await
}

/// Ask before discarding an entry, then discard it
pub async fn discard(
    _session: ApiSession,
    State(engine): State<Engine>,
    Path((kind, entity)): Path<(String, String)>,
    Query(query): Query<ReviewQuery>,
    axum::Form(form): axum::Form<DiscardForm>,
) -> Result<Response> {
    let Some(entity) = load(&engine, &kind, &entity).await? else {
        return queue(&engine, query).await;
    };
    let Some(item) = (match &entity {
        AnyEntity::Publication(publication) => publication.review_item().await?,
        AnyEntity::Work(work) => work.review_item().await?,
        AnyEntity::Person(person) => person.review_item().await?,
    }) else {
        return queue(&engine, query).await;
    };

    let case = if is_draft(&item.entity) {
        "draft"
    } else if item.proposals > 0 {
        "proposals"
    } else {
        // Only the task keeps this entry in the queue, so there is nothing to confirm.
        return finish(&engine, item, query).await;
    };
    if form.confirmed.as_deref() == Some(case) {
        return finish(&engine, item, query).await;
    }
    let dialog = dialog(&item, case, &query).await?;
    Ok(Html(dialog.to_string()).into_response())
}

async fn finish(engine: &Engine, item: ReviewItem, query: ReviewQuery) -> Result<Response> {
    match item.discard().await {
        Ok(()) | Err(scorarium_engine::Error::NotFound) => queue(engine, query).await,
        Err(e) => Err(e.into()),
    }
}

fn is_draft(entity: &AnyEntity) -> bool {
    match entity {
        AnyEntity::Publication(publication) => publication.is_draft(),
        AnyEntity::Work(work) => work.is_draft(),
        AnyEntity::Person(person) => person.is_draft(),
    }
}

async fn dialog(item: &ReviewItem, case: &'static str, query: &ReviewQuery) -> Result<Confirm> {
    let subject = match &item.entity {
        AnyEntity::Publication(publication) => untitled(publication),
        AnyEntity::Work(work) => work.title(),
        AnyEntity::Person(person) => person.name(),
    };
    let (title, message, also_deleted, action) = if case == "draft" {
        let mut also_deleted = Vec::new();
        let message = match &item.entity {
            AnyEntity::Publication(_) => {
                also_deleted = vec![
                    "its digital files".to_owned(),
                    "orphaned works".to_owned(),
                    "orphaned people".to_owned(),
                ];
                format!("Discarding {subject} also deletes:")
            }
            AnyEntity::Work(work) => {
                let publications = work.publications().await?;
                let titles = publications.iter().map(untitled);
                join(titles).map_or_else(String::new, |t| format!("It is removed from {t}"))
            }
            AnyEntity::Person(person) => {
                let works = person.works().await?;
                let publications = person.publications().await?;
                let titles = works
                    .iter()
                    .map(Work::title)
                    .chain(publications.iter().map(untitled));
                join(titles).map_or_else(String::new, |t| format!("They are removed from {t}"))
            }
        };
        (
            format!("Discard {subject}?"),
            message,
            also_deleted,
            "Discard draft",
        )
    } else {
        (
            "Reject all proposals?".to_owned(),
            format!(
                "This rejects {} for {subject}",
                count(item.proposals, "edit proposal")
            ),
            vec![],
            "Reject proposals",
        )
    };
    Ok(Confirm {
        id: "review-discard".to_owned(),
        title,
        message,
        also_deleted,
        action,
        url: format!(
            "/api/review/{}/{}/discard?{}",
            item.entity.id().kind(),
            item.entity.id().get(),
            review_queue::query(&query.sort.clone().unwrap_or_default()),
        ),
        target: Some("#review-queue-dialog".to_owned()),
        fields: vec![("confirmed", case.to_owned())],
    })
}

fn untitled(publication: &Publication) -> &str {
    publication.title().unwrap_or("Untitled")
}

/// Join titles for a sentence, or return `None` when there are none
fn join<'a>(titles: impl Iterator<Item = &'a str>) -> Option<String> {
    let titles: Vec<&str> = titles.collect();
    (!titles.is_empty()).then(|| titles.join(", "))
}

/// Return the review queue widget fragment as a response
async fn queue(engine: &Engine, query: ReviewQuery) -> Result<Response> {
    let sort = query.sort.unwrap_or_default();
    let page = Html(ReviewQueue::new(engine, sort).await?.to_string());
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
