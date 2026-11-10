use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use askama::Template;
use chrono::Utc;
use scorarium_engine::Engine;
use scorarium_engine::enrich::{Enrichment, EnrichmentState, EnrichmentStatus};
use scorarium_engine::entity::{AnyEntity, AnyEntityId, Contributed, Entity, Timestamp};
use scorarium_engine::person::{Person, PersonId};
use scorarium_engine::publication::{self, Publication};
use scorarium_engine::review::ReviewItem;
use scorarium_engine::work::{self, Work, WorkId};

use crate::error::Result;
use crate::hit::Hit;
use crate::sort::{self, Column as _};

#[derive(Template)]
#[template(path = "widget/review_queue.html")]
pub(crate) struct ReviewQueue {
    count: usize,
    sort: Sort,
    /// The top-level entries, arranged in display order
    entries: Vec<ReviewEntry>,
}

/// A column of the review queue that can be sorted
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Column {
    /// The time since the entry was queued, which is the default order
    #[default]
    Age,
    Entry,
    Library,
    Enrichment,
    Proposals,
}

impl sort::Column for Column {
    const ALL: &'static [Self] = &[
        Column::Age,
        Column::Entry,
        Column::Library,
        Column::Enrichment,
        Column::Proposals,
    ];

    fn name(self) -> &'static str {
        match self {
            Column::Age => "age",
            Column::Entry => "entry",
            Column::Library => "library",
            Column::Enrichment => "enrichment",
            Column::Proposals => "proposals",
        }
    }

    fn descending_first(self) -> bool {
        self == Column::Proposals
    }
}

pub(crate) type Sort = sort::Sort<Column>;

/// Write the query string that draws a queue in the given order again
///
/// The widget appends this to every route it posts to, because the URL of the page that embeds the
/// queue does not necessarily carry the queue's order.
pub(crate) fn query(sort: &Sort) -> String {
    let direction = if sort.descending { "-" } else { "" };
    format!("sort={direction}{}", sort.column.name())
}

impl ReviewQueue {
    /// Report whether any top-level entry has nested entries that can be collapsed
    fn has_nested(&self) -> bool {
        self.entries.iter().any(|entry| !entry.nested.is_empty())
    }

    /// Read the review queue, arrange it, and sort its top-level entries
    pub async fn new(engine: &Engine, sort: Sort) -> Result<ReviewQueue> {
        let count = engine.review_count().await?;
        let queue = engine.review_queue().await?;
        let drafts: HashMap<AnyEntityId, &ReviewItem> = queue
            .iter()
            .filter(|item| is_nestable(item))
            .map(|item| (item.entity.id(), item))
            .collect();

        // Finding what an entry carries costs queries, and only drafts can be carried.
        let mut carried = HashMap::new();
        if !drafts.is_empty() {
            for item in queue.iter().filter(|item| !is_nestable(item)) {
                carried.insert(item.entity.id(), nested_ids(item, &drafts).await?);
            }
        }

        // A draft that no entry carries stays at the top level, so that the queue never hides a draft.
        let placed: HashSet<AnyEntityId> = carried.values().flatten().copied().collect();
        let query = query(&sort);
        let mut entries: Vec<ReviewEntry> = queue
            .iter()
            .rev()
            .filter(|item| !placed.contains(&item.entity.id()))
            .map(|item| {
                let nested = carried
                    .get(&item.entity.id())
                    .into_iter()
                    .flatten()
                    .map(|id| ReviewEntry::new(drafts[id], Vec::new(), &query))
                    .collect();
                ReviewEntry::new(item, nested, &query)
            })
            .collect();
        // The sort is stable, so that ties keep the default order.
        entries.sort_by(|a, b| a.compare(b, &sort));
        Ok(ReviewQueue {
            count,
            sort,
            entries,
        })
    }
}

impl askama::filters::HtmlSafe for ReviewQueue {}

