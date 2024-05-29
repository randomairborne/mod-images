use axum::body::Bytes;

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
    let webp = tokio::task::spawn_blocking(move || convert_image(image)).await??;
    trace!("Encoded JPEG, uploading");
    state
        .bucket
        .put_object_with_content_type(format!("{id}/{seq}.jpeg"), &webp, "image/webp")
        .await?;
    Ok(())
}

#[instrument(skip_all)]
pub fn convert_image(data: Bytes) -> Result<Vec<u8>, Error> {
    let image = image::load_from_memory(&data)?;
    trace!("Loaded image");
    let encoder = webp::Encoder::from_image(&image).map_err(|e| Error::WebPStr(e.to_string()))?;
    let bytes = encoder.encode(80.0).to_vec();
    Ok(bytes)
}
