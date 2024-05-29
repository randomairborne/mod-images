use std::net::SocketAddr;

use askama_axum::Template;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use axum_extra::routing::RouterExt;
use oauth2::{
    basic::BasicErrorResponseType, HttpClientError, RequestTokenError, StandardErrorResponse,
};
use rand::{distributions::Alphanumeric, Rng};
use tokio::net::TcpListener;
use tower_http::{compression::CompressionLayer, services::ServeDir};
use tracing::Level;

pub use crate::state::AppState;

mod auth;
mod handler;
mod interact;
mod signature_validation;
mod state;
mod upload;

#[macro_use]
extern crate tracing;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(Level::TRACE)
        .json()
        .init();
    let state = AppState::new().await;

    interact::register_commands(&state)
        .await
        .expect("Failed to register commands");

    let app = router(state);

    let bind_address = SocketAddr::from(([0, 0, 0, 0], 8080));
    info!(%bind_address, "Binding to address");
    let tcp = TcpListener::bind(bind_address).await.unwrap();
    info!(%bind_address, "Server listening on socket");
    axum::serve(tcp, app)
        .with_graceful_shutdown(vss::shutdown_signal())
        .await
        .unwrap();
}

pub fn router(state: AppState) -> Router {
    let serve_dir = ServeDir::new(AppState::asset_dir())
        .append_index_html_on_directories(false)
        .precompressed_br()
        .precompressed_deflate()
        .precompressed_gzip()
        .precompressed_zstd();

    let mut router = Router::new()
        .route("/", get(handler::index))
        .route("/upload", post(handler::upload));
    let auth = axum::middleware::from_fn_with_state(state.clone(), auth::middleware);

    if std::env::var("PUBLICLY_READABLE").is_ok_and(check_truthy) {
        router = router
            .layer(auth)
            .route_with_tsr("/:id", get(handler::view))
    } else {
        router = router
            .route_with_tsr("/:id", get(handler::view))
            .layer(auth)
    }

    router
        .route("/oauth2/callback", get(auth::authenticate))
        .route("/interactions", post(handler::interaction))
        .nest_service("/assets", serve_dir)
        .layer(CompressionLayer::new())
        .with_state(state)
}

fn check_truthy(data: String) -> bool {
    let d = data.to_ascii_lowercase();
    !(d == "f" || d == "false" || d == "0" || d == "n" || d == "no")
}

type CodeExchangeFailure =
    RequestTokenError<reqwest::Error, StandardErrorResponse<BasicErrorResponseType>>;

type RequestTokenFailure = RequestTokenError<
    HttpClientError<reqwest::Error>,
    StandardErrorResponse<BasicErrorResponseType>,
>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("S3 error")]
    S3(#[from] s3::error::S3Error),
    #[error("Redis error")]
    Redis(#[from] redis::RedisError),
    #[error("HTTP error")]
    Http(#[from] reqwest::Error),
    #[error("Discord API HTTP error")]
    DiscordApiRequestValidate(#[from] twilight_validate::request::ValidationError),
    #[error("Discord API HTTP error")]
    DiscordApiHttp(#[from] twilight_http::Error),
    #[error("Discord API model error")]
    DiscordApiDeserializeModel(#[from] twilight_http::response::DeserializeBodyError),
    #[error("Templating error")]
    Askama(#[from] askama::Error),
    #[error("JSON error")]
    Json(#[from] serde_json::Error),
    #[error("Image load error")]
    Image(#[from] image::ImageError),
    #[error("OAuth2 token error")]
    OAuth2RequestToken(#[from] RequestTokenFailure),
    #[error("OAuth2 URL parse error")]
    OAuth2Url(#[from] oauth2::url::ParseError),
    #[error("Join error")]
    Join(#[from] tokio::task::JoinError),
    #[error("OAuth2 Code Exchange failed")]
    CodeExchangeFailed(#[from] CodeExchangeFailure),
    #[error("Missing required header with name {0}")]
    MissingHeader(&'static str),
    #[error("Failed to extract secure interaction")]
    InvalidSignature(#[from] signature_validation::ExtractFailure),
    #[error("Invalid OAuth2 State")]
    InvalidState,
    #[error("You do not have the required role to access this application")]
    NoPermissions,
    #[error("You must authenticate to use this application")]
    Unauthorized,
    #[error("404 Page Not Found")]
    NotFound,
    #[error("Discord did not send CommandData!")]
    MissingCommandData,
    #[error("Missing target ID")]
    MissingTarget,
    #[error("Missing discord resolved data")]
    NoResolvedData,
    #[error("Message not sent in resolved data")]
    MessageNotFound,
}

#[derive(Template)]
#[template(path = "error.hbs", ext = "html", escape = "html")]
struct ErrorTemplate {
    error: Error,
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = self.status();
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            error!(source = ?self, "Error handling request");
        } else {
            debug!(source = ?self, "Failed to handle request");
        }

        let err_resp = IntoResponse::into_response(ErrorTemplate { error: self });

        (status, err_resp).into_response()
    }
}

impl Error {
    fn status(&self) -> StatusCode {
        match self {
            Error::S3(_)
            | Error::Redis(_)
            | Error::Http(_)
            | Error::DiscordApiRequestValidate(_)
            | Error::DiscordApiHttp(_)
            | Error::DiscordApiDeserializeModel(_)
            | Error::Askama(_)
            | Error::Json(_)
            | Error::Join(_)
            | Error::OAuth2Url(_)
            | Error::OAuth2RequestToken(_)
            | Error::MissingCommandData
            | Error::MissingTarget
            | Error::NoResolvedData
            | Error::MessageNotFound => StatusCode::INTERNAL_SERVER_ERROR,
            Error::InvalidState
            | Error::CodeExchangeFailed(_)
            | Error::Image(_)
            | Error::MissingHeader(_) => StatusCode::BAD_REQUEST,
            Error::NoPermissions => StatusCode::FORBIDDEN,
            Error::Unauthorized | Error::InvalidSignature(_) => StatusCode::UNAUTHORIZED,
            Error::NotFound => StatusCode::NOT_FOUND,
        }
    }
}

pub fn randstring(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}
