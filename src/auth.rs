use axum::{
    extract::{Query, State},
    http::Request,
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::{cookie::Cookie, CookieJar};
use oauth2::{
    reqwest::async_http_client, AuthorizationCode, CsrfToken, PkceCodeChallenge, PkceCodeVerifier,
    Scope, TokenResponse,
};
use redis::AsyncCommands;

use crate::{AppState, Error};

const API_URL: &str = "https://discord.com/api/v10";

pub async fn middleware<B>(
    State(state): State<AppState>,
    cookies: CookieJar,
    request: Request<B>,
    next: Next<B>,
) -> Response {
    let Some(token) = cookies.get("token") else {
        return Redirect::to("/oauth2").into_response();
    };
    let auth_check = redis_exists(&state, token.value()).await;
    match auth_check {
        Ok(()) => next.run(request).await,
        Err(Error::Unauthorized) => Redirect::to("/oauth2").into_response(),
        Err(source) => source.into_response(),
    }
}

async fn redis_exists(state: &AppState, key: &str) -> Result<(), Error> {
    let value: Option<bool> = state.redis.get().await?.get(key).await?;
    if value.is_none() {
        return Err(Error::Unauthorized);
    }
    Ok(())
}

pub async fn redirect(State(state): State<AppState>) -> Result<Redirect, Error> {
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let (auth_url, csrf_token) = state
        .oauth
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new("identify guilds.members.read".to_string()))
        .set_pkce_challenge(pkce_challenge)
        .url();
    state
        .redis
        .get()
        .await?
        .set_ex(
            format!("token:csrf:{}", csrf_token.secret()),
            pkce_verifier.secret(),
            600,
        )
        .await?;
    Ok(Redirect::to(auth_url.as_str()))
}

#[axum::debug_handler]
pub async fn authenticate(
    State(state): State<AppState>,
    Query(query): Query<SetIdQuery>,
) -> Result<(CookieJar, Redirect), Error> {
    let pkce_secret = state
        .redis
        .get()
        .await?
        .get_del::<String, Option<String>>(format!("token:csrf:{}", query.state))
        .await?
        .ok_or(Error::InvalidState)?;
    let pkce_verifier = PkceCodeVerifier::new(pkce_secret);
    let token_result = state
        .oauth
        .exchange_code(AuthorizationCode::new(query.code))
        .set_pkce_verifier(pkce_verifier)
        .request_async(async_http_client)
        .await
        .map_err(|_| Error::CodeExchangeFailed)?;
    let me: twilight_model::guild::Member = state
        .http
        .get(format!("{API_URL}/users/@me/guilds/{}/member", state.guild))
        .bearer_auth(token_result.access_token().secret())
        .send()
        .await?
        .json()
        .await?;
    tokio::spawn(async move {
        if let Some(rt) = token_result.refresh_token() {
            state.oauth.revoke_token(rt.into()).ok();
        }
        state
            .oauth
            .revoke_token(token_result.access_token().into())
            .ok();
    });
    // if no role the user has is contained within the allowed_roles, they shall not pass
    if !me.roles.iter().any(|v| state.allowed_roles.contains(v)) {
        return Err(Error::NoPermissions);
    }
    let token = crate::randstring(64);
    state
        .redis
        .get()
        .await?
        .set_ex(format!("token:auth:{token}"), true, 86400)
        .await?;
    let jar = CookieJar::new().add(Cookie::new("token".to_owned(), token));
    Ok((jar, Redirect::to("/")))
}

#[derive(serde::Deserialize)]
pub struct SetIdQuery {
    code: String,
    state: String,
}
