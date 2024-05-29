use std::io::Cursor;

use axum::body::Bytes;
use image::ImageFormat;

use crate::{AppState, Error};

pub async fn upload(state: AppState, image: Bytes) -> Result<String, Error> {
    let id = crate::randstring(16);
    upload_raw(state, &id, 0, image).await?;
    Ok(id)
}

pub async fn upload_raw(state: AppState, id: &str, seq: u64, image: Bytes) -> Result<(), Error> {
    let jpeg = tokio::task::spawn_blocking(move || convert_image(image)).await??;
    state
        .bucket
        .put_object_with_content_type(format!("{id}/{seq}.jpeg"), &jpeg, "image/jpeg")
        .await?;
    Ok(())
}

pub fn convert_image(data: Bytes) -> Result<Vec<u8>, Error> {
    let image = image::load_from_memory(&data)?;
    let mut output = Vec::new();
    let mut output_cursor = Cursor::new(&mut output);
    image.write_to(&mut output_cursor, ImageFormat::Jpeg)?;
    Ok(output)
}
