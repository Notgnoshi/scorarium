use scorarium_engine::entity::{self, Entity, ProposalId, Source};

/// All proposals suggesting a given value
pub(crate) struct Candidate<V> {
    pub value: V,
    /// The id and source of each proposal, oldest first
    pub proposals: Vec<(ProposalId, Source)>,
}

impl<V> Candidate<V> {
    /// Join the display names of the sources, such as "Open Library, DNB"
    pub fn sources(&self) -> String {
        let names: Vec<_> = self.proposals.iter().map(|(_, s)| s.name()).collect();
        names.join(", ")
    }
}

pub(crate) fn candidates<E: Entity>(
    proposals: Vec<entity::Proposal<E>>,
) -> Vec<Candidate<E::Proposal>> {
    let mut grouped: Vec<Candidate<E::Proposal>> = Vec::new();
    for proposal in proposals {
        let entry = (proposal.id, proposal.source);
        match grouped.iter_mut().find(|c| c.value == proposal.value) {
            Some(candidate) => candidate.proposals.push(entry),
            None => grouped.push(Candidate {
                value: proposal.value,
                proposals: vec![entry],
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
            })
        })
        .collect()
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
        let grouped = candidates(vec![
            proposal("1", "Erik Satie"),
            proposal("2", "Eric Satie"),
            proposal("3", "Erik Satie"),
        ]);
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
