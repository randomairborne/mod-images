use std::{sync::Arc, time::Duration};

use oauth2::{
    basic::{
        BasicClient, BasicErrorResponse, BasicRevocationErrorResponse,
        BasicTokenIntrospectionResponse, BasicTokenResponse,
    },
    AuthUrl, ClientId, ClientSecret, EndpointNotSet, EndpointSet, RedirectUrl, RevocationUrl,
    StandardRevocableToken, TokenUrl,
};
use redis::{aio::MultiplexedConnection, AsyncCommands};
use reqwest::{Client, ClientBuilder};
use s3::{creds::Credentials, Bucket, Region};
use twilight_model::id::{
    marker::{ApplicationMarker, GuildMarker},
    Id,
};
use valk_utils::{get_var, parse_var};

use crate::{signature_validation::Key, Error};

#[derive(Clone)]
#[allow(clippy::module_name_repetitions)]
pub struct AppState {
    pub bucket: Arc<Bucket>,
    pub http: Client,
    pub redis: MultiplexedConnection,
    pub guild: Id<GuildMarker>,
    pub oauth: Arc<OAuth2Client>,
    pub discord: Arc<Discord>,
    pub root_url: Arc<str>,
}

impl AppState {
    pub async fn new() -> Self {
        trace!("Building state");
        let discord = Arc::new(Discord::new().await);
        let root_url = get_root_url();
        Self {
            bucket: get_bucket().into(),
            http: get_http(),
            redis: get_redis().await,
            guild: parse_var("GUILD"),
            oauth: get_oauth(discord.application_id, &root_url).into(),
            discord,
            root_url,
        }
    }

    #[allow(clippy::missing_errors_doc)]
    pub async fn redis_exists(&mut self, key: &str) -> Result<bool, Error> {
        let value: Option<bool> = self.redis.get(key).await?;
        Ok(value.is_some())
    }

    #[must_use]
    pub fn asset_dir() -> String {
        std::env::var("ASSET_DIR").unwrap_or_else(|_v| "./assets/".to_string())
    }

    #[must_use]
    pub fn template_dir() -> String {
        std::env::var("TEMPLATE_DIR").unwrap_or_else(|_v| "./templates/".to_string())
    }
}

fn get_bucket() -> Bucket {
    trace!("Connecting to S3");
    let name: String = parse_var("BUCKET_NAME");
    let endpoint = parse_var("S3_ENDPOINT");
    let region = parse_var("S3_REGION");
    let access_key_id: String = parse_var("S3_ACCESS_KEY_ID");
    let secret_access_key: String = parse_var("S3_SECRET_ACCESS_KEY");
    let region = Region::Custom { region, endpoint };
    let credentials = Credentials::new(
        Some(&access_key_id),
        Some(&secret_access_key),
        None,
        None,
        None,
    )
    .unwrap();
    Bucket::new(&name, region, credentials).unwrap()
}

fn get_http() -> Client {
    ClientBuilder::new()
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION"),
            " (+https://github.com/randomairborne/mod-images)"
        ))
        .build()
        .unwrap()
}

async fn get_redis() -> MultiplexedConnection {
    trace!("Loading redis");
    let url: String = parse_var("REDIS_URL");
    let client = redis::Client::open(url).expect("Could not open redis connection");
    trace!("Loaded redis, testing connection..");
    let mux = client
        .get_multiplexed_tokio_connection_with_response_timeouts(
            Duration::from_secs(5),
            Duration::from_secs(10),
        )
        .await
        .expect("Could not open mux connection");
    trace!("Redis connection succeeded");
    mux
}

type OAuth2Client = oauth2::Client<
    BasicErrorResponse,
    BasicTokenResponse,
    BasicTokenIntrospectionResponse,
    StandardRevocableToken,
    BasicRevocationErrorResponse,
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointSet,
    EndpointSet,
>;

fn get_oauth(id: Id<ApplicationMarker>, root_url: &str) -> OAuth2Client {
    let client_id = ClientId::new(id.to_string());
    let client_secret = ClientSecret::new(parse_var("CLIENT_SECRET"));
    let auth_url = AuthUrl::new("https://discord.com/oauth2/authorize".to_owned()).unwrap();
    let token_url = TokenUrl::new("https://discord.com/api/oauth2/token".to_owned()).unwrap();
    let revocation_url =
        RevocationUrl::new("https://discord.com/api/oauth2/token/revoke".to_owned()).unwrap();
    let redirect_url = RedirectUrl::new(format!("{root_url}/oauth2/callback")).unwrap();
    trace!(?redirect_url, "Built redirect url");
    BasicClient::new(client_id)
        .set_auth_uri(auth_url)
        .set_client_secret(client_secret)
        .set_token_uri(token_url)
        .set_revocation_url(revocation_url)
        .set_redirect_uri(redirect_url)
}

fn get_root_url() -> Arc<str> {
    let root_url = get_var("ROOT_URL");
    let root_url = root_url.trim_end_matches('/');
    root_url.into()
}

pub struct Discord {
    pub client: twilight_http::Client,
    pub application_id: Id<ApplicationMarker>,
    pub verify_key: Key,
}

impl Discord {
    pub async fn new() -> Self {
        let token = get_var("DISCORD_TOKEN");
        let client = twilight_http::Client::new(token);
        let application = client
            .current_user_application()
            .await
            .expect("Failed to get own info")
            .model()
            .await
            .expect("Failed to parse own info");
        Self {
            client,
            application_id: application.id,
            verify_key: Key::from_hex(application.verify_key.as_bytes())
                .expect("Could not build verifying key"),
        }
    }
}
