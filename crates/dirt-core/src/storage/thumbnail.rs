//! Thumbnail generation utilities for image attachments.

use std::io::Cursor;

use image::{codecs::jpeg::JpegEncoder, GenericImageView, ImageFormat};

use crate::{Error, Result};

/// Output format for generated thumbnails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailFormat {
    Jpeg,
    Png,
    WebP,
}

impl ThumbnailFormat {
    const fn as_image_format(self) -> ImageFormat {
        match self {
            Self::Jpeg => ImageFormat::Jpeg,
            Self::Png => ImageFormat::Png,
            Self::WebP => ImageFormat::WebP,
        }
    }
}

/// Configuration for thumbnail generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThumbnailOptions {
    /// Maximum output width in pixels.
    pub max_width: u32,
    /// Maximum output height in pixels.
    pub max_height: u32,
    /// Output image format.
    pub format: ThumbnailFormat,
    /// JPEG quality (only used when `format` is [`ThumbnailFormat::Jpeg`]).
    pub jpeg_quality: u8,
}

impl Default for ThumbnailOptions {
    fn default() -> Self {
        Self {
            max_width: 512,
            max_height: 512,
            format: ThumbnailFormat::Jpeg,
            jpeg_quality: 80,
        }
    }
}

/// Generated thumbnail payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbnailImage {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub format: ThumbnailFormat,
}

/// Generate a thumbnail image from source bytes.
///
/// The image is resized to fit within `max_width` x `max_height` while preserving
/// aspect ratio. Images smaller than the target bounds are not upscaled.
pub fn generate_thumbnail(
    source_bytes: &[u8],
    options: ThumbnailOptions,
) -> Result<ThumbnailImage> {
    if source_bytes.is_empty() {
        return Err(Error::InvalidInput(
            "Thumbnail source bytes cannot be empty".to_string(),
        ));
    }
    if options.max_width == 0 || options.max_height == 0 {
        return Err(Error::InvalidInput(
            "Thumbnail max dimensions must be greater than zero".to_string(),
        ));
    }

    let source = image::load_from_memory(source_bytes).map_err(|error| {
        Error::InvalidInput(format!(
            "Failed to decode source image for thumbnail generation: {error}"
        ))
    })?;

    let (source_width, source_height) = source.dimensions();
    let resized = if source_width <= options.max_width && source_height <= options.max_height {
        source
    } else {
        source.thumbnail(options.max_width, options.max_height)
    };
    let (width, height) = resized.dimensions();

    let bytes = encode_thumbnail(&resized, options)?;

    Ok(ThumbnailImage {
        bytes,
        width,
        height,
        format: options.format,
    })
}

fn encode_thumbnail(image: &image::DynamicImage, options: ThumbnailOptions) -> Result<Vec<u8>> {
    let mut cursor = Cursor::new(Vec::new());

    match options.format {
        ThumbnailFormat::Jpeg => {
            let mut encoder = JpegEncoder::new_with_quality(&mut cursor, options.jpeg_quality);
            encoder.encode_image(image).map_err(|error| {
                Error::InvalidInput(format!("Failed to encode JPEG thumbnail: {error}"))
            })?;
        }
        ThumbnailFormat::Png | ThumbnailFormat::WebP => {
            image
                .write_to(&mut cursor, options.format.as_image_format())
                .map_err(|error| {
                    Error::InvalidInput(format!("Failed to encode thumbnail image: {error}"))
                })?;
        }
    }

    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    fn source_png(width: u32, height: u32) -> Vec<u8> {
        let image = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_fn(width, height, |_x, _y| {
            Rgba([120, 90, 240, 255])
        });

        let mut cursor = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut cursor, ImageFormat::Png)
            .unwrap();
        cursor.into_inner()
    }

    #[test]
    fn generate_thumbnail_bounds_dimensions_and_preserves_ratio() {
        let source = source_png(800, 600);
        let result = generate_thumbnail(
            &source,
            ThumbnailOptions {
                max_width: 200,
                max_height: 200,
                format: ThumbnailFormat::Jpeg,
                jpeg_quality: 85,
            },
        )
        .unwrap();

        assert_eq!(result.width, 200);
        assert_eq!(result.height, 150);
        assert!(!result.bytes.is_empty());
    }

    #[test]
    fn generate_thumbnail_does_not_upscale_small_images() {
        let source = source_png(80, 40);
        let result = generate_thumbnail(
            &source,
            ThumbnailOptions {
                max_width: 200,
                max_height: 200,
                format: ThumbnailFormat::Png,
                jpeg_quality: 80,
            },
        )
        .unwrap();

        assert_eq!(result.width, 80);
        assert_eq!(result.height, 40);
    }

    #[test]
    fn generate_thumbnail_rejects_invalid_source() {
        let err = generate_thumbnail(b"not-an-image", ThumbnailOptions::default()).unwrap_err();
        match err {
            Error::InvalidInput(message) => {
                assert!(message.contains("decode"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
