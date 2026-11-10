use askama::Template;
use axum::Extension;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use scorarium_engine::Engine;
use scorarium_engine::entity::Entity;
use scorarium_engine::library::{Library, LibraryId};
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

#[derive(Deserialize)]
pub(crate) struct Form {
    text: String,
}

#[derive(Template)]
#[template(path = "page/library/import.html")]
struct Import {
    base: Base,
    library: LibraryId,
    text: String,
    holdings: Holdings,
    error: Option<String>,
}

fn render(
    header: Header,
    library: &Library,
    text: String,
    holdings: Holdings,
    status: StatusCode,
    error: Option<String>,
) -> Result<Response> {
    let parents = vec![
        Crumb::new("Libraries", "/"),
        Crumb::new(library.name(), format!("/library/{}", library.id())),
    ];
    let page = Import {
        base: Base::new(header, "Import a publication", parents),
        library: library.id(),
        text,
        holdings,
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
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    let holdings = Holdings::empty(library.id(), settings.files_allowed(&engine));
    render(
        header,
        &library,
        String::new(),
        holdings,
        StatusCode::OK,
        None,
    )
}

/// Read the typed text as an identifier if it is one, and as a title otherwise
fn seed(text: &str) -> ImportSeed {
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
    let mut draft = library.import(seed(text), first.as_new()).await?;
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
    match import(&engine, &library, &form.text, &holdings).await {
        Ok(edit) => Ok(Redirect::to(&edit).into_response()),
        Err(e) => {
            let (status, error) = form_error(e)?;
            let empty = Holdings::empty(library.id(), settings.files_allowed(&engine));
            let holdings = empty.resubmitted(&engine, &holdings);
            render(header, &library, form.text, holdings, status, Some(error))
        }
    }
}
