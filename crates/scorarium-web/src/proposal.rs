use std::collections::HashMap;
use std::fmt;

use scorarium_engine::Error;
use scorarium_engine::entity::{self, Entity, ProposalId, Source};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ProposalState {
    #[default]
    Pending,
    Accepted,
    Rejected,
}

impl fmt::Display for ProposalState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ProposalState::Pending => "pending",
            ProposalState::Accepted => "accepted",
            ProposalState::Rejected => "rejected",
        })
    }
}

/// The posted `proposal[<id>]` fields
#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct ProposalStates {
    proposal: HashMap<String, ProposalState>,
    reject_remaining: Option<String>,
}

impl ProposalStates {
    /// Return whether the posted form asked to reject the pending proposals too
    pub fn rejects_remaining(&self) -> bool {
        self.reject_remaining.is_some()
    }

    /// Delete each accepted or rejected proposal of `entity`, and the pending ones if requested
    pub async fn apply<E: Entity>(&self, entity: &mut E) -> scorarium_engine::Result<()> {
        let reject_all = self.rejects_remaining();
        if reject_all && let Some(enrichment) = entity.enrichment() {
            enrichment.cancel();
        }
        for (id, state) in &self.proposal {
            let Ok(id) = id.parse::<ProposalId>() else {
                continue;
            };
            if *state == ProposalState::Pending && !reject_all {
                continue;
            }
            match entity.reject(id).await {
                // Another session may have deleted the proposal first.
                Ok(()) | Err(Error::NotFound) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn get(&self, id: ProposalId) -> ProposalState {
        self.proposal
            .get(&id.to_string())
            .copied()
            .unwrap_or_default()
    }
}

/// All proposals suggesting a given value
pub(crate) struct Candidate<V> {
    pub value: V,
    /// The id and source of each proposal, oldest first
    pub proposals: Vec<(ProposalId, Source)>,
    pub state: ProposalState,
}

impl<V> Candidate<V> {
    /// Join the display names of the sources, such as "Open Library, DNB"
    pub fn sources(&self) -> String {
        let names: Vec<_> = self.proposals.iter().map(|(_, s)| s.name()).collect();
        names.join(", ")
    }
}

/// Group equal values, each in the state that `states` posted for it
pub(crate) fn candidates<E: Entity>(
    proposals: Vec<entity::Proposal<E>>,
    states: &ProposalStates,
) -> Vec<Candidate<E::Proposal>> {
    let mut grouped: Vec<Candidate<E::Proposal>> = Vec::new();
    for proposal in proposals {
        let entry = (proposal.id, proposal.source);
        let state = states.get(proposal.id);
        match grouped.iter_mut().find(|c| c.value == proposal.value) {
            Some(candidate) => {
                candidate.proposals.push(entry);
                if candidate.state == ProposalState::Pending {
                    candidate.state = state;
                }
            }
            None => grouped.push(Candidate {
                value: proposal.value,
                proposals: vec![entry],
                state,
            }),
        }
    }
    grouped
}

/// Keep the candidates that `value` returns something for
pub(crate) fn pick<V, T>(
    candidates: &[Candidate<V>],
    value: impl Fn(&V) -> Option<T>,
) -> Vec<Candidate<T>> {
    candidates
        .iter()
        .filter_map(|c| {
            Some(Candidate {
                value: value(&c.value)?,
                proposals: c.proposals.clone(),
                state: c.state,
            })
        })
        .collect()
}

/// Join the sources of the candidate that holds the first of `ids`
pub(crate) fn sources<V>(candidates: &[Candidate<V>], ids: &[String]) -> Option<String> {
    let first = ids.first()?;
    candidates
        .iter()
        .find(|c| c.proposals.iter().any(|(id, _)| id.to_string() == *first))
        .map(Candidate::sources)
}

#[cfg(test)]
mod tests {
    use scorarium_engine::person::{self, Person};

    use super::*;

    #[test]
    fn candidates_group_equal_values_in_first_seen_order() {
        let proposal = |id: &str, name: &str| entity::Proposal::<Person> {
            id: id.parse().unwrap(),
            source: Source::OpenLibrary,
            created_at: chrono::Utc::now(),
            value: person::Proposal::Name(name.to_owned()),
        };
        let grouped = candidates(
            vec![
                proposal("1", "Erik Satie"),
                proposal("2", "Eric Satie"),
                proposal("3", "Erik Satie"),
            ],
            &ProposalStates::default(),
        );
        let ids = |candidate: &Candidate<person::Proposal>| -> Vec<i64> {
            candidate.proposals.iter().map(|(id, _)| id.get()).collect()
        };
        let summary: Vec<_> = grouped.iter().map(|c| (c.value.clone(), ids(c))).collect();
        assert_eq!(
            summary,
            [
                (person::Proposal::Name("Erik Satie".into()), vec![1, 3]),
                (person::Proposal::Name("Eric Satie".into()), vec![2]),
            ]
        );
    }
}
