//! A picture pasted into the page (interface design, section 7.3), handed
//! from the script to the app.
//!
//! The web view has the pasted picture as a file of the paste; the app's
//! own clipboard reader would not know which paste it was, and reading
//! the clipboard for every paste would read what the person did not mean
//! to give. So the script takes the files of a paste that has no text, as
//! the page's text field is pasted into, and sends their bytes here as
//! text in Base64; a paste with text in it pastes the text, as before.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use dioxus::prelude::*;
use serde::Deserialize;

/// A file pasted into the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pasted {
    /// Its name, as the web view gives it ("image.png" for a picture
    /// copied from most programs).
    pub name: String,
    /// Its media type, as the web view gives it.
    pub media_type: String,
    /// Its size, in bytes.
    pub size: u64,
    /// Its bytes, or `None` for a file the script did not send: one past
    /// 16 MiB, far more than may be attached.
    pub bytes: Option<Vec<u8>>,
}

/// One pasted file as the script sends it.
#[derive(Debug, Deserialize)]
struct Sent {
    name: String,
    #[serde(rename = "type")]
    media_type: String,
    size: u64,
    data: Option<String>,
}

/// Listens for files pasted into the field `selector` names, for as long
/// as the component lives. A file whose bytes were sent and do not arrive
/// whole is left out.
pub fn use_pasted_files(selector: &'static str, on_pasted: impl FnMut(Vec<Pasted>) + 'static) {
    let on_pasted = use_callback(on_pasted);
    use_hook(move || {
        spawn(async move {
            let quoted = serde_json::Value::String(selector.to_owned());
            let mut channel = document::eval(&format!(
                "DirtUi.onPastedFiles({quoted}, files => dioxus.send(files)); \
                 await new Promise(() => {{}});"
            ));
            loop {
                match channel.recv::<Vec<Sent>>().await {
                    Ok(sent) => {
                        let files: Vec<Pasted> = sent
                            .into_iter()
                            .filter_map(|file| {
                                let bytes = match file.data {
                                    Some(data) => Some(STANDARD.decode(data).ok()?),
                                    None => None,
                                };
                                Some(Pasted {
                                    bytes,
                                    size: file.size,
                                    name: file.name,
                                    media_type: file.media_type,
                                })
                            })
                            .collect();
                        if !files.is_empty() {
                            on_pasted.call(files);
                        }
                    }
                    Err(error) => {
                        tracing::warn!("Pasted files are no longer heard: {error}");
                        break;
                    }
                }
            }
        });
    });
}
