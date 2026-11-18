//! The banner on every device already in when a new one waits to be let
//! in (interface design, sections 6.8 and 9.2): "Work laptop wants to
//! join", with **Review**, which opens Settings at Devices comparing the
//! code with it.
//!
//! A device learns that another waits when it syncs, from the count the
//! sync brings; the name is asked of the server once the count says there
//! is one. Dismissed, the banner stays away from that device.

use std::collections::HashSet;

use dioxus::prelude::*;
use dirt_core::api::DeviceInfo;

use super::{settings_nav, Linked};
use crate::appearance::Platform;
use crate::components::Banner;

#[component]
pub fn JoinBanner(
    linked: Linked,
    #[props(default = Platform::current())] platform: Platform,
) -> Element {
    let nav = settings_nav();
    let mut waiting = use_signal(|| None::<DeviceInfo>);
    let mut dismissed = use_signal(HashSet::<uuid::Uuid>::new);
    let _ask = use_resource(move || {
        let pending = linked.status.read().pending_devices;
        let ready = linked.state.read().is_ready();
        let session = linked.session.read().clone();
        // Asked again as Settings closes: a device may have been let in
        // there, or turned away.
        let settings_open = nav.is_open();
        async move {
            if settings_open {
                return;
            }
            let Some(session) = session.filter(|_| ready && pending > 0) else {
                waiting.set(None);
                return;
            };
            match session.waiting_devices().await {
                Ok(devices) => {
                    let first = devices
                        .into_iter()
                        .find(|device| !dismissed.peek().contains(&device.id));
                    waiting.set(first);
                }
                Err(error) => tracing::warn!("Could not ask which device waits: {error}"),
            }
        }
    });
    // Settings covers it, and is where the device is let in.
    let shown = waiting.read().clone().filter(|_| !nav.is_open());
    let title = shown
        .as_ref()
        .map(|device| format!("{} wants to join", device.name))
        .unwrap_or_default();
    rsx! {
        Banner {
            open: shown.is_some(),
            title,
            detail: if platform == Platform::Android {
                "Tap to compare and let it in"
            } else {
                "It needs your key to read notes"
            },
            action: "Review",
            platform,
            on_action: move |()| {
                if let Some(device) = waiting.take() {
                    nav.review(device);
                }
            },
            on_dismiss: move |()| {
                if let Some(device) = waiting.take() {
                    dismissed.write().insert(device.id);
                }
            },
        }
    }
}
