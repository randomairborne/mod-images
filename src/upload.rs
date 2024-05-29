use std::io::Cursor;

use axum::body::Bytes;
use image::ImageFormat;

use crate::{AppState, Error};

#[instrument(skip_all)]
pub async fn upload(state: AppState, image: Bytes) -> Result<String, Error> {
    let id = crate::randstring(16);
    trace!(id, "Creating single-image upload entry");
    upload_raw(state, &id, 0, image).await?;
    Ok(id)
}

#[instrument(skip(state, image))]
pub async fn upload_raw(state: AppState, id: &str, seq: u64, image: Bytes) -> Result<(), Error> {
    let jpeg = tokio::task::spawn_blocking(move || convert_image(image)).await??;
    trace!("Encoded JPEG, uploading");
    state
        .bucket
        .put_object_with_content_type(format!("{id}/{seq}.jpeg"), &jpeg, "image/jpeg")
        .await?;
    Ok(())
}

#[instrument(skip_all)]
pub fn convert_image(data: Bytes) -> Result<Vec<u8>, Error> {
    let image = image::load_from_memory(&data)?;
    trace!("Loaded image");
    let mut output = Vec::new();
    let mut output_cursor = Cursor::new(&mut output);
    image.write_to(&mut output_cursor, ImageFormat::Jpeg)?;
    Ok(output)
}
