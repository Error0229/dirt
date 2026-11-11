//! The token check against tokens signed by a key made in the test. No
//! network and no Clerk.

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use jsonwebtoken::{encode, EncodingKey, Header};
use rsa::pkcs1::EncodeRsaPrivateKey;
use rsa::traits::PublicKeyParts;
use rsa::RsaPrivateKey;
use serde_json::{json, Value};

use super::*;

const ISSUER: &str = "https://issuer.test";
const CLIENT_ID: &str = "dirt-client";
const NONCE: &str = "nonce-for-this-attempt";
const KEY_ID: &str = "test-key-1";

struct Issuer {
    signing_key: EncodingKey,
    published: JwkSet,
    modulus: Vec<u8>,
}

fn make_issuer() -> Issuer {
    let private = RsaPrivateKey::new(&mut rand_core::OsRng, 2048).expect("key generation");
    let der = private.to_pkcs1_der().expect("der");
    let public = private.to_public_key();
    let modulus = public.n().to_bytes_be();
    let published = serde_json::from_value(json!({
        "keys": [{
            "kty": "RSA", "use": "sig", "alg": "RS256", "kid": KEY_ID,
            "n": URL_SAFE_NO_PAD.encode(&modulus),
            "e": URL_SAFE_NO_PAD.encode(public.e().to_bytes_be()),
        }]
    }))
    .expect("key set");
    Issuer {
        signing_key: EncodingKey::from_rsa_der(der.as_bytes()),
        published,
        modulus,
    }
}

/// Made once: generating a key is slow in a debug build.
fn issuer() -> &'static Issuer {
    static ISSUER_KEYS: OnceLock<Issuer> = OnceLock::new();
    ISSUER_KEYS.get_or_init(make_issuer)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_secs()
}

fn good_claims() -> Value {
    json!({
        "iss": ISSUER, "sub": "user_123", "aud": [CLIENT_ID],
        "exp": now() + 300, "iat": now(), "nonce": NONCE,
        "email": "someone@example.com",
    })
}

fn sign(claims: &Value, key: &EncodingKey, key_id: Option<&str>) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = key_id.map(str::to_owned);
    encode(&header, claims, key).expect("signing")
}

fn verify(token: &str) -> Result<SignIn, VerifyError> {
    verify_id_token(token, &issuer().published, ISSUER, CLIENT_ID, NONCE)
}

fn with(mut claims: Value, name: &str, value: Value) -> Value {
    claims[name] = value;
    claims
}

fn without(mut claims: Value, name: &str) -> Value {
    claims.as_object_mut().expect("object").remove(name);
    claims
}

#[test]
fn a_valid_token_yields_the_user_id_and_email() {
    let token = sign(&good_claims(), &issuer().signing_key, Some(KEY_ID));
    assert_eq!(
        verify(&token).unwrap(),
        SignIn {
            user_id: "user_123".to_owned(),
            email: Some("someone@example.com".to_owned()),
        }
    );
}

#[test]
fn the_audience_may_be_a_single_name_or_a_list() {
    let claims = with(good_claims(), "aud", json!(CLIENT_ID));
    let token = sign(&claims, &issuer().signing_key, Some(KEY_ID));
    assert!(verify(&token).is_ok());
}

#[test]
fn a_tampered_token_is_rejected() {
    let token = sign(&good_claims(), &issuer().signing_key, Some(KEY_ID));
    let parts: Vec<&str> = token.split('.').collect();
    let forged_payload = URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&with(good_claims(), "sub", json!("user_999"))).expect("json"));
    let forged = format!("{}.{forged_payload}.{}", parts[0], parts[2]);
    assert!(matches!(verify(&forged), Err(VerifyError::Rejected(_))));
}

#[test]
fn expired_wrong_client_and_wrong_issuer_tokens_are_rejected() {
    for claims in [
        with(good_claims(), "exp", json!(now() - 3600)),
        with(good_claims(), "aud", json!("some-other-client")),
        with(good_claims(), "iss", json!("https://evil.test")),
    ] {
        let token = sign(&claims, &issuer().signing_key, Some(KEY_ID));
        assert!(matches!(verify(&token), Err(VerifyError::Rejected(_))));
    }
}

#[test]
fn a_token_with_another_nonce_or_none_is_rejected() {
    for claims in [
        with(good_claims(), "nonce", json!("nonce-from-another-attempt")),
        without(good_claims(), "nonce"),
    ] {
        let token = sign(&claims, &issuer().signing_key, Some(KEY_ID));
        assert!(matches!(verify(&token), Err(VerifyError::WrongNonce)));
    }
}

#[test]
fn a_token_missing_a_required_claim_is_rejected() {
    for missing in ["exp", "aud", "sub"] {
        let token = sign(
            &without(good_claims(), missing),
            &issuer().signing_key,
            Some(KEY_ID),
        );
        assert!(
            verify(&token).is_err(),
            "accepted a token without {missing}"
        );
    }
}

#[test]
fn a_token_signed_by_another_key_under_the_same_name_is_rejected() {
    let impostor = make_issuer();
    let token = sign(&good_claims(), &impostor.signing_key, Some(KEY_ID));
    assert!(matches!(verify(&token), Err(VerifyError::Rejected(_))));
}

#[test]
fn a_token_naming_an_unpublished_key_or_no_key_is_rejected() {
    let token = sign(&good_claims(), &issuer().signing_key, Some("retired-key"));
    assert!(matches!(verify(&token), Err(VerifyError::UnknownKey)));
    let token = sign(&good_claims(), &issuer().signing_key, None);
    assert!(matches!(verify(&token), Err(VerifyError::UnknownKey)));
}

/// The classic downgrade: sign with HS256, using the public key as the
/// shared secret, and hope the verifier lets the token pick its algorithm.
#[test]
fn a_token_that_picks_a_symmetric_algorithm_is_rejected() {
    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some(KEY_ID.to_owned());
    let token = encode(
        &header,
        &good_claims(),
        &EncodingKey::from_secret(&issuer().modulus),
    )
    .expect("signing");
    assert!(matches!(
        verify(&token),
        Err(VerifyError::WrongAlgorithm(Algorithm::HS256))
    ));
}

#[test]
fn text_that_is_not_a_token_is_rejected() {
    assert!(matches!(
        verify("not-a-token"),
        Err(VerifyError::Malformed(_))
    ));
    assert!(matches!(verify(""), Err(VerifyError::Malformed(_))));
}

#[tokio::test]
async fn fixed_keys_check_a_token_without_any_network() {
    let keys = Keys::Fixed(issuer().published.clone());
    let token = sign(&good_claims(), &issuer().signing_key, Some(KEY_ID));
    let sign_in = keys.verify(&token, ISSUER, CLIENT_ID, NONCE).await.unwrap();
    assert_eq!(sign_in.user_id, "user_123");
}
