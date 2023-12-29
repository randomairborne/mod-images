use axum::{response::Response, routing::get, Router};
use rand::{distributions::Alphanumeric, Rng};
use reqwest::StatusCode;
pub use state::AppState;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod auth;
mod handler;
mod state;

#[tokio::main]
async fn main() {
    start_tracing();
    let state = AppState::new().await;
    let app = Router::new()
        .route("/", get(handler::index))
        .route("/:id", get(handler::view))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::middleware,
        ))
        .route("/oauth2", get(auth::redirect))
        .route("/oauth2/callback", get(auth::authenticate))
        .with_state(state);
    axum::Server::bind(&([0, 0, 0, 0], 8080).into())
        .serve(app.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("S3 error")]
    S3(#[from] s3::error::S3Error),
    #[error("Redis error")]
    Redis(#[from] redis::RedisError),
    #[error("Redis pool error")]
    DeadpoolRedis(#[from] deadpool_redis::PoolError),
    #[error("HTTP error")]
    Http(#[from] reqwest::Error),
    #[error("Templating error")]
    Tera(#[from] tera::Error),
    #[error("Invalid OAuth2 State")]
    InvalidState,
    #[error("OAuth2 Code Exchange failed")]
    CodeExchangeFailed,
    #[error("You do not have the required role to access this application")]
    NoPermissions,
    #[error("You must authenticate to use this application")]
    Unauthorized,
}

impl axum::response::IntoResponse for Error {
    fn into_response(self) -> Response {
        (self.status(), self.to_string()).into_response()
    }
}

impl Error {
    fn status(&self) -> StatusCode {
        match self {
            Error::S3(_)
            | Error::Redis(_)
            | Error::DeadpoolRedis(_)
            | Error::Http(_)
            | Error::Tera(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Error::InvalidState | Error::CodeExchangeFailed => StatusCode::BAD_REQUEST,
            Error::NoPermissions => StatusCode::FORBIDDEN,
            Error::Unauthorized => StatusCode::UNAUTHORIZED,
        }
    }
}

async fn shutdown_signal() {
    #[cfg(target_family = "unix")]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut interrupt = signal(SignalKind::interrupt()).expect("Failed to listen to sigint");
        let mut quit = signal(SignalKind::quit()).expect("Failed to listen to sigquit");
        let mut terminate = signal(SignalKind::terminate()).expect("Failed to listen to sigterm");

        tokio::select! {
            _ = interrupt.recv() => {},
            _ = quit.recv() => {},
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(target_family = "unix"))]
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen to ctrl+c");
}

pub fn start_tracing() {
    let env_filter = tracing_subscriber::EnvFilter::builder()
        .with_default_directive(concat!(env!("CARGO_PKG_NAME"), "=info").parse().unwrap())
        .with_env_var("LOG")
        .from_env()
        .expect("failed to parse env");
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(env_filter)
        .init();
}

pub fn randstring(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}
