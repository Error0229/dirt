//! The design's tokens (interface design, section 4), and the style sheet
//! made from them.
//!
//! The colours are written once, here. [`css`] makes the style sheet's
//! custom properties from these tables, and the contrast test reads the
//! same tables, so a colour cannot be changed without being checked.

use std::fmt::Write as _;

use dirt_core::models::Accent;

/// A colour, as `0xRRGGBB`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub u32);

impl Color {
    /// White: text on a filled button in the light theme.
    pub const WHITE: Self = Self(0x00FF_FFFF);

    /// The red, green and blue parts, 0 to 255.
    #[must_use]
    pub const fn rgb(self) -> (u8, u8, u8) {
        let [_, red, green, blue] = self.0.to_be_bytes();
        (red, green, blue)
    }

    /// The colour as CSS: `#RRGGBB`.
    #[must_use]
    pub fn hex(self) -> String {
        format!("#{:06X}", self.0 & 0x00FF_FFFF)
    }

    /// The colour as CSS at `percent` opacity.
    #[must_use]
    pub fn with_alpha(self, percent: u8) -> String {
        let (red, green, blue) = self.rgb();
        format!("rgba({red},{green},{blue},.{percent:02})")
    }
}

/// Light or dark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// The light theme.
    Light,
    /// The dark theme.
    Dark,
}

impl Scheme {
    /// Both schemes.
    pub const BOTH: [Self; 2] = [Self::Light, Self::Dark];
}

/// A colour that does not change with the accent.
#[derive(Debug, Clone, Copy)]
pub struct Neutral {
    /// The custom property's name, without the dashes.
    pub name: &'static str,
    /// In the light theme.
    pub light: Color,
    /// In the dark theme.
    pub dark: Color,
}

impl Neutral {
    const fn new(name: &'static str, light: u32, dark: u32) -> Self {
        Self {
            name,
            light: Color(light),
            dark: Color(dark),
        }
    }

    /// The colour in `scheme`.
    #[must_use]
    pub const fn in_scheme(&self, scheme: Scheme) -> Color {
        match scheme {
            Scheme::Light => self.light,
            Scheme::Dark => self.dark,
        }
    }
}

/// Section 4.1's table, without the five colours an accent replaces.
///
/// `on-danger` is not in the first version of that table: it is the text on
/// the red fill of a final confirmation, which was white in both themes and
/// failed on the dark theme's lighter red.
pub const NEUTRALS: &[Neutral] = &[
    Neutral::new("bg", 0x00FA_F8F5, 0x0016_1412),
    Neutral::new("surface", 0x00FF_FFFF, 0x001E_1B18),
    Neutral::new("surface-2", 0x00F3_F0EB, 0x0027_231F),
    Neutral::new("surface-3", 0x00EA_E5DE, 0x0030_2B26),
    Neutral::new("border", 0x00E7_E2DA, 0x0033_2E29),
    Neutral::new("border-strong", 0x00D3_CBC0, 0x0046_3F38),
    Neutral::new("border-control", 0x008F_877D, 0x0077_6B5F),
    Neutral::new("text", 0x001F_1B16, 0x00ED_E8E1),
    Neutral::new("text-2", 0x0066_5F56, 0x00A9_A096),
    Neutral::new("text-3", 0x0073_6B62, 0x0093_8B81),
    Neutral::new("success", 0x002F_7D4F, 0x005D_BB84),
    Neutral::new("warning", 0x009A_5D10, 0x00E0_A045),
    Neutral::new("danger", 0x00BE_3A2B, 0x00E8_6A5A),
    Neutral::new("danger-soft", 0x00F9_E3E0, 0x003A_1E1A),
    Neutral::new("on-danger", 0x00FF_FFFF, 0x001A_0F0A),
    Neutral::new("mark", 0x00F7_DDB0, 0x005A_3F1C),
    Neutral::new("inverse", 0x002A_2520, 0x00ED_E8E1),
    Neutral::new("on-inverse", 0x00F5_F1EB, 0x001E_1B18),
];

