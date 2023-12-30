use std::{str::FromStr, sync::Arc};

use deadpool_redis::{Manager, Pool, Runtime};
use oauth2::{
    basic::BasicClient, AuthUrl, ClientId, ClientSecret, RedirectUrl, RevocationUrl, TokenUrl,
};
use redis::AsyncCommands;
use reqwest::{Client, ClientBuilder};
use s3::{creds::Credentials, Bucket, Region};
use tera::Tera;
use twilight_model::id::{marker::GuildMarker, Id};

use crate::Error;

#[derive(Clone)]
pub struct AppState {
    pub bucket: Arc<Bucket>,
    pub tera: Arc<Tera>,
    pub http: Client,
    pub redis: Pool,
    pub guild: Id<GuildMarker>,
    pub oauth: Arc<BasicClient>,
}

impl AppState {
    pub async fn new() -> Self {
        trace!("Building state");
        Self {
            bucket: get_bucket().into(),
            tera: get_tera().into(),
            http: get_http(),
            redis: get_redis().await,
            guild: parse_var("GUILD"),
            oauth: get_oauth().into(),
        }
    }

    pub async fn redis_exists(&self, key: &str) -> Result<bool, Error> {
        let value: Option<bool> = self.redis.get().await?.get(key).await?;
        Ok(value.is_some())
    }

    pub fn asset_dir() -> String {
        std::env::var("ASSET_DIR").unwrap_or_else(|_v| "./assets/".to_string())
    }

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
            "",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .unwrap()
}

fn get_tera() -> Tera {
    trace!("Loading templates");
    let glob = format!(
        "{}/**/*.jinja",
        AppState::template_dir().trim_end_matches('/')
    );
    let mut tera = Tera::new(&glob).unwrap();
    tera.autoescape_on(vec!["jinja"]);
    for template in tera.get_template_names() {
        trace!(template, "Loaded template");
    }
    trace!(
        "Loaded {} tera templates",
        tera.get_template_names().count()
    );
    tera
}

async fn get_redis() -> Pool {
    trace!("Loading redis");
    let url: String = parse_var("REDIS_URL");
    let redis_mgr = Manager::new(url).expect("failed to connect to redis");
    let redis = Pool::builder(redis_mgr)
        .runtime(Runtime::Tokio1)
        .build()
        .unwrap();
    trace!("Loaded redis, testing connection..");
    redis.get().await.expect("Failed to load redis");
    trace!("Redis connection succeeded");
    redis
}

fn get_oauth() -> BasicClient {
    let client_id = ClientId::new(parse_var("CLIENT_ID"));
    let client_secret = ClientSecret::new(parse_var("CLIENT_SECRET"));
    let root_url: String = parse_var("ROOT_URL");
    let root_url = root_url.trim_end_matches('/');
    let auth_url = AuthUrl::new("https://discord.com/oauth2/authorize".to_owned()).unwrap();
    let token_url = TokenUrl::new("https://discord.com/api/oauth2/token".to_owned()).unwrap();
    let revocation_url =
        RevocationUrl::new("https://discord.com/api/oauth2/token/revoke".to_owned()).unwrap();
    let redirect_url = RedirectUrl::new(format!("{root_url}/oauth2/callback")).unwrap();
    trace!(?redirect_url, "Built redirect url");
    BasicClient::new(client_id, Some(client_secret), auth_url, Some(token_url))
        .set_revocation_uri(revocation_url)
        .set_redirect_uri(redirect_url)
}

fn parse_var<T>(name: &str) -> T
where
    T: FromStr,
    T::Err: std::fmt::Debug,
{
    std::env::var(name)
        .unwrap_or_else(|_| panic!("{name} required in the environment"))
        .parse()
        .unwrap_or_else(|_| panic!("{name} must be a valid {}", std::any::type_name::<T>()))
}
