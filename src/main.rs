#![warn(clippy::all, clippy::nursery, clippy::pedantic)]
use std::{net::SocketAddr, sync::Arc};

use askama_axum::Template;
use axum::{
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, RequestExt, Router,
};
use axum_extra::routing::RouterExt;
use oauth2::{
    basic::BasicErrorResponseType, HttpClientError, RequestTokenError, StandardErrorResponse,
};
use rand::{distributions::Alphanumeric, Rng};
use tokio::net::TcpListener;
use tower_http::{compression::CompressionLayer, services::ServeDir};
use tower_sombrero::{
    csp::CspNonce,
    headers::{ContentSecurityPolicy, CspSource},
    Sombrero,
};
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

    let csp = ContentSecurityPolicy::strict_default()
        .script_src([
            CspSource::Nonce,
            CspSource::StrictDynamic,
            CspSource::UnsafeInline,
        ])
        .style_src(CspSource::Nonce)
        .base_uri(CspSource::None)
        .img_src([CspSource::Host(state.bucket.url()), CspSource::SelfOrigin]);
    let sombrero = Sombrero::default().content_security_policy(csp);

    let router = Router::new()
        .route("/", get(handler::index))
        .route("/upload", post(handler::upload));
    let auth = axum::middleware::from_fn_with_state(state.clone(), auth::middleware);

    let router = if std::env::var("PUBLICLY_READABLE")
        .as_deref()
        .is_ok_and(check_truthy)
    {
        router
            .layer(auth)
            .route_with_tsr("/:id", get(handler::view))
    } else {
        router
            .route_with_tsr("/:id", get(handler::view))
            .layer(auth)
    };

    router
        .route("/oauth2/callback", get(auth::authenticate))
        .route("/interactions", post(handler::interaction))
        .nest_service("/assets", serve_dir)
        .layer(CompressionLayer::new())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            error_middleware,
        ))
        .layer(sombrero)
        .with_state(state)
}

fn check_truthy(data: &str) -> bool {
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
    #[error("WebP reported an unusual error: {0}")]
    WebPStr(String),
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
    root_url: Arc<str>,
    error: Arc<Error>,
    nonce: String,
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = self.status();
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            error!(source = ?self, "Error handling request");
        } else {
            debug!(source = ?self, "Failed to handle request");
        }

        (status, Extension(Arc::new(self)), Body::empty()).into_response()
    }
}

impl Error {
    const fn status(&self) -> StatusCode {
        match self {
            Self::S3(_)
            | Self::Redis(_)
            | Self::Http(_)
            | Self::DiscordApiRequestValidate(_)
            | Self::DiscordApiHttp(_)
            | Self::DiscordApiDeserializeModel(_)
            | Self::Askama(_)
            | Self::Json(_)
            | Self::Join(_)
            | Self::OAuth2Url(_)
            | Self::OAuth2RequestToken(_)
            | Self::WebPStr(_)
            | Self::MissingCommandData
            | Self::MissingTarget
            | Self::NoResolvedData
            | Self::MessageNotFound => StatusCode::INTERNAL_SERVER_ERROR,
            Self::InvalidState
            | Self::CodeExchangeFailed(_)
            | Self::Image(_)
            | Self::MissingHeader(_) => StatusCode::BAD_REQUEST,
            Self::NoPermissions => StatusCode::FORBIDDEN,
            Self::Unauthorized | Self::InvalidSignature(_) => StatusCode::UNAUTHORIZED,
            Self::NotFound => StatusCode::NOT_FOUND,
        }
    }
}

async fn error_middleware(State(state): State<AppState>, mut req: Request, next: Next) -> Response {
    let nonce = match req.extract_parts::<CspNonce>().await {
        Ok(CspNonce(n)) => n,
        Err(err) => return err.into_response(),
    };
    let resp = next.run(req).await;
    if let Some(error) = resp.extensions().get::<Arc<Error>>().cloned() {
        let status = error.status();
        let error = ErrorTemplate {
            root_url: state.root_url,
            error,
            nonce,
        };
        (status, error).into_response()
    } else {
        resp
    }
}

pub fn randstring(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}
