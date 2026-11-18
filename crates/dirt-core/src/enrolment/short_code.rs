//! The code a person compares on two devices: seven pictures, each with
//! its name (interface design, section 6.13), and the same code as three
//! groups of four digits for a screen that cannot draw pictures.
//!
//! Both come from the same six bytes of the shared secret: the pictures
//! from their first 42 bits, six to a picture, and the digits from their
//! first 39 bits, thirteen to a group. So two devices with the same
//! pictures have the same digits; two with the same digits agree on 39 of
//! the pictures' 42 bits, and may still differ in the last picture or two.
//! Comparing pictures is the stronger check, and is what the apps ask for.
//! The pictures and their names are the table of Matrix's short-code
//! verification, which `vodozemac` gives the indices into.

use std::fmt;

/// One of the pictures of a short code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Picture {
    /// The emoji.
    pub emoji: &'static str,
    /// What it is called, in English: said beside it, so that two devices
    /// whose fonts draw it differently are still compared by its name.
    pub name: &'static str,
}

/// The 64 pictures, in the order of Matrix's specification
/// (client-server API, "SAS method: emoji").
const PICTURES: [Picture; 64] = {
    const fn p(emoji: &'static str, name: &'static str) -> Picture {
        Picture { emoji, name }
    }
    [
        p("🐶", "Dog"),
        p("🐱", "Cat"),
        p("🦁", "Lion"),
        p("🐎", "Horse"),
        p("🦄", "Unicorn"),
        p("🐷", "Pig"),
        p("🐘", "Elephant"),
        p("🐰", "Rabbit"),
        p("🐼", "Panda"),
        p("🐓", "Rooster"),
        p("🐧", "Penguin"),
        p("🐢", "Turtle"),
        p("🐟", "Fish"),
        p("🐙", "Octopus"),
        p("🦋", "Butterfly"),
        p("🌷", "Flower"),
        p("🌳", "Tree"),
        p("🌵", "Cactus"),
        p("🍄", "Mushroom"),
        p("🌏", "Globe"),
        p("🌙", "Moon"),
        p("☁️", "Cloud"),
        p("🔥", "Fire"),
        p("🍌", "Banana"),
        p("🍎", "Apple"),
        p("🍓", "Strawberry"),
        p("🌽", "Corn"),
        p("🍕", "Pizza"),
        p("🎂", "Cake"),
        p("❤️", "Heart"),
        p("😀", "Smiley"),
        p("🤖", "Robot"),
        p("🎩", "Hat"),
        p("👓", "Glasses"),
        p("🔧", "Spanner"),
        p("🎅", "Santa"),
        p("👍", "Thumbs Up"),
        p("☂️", "Umbrella"),
        p("⌛", "Hourglass"),
        p("⏰", "Clock"),
        p("🎁", "Gift"),
        p("💡", "Light Bulb"),
        p("📕", "Book"),
        p("✏️", "Pencil"),
        p("📎", "Paperclip"),
        p("✂️", "Scissors"),
        p("🔒", "Lock"),
        p("🔑", "Key"),
        p("🔨", "Hammer"),
        p("☎️", "Telephone"),
        p("🏁", "Flag"),
        p("🚂", "Train"),
        p("🚲", "Bicycle"),
        p("✈️", "Aeroplane"),
        p("🚀", "Rocket"),
        p("🏆", "Trophy"),
        p("⚽", "Ball"),
        p("🎸", "Guitar"),
        p("🎺", "Trumpet"),
        p("🔔", "Bell"),
        p("⚓", "Anchor"),
        p("🎧", "Headphones"),
        p("📁", "Folder"),
        p("📌", "Pin"),
    ]
};

/// The code both devices show while the person compares them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShortCode {
    pictures: [u8; 7],
    digits: (u16, u16, u16),
}

impl ShortCode {
    pub(crate) const fn new(pictures: [u8; 7], digits: (u16, u16, u16)) -> Self {
        Self { pictures, digits }
    }

    /// The seven pictures, in order.
    #[must_use]
    pub fn pictures(&self) -> [Picture; 7] {
        // Each index is six bits, so always in the table.
        self.pictures
            .map(|index| PICTURES[usize::from(index) % PICTURES.len()])
    }

    /// The same code as three groups of four digits: "1234 5678 9012".
    #[must_use]
    pub fn digits(&self) -> String {
        let (a, b, c) = self.digits;
        format!("{a} {b} {c}")
    }
}

impl fmt::Display for ShortCode {
    /// The pictures with their names, then the digits, on one line, for a
    /// terminal: "🐶 Dog · 🔑 Key · … (1234 5678 9012)".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let named: Vec<String> = self
            .pictures()
            .iter()
            .map(|picture| format!("{} {}", picture.emoji, picture.name))
            .collect();
        write!(f, "{} ({})", named.join(" · "), self.digits())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_matrix_s() {
        // Spot checks against the specification's table.
        assert_eq!(PICTURES[0].name, "Dog");
        assert_eq!(PICTURES[21].name, "Cloud");
        assert_eq!(PICTURES[47].name, "Key");
        assert_eq!(PICTURES[63].name, "Pin");
        let mut names: Vec<&str> = PICTURES.iter().map(|picture| picture.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 64);
    }

    #[test]
    fn a_code_reads_as_pictures_and_digits() {
        let code = ShortCode::new([0, 1, 2, 3, 4, 5, 63], (1234, 5678, 9012));
        assert_eq!(
            code.pictures()[6],
            Picture {
                emoji: "📌",
                name: "Pin"
            }
        );
        assert_eq!(code.digits(), "1234 5678 9012");
        assert!(code.to_string().starts_with("🐶 Dog · 🐱 Cat"));
        assert!(code.to_string().ends_with("📌 Pin (1234 5678 9012)"));
    }
}