/// One entry, either top-level or nested
#[derive(Template)]
#[template(path = "widget/review_entry.html")]
pub(crate) struct ReviewEntry {
    id: AnyEntityId,
    /// The thumbnail, title, and secondary text, linked to the edit page
    hit: Hit,
    draft: bool,
    private: bool,
    enrichment: EnrichmentCell,
    proposals: usize,
    queued_at: Timestamp,
    /// The children draft works and draft people that the entry carries
    nested: Vec<ReviewEntry>,
    /// The query string that keeps the queue's order across the entry's routes
    query: String,
}

impl ReviewEntry {
    fn new(item: &ReviewItem, nested: Vec<ReviewEntry>, query: &str) -> ReviewEntry {
        let (library, draft) = match &item.entity {
            AnyEntity::Publication(publication) => (publication.library(), publication.is_draft()),
            AnyEntity::Work(work) => (work.library(), work.is_draft()),
            AnyEntity::Person(person) => (person.library(), person.is_draft()),
        };
        let mut hit = Hit::new(library, item.library_name.clone(), &item.entity);
        hit.href.push_str("/edit");
        ReviewEntry {
            id: item.entity.id(),
            hit,
            draft,
            private: item.private,
            enrichment: EnrichmentCell::new(
                item.entity.id(),
                item.enrichment.as_ref().map(Enrichment::status),
                query,
            ),
            proposals: item.proposals,
            queued_at: item.queued_at,
            nested,
            query: query.to_owned(),
        }
    }

    /// Order two top-level entries by the sorted column
    fn compare(&self, other: &ReviewEntry, sort: &Sort) -> Ordering {
        let descending = sort.descending;
        match sort.column {
            Column::Age => {
                // Ascending age is newest first, so the queued time compares in reverse.
                sort::compare(Some(other.queued_at), Some(self.queued_at), descending)
            }
            Column::Entry => sort::compare(
                Some(self.hit.title.to_lowercase()),
                Some(other.hit.title.to_lowercase()),
                descending,
            ),
            Column::Library => sort::compare(
                Some(self.hit.library_name.to_lowercase()),
                Some(other.hit.library_name.to_lowercase()),
                descending,
            ),
            Column::Enrichment => {
                sort::compare(self.enrichment.rank(), other.enrichment.rank(), descending)
            }
            Column::Proposals => sort::compare(
                Some(self.all_proposals()),
                Some(other.all_proposals()),
                descending,
            ),
        }
    }

    /// Count the entry's own proposals and those of its nested entries
    fn all_proposals(&self) -> usize {
        self.proposals + self.nested.iter().map(|n| n.proposals).sum::<usize>()
    }

    /// Summarize the nested entries that a collapsed entry hides, such as "30 draft works, 5
    /// draft people"
    fn hidden(&self) -> String {
        let works = self
            .nested
            .iter()
            .filter(|n| matches!(n.id, AnyEntityId::Work(_)))
            .count();
        let people = self
            .nested
            .iter()
            .filter(|n| matches!(n.id, AnyEntityId::Person(_)))
            .count();
        let mut parts = Vec::new();
        if works > 0 {
            parts.push(format!(
                "{works} draft {}",
                if works == 1 { "work" } else { "works" }
            ));
        }
        if people > 0 {
            parts.push(format!(
                "{people} draft {}",
                if people == 1 { "person" } else { "people" }
            ));
        }
        parts.join(", ")
    }

    fn kind_label(&self) -> &'static str {
        crate::icon::Icon::kind(self.id.kind(), None).label()
    }

    fn age(&self) -> String {
        relative_age(self.queued_at, Utc::now())
    }
}

impl askama::filters::HtmlSafe for ReviewEntry {}

#[derive(Template)]
#[template(path = "widget/enrichment.html")]
pub(crate) struct EnrichmentCell {
    entity: AnyEntityId,
    state: Option<EnrichmentState>,
    /// The count beside the icon, such as "2 of 5"
    text: String,
    action: Option<Action>,
    /// The query string that keeps the queue's order across the cell's routes
    query: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Cancel,
    Retry,
}

