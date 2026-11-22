//! The files of a note, as both apps show them (interface design, sections
//! 6.5, 6.16 and 7.3).
//!
//! - [`FileShelf`]: the open note's files at the foot of its page, folded
//!   to a line or unfolded to their tiles.
//! - [`FileTiles`]: the tiles, on the shelf.
//! - [`FilePreview`]: the preview over the screen.
//! - [`NoteFiles`], from [`use_note_files`]: what the two share, read from
//!   `dirt-core`.
//! - [`kind`]: what kind of file a file is, as the preview sees it.
//! - [`served`]: the bytes the preview shows, in memory, and the answer to
//!   the web view's request for them. Each app registers a handler for
//!   [`served::ADDRESS`] that calls [`served::answer`].
//! - [`pdf`]: the bundled PDF renderer.
//! - [`use_pasted_files`]: a picture pasted into the page.
//!
//! Every byte is had from `dirt-core`, which fetches and decrypts; nothing
//! here is cryptography or writes a file.

pub mod kind;
mod note_files;
mod pasted;
pub mod pdf;
mod preview;
pub mod served;
mod shelf;
mod tile;

pub use note_files::{use_note_files, FileSource, NoteFiles, Showing};
pub use pasted::{use_pasted_files, Pasted};
pub use preview::FilePreview;
pub use shelf::{
    count_told, count_words, mini_element_id, FileMini, FileShelf, ShelfFrame, UNFOLDED_MOST,
};
pub use tile::{tile_element_id, FileTile, FileTiles};
