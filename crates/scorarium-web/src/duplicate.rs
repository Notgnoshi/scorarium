use scorarium_engine::duplicate::DuplicateReason;

pub(crate) fn describe_reasons(reasons: &[DuplicateReason]) -> String {
    reasons
        .iter()
        .map(|reason| match reason {
            DuplicateReason::Identifier => "same identifier",
            DuplicateReason::Title => "same title",
            DuplicateReason::Name => "same name",
            DuplicateReason::Record => "same record",
            DuplicateReason::CatalogNumber => "same catalog number",
        })
        .collect::<Vec<_>>()
        .join(", ")
}
