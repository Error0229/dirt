//! Mobile app shell — placeholder.
//!
//! The full Phase-0 shell (~2,300 lines, Supabase-era) was wired to
//! `dirt_core::auth`, `dirt_core::media`, `MediaApiClient`,
//! `auth_session`, `Attachment`, `SyncConflict`, and embedded-replica
//! sync — all of which were removed in the Phase 1 server rewrite (see
//! `docs/DESIGN.md` and the post-PR-#225 status section in the live
//! design doc). Carrying that stale code through this PR meant the
//! android-target build couldn't compile, which is why the shell is
//! reduced to this placeholder for now.
//!
//! The mobile sync-worker rewrite is the next milestone after Phase 1
//! lands. When it does, the shell will be rebuilt against the new
//! `dirt_core::sync::api_client::ApiClient` + `SyncEngine` flow that
//! desktop and CLI already use.

use dioxus::prelude::*;

#[component]
pub fn AppShell() -> Element {
    rsx! {
        div {
            style: "padding: 24px; font-family: system-ui, -apple-system, sans-serif; color: #111827; line-height: 1.5;",
            h1 { style: "margin: 0 0 12px 0; font-size: 20px;", "dirt-mobile" }
            p {
                style: "margin: 0 0 12px 0;",
                "The mobile shell is being rewritten on top of the new sync engine. "
                "Until that lands, capture from the desktop app or the CLI:"
            }
            pre {
                style: "background: #f3f4f6; padding: 12px; border-radius: 6px; font-size: 13px; overflow-x: auto;",
                "cargo run -p dirt-cli \"my note\""
            }
            p {
                style: "margin: 12px 0 0 0; color: #6b7280; font-size: 13px;",
                "Track progress on the next milestone in the project's design doc."
            }
        }
    }
}
