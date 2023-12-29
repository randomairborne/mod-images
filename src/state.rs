use std::{str::FromStr, sync::Arc};

use deadpool_redis::{Manager, Pool, Runtime};
use oauth2::{basic::BasicClient, AuthUrl, ClientId, ClientSecret, RevocationUrl, TokenUrl};
use reqwest::{Client, ClientBuilder};
use s3::{creds::Credentials, Bucket, Region};
use tera::Tera;
use twilight_model::id::{
    marker::{GuildMarker, RoleMarker},
    Id,
};

#[derive(Clone)]
pub struct AppState {
    pub bucket: Arc<Bucket>,
    pub tera: Arc<Tera>,
    pub http: Client,
    pub redis: Pool,
    pub allowed_roles: Arc<[Id<RoleMarker>]>,
    pub guild: Id<GuildMarker>,
    pub oauth: Arc<BasicClient>,
}

impl AppState {
    pub async fn new() -> Self {
        Self {
            bucket: get_bucket().into(),
            tera: get_tera().into(),
            http: get_http(),
            redis: get_redis().await,
            allowed_roles: get_roles().into(),
            guild: parse_var("GUILD"),
            oauth: get_oauth().into(),
        }
    }
}

fn get_bucket() -> Bucket {
    let name: String = parse_var("BUCKET_NAME");
    let account_id = parse_var("R2_ACCOUNT_ID");
    let access_token: String = parse_var("R2_ACCESS_TOKEN");
    let region = Region::R2 { account_id };
    let credentials = Credentials::new(Some(&access_token), None, None, None, None).unwrap();
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
    let mut tera = Tera::new("./templates/*.jinja").unwrap();
    tera.autoescape_on(vec!["jinja"]);
    tera
}

fn get_roles() -> Vec<Id<RoleMarker>> {
    let roles_str: String = parse_var("ROLES");
    roles_str.split(',').map(|v| v.parse().unwrap()).collect()
}

async fn get_redis() -> Pool {
    let url: String = parse_var("REDIS_URL");
    let redis_mgr = Manager::new(url).expect("failed to connect to redis");
    let redis = Pool::builder(redis_mgr)
        .runtime(Runtime::Tokio1)
        .build()
        .unwrap();
    redis.get().await.expect("Failed to load redis");
    redis
}

fn get_oauth() -> BasicClient {
    let client_id = ClientId::new(parse_var("CLIENT_ID"));
    let client_secret = ClientSecret::new(parse_var("CLIENT_SECRET"));
    let auth_url = AuthUrl::new("https://discord.com/oauth2/authorize".to_owned()).unwrap();
    let token_url = TokenUrl::new("https://discord.com/api/oauth2/token".to_owned()).unwrap();
    let revocation_url =
        RevocationUrl::new("https://discord.com/api/oauth2/token/revoke".to_owned()).unwrap();
    BasicClient::new(client_id, Some(client_secret), auth_url, Some(token_url))
        .set_revocation_uri(revocation_url)
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