/// The neutral colour called `name` in `scheme`.
///
/// # Panics
///
/// If there is no neutral of that name: the names are this file's own.
#[must_use]
pub fn neutral(name: &str, scheme: Scheme) -> Color {
    NEUTRALS
        .iter()
        .find(|neutral| neutral.name == name)
        .map_or_else(
            || panic!("no neutral colour is called {name}"),
            |neutral| neutral.in_scheme(scheme),
        )
}

/// The five colours an accent sets (section 4.1.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccentColors {
    /// `--accent`: tags, the primary button, focus.
    pub accent: Color,
    /// `--accent-hover`: the primary button under the pointer.
    pub hover: Color,
    /// `--accent-soft`: the focus halo, a tag token.
    pub soft: Color,
    /// `--on-accent`: text and icons on the accent.
    pub on_accent: Color,
    /// `--accent-inverse`: **Undo** on a toast.
    pub inverse: Color,
}

const fn accent_set(
    accent: u32,
    hover: u32,
    soft: u32,
    on_accent: u32,
    inverse: u32,
) -> AccentColors {
    AccentColors {
        accent: Color(accent),
        hover: Color(hover),
        soft: Color(soft),
        on_accent: Color(on_accent),
        inverse: Color(inverse),
    }
}

/// The colours of `accent` in `scheme`.
#[must_use]
pub const fn accent_colors(accent: Accent, scheme: Scheme) -> AccentColors {
    match (accent, scheme) {
        (Accent::Clay, Scheme::Light) => accent_set(
            0x00AC_4A26,
            0x0093_3F20,
            0x00F8_E6DD,
            0x00FF_FFFF,
            0x00F2_9A76,
        ),
        (Accent::Clay, Scheme::Dark) => accent_set(
            0x00E2_774F,
            0x00EC_8B66,
            0x003A_2419,
            0x001A_0F0A,
            0x00B2_471F,
        ),
        (Accent::Moss, Scheme::Light) => accent_set(
            0x003C_754B,
            0x0033_6340,
            0x00E3_EFE5,
            0x00FF_FFFF,
            0x008F_D0A0,
        ),
        (Accent::Moss, Scheme::Dark) => accent_set(
            0x006F_BF85,
            0x0085_CC98,
            0x001E_3324,
            0x000E_1A12,
            0x002F_6B40,
        ),
        (Accent::Lake, Scheme::Light) => accent_set(
            0x002E_6AB0,
            0x0028_5B98,
            0x00E2_ECF7,
            0x00FF_FFFF,
            0x008D_BDF0,
        ),
        (Accent::Lake, Scheme::Dark) => accent_set(
            0x006F_A8E8,
            0x0088_B9EE,
            0x001A_2A3D,
            0x000B_1626,
            0x0028_5E9E,
        ),
        (Accent::Plum, Scheme::Light) => accent_set(
            0x008A_4A9E,
            0x0074_3C86,
            0x00F1_E5F4,
            0x00FF_FFFF,
            0x00D6_A8E4,
        ),
        (Accent::Plum, Scheme::Dark) => accent_set(
            0x00C7_92D8,
            0x00D3_A6E1,
            0x0033_233A,
            0x001C_1020,
            0x007A_3F8C,
        ),
        (Accent::Honey, Scheme::Light) => accent_set(
            0x008D_610D,
            0x0078_530B,
            0x00F6_EBD3,
            0x00FF_FFFF,
            0x00EA_C47A,
        ),
        (Accent::Honey, Scheme::Dark) => accent_set(
            0x00E0_B14F,
            0x00E8_C06A,
            0x003A_2E14,
            0x001C_1505,
            0x008A_5E0C,
        ),
        (Accent::Graphite, Scheme::Light) => accent_set(
            0x003D_3832,
            0x002A_2622,
            0x00EC_E8E2,
            0x00FF_FFFF,
            0x00E2_DCD3,
        ),
        (Accent::Graphite, Scheme::Dark) => accent_set(
            0x00D9_D2C8,
            0x00E8_E2D9,
            0x002E_2A26,
            0x001A_1714,
            0x003D_3832,
        ),
    }
}

/// How strong the focus ring's colour is: the accent at this many percent.
const fn focus_percent(scheme: Scheme) -> u8 {
    match scheme {
        Scheme::Light => 45,
        Scheme::Dark => 55,
    }
}

