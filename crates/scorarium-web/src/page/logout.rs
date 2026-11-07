use axum::extract::State;
use axum::response::{IntoResponse, Redirect};
use scorarium_engine::Engine;

use crate::auth::SessionCookie;

pub async fn post(State(engine): State<Engine>, cookie: SessionCookie) -> impl IntoResponse {
    if let Some(token) = cookie.token() {
        engine.logout(&token);
    }
    (cookie.clear(), Redirect::to("/"))
}
