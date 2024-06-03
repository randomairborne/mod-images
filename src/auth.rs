use axum::{
    extract::{Query, Request, State},
    http::Uri,
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::{
    cookie::{Cookie, SameSite},
    CookieJar,
};
use oauth2::{
    basic::BasicTokenResponse, AuthorizationCode, CsrfToken, PkceCodeChallenge, PkceCodeVerifier,
    Scope, TokenResponse,
};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use time::Duration;
use twilight_http::client::ClientBuilder;
use twilight_model::{guild::Permissions, id::Id};

use crate::{AppState, Error};

#[derive(Serialize, Deserialize, Clone)]
struct OAuth2RoundtripData {
    pkce: String,
    redirect: String,
}

pub async fn middleware(
    State(mut state): State<AppState>,
    cookies: CookieJar,
    uri: Uri,
    request: Request,
    next: Next,
) -> Response {
    let Some(token) = cookies.get("token") else {
        return oauthify(state, uri).await.into_response();
    };
    let redis_key = format!("token:auth:{}", token.value());
    match state.redis_exists(&redis_key).await {
        Ok(true) => next.run(request).await,
        Ok(false) => oauthify(state, uri).await.into_response(),
        Err(e) => e.into_response(),
    }
}

async fn oauthify(mut state: AppState, uri: Uri) -> Result<Redirect, Error> {
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let (auth_url, csrf_token) = state
        .oauth
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new("identify".to_string()))
        .add_scope(Scope::new("guilds".to_string()))
        .set_pkce_challenge(pkce_challenge)
        .add_extra_param("prompt", "none")
        //.add_extra_param("integration_types", "1") // this was breaking it, and we disabled the command anyway
        .url();
    let roundtrip = OAuth2RoundtripData {
        pkce: pkce_verifier.secret().to_string(),
        redirect: uri.path().to_string(),
    };
    state
        .redis
        .set_ex(
            format!("token:csrf:{}", csrf_token.secret()),
            serde_json::to_string(&roundtrip)?,
            600,
        )
        .await?;
    Ok(Redirect::to(auth_url.as_str()))
}

pub async fn authenticate(
    State(mut state): State<AppState>,
    Query(query): Query<SetIdQuery>,
) -> Result<(CookieJar, Redirect), Error> {
    let roundtrip_data = state
        .redis
        .get_del::<String, Option<String>>(format!("token:csrf:{}", query.state))
        .await?
        .ok_or(Error::InvalidState)?;
    let roundtrip_data: OAuth2RoundtripData = serde_json::from_str(&roundtrip_data)?;
    let pkce_verifier = PkceCodeVerifier::new(roundtrip_data.pkce);
    let token_response = state
        .oauth
        .exchange_code(AuthorizationCode::new(query.code))
        .set_pkce_verifier(pkce_verifier)
        .request_async(&state.http)
        .await?;
    let token = format!("Bearer {}", token_response.access_token().secret());
    let client = ClientBuilder::new().token(token).build();
    let guilds = client
        .current_user_guilds()
        .after(Id::new(state.guild.get() - 1))
        .limit(1)
        .await?
        .model()
        .await?;

    tokio::spawn(revoke_tokens(state.clone(), token_response));

    let Some(guild) = guilds.first() else {
        return Err(Error::NoPermissions);
    };
    if !guild.permissions.contains(Permissions::MODERATE_MEMBERS) || guild.id != state.guild {
        return Err(Error::NoPermissions);
    }

    let token = crate::randstring(64);
    state
        .redis
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
    Ok((jar, Redirect::to(&roundtrip_data.redirect)))
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
