use std::collections::{HashMap, HashSet};

use askama::Template;
use chrono::Utc;
use scorarium_engine::Engine;
use scorarium_engine::entity::{AnyEntity, AnyEntityId, Contributed, Entity, Timestamp};
use scorarium_engine::person::{Person, PersonId};
use scorarium_engine::publication::{self, Publication};
use scorarium_engine::review::ReviewItem;
use scorarium_engine::work::{self, Work, WorkId};

use crate::error::Result;
use crate::hit::Hit;

#[derive(Template)]
#[template(path = "widget/review_queue.html")]
pub(crate) struct ReviewQueue {
    count: usize,
    /// The top-level entries, arranged in display order
    entries: Vec<ReviewEntry>,
}

impl ReviewQueue {
    /// Read the review queue and arrange it
    pub async fn new(engine: &Engine) -> Result<ReviewQueue> {
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
        let entries = queue
            .iter()
            .rev()
            .filter(|item| !placed.contains(&item.entity.id()))
            .map(|item| {
                let nested = carried
                    .get(&item.entity.id())
                    .into_iter()
                    .flatten()
                    .map(|id| ReviewEntry::new(drafts[id], Vec::new()))
                    .collect();
                ReviewEntry::new(item, nested)
            })
            .collect();
        Ok(ReviewQueue { count, entries })
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
    proposals: usize,
    queued_at: Timestamp,
    /// The children draft works and draft people that the entry carries
    nested: Vec<ReviewEntry>,
}

impl ReviewEntry {
    fn new(item: &ReviewItem, nested: Vec<ReviewEntry>) -> ReviewEntry {
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
            proposals: item.proposals,
            queued_at: item.queued_at,
            nested,
        }
    }

    fn kind_label(&self) -> &'static str {
        crate::icon::Icon::kind(self.id.kind(), None).label()
    }

    fn age(&self) -> String {
        relative_age(self.queued_at, Utc::now())
    }
}

impl askama::filters::HtmlSafe for ReviewEntry {}

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

        let queue = ReviewQueue::new(&app.engine).await.unwrap();

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
}
