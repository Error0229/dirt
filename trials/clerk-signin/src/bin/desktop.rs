//! Signs in through Clerk the way the desktop app and the CLI will.
//!
//! ```text
//! CLERK_ISSUER=https://<instance> CLERK_OAUTH_CLIENT_ID=<id> \
//!   cargo run --bin desktop [-- --no-browser]
//! ```
//!
//! With `--no-browser` it prints the address to open instead of opening it.
//! Token values are never printed, only what they contain.

use std::time::Duration;

use clerk_signin_trial::flow::{
    bind_loopback, discover, exchange_code, fetch_keys, peek_claims, wait_for_code, AnyError,
    Attempt,
};
use clerk_signin_trial::verify::verify_id_token;

const SCOPES: &str = "openid profile email";

#[tokio::main]
async fn main() -> Result<(), AnyError> {
    let issuer = std::env::var("CLERK_ISSUER").map_err(|_| "CLERK_ISSUER is not set")?;
    let client_id =
        std::env::var("CLERK_OAUTH_CLIENT_ID").map_err(|_| "CLERK_OAUTH_CLIENT_ID is not set")?;
    let open_browser = !std::env::args().any(|arg| arg == "--no-browser");
    let scopes = std::env::var("TRIAL_SCOPES").unwrap_or_else(|_| SCOPES.to_owned());

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let discovery = discover(&http, &issuer).await?;
    println!("issuer: {}", discovery.issuer);

    let (listener, redirect_uri) = bind_loopback().await?;
    println!("redirect: {redirect_uri}");
    let attempt = Attempt::new(redirect_uri);
    let url = attempt.authorization_url(&discovery, &client_id, &scopes)?;

    if open_browser && webbrowser::open(url.as_str()).is_ok() {
        println!("opened the browser; waiting for sign-in");
    } else {
        println!("open this address to sign in:\n{url}");
    }

    let code = wait_for_code(listener, &attempt.state, Duration::from_secs(600)).await?;
    println!(
        "callback received: code of {} characters, state matches",
        code.len()
    );

    let tokens = exchange_code(&http, &discovery, &client_id, &attempt, &code).await?;
    println!(
        "token response: token_type={:?} expires_in={:?} scope={:?} refresh_token={}",
        tokens.token_type,
        tokens.expires_in,
        tokens.scope,
        if tokens.refresh_token.is_some() {
            "issued"
        } else {
            "not issued"
        },
    );
    match peek_claims(&tokens.access_token) {
        Some(claims) => println!("access token is a signed token with claims: {claims}"),
        None => println!(
            "access token is opaque ({} characters)",
            tokens.access_token.len()
        ),
    }

    let id_token = tokens.id_token.ok_or("no ID token in the response")?;
    println!(
        "ID token header: {:?}",
        jsonwebtoken::decode_header(&id_token)?
    );
    let keys = fetch_keys(&http, &discovery).await?;
    let claims = verify_id_token(
        &id_token,
        &keys,
        &discovery.issuer,
        &client_id,
        &attempt.nonce,
    )?;
    println!(
        "ID token verified. claims: {}",
        serde_json::to_string(&claims)?
    );
    println!("user ID: {}", claims.sub);

    let userinfo: serde_json::Value = http
        .get(&discovery.userinfo_endpoint)
        .bearer_auth(&tokens.access_token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    println!("userinfo: {userinfo}");
    Ok(())
}