impl Action {
    fn route(self) -> &'static str {
        match self {
            Action::Cancel => "cancel",
            Action::Retry => "retry",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Action::Cancel => "Cancel",
            Action::Retry => "Retry",
        }
    }
}

impl EnrichmentCell {
    pub fn new(
        entity: AnyEntityId,
        status: Option<EnrichmentStatus>,
        query: &str,
    ) -> EnrichmentCell {
        let blank = EnrichmentCell {
            entity,
            state: None,
            text: String::new(),
            action: None,
            query: query.to_owned(),
        };
        let Some(status) = status else {
            return blank;
        };
        let total = status.successful + status.pending + status.failed;
        if total == 0 {
            return blank;
        }
        let state = status.state();
        let (text, action) = match state {
            EnrichmentState::InProgress => {
                let mut text = format!("{} of {total}", status.successful + status.failed);
                if status.failed > 0 {
                    text.push_str(&format!(", {} failed", status.failed));
                }
                (text, Some(Action::Cancel))
            }
            EnrichmentState::Failed => (
                format!("{} of {total} failed", status.failed),
                Some(Action::Retry),
            ),
            EnrichmentState::Cancelled => ("Cancelled".to_owned(), Some(Action::Retry)),
            EnrichmentState::Completed => (format!("{total} of {total}"), None),
        };
        EnrichmentCell {
            state: Some(state),
            text,
            action,
            ..blank
        }
    }

    /// Rank the state for sorting, with a blank cell having no rank
    fn rank(&self) -> Option<u8> {
        self.state.as_ref().map(|state| match state {
            EnrichmentState::Failed => 0,
            EnrichmentState::Cancelled => 1,
            EnrichmentState::InProgress => 2,
            EnrichmentState::Completed => 3,
        })
    }
}

impl askama::filters::HtmlSafe for EnrichmentCell {}

fn relative_age(queued_at: Timestamp, now: Timestamp) -> String {
    let age = now - queued_at;
    if age.num_days() > 0 {
        format!("{}d", age.num_days())
    } else if age.num_hours() > 0 {
        format!("{}h", age.num_hours())
    } else if age.num_minutes() > 0 {
        format!("{}m", age.num_minutes())
    } else {
        "now".to_owned()
    }
}

fn is_nestable(item: &ReviewItem) -> bool {
    match &item.entity {
        AnyEntity::Publication(_) => false,
        AnyEntity::Work(work) => work.is_draft(),
        AnyEntity::Person(person) => person.is_draft(),
    }
}

async fn nested_ids(
    item: &ReviewItem,
    drafts: &HashMap<AnyEntityId, &ReviewItem>,
) -> Result<Vec<AnyEntityId>> {
    let proposed = item.proposals > 0;
    let (works, mut people) = match &item.entity {
        AnyEntity::Publication(publication) => publication_carries(publication, proposed).await?,
        AnyEntity::Work(work) => (Vec::new(), work_people(work, proposed).await?),
        AnyEntity::Person(_) => (Vec::new(), Vec::new()),
    };

    let mut nested = Vec::new();
    for id in works.into_iter().map(AnyEntityId::Work) {
        if let Some(draft) = drafts.get(&id)
            && let AnyEntity::Work(work) = &draft.entity
            && !nested.contains(&id)
        {
            nested.push(id);
            // Only nest one-level deep because arbitrary depth is too overwhelming
            people.extend(work_people(work, draft.proposals > 0).await?);
        }
    }

    let mut draft_people: Vec<&Person> = Vec::new();
    for id in people.into_iter().map(AnyEntityId::Person) {
        if let Some(AnyEntity::Person(person)) = drafts.get(&id).map(|item| &item.entity)
            && !draft_people.iter().any(|other| other.id() == person.id())
        {
            draft_people.push(person);
        }
    }
    draft_people.sort_by(|a, b| a.sort_name().cmp(b.sort_name()));
    nested.extend(
        draft_people
            .iter()
            .map(|person| AnyEntityId::Person(person.id())),
    );
    Ok(nested)
}

