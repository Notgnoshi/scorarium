use std::collections::HashMap;

use askama::Template;
use axum::extract::State;
use axum::response::Html;
use scorarium_engine::Engine;
use scorarium_engine::entity::Entity;
use scorarium_engine::holding::HoldingKind;
use scorarium_engine::value::title_sort_key;

use crate::auth::Session;
use crate::error::Result;
use crate::page::Base;
use crate::redirect::back_query;
use crate::widget::breadcrumbs::Crumb;
use crate::widget::header::Header;

#[derive(Template)]
#[template(path = "page/loans.html")]
struct Loans {
    base: Base,
    loans: Vec<Loan>,
}

/// A lent copy and the publication it belongs to
struct Loan {
    borrower: String,
    title: String,
    /// The publication page
    href: String,
    library: String,
    location: Option<String>,
    /// The route that marks the copy as returned and comes back to this page
    return_url: String,
}

pub async fn get(
    _session: Session,
    State(engine): State<Engine>,
    header: Header,
) -> Result<Html<String>> {
    let libraries: HashMap<_, _> = engine
        .libraries()
        .await?
        .into_iter()
        .map(|library| (library.id(), library.name().to_owned()))
        .collect();
    let mut loans = Vec::new();
    for publication in engine.publications_on_loan().await? {
        let href = format!(
            "/library/{}/publication/{}",
            publication.library(),
            publication.id()
        );
        for holding in publication.holdings() {
            let Some(borrower) = &holding.loaned_to else {
                continue;
            };
            let location = match &holding.kind {
                HoldingKind::Physical { location } => location.clone(),
                HoldingKind::Digital { .. } => None,
            };
            loans.push(Loan {
                borrower: borrower.clone(),
                title: publication.title().unwrap_or("Untitled").to_owned(),
                library: libraries
                    .get(&publication.library())
                    .cloned()
                    .unwrap_or_default(),
                location,
                return_url: format!(
                    "{href}/holding/{}/return{}",
                    holding.id,
                    back_query("/settings/loans")
                ),
                href: href.clone(),
            });
        }
    }
    loans.sort_by_cached_key(|loan| (loan.borrower.to_lowercase(), title_sort_key(&loan.title)));
    let page = Loans {
        base: Base::new(
            header,
            "Loans",
            vec![
                Crumb::new("Libraries", "/"),
                Crumb::new("Settings", "/settings"),
            ],
        ),
        loans,
    };
    Ok(Html(page.render()?))
}
