pub mod enrich;
pub mod archive {
    pub use scorarium_archive::{
        Accepted, Action, Archive, AuditEntry, AuditSubject, CatalogNumber, CatalogNumberEntry,
        Contributor, ContributorInput, DraftPublication, DraftWorkSummary, Entity, EntityRef,
        HoldingErrors, HoldingKind, HoldingRawInput, IdentifierRawInput, Library, Lookup, NotFound,
        PasswordCheck, PendingImport, Person, PersonErrors, PersonName, PersonRawInput, PersonRef,
        PersonSummary, Publication, PublicationErrors, PublicationPost, PublicationRawInput,
        SuggestField, Suggested, Suggestion, ValidationError, Work, WorkErrors, WorkPost,
        WorkRawInput, WorkRef, WorkSummary, identifier, parse_holdings, same_name,
    };
}

pub mod client {
    pub use scorarium_client::open_library::WorkHit;
    pub use scorarium_client::{Client, UserAgent};
}
