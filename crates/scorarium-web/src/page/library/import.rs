use askama::Template;
use axum::Extension;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use scorarium_engine::Engine;
use scorarium_engine::entity::{Entity, EntityKind};
use scorarium_engine::library::{Library, LibraryId};
use scorarium_engine::link::{ExternalRecord, HttpUrl};
use scorarium_engine::publication::{Identifier, IdentifierKind, ImportSeed};
use serde::Deserialize;

use crate::Settings;
use crate::auth::Session;
use crate::error::Result;
use crate::forms::Forms;
use crate::page::library::view;
use crate::page::{Base, Crumb, form_error};
use crate::widget::header::Header;
use crate::widget::holdings::{AddKind, AddedHolding, Holdings, HoldingsForm};
use crate::widget::review_queue::{ReviewQueue, Size};
use crate::widget::typeahead::Typeahead;

#[derive(Deserialize)]
pub(crate) struct Form {
    #[serde(default)]
    text: String,
    #[serde(default)]
    record: String,
    more: Option<String>,
}

impl Form {
    /// Get the picked candidate's record, if the form carries a recognized one
    fn record(&self) -> Option<(HttpUrl, ExternalRecord)> {
        let url: HttpUrl = self.record.parse().ok()?;
        let record = ExternalRecord::recognize(EntityKind::Publication, &url)?;
        Some((url, record))
    }
}

#[derive(Template)]
#[template(path = "page/library/import.html")]
struct Import {
    base: Base,
    library: LibraryId,
    text: Typeahead,
    record: Option<HttpUrl>,
    more: bool,
    holdings: Holdings,
    queue: ReviewQueue,
    error: Option<String>,
}

/// Draw the page with the form as submitted
async fn render(
    engine: &Engine,
    header: Header,
    library: &Library,
    form: Form,
    holdings: Holdings,
    error: Option<(StatusCode, String)>,
) -> Result<Response> {
    let parents = vec![
        Crumb::new("Libraries", "/"),
        Crumb::new(library.name(), format!("/library/{}", library.id())),
    ];
    let (status, error) = match error {
        Some((status, message)) => (status, Some(message)),
        None => (StatusCode::OK, None),
    };
    let queue = ReviewQueue::new(engine, Size::Short, Some(library.id())).await?;
    let record = form.record().map(|(url, _)| url);
    let page = Import {
        base: Base::new(header, "Import a publication", parents),
        library: library.id(),
        text: Typeahead {
            id: "text".to_owned(),
            name: "text".to_owned(),
            url: "/api/import/candidates".to_owned(),
            placeholder: "978-0-486-25978-9".to_owned(),
            value: form.text,
            picked: Some((
                "record",
                record.as_ref().map(ToString::to_string).unwrap_or_default(),
            )),
            pick_required: None,
            attrs: vec![
                ("required", String::new()),
                ("autofocus", String::new()),
                ("data-no-default-pick", String::new()),
            ],
        },
        record,
        more: form.more.is_some(),
        holdings,
        queue,
        error,
    };
    Ok((status, Html(page.render()?)).into_response())
}

pub async fn get(
    _session: Session,
    State(engine): State<Engine>,
    Extension(settings): Extension<Settings>,
    header: Header,
    Path(library): Path<String>,
    Forms(form, holdings): Forms<Form, HoldingsForm>,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    let empty = Holdings::empty(library.id(), settings.files_allowed(&engine));
    let holdings = empty.resubmitted(&engine, &holdings);
    render(&engine, header, &library, form, holdings, None).await
}

/// Seed the draft with the picked candidate if there is one, and otherwise read the typed text as
/// an identifier if it is one, and as a title if not
fn seed(text: &str, record: Option<ExternalRecord>) -> ImportSeed {
    if record.is_some() {
        return ImportSeed::Title {
            title: text.to_owned(),
            record,
        };
    }
    let identifier = [IdentifierKind::Ismn, IdentifierKind::Isbn]
        .into_iter()
        .find_map(|kind| Identifier::new(kind, text).ok());
    match identifier {
        Some(identifier) => ImportSeed::Identifier {
            identifier,
            record: None,
        },
        None => ImportSeed::Title {
            title: text.to_owned(),
            record: None,
        },
    }
}

/// List the holdings to import, which defaults to one physical holding with no location
fn holdings_to_add(holdings: &HoldingsForm) -> scorarium_engine::Result<Vec<AddedHolding>> {
    let mut added = holdings.added();
    if added.is_empty() {
        if holdings.kind() == AddKind::Digital {
            return Err(scorarium_engine::Error::InvalidValue(
                "Add a file, or switch to Physical.".to_owned(),
            ));
        }
        added.push(AddedHolding::Physical {
            location: String::new(),
        });
    }
    Ok(added)
}

/// Create the draft, and return the path of its edit page
async fn import(
    engine: &Engine,
    library: &Library,
    text: &str,
    record: Option<ExternalRecord>,
    holdings: &HoldingsForm,
) -> scorarium_engine::Result<String> {
    let added = holdings_to_add(holdings)?;
    let gone = added
        .iter()
        .any(|holding| matches!(holding, AddedHolding::Digital(id) if engine.staged(id).is_none()));
    if gone {
        return Err(scorarium_engine::Error::InvalidValue(
            "A file is no longer available. Choose it again.".to_owned(),
        ));
    }
    let (first, rest) = added.split_first().expect("holdings_to_add is not empty");
    let mut draft = library.import(seed(text, record), first.as_new()).await?;
    // The draft exists by now, so a failure to add a later holding is logged and not shown. The
    // edit page can add the holding again.
    for holding in rest {
        if let Err(e) = draft.add_holding(holding.as_new()).await {
            tracing::error!("Failed to add a holding to draft {}: {e}", draft.id());
        }
    }
    Ok(format!(
        "/library/{}/publication/{}/edit",
        library.id(),
        draft.id()
    ))
}

pub async fn post(
    _session: Session,
    State(engine): State<Engine>,
    Extension(settings): Extension<Settings>,
    header: Header,
    Path(library): Path<String>,
    Forms(form, holdings): Forms<Form, HoldingsForm>,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    let record = form.record().map(|(_, record)| record);
    match import(&engine, &library, &form.text, record, &holdings).await {
        Ok(_) if form.more.is_some() => {
            // The next import keeps the holdings but not the files, because a file belongs to one
            // publication.
            let more = format!(
                "/library/{}/import?more=on&{}",
                library.id(),
                holdings.query_without_staged()
            );
            Ok(Redirect::to(&more).into_response())
        }
        Ok(edit) => Ok(Redirect::to(&edit).into_response()),
        Err(e) => {
            let (status, error) = form_error(e)?;
            let empty = Holdings::empty(library.id(), settings.files_allowed(&engine));
            let holdings = empty.resubmitted(&engine, &holdings);
            render(
                &engine,
                header,
                &library,
                form,
                holdings,
                Some((status, error)),
            )
            .await
        }
    }
}
