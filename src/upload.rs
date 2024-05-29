use std::{io::Cursor, sync::Arc};

use axum::body::Bytes;
use image::ImageFormat;

use crate::{AppState, Error};

pub async fn upload(state: &AppState, body: Bytes) -> Result<String, Error> {
    let jpeg = tokio::task::spawn_blocking(move || convert_image(body)).await??;
    let id = crate::randstring(16);
    state
        .bucket
        .put_object_with_content_type(format!("{id}/0.jpeg"), &jpeg, "image/jpeg")
        .await?;
    Ok(id)
}

pub fn convert_image(data: Bytes) -> Result<Vec<u8>, Error> {
    let image = image::load_from_memory(&data)?;
    let mut output = Vec::new();
    let mut output_cursor = Cursor::new(&mut output);
    image.write_to(&mut output_cursor, ImageFormat::Jpeg)?;
    Ok(output)
}
