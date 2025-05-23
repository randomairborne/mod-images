use std::sync::Arc;

use askama::Template;
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use serde::Serialize;
use tower_sombrero::csp::CspNonce;
use twilight_model::{
    http::interaction::InteractionResponse,
    id::{marker::ApplicationMarker, Id},
};

use crate::{
    signature_validation::{SIGNATURE_HEADER, TIMESTAMP_HEADER},
    AppState, Error, TemplateWrapper,
};

#[derive(Template)]
#[template(path = "index.hbs", escape = "html")]
pub struct Index {
    root_url: Arc<str>,
    application_id: Id<ApplicationMarker>,
    nonce: String,
}

pub async fn index(
    State(state): State<AppState>,
    CspNonce(nonce): CspNonce,
) -> TemplateWrapper<Index> {
    TemplateWrapper(Index {
        root_url: state.root_url,
        application_id: state.discord.application_id,
        nonce,
    })
}

#[derive(Template)]
#[template(path = "view.hbs", escape = "html")]
pub struct View {
    root_url: Arc<str>,
    img_srcs: Vec<String>,
    application_id: Id<ApplicationMarker>,
    nonce: String,
}

pub async fn view(
    State(state): State<AppState>,
    Path(id): Path<String>,
    CspNonce(nonce): CspNonce,
) -> Result<TemplateWrapper<View>, Error> {
    let bucket_listing = state.bucket.list(format!("{id}/"), None).await?;
    let mut img_srcs = Vec::new();
    for listing in bucket_listing {
        img_srcs.reserve(listing.contents.len());
        for file in listing.contents {
            let url = state.bucket.presign_get(file.key, 10, None).await?;
            img_srcs.push(url);
        }
    }
    if img_srcs.is_empty() {
        return Err(Error::NotFound);
    }
    Ok(TemplateWrapper(View {
        root_url: state.root_url,
        img_srcs,
        application_id: state.discord.application_id,
        nonce,
    }))
}

#[derive(Serialize)]
pub struct Upload {
    id: String,
}

pub async fn upload(State(state): State<AppState>, body: Bytes) -> Result<Json<Upload>, Error> {
    crate::upload::upload(state, body)
        .await
        .map(|id| Json(Upload { id }))
}

pub async fn interaction(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<InteractionResponse>, Error> {
    let signature = headers
        .get(SIGNATURE_HEADER)
        .ok_or(Error::MissingHeader(SIGNATURE_HEADER))?;
    let timestamp = headers
        .get(TIMESTAMP_HEADER)
        .ok_or(Error::MissingHeader(TIMESTAMP_HEADER))?;

    let interaction = crate::signature_validation::extract_interaction(
        signature.as_bytes(),
        timestamp.as_bytes(),
        body.as_ref(),
        &state.discord.verify_key,
    )?;

    let response = crate::interact::interact(state, interaction).await;
    Ok(Json(response))
}