/// List the works and the people that a publication contains, and those it proposes if `proposed`
async fn publication_carries(
    publication: &Publication,
    proposed: bool,
) -> Result<(Vec<WorkId>, Vec<PersonId>)> {
    let mut works: Vec<WorkId> = publication.works().await?.iter().map(Work::id).collect();
    let mut people: Vec<PersonId> = publication
        .contributors()
        .iter()
        .map(|contributor| contributor.person)
        .collect();
    let proposals = if proposed {
        publication.proposals().await?
    } else {
        Vec::new()
    };
    for proposal in proposals {
        match proposal.value {
            publication::Proposal::Contents(work) => works.push(work.id()),
            publication::Proposal::Contributor { person, .. } => people.push(person.id()),
            _ => {}
        }
    }
    Ok((works, people))
}

/// List the people of a work's contributors, and of its proposed contributors if `proposed`
async fn work_people(work: &Work, proposed: bool) -> Result<Vec<PersonId>> {
    let mut people: Vec<PersonId> = work
        .contributors()
        .iter()
        .map(|contributor| contributor.person)
        .collect();
    let proposals = if proposed {
        work.proposals().await?
    } else {
        Vec::new()
    };
    for proposal in proposals {
        if let work::Proposal::Contributor { person, .. } = proposal.value {
            people.push(person.id());
        }
    }
    Ok(people)
}

#[cfg(test)]
mod tests {
    use scorarium_engine::entity::{Contributed, Entity};
    use scorarium_engine::holding::NewHolding;
    use scorarium_engine::publication::ImportSeed;
    use scorarium_test::TestApp;

    use super::*;

    #[tokio::test]
    async fn a_shared_draft_is_nested_beneath_each_publication() {
        let app = TestApp::logged_out(false).await;
        let library = app
            .engine
            .create_library("Sheet music", false)
            .await
            .unwrap();
        let import = async |title: &str| {
            let seed = ImportSeed::Title {
                title: title.into(),
                record: None,
            };
            let holding = NewHolding::Physical { location: None };
            library.import(seed, holding).await.unwrap()
        };
        let mut first = import("Gymnopedies").await;
        let mut second = import("Piano Classics").await;
        let mut work = first.create_work("Gymnopedie No. 1").await.unwrap();
        let person = work
            .create_contributor("Erik Satie", "composer".parse().unwrap())
            .await
            .unwrap();
        second.add_work(work.id()).await.unwrap();

        let queue = ReviewQueue::new(&app.engine, Sort::default())
            .await
            .unwrap();

        let nested = vec![
            AnyEntityId::Work(work.id()),
            AnyEntityId::Person(person.id()),
        ];
        let arranged: Vec<(AnyEntityId, Vec<AnyEntityId>)> = queue
            .entries
            .iter()
            .map(|entry| (entry.id, entry.nested.iter().map(|n| n.id).collect()))
            .collect();
        assert_eq!(
            arranged,
            vec![
                (AnyEntityId::Publication(second.id()), nested.clone()),
                (AnyEntityId::Publication(first.id()), nested),
            ]
        );
    }

    #[test]
    fn the_cell_reports_progress_and_offers_the_right_action() {
        let status = |successful, pending, failed, cancelled| EnrichmentStatus {
            successful,
            pending,
            failed,
            cancelled,
        };
        let entity = AnyEntityId::Publication("1".parse().unwrap());
        for (status, text, action) in [
            (status(2, 3, 0, false), "2 of 5", Some(Action::Cancel)),
            (
                status(2, 2, 1, false),
                "3 of 5, 1 failed",
                Some(Action::Cancel),
            ),
            (status(4, 0, 1, false), "1 of 5 failed", Some(Action::Retry)),
            (status(1, 0, 2, true), "Cancelled", Some(Action::Retry)),
            (status(5, 0, 0, false), "5 of 5", None),
            (status(0, 0, 0, false), "", None),
        ] {
            let cell = EnrichmentCell::new(entity, Some(status), "");
            assert_eq!(
                (cell.text.as_str(), cell.action),
                (text, action),
                "{status:?}"
            );
        }
    }
}
