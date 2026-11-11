//! On Android: a one-screen app that runs the trial on start and shows the
//! report. The report is also written to `report.txt` in the app's files
//! folder, where `adb shell run-as` can read it.
//!
//! On the host: prints the report.

use rusqlite_android_trial::{data_folder, run_trial};

#[cfg(target_os = "android")]
fn main() {
    dioxus::launch(app::App);
}

#[cfg(not(target_os = "android"))]
fn main() {
    let report = run_trial(&data_folder());
    print!("{}", report.to_text());
    if !report.passed() {
        std::process::exit(1);
    }
}

#[cfg(target_os = "android")]
mod app {
    use dioxus::prelude::*;

    use super::{data_folder, run_trial};

    #[component]
    pub fn App() -> Element {
        let report = use_hook(|| {
            let folder = data_folder();
            let report = run_trial(&folder);
            let _ = std::fs::write(folder.join("report.txt"), report.to_text());
            report
        });
        let passed = report.passed();

        rsx! {
            div { style: "font-family: sans-serif; padding: 48px 16px 16px; font-size: 14px;",
                h2 { "rusqlite on Android" }
                p {
                    style: if passed { "color: #1b7f3b; font-weight: bold;" } else { "color: #b3261e; font-weight: bold;" },
                    if passed { "RESULT: PASS" } else { "RESULT: FAIL" }
                }
                for line in report.lines.iter() {
                    p { style: "margin: 6px 0; word-break: break-word;",
                        span {
                            style: match line.ok {
                                Some(true) => "color: #1b7f3b; font-weight: bold;",
                                Some(false) => "color: #b3261e; font-weight: bold;",
                                None => "color: #666;",
                            },
                            match line.ok {
                                Some(true) => "PASS ",
                                Some(false) => "FAIL ",
                                None => "",
                            }
                        }
                        b { "{line.label}: " }
                        "{line.value}"
                    }
                }
            }
        }
    }
}
