//! Trial 3, Android half: sign in through Clerk from an Android app.
//!
//! The app opens the system browser on Clerk's sign-in page and gets the
//! result back through the custom scheme `dev.dirt.app://callback`:
//!
//! 1. "Sign in" saves the attempt (PKCE verifier, state, nonce) to a file,
//!    then opens the browser. Saving first matters: Android may kill the app
//!    while the browser is in front.
//! 2. Clerk redirects to the custom scheme. Android delivers that to
//!    `MainActivity`, which writes the address to `redirect.txt`.
//! 3. This code sees the file, loads the saved attempt, exchanges the code,
//!    and verifies the ID token with the same function the desktop uses.
//!
//! Every line shown on screen is also appended to `report.txt` in the app's
//! files folder, for `adb shell run-as dev.dirt.trial.signin cat files/report.txt`.
//! The report holds no email address or name.

#[cfg(not(target_os = "android"))]
fn main() {
    eprintln!("This app runs only on Android. Build it with dx; see the trial write-up.");
}

#[cfg(target_os = "android")]
fn main() {
    dioxus::launch(app::App);
}

#[cfg(target_os = "android")]
mod app {
    use std::io::Write as _;
    use std::path::PathBuf;
    use std::time::Duration;

    use clerk_signin_trial::flow::{
        discover, exchange_code, fetch_keys, http_client, AnyError, Attempt,
    };
    use clerk_signin_trial::verify::verify_id_token;
    use dioxus::prelude::*;
    use serde_json::{json, Value};
    use url::Url;

    // Public identifiers of the Clerk development instance and its OAuth
    // application. Neither is a secret: a public client has none.
    const ISSUER: &str = "https://ace-buck-9704.clerk.accounts.dev";
    const CLIENT_ID: &str = "rzAdLbprY1LizqO9";
    const REDIRECT: &str = "dev.dirt.app://callback";
    const SCOPES: &str = "openid profile email";

    /// The app's private files folder, from the process name, which is the
    /// package name. `MainActivity` writes to the same folder as `filesDir`.
    fn folder() -> PathBuf {
        let package = std::fs::read("/proc/self/cmdline")
            .ok()
            .and_then(|bytes| {
                let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
                String::from_utf8(bytes[..end].to_vec()).ok()
            })
            .unwrap_or_default();
        PathBuf::from("/data/data").join(package).join("files")
    }

    fn say(mut lines: Signal<Vec<String>>, text: impl Into<String>) {
        let text = text.into();
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(folder().join("report.txt"))
        {
            let _ = writeln!(file, "{text}");
        }
        lines.write().push(text);
    }

    /// Saves a new attempt, then opens the browser.
    async fn start() -> Result<String, AnyError> {
        let http = http_client()?;
        let discovery = discover(&http, ISSUER).await?;
        let attempt = Attempt::new(REDIRECT.to_owned());
        let address = attempt.authorization_url(&discovery, CLIENT_ID, SCOPES)?;

        std::fs::create_dir_all(folder())?;
        let saved = json!({ "attempt": attempt, "started_by_process": std::process::id() });
        std::fs::write(folder().join("attempt.json"), serde_json::to_vec(&saved)?)?;

        webbrowser::open(address.as_str())?;
        Ok(format!(
            "attempt saved by process {}; browser opened",
            std::process::id()
        ))
    }

    /// The redirect address `MainActivity` left, if any. Taking it removes it.
    fn take_redirect() -> Option<String> {
        let path = folder().join("redirect.txt");
        let address = std::fs::read_to_string(&path).ok()?;
        let _ = std::fs::remove_file(&path);
        Some(address)
    }

    /// Finishes the saved attempt with the redirect address.
    async fn finish(address: &str) -> Result<Vec<String>, AnyError> {
        let mut report = Vec::new();
        let path = folder().join("attempt.json");
        let saved: Value = serde_json::from_slice(
            &std::fs::read(&path).map_err(|_| "a redirect arrived but no attempt is saved")?,
        )?;
        // An attempt is used once. Removing it first means a redirect that is
        // delivered twice cannot be finished twice.
        std::fs::remove_file(&path)?;
        let attempt: Attempt = serde_json::from_value(saved["attempt"].clone())?;
        let started_by = saved["started_by_process"].as_u64().unwrap_or(0);

        let redirect = Url::parse(address.trim())?;
        let find = |name: &str| {
            redirect
                .query_pairs()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.into_owned())
        };
        if let Some(error) = find("error") {
            return Err(format!("the provider returned {error}").into());
        }
        let code = find("code").ok_or("the redirect carries no code")?;
        if find("state").as_deref() != Some(attempt.state.as_str()) {
            return Err("the state does not match the saved attempt".into());
        }

        let http = http_client()?;
        let discovery = discover(&http, ISSUER).await?;
        let issuer_note = match find("iss") {
            Some(issuer) if issuer == discovery.issuer => "issuer matches",
            Some(_) => return Err("the redirect names a different issuer".into()),
            None => "no issuer in the redirect",
        };
        report.push(format!(
            "redirect received through {}://: code of {} characters, state matches, {issuer_note}",
            redirect.scheme(),
            code.len()
        ));
        let now = u64::from(std::process::id());
        report.push(format!(
            "attempt started by process {started_by}, finished by process {now}: {}",
            if started_by == now {
                "the app stayed alive"
            } else {
                "the app was restarted in between"
            }
        ));

        let tokens = exchange_code(&http, &discovery, CLIENT_ID, &attempt, &code).await?;
        let id_token = tokens.id_token.ok_or("no ID token in the response")?;
        let keys = fetch_keys(&http, &discovery).await?;
        let claims = verify_id_token(
            &id_token,
            &keys,
            &discovery.issuer,
            CLIENT_ID,
            &attempt.nonce,
        )?;

        let mut names: Vec<&str> = claims.other.keys().map(String::as_str).collect();
        names.extend(["iss", "sub", "exp", "nonce"]);
        names.sort_unstable();
        report.push("ID token verified".to_owned());
        report.push(format!("user ID: {}", claims.sub));
        report.push(format!("claims: {}", names.join(", ")));
        report.push(format!(
            "refresh token: {}",
            if tokens.refresh_token.is_some() {
                "issued"
            } else {
                "not issued"
            }
        ));
        report.push("RESULT: PASS".to_owned());
        Ok(report)
    }

    #[component]
    pub fn App() -> Element {
        let lines = use_signal(Vec::<String>::new);

        // Watches for the redirect for as long as the app runs. A redirect
        // that arrived while the app was dead is found on the first pass.
        use_future(move || async move {
            say(
                lines,
                format!("app started as process {}", std::process::id()),
            );
            loop {
                if let Some(address) = take_redirect() {
                    match finish(&address).await {
                        Ok(report) => report.into_iter().for_each(|line| say(lines, line)),
                        Err(error) => say(lines, format!("RESULT: FAIL: {error}")),
                    }
                }
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
        });

        rsx! {
            div { style: "font-family: sans-serif; padding: 48px 16px 16px; font-size: 14px;",
                h2 { "Clerk sign-in on Android" }
                button {
                    style: "font-size: 18px; padding: 12px 24px; margin: 8px 0 16px;",
                    onclick: move |_| {
                        spawn(async move {
                            match start().await {
                                Ok(line) => say(lines, line),
                                Err(error) => say(lines, format!("could not start: {error}")),
                            }
                        });
                    },
                    "Sign in"
                }
                for line in lines.read().iter() {
                    p { style: "margin: 6px 0; word-break: break-word;", "{line}" }
                }
            }
        }
    }
}
