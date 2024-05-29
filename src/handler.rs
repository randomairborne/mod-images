use askama_axum::Template;
use axum::{
    body::Bytes,
    extract::{Path, State},
    Json,
};
use serde::Serialize;

use crate::{AppState, Error};

#[derive(Template)]
#[template(path = "index.hbs", ext = "html", escape = "html")]
pub struct Index;

pub async fn index() -> Index {
    Index
}

#[derive(Template)]
#[template(path = "view.hbs", ext = "html", escape = "html")]
pub struct View {
    img_srcs: Vec<String>,
}

pub async fn view(State(state): State<AppState>, Path(id): Path<String>) -> Result<View, Error> {
    let bucket_listing = state.bucket.list(format!("{id}/"), None).await?;
    let mut img_srcs = Vec::new();
    for listing in bucket_listing {
        img_srcs.reserve(listing.contents.len());
        for file in listing.contents {
            let url = state.bucket.presign_get(file.key, 10, None)?;
            img_srcs.push(url);
        }
    }
    if img_srcs.is_empty() {
        return Err(Error::NotFound);
    }
    Ok(View { img_srcs })
}

#[derive(Serialize)]
pub struct Upload {
    id: String,
}

pub async fn upload(State(state): State<AppState>, body: Bytes) -> Result<Json<Upload>, Error> {
    crate::upload::upload(&state, body)
        .await
        .map(|id| Json(Upload { id }))
}
