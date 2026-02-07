//! Storage abstractions for media/object backends.

mod r2;
mod thumbnail;

pub use r2::{MediaStorage, R2Config, R2Storage};
pub use thumbnail::{generate_thumbnail, ThumbnailFormat, ThumbnailImage, ThumbnailOptions};
