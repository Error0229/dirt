//! A stand-in for the identity provider.
//!
//! It speaks the part of the OIDC protocol that the real provider does for a
//! public client: a discovery document, an authorization endpoint that sends
//! the browser back with a code, and a token endpoint that exchanges the code
//! for an ID token after checking the PKCE verifier. It listens on a port of
//! this machine, because the client reaches a provider over HTTP whoever it
//! is.
//!
//! There is no sign-in page. The "person" is named in the request the test's
//! browser makes: `&as=alice` signs in as `alice`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

/// A code that was handed out and what it stands for.
struct Grant {
    user: String,
    nonce: String,
    challenge: String,
    redirect_uri: String,
    client_id: String,
}

#[derive(Clone)]
struct Provider {
    address: String,
    grants: Arc<Mutex<HashMap<String, Grant>>>,
}

/// Starts a provider and returns its issuer address. It runs until the test
/// that started it ends.
pub async fn start() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind the provider");
    let address = format!(
        "http://127.0.0.1:{}",
        listener.local_addr().expect("port").port()
    );
    let provider = Provider {
        address: address.clone(),
        grants: Arc::new(Mutex::new(HashMap::new())),
    };
    let app = Router::new()
        .route("/.well-known/openid-configuration", get(discovery))
        .route("/oauth/authorize", get(authorize))
        .route("/oauth/token", post(token))
        .with_state(provider);
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    address
}

async fn discovery(State(provider): State<Provider>) -> Json<serde_json::Value> {
    Json(json!({
        "issuer": provider.address,
        "authorization_endpoint": format!("{}/oauth/authorize", provider.address),
        "token_endpoint": format!("{}/oauth/token", provider.address),
        "authorization_response_iss_parameter_supported": true,
    }))
}

#[derive(Deserialize)]
struct Authorize {
    response_type: String,
    client_id: String,
    redirect_uri: String,
    state: String,
    nonce: String,
    code_challenge: String,
    code_challenge_method: String,
    /// Who signs in. A real provider learns this from its sign-in page.
    #[serde(rename = "as")]
    user: String,
}

async fn authorize(State(provider): State<Provider>, Query(request): Query<Authorize>) -> Response {
    if request.response_type != "code" || request.code_challenge_method != "S256" {
        return (StatusCode::BAD_REQUEST, "unsupported request").into_response();
    }
    let code = dirt_core::crypto::random_token();
    let location = format!(
        "{}?code={code}&state={}&iss={}",
        request.redirect_uri, request.state, provider.address
    );
    provider.grants.lock().expect("grants").insert(
        code,
        Grant {
            user: request.user,
            nonce: request.nonce,
            challenge: request.code_challenge,
            redirect_uri: request.redirect_uri,
            client_id: request.client_id,
        },
    );
    (StatusCode::FOUND, [(header::LOCATION, location)]).into_response()
}

#[derive(Deserialize)]
struct Exchange {
    grant_type: String,
    code: String,
    redirect_uri: String,
    client_id: String,
    code_verifier: String,
}

async fn token(State(provider): State<Provider>, Form(request): Form<Exchange>) -> Response {
    // A code works once, whatever becomes of the exchange.
    let grant = provider
        .grants
        .lock()
        .expect("grants")
        .remove(&request.code);
    let Some(grant) = grant else {
        return (StatusCode::BAD_REQUEST, "unknown or used code").into_response();
    };
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(request.code_verifier.as_bytes()));
    if request.grant_type != "authorization_code"
        || challenge != grant.challenge
        || request.redirect_uri != grant.redirect_uri
        || request.client_id != grant.client_id
    {
        return (
            StatusCode::BAD_REQUEST,
            "the exchange does not match the code",
        )
            .into_response();
    }
    Json(json!({
        "id_token": crate::id_token(&provider.address, &grant.user, &grant.nonce),
        // A client has no use for these and must not keep them.
        "access_token": "an-access-token",
        "refresh_token": "a-refresh-token",
        "token_type": "Bearer",
    }))
    .into_response()
}
