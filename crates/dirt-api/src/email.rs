//! Outbound email for the magic-code flow.
//!
//! Phase 2.1 ships only the `Log` mode: the magic code is written to
//! `tracing::info!` and never leaves the process. This is enough for
//! integration tests, local development, and the very first end-to-end
//! mobile/desktop wiring runs without provisioning Resend.
//!
//! Phase 2.3 adds a `Resend` variant. The `EmailSender` struct is set up
//! so that change is local — extend the inner enum, branch in
//! `send_magic_code`, and update `from_env` to choose the right mode.

use crate::error::AppError;

#[cfg(test)]
use std::sync::{Arc, Mutex};

/// Sender mode picked at startup. Kept private — callers go through
/// `EmailSender::send_magic_code`, not the variant directly.
enum Mode {
    /// Print the code to the server log instead of sending email.
    /// The right default for `cargo run` and tests; explicit because
    /// "I'm not actually emailing this" should be a deliberate choice.
    Log,
    /// Test-only: stash every (email, code) pair into a shared `Vec`
    /// the test can inspect. Lets the round-trip test parse the actual
    /// code that `/v1/auth/request` minted, instead of seeding a
    /// parallel one and pretending.
    #[cfg(test)]
    Capture(CapturedSends),
}

#[cfg(test)]
pub type CapturedSends = Arc<Mutex<Vec<(String, String)>>>;

pub struct EmailSender {
    mode: Mode,
}

impl EmailSender {
    /// Always-log sender. The constructor every test reaches for.
    #[must_use]
    pub const fn log_only() -> Self {
        Self { mode: Mode::Log }
    }

    /// Test-only: build a sender that records every send into the
    /// returned `Arc<Mutex<Vec>>`, alongside the sender itself.
    #[cfg(test)]
    pub fn capture() -> (Self, CapturedSends) {
        let captured = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                mode: Mode::Capture(captured.clone()),
            },
            captured,
        )
    }

    /// Pick a mode from the process environment.
    ///
    /// Phase 2.1 only knows the `Log` mode. Phase 2.3 will look for
    /// `RESEND_API_KEY` here and switch to a Resend HTTPS sender — at
    /// which point this function reads the env, so the
    /// `missing_const_for_fn` lint will resolve itself naturally.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn from_env() -> Self {
        Self::log_only()
    }

    /// Deliver a magic code to `email`. Errors here propagate as
    /// `AppError::Internal` so the request handler maps them onto a 500
    /// — the user can't fix a Resend outage themselves, retrying the
    /// `/v1/auth/request` is the right user-facing prompt.
    ///
    /// `async` is kept on the signature even in pure-`Log` mode because
    /// Phase 2.3 introduces an `await` on the Resend HTTPS call and we
    /// don't want every caller to switch from sync to async then.
    #[allow(clippy::unused_async)]
    pub async fn send_magic_code(&self, email: &str, code: &str) -> Result<(), AppError> {
        match &self.mode {
            Mode::Log => {
                // The "we tried to email you" record stays at info for
                // observability; the raw code drops to debug so a
                // RUST_LOG=info deploy doesn't write live magic codes
                // into a log aggregator. RUST_LOG=debug is the explicit
                // opt-in for "I want to see codes in the dev console."
                tracing::info!(target: "dirt_api::email", "[dev email] to={email}");
                tracing::debug!(target: "dirt_api::email", "[dev email] to={email} code={code}");
                Ok(())
            }
            #[cfg(test)]
            Mode::Capture(captured) => {
                captured
                    .lock()
                    .expect("CapturedSends mutex poisoned")
                    .push((email.to_string(), code.to_string()));
                Ok(())
            }
        }
    }
}

impl Default for EmailSender {
    fn default() -> Self {
        Self::log_only()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn log_mode_send_succeeds() {
        let sender = EmailSender::log_only();
        sender
            .send_magic_code("user@example.com", "123456")
            .await
            .unwrap();
    }
}
