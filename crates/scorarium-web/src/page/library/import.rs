use askama::Template;
use axum::Extension;
use axum::extract::{Path, State};
use axum::http::{StatusCode, Uri};
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
use crate::forms::FormBody;
use crate::page::library::view;
use crate::page::{Base, Crumb, form_error};
use crate::redirect::back_query;
use crate::widget::header::Header;
use crate::widget::holdings::{AddedHolding, Holdings, HoldingsForm};
use crate::widget::refusals::Refusals;
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
    refusals: Refusals,
}

/// Draw the page with the form as submitted
async fn render(
    engine: &Engine,
    header: Header,
    library: &Library,
    form: Form,
    holdings: Holdings,
    opener: Option<&str>,
    refusals: Refusals,
) -> Result<Response> {
    let parents = vec![
        Crumb::new("Libraries", "/"),
        Crumb::new(library.name(), format!("/library/{}", library.id())),
    ];
    let status = if refusals.is_empty() {
        StatusCode::OK
    } else {
        StatusCode::BAD_REQUEST
    };
    let queue = ReviewQueue::new(engine, Size::Short, Some(library.id()), opener).await?;
    let record = form.record().map(|(url, _)| url);
    let mut attrs = vec![
        ("required", String::new()),
        ("data-no-default-pick", String::new()),
    ];
    if refusals.is_empty() {
        attrs.push(("autofocus", String::new()));
    }
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
                "record".to_owned(),
                record.as_ref().map(ToString::to_string).unwrap_or_default(),
            )),
            pick_required: None,
            refusal: None,
            attrs,
        },
        record,
        more: form.more.is_some(),
        holdings,
        queue,
        refusals,
    };
    Ok((status, Html(page.render()?)).into_response())
}

pub async fn get(
    _session: Session,
    State(engine): State<Engine>,
    Extension(settings): Extension<Settings>,
    header: Header,
    uri: Uri,
    Path(library): Path<String>,
    body: FormBody,
) -> Result<Response> {
    let form: Form = body.read()?;
    let mut holdings: HoldingsForm = body.read()?;
    if holdings.is_empty() {
        holdings = HoldingsForm::starting();
    }
    let library = view::load(&engine, &library).await?;
    let refusals = Refusals::default();
    let holdings = Holdings::new(
        &engine,
        library.id(),
        &holdings,
        &refusals,
        settings.files_allowed(&engine),
    );
    render(
        &engine,
        header,
        &library,
        form,
        holdings,
        uri.path_and_query().map(|p| p.as_str()),
        refusals,
    )
    .await
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

/// Create the draft, and return the path of its edit page
async fn import(
    library: &Library,
    text: &str,
    record: Option<ExternalRecord>,
    holdings: &[AddedHolding],
) -> scorarium_engine::Result<String> {
    let (first, rest) = holdings
        .split_first()
        .expect("A parsed form with no refusal adds a holding");
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
    uri: Uri,
    Path(library): Path<String>,
    body: FormBody,
) -> Result<Response> {
    let form: Form = body.read()?;
    let holdings: HoldingsForm = body.read()?;
    let library = view::load(&engine, &library).await?;
    let record = form.record().map(|(_, record)| record);
    let mut refusals = Refusals::default();
    let edits = holdings.parse(&engine, &[], &mut refusals);
    if refusals.is_empty() {
        match import(&library, &form.text, record, &edits.add).await {
            Ok(edit) => {
                // The next import keeps the physical holdings but not the files, because a file
                // belongs to one publication.
                let query = holdings.physical_query();
                let path = if form.more.is_some() {
                    format!("/library/{}/import?more=on&{query}", library.id())
                } else {
                    let opener = format!("/library/{}/import?{query}", library.id());
                    format!("{edit}{}", back_query(&opener))
                };
                return Ok(Redirect::to(&path).into_response());
            }
            Err(e) => {
                let (_, message) = form_error(e)?;
                refusals.refuse(message);
            }
        }
    }
    let holdings = Holdings::new(
        &engine,
        library.id(),
        &holdings,
        &refusals,
        settings.files_allowed(&engine),
    );
    render(
        &engine,
        header,
        &library,
        form,
        holdings,
        uri.path_and_query().map(|p| p.as_str()),
        refusals,
    )
    .await
}
