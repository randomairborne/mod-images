use axum::{extract::State, response::Html};
use serde::Serialize;
use tera::Context;

use crate::{AppState, Error};

#[derive(Serialize)]
pub struct Index {}

pub async fn index(State(state): State<AppState>) -> Result<Html<String>, Error> {
    let ctx = Index {};
    let ctx = Context::from_serialize(ctx)?;
    Ok(Html(state.tera.render("index.jinja", &ctx)?))
}

#[derive(Serialize)]
pub struct View {}

pub async fn view(State(state): State<AppState>) -> Result<Html<String>, Error> {
    let ctx = View {};
    let ctx = Context::from_serialize(ctx)?;
    Ok(Html(state.tera.render("view.jinja", &ctx)?))
}
