use std::borrow::Cow;

use axum::{
    extract::{Query, Request, State},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::{
    cookie::{Cookie, SameSite},
    CookieJar,
};
use oauth2::{
    basic::BasicTokenResponse, reqwest::async_http_client, AuthorizationCode, CsrfToken,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
};
use redis::AsyncCommands;
use time::Duration;
use twilight_http::client::ClientBuilder;
use twilight_model::{guild::Permissions, id::Id};

use crate::{AppState, Error};

pub async fn middleware(
    State(state): State<AppState>,
    cookies: CookieJar,
    request: Request,
    next: Next,
) -> Response {
    let Some(token) = cookies.get("token") else {
        return oauthify(state).await.into_response();
    };
    let redis_key = format!("token:auth:{}", token.value());
    match state.redis_exists(&redis_key).await {
        Ok(true) => next.run(request).await,
        Ok(false) => oauthify(state).await.into_response(),
        Err(e) => e.into_response(),
    }
}

async fn oauthify(state: AppState) -> Result<Redirect, Error> {
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let redirect = RedirectUrl::new(format!("{}/oauth2/callback", state.root_url))?;
    trace!("Build redirect url", redirect);
    let (auth_url, csrf_token) = state
        .oauth
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new("identify guilds".to_string()))
        .set_pkce_challenge(pkce_challenge)
        .set_redirect_uri(Cow::Owned(redirect))
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
    let token_response = state
        .oauth
        .exchange_code(AuthorizationCode::new(query.code))
        .set_pkce_verifier(pkce_verifier)
        .request_async(async_http_client)
        .await?;
    let token = format!("Bearer {}", token_response.access_token().secret());
    let client = ClientBuilder::new().token(token).build();
    let guilds = client
        .current_user_guilds()
        .after(Id::new(state.guild.get() - 1))
        .limit(1)?
        .await?
        .model()
        .await?;
    tokio::spawn(revoke_tokens(state.clone(), token_response));
    let Some(guild) = guilds.first() else {
        return Err(Error::NoPermissions);
    };
    if !guild.permissions.contains(Permissions::MODERATE_MEMBERS) {
        return Err(Error::NoPermissions);
    }
    let token = crate::randstring(64);
    state
        .redis
        .get()
        .await?
        .set_ex(format!("token:auth:{token}"), true, 86400)
        .await?;
    let cookie = Cookie::build(("token".to_owned(), token))
        .secure(true)
        .same_site(SameSite::Lax)
        .http_only(true)
        .max_age(Duration::days(1))
        .path("/")
        .build();
    let jar = CookieJar::new().add(cookie);
    Ok((jar, Redirect::to("/")))
}

#[derive(serde::Deserialize)]
pub struct SetIdQuery {
    code: String,
    state: String,
}

async fn revoke_tokens(state: AppState, token_response: BasicTokenResponse) {
    tokio::spawn(async move {
        if let Some(rt) = token_response.refresh_token() {
            state.oauth.revoke_token(rt.into()).ok();
        }
        state
            .oauth
            .revoke_token(token_response.access_token().into())
            .ok();
    });
}
