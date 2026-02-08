//! Storage abstractions for media/object backends.

mod r2;
mod thumbnail;
mod voice_memo;

pub use r2::{MediaStorage, R2Config, R2Storage};
pub use thumbnail::{generate_thumbnail, ThumbnailFormat, ThumbnailImage, ThumbnailOptions};
pub use voice_memo::{encode_voice_memo_wav, estimate_voice_memo_duration_ms, VoiceMemoOptions};