/// The three shadows (section 4.3). The dark theme's are deeper and blacker.
const fn shadows(scheme: Scheme) -> &'static str {
    match scheme {
        Scheme::Light => {
            "--shadow-1:0 1px 2px rgba(31,27,22,.06),0 1px 1px rgba(31,27,22,.04);\
             --shadow-2:0 6px 16px rgba(31,27,22,.10),0 2px 4px rgba(31,27,22,.06);\
             --shadow-3:0 24px 56px rgba(31,27,22,.18),0 6px 14px rgba(31,27,22,.08);"
        }
        Scheme::Dark => {
            "--shadow-1:0 1px 2px rgba(0,0,0,.4);\
             --shadow-2:0 8px 20px rgba(0,0,0,.45),0 2px 4px rgba(0,0,0,.3);\
             --shadow-3:0 28px 60px rgba(0,0,0,.6),0 6px 14px rgba(0,0,0,.4);"
        }
    }
}

/// What does not change with the theme: type, spacing and radii (sections
/// 4.2 and 4.3). `--note-size` is the text size setting; the root component
/// sets it.
const FIXED: &str = "\
--font:\"Inter\",ui-sans-serif,system-ui,-apple-system,\"Segoe UI\",Roboto,sans-serif;\
--mono:\"JetBrains Mono\",ui-monospace,\"Cascadia Code\",Consolas,monospace;\
--note-size:15px;\
--s-1:4px;--s-2:8px;--s-3:12px;--s-4:16px;--s-5:20px;--s-6:24px;--s-7:32px;--s-8:44px;\
--r-s:6px;--r-b:8px;--r-m:10px;--r-l:14px;--r-card:16px;--r-sheet:24px;--r-bar:26px;--r-full:999px;";

/// The custom properties an accent sets, with the focus ring made from it.
fn accent_properties(accent: Accent, scheme: Scheme) -> String {
    let colors = accent_colors(accent, scheme);
    format!(
        "--accent:{};--accent-hover:{};--accent-soft:{};--on-accent:{};--accent-inverse:{};--focus:{};",
        colors.accent.hex(),
        colors.hover.hex(),
        colors.soft.hex(),
        colors.on_accent.hex(),
        colors.inverse.hex(),
        colors.accent.with_alpha(focus_percent(scheme)),
    )
}

/// Every property that changes with the theme, for the default accent.
fn scheme_properties(scheme: Scheme) -> String {
    let mut css = String::new();
    for neutral in NEUTRALS {
        let _ = write!(
            css,
            "--{}:{};",
            neutral.name,
            neutral.in_scheme(scheme).hex()
        );
    }
    css.push_str(&accent_properties(Accent::Clay, scheme));
    // Each accent's own colour, for the dots that offer it (section 6.11).
    for accent in Accent::ALL {
        let _ = write!(
            css,
            "--dot-{}:{};",
            accent.name(),
            accent_colors(accent, scheme).accent.hex()
        );
    }
    css.push_str(shadows(scheme));
    css.push_str(match scheme {
        Scheme::Light => "color-scheme:light;",
        Scheme::Dark => "color-scheme:dark;",
    });
    css
}

/// The tokens as a style sheet.
///
/// The dark theme is under `[data-theme="dark"]`, and under
/// `prefers-color-scheme: dark` unless `[data-theme="light"]`, so that with
/// no theme chosen the system's is followed without any code running.
#[must_use]
pub fn css() -> String {
    let mut css = String::with_capacity(8 * 1024);
    let light = scheme_properties(Scheme::Light);
    let dark = scheme_properties(Scheme::Dark);

    let _ = write!(css, ":root{{{FIXED}{light}}}");
    let _ = write!(
        css,
        "@media (prefers-color-scheme:dark){{:root:not([data-theme=\"light\"]){{{dark}}}}}"
    );
    let _ = write!(css, ":root[data-theme=\"dark\"]{{{dark}}}");

    // Clay is the default and needs no rule of its own.
    let others = || {
        Accent::ALL
            .into_iter()
            .filter(|accent| *accent != Accent::Clay)
    };
    for accent in others() {
        let _ = write!(
            css,
            ":root[data-accent=\"{}\"]{{{}}}",
            accent.name(),
            accent_properties(accent, Scheme::Light)
        );
    }
    css.push_str("@media (prefers-color-scheme:dark){");
    for accent in others() {
        let _ = write!(
            css,
            ":root:not([data-theme=\"light\"])[data-accent=\"{}\"]{{{}}}",
            accent.name(),
            accent_properties(accent, Scheme::Dark)
        );
    }
    css.push('}');
    for accent in others() {
        let _ = write!(
            css,
            ":root[data-theme=\"dark\"][data-accent=\"{}\"]{{{}}}",
            accent.name(),
            accent_properties(accent, Scheme::Dark)
        );
    }
    css
}

