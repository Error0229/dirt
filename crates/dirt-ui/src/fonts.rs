//! The two typefaces, bundled with the apps so that nothing is fetched at
//! run time (interface design, section 4.2).
//!
//! Inter is the variable font: the design uses weights between the named
//! ones (550, 560). The monospaced one, `JetBrains Mono`, is used at one
//! weight, 500, for shortcuts and recovery words. Both are under the SIL
//! Open Font License; the licences are beside the files in `assets/fonts`.

use dioxus::prelude::*;

/// Inter, every weight from 100 to 900 in one file.
pub const INTER: Asset = asset!("/assets/fonts/InterVariable.woff2");

/// `JetBrains Mono`, weight 500.
pub const JETBRAINS_MONO: Asset = asset!("/assets/fonts/JetBrainsMono-Medium.woff2");

/// The `@font-face` rules for the two. Text is drawn in the system's font
/// until a file has been read (`swap`): nothing waits for a font.
#[must_use]
pub fn css() -> String {
    format!(
        "@font-face{{font-family:\"Inter\";font-style:normal;font-weight:100 900;font-display:swap;\
         src:url(\"{INTER}\") format(\"woff2\")}}\
         @font-face{{font-family:\"JetBrains Mono\";font-style:normal;font-weight:500;font-display:swap;\
         src:url(\"{JETBRAINS_MONO}\") format(\"woff2\")}}"
    )
}