/// The colours as Android resources.
///
/// They are for the native parts of the Android app that are not drawn in
/// the web view: the capture sheet, the widget and the tile (section 8.5).
/// One file for each scheme, `values` and
/// `values-night`, so that Android picks the one in use. Every neutral is
/// there as `dirt_<name>`, and every accent's colours as
/// `dirt_accent_<accent>`, `dirt_accent_soft_<accent>` and
/// `dirt_on_accent_<accent>`.
///
/// The files themselves are in `dirt-mobile`, and a test there holds them
/// to this, so that the colours are still written only here.
#[must_use]
pub fn android_colours(scheme: Scheme) -> String {
    let name = |text: &str| text.replace('-', "_");
    let hex = |color: Color| format!("#FF{:06X}", color.0 & 0x00FF_FFFF);
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <!-- Made from dirt_ui::tokens by `dirt_ui::tokens::android_colours`;\n     \
         not to be changed by hand. dirt-mobile's test `colours_are_the_tokens`\n     \
         writes it again with DIRT_WRITE_ANDROID_COLOURS=1. -->\n<resources>\n",
    );
    for neutral in NEUTRALS {
        let _ = writeln!(
            xml,
            "    <color name=\"dirt_{}\">{}</color>",
            name(neutral.name),
            hex(neutral.in_scheme(scheme))
        );
    }
    for accent in Accent::ALL {
        let colors = accent_colors(accent, scheme);
        let accent = accent.name();
        let _ = writeln!(
            xml,
            "    <color name=\"dirt_accent_{accent}\">{}</color>",
            hex(colors.accent)
        );
        let _ = writeln!(
            xml,
            "    <color name=\"dirt_accent_soft_{accent}\">{}</color>",
            hex(colors.soft)
        );
        let _ = writeln!(
            xml,
            "    <color name=\"dirt_on_accent_{accent}\">{}</color>",
            hex(colors.on_accent)
        );
    }
    xml.push_str("</resources>\n");
    xml
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colour_is_written_as_css() {
        assert_eq!(Color(0x00AC_4A26).hex(), "#AC4A26");
        assert_eq!(Color(0x00AC_4A26).with_alpha(45), "rgba(172,74,38,.45)");
        assert_eq!(Color(0x0000_0A0B).hex(), "#000A0B");
    }

    #[test]
    fn the_sheet_has_both_themes_and_every_accent() {
        let css = css();
        assert!(css.starts_with(":root{"));
        assert!(css.contains("--bg:#FAF8F5;"));
        assert!(css.contains(":root[data-theme=\"dark\"]{--bg:#161412;"));
        assert!(css.contains(
            "@media (prefers-color-scheme:dark){:root:not([data-theme=\"light\"]){--bg:#161412;"
        ));
        for accent in Accent::ALL {
            if accent == Accent::Clay {
                continue;
            }
            let name = accent.name();
            assert!(css.contains(&format!(":root[data-accent=\"{name}\"]{{--accent:")));
            assert!(css.contains(&format!(
                ":root[data-theme=\"dark\"][data-accent=\"{name}\"]{{--accent:"
            )));
            assert!(css.contains(&format!(
                ":root:not([data-theme=\"light\"])[data-accent=\"{name}\"]{{--accent:"
            )));
        }
        // The mockup's own focus rings, which are the accent at 45% and 55%.
        assert!(css.contains("--focus:rgba(172,74,38,.45);"));
        assert!(css.contains("--focus:rgba(226,119,79,.55);"));
        assert_eq!(css.matches('{').count(), css.matches('}').count());
    }
}
