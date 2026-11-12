//! A small picture of a picture.
//!
//! The server cannot make one: it never sees a file. So the device that
//! attaches a picture makes its thumbnail, and the thumbnail is a file of
//! its own, with its own key, sent and fetched like any other. A list of
//! attachments can then show pictures without fetching them whole.
//!
//! A file is tried as a picture whatever it says it is: what it is, is
//! read from its first bytes. One that is not a picture this can read
//! simply has no thumbnail.

use std::io::Cursor;
use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageDecoder, ImageReader, Limits, RgbImage};

/// A thumbnail fits in a square of this many pixels a side.
pub const EDGE: u32 = 256;

/// The most memory opening a picture may take. A picture is opened whole
/// to be made small, at three or four bytes a pixel, so this is a
/// photograph of about forty million pixels. A larger one is attached
/// without a thumbnail. The limit is also what stops a small file that
/// claims to be an enormous picture.
const MAX_OPEN_BYTES: u64 = 128 * 1024 * 1024;

/// The longest side a picture may have and still be opened.
const MAX_SIDE: u32 = 20_000;

/// How hard the thumbnail is compressed, of a hundred.
const QUALITY: u8 = 80;

/// Makes the thumbnail of the picture at `path`.
///
/// It is a JPEG that fits in [`EDGE`] pixels a side, upright, with nothing
/// of the original but its pixels: where and when a photograph was taken
/// is not carried over. `None` if the file is not a picture this can
/// read, or is too large to open.
#[must_use]
pub fn make(path: &Path) -> Option<Vec<u8>> {
    make_within(path, MAX_OPEN_BYTES)
}

/// [`make`], with the most memory that opening the picture may take.
fn make_within(path: &Path, most_bytes: u64) -> Option<Vec<u8>> {
    let mut reader = ImageReader::open(path).ok()?.with_guessed_format().ok()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(most_bytes);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().ok()?;
    // The limits above bound what the decoder sets aside for its own
    // work. The opened picture itself is set aside by the call below,
    // which looks at no limit, so it is checked here, against what the
    // file claims to be, before anything of that size exists.
    if decoder.total_bytes() > most_bytes {
        return None;
    }
    // A camera stores a photograph as its sensor lies and notes which way
    // up it was held.
    let orientation = decoder.orientation().ok()?;
    let mut picture = DynamicImage::from_decoder(decoder).ok()?;
    picture.apply_orientation(orientation);
    // Made smaller, never larger.
    let small = if picture.width() > EDGE || picture.height() > EDGE {
        picture.thumbnail(EDGE, EDGE)
    } else {
        picture
    };
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(Cursor::new(&mut jpeg), QUALITY)
        .encode_image(&on_white(&small))
        .ok()?;
    Some(jpeg)
}

/// The picture with what is see-through in it shown on white. A JPEG has
/// no transparency, and left alone a see-through background comes out
/// black.
fn on_white(picture: &DynamicImage) -> RgbImage {
    let rgba = picture.to_rgba8();
    let mut flat = RgbImage::new(rgba.width(), rgba.height());
    for (from, to) in rgba.pixels().zip(flat.pixels_mut()) {
        let alpha = u16::from(from.0[3]);
        for channel in 0..3 {
            let mixed = (u16::from(from.0[channel]) * alpha + 255 * (255 - alpha) + 127) / 255;
            to.0[channel] = u8::try_from(mixed).unwrap_or(u8::MAX);
        }
    }
    flat
}

#[cfg(test)]
mod tests {
    use image::{ImageFormat, Rgba, RgbaImage};

    use super::*;

    fn saved(
        folder: &tempfile::TempDir,
        picture: &RgbaImage,
        format: ImageFormat,
    ) -> std::path::PathBuf {
        let path = folder.path().join("picture");
        let picture = DynamicImage::ImageRgba8(picture.clone());
        if format == ImageFormat::Jpeg {
            picture.to_rgb8().save_with_format(&path, format).unwrap();
        } else {
            picture.save_with_format(&path, format).unwrap();
        }
        path
    }

    fn opened(jpeg: &[u8]) -> DynamicImage {
        assert_eq!(image::guess_format(jpeg).unwrap(), ImageFormat::Jpeg);
        image::load_from_memory(jpeg).unwrap()
    }

    #[test]
    fn a_large_picture_is_made_small_and_keeps_its_shape() {
        let folder = tempfile::tempdir().unwrap();
        let wide = RgbaImage::from_pixel(1_200, 300, Rgba([200, 40, 40, 255]));
        for format in [
            ImageFormat::Png,
            ImageFormat::Jpeg,
            ImageFormat::Gif,
            ImageFormat::WebP,
        ] {
            let thumbnail = opened(&make(&saved(&folder, &wide, format)).unwrap());
            assert_eq!(
                (thumbnail.width(), thumbnail.height()),
                (256, 64),
                "{format:?}"
            );
            // Still red, within what the compression changes.
            let pixel = thumbnail.to_rgb8().get_pixel(128, 32).0;
            assert!(
                pixel[0] > 170 && pixel[1] < 80 && pixel[2] < 80,
                "{format:?} {pixel:?}"
            );
        }
        let tall = RgbaImage::from_pixel(300, 1_200, Rgba([0, 0, 0, 255]));
        let thumbnail = opened(&make(&saved(&folder, &tall, ImageFormat::Png)).unwrap());
        assert_eq!((thumbnail.width(), thumbnail.height()), (64, 256));
    }

    #[test]
    fn a_small_picture_is_not_made_larger() {
        let folder = tempfile::tempdir().unwrap();
        let icon = RgbaImage::from_pixel(40, 30, Rgba([10, 120, 10, 255]));
        let thumbnail = opened(&make(&saved(&folder, &icon, ImageFormat::Png)).unwrap());
        assert_eq!((thumbnail.width(), thumbnail.height()), (40, 30));
    }

    #[test]
    fn what_is_see_through_is_shown_on_white() {
        let folder = tempfile::tempdir().unwrap();
        let mut logo = RgbaImage::from_pixel(64, 64, Rgba([0, 0, 0, 0]));
        for x in 0..32 {
            for y in 0..64 {
                logo.put_pixel(x, y, Rgba([0, 0, 200, 255]));
            }
        }
        let thumbnail = opened(&make(&saved(&folder, &logo, ImageFormat::Png)).unwrap()).to_rgb8();
        let clear = thumbnail.get_pixel(56, 32).0;
        assert!(clear.iter().all(|channel| *channel > 235), "{clear:?}");
        let solid = thumbnail.get_pixel(8, 32).0;
        assert!(solid[2] > 150 && solid[0] < 60, "{solid:?}");
    }

    #[test]
    fn a_file_that_is_not_a_picture_has_no_thumbnail() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("notes.png");
        // It says it is a picture. It is not.
        std::fs::write(&path, b"minutes of the meeting").unwrap();
        assert_eq!(make(&path), None);
        std::fs::write(&path, b"").unwrap();
        assert_eq!(make(&path), None);
        assert_eq!(make(&folder.path().join("no such file")), None);

        // A picture cut short.
        let whole = saved(
            &folder,
            &RgbaImage::from_pixel(600, 600, Rgba([9, 9, 9, 255])),
            ImageFormat::Png,
        );
        let bytes = std::fs::read(&whole).unwrap();
        std::fs::write(&whole, &bytes[..bytes.len() / 2]).unwrap();
        assert_eq!(make(&whole), None);
    }

    /// A photograph is stored as the camera's sensor lay, with a note of
    /// which way up the camera was held. The thumbnail is upright.
    #[test]
    fn a_photograph_taken_sideways_is_shown_upright() {
        use image::ImageEncoder;

        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("photo.jpg");
        // Wide as stored: red on the left, blue on the right.
        let mut stored = image::RgbImage::from_pixel(400, 200, image::Rgb([220, 20, 20]));
        for x in 200..400 {
            for y in 0..200 {
                stored.put_pixel(x, y, image::Rgb([20, 20, 220]));
            }
        }
        // The note a camera leaves: turn a quarter clockwise to see it
        // upright. One entry, in the layout cameras use.
        let turn_clockwise: Vec<u8> = [
            b"MM\x00\x2a\x00\x00\x00\x08".as_slice(),
            b"\x00\x01",
            b"\x01\x12\x00\x03\x00\x00\x00\x01\x00\x06\x00\x00",
            b"\x00\x00\x00\x00",
        ]
        .concat();
        let mut encoder = JpegEncoder::new_with_quality(std::fs::File::create(&path).unwrap(), 95);
        encoder.set_exif_metadata(turn_clockwise).unwrap();
        encoder
            .write_image(&stored, 400, 200, image::ExtendedColorType::Rgb8)
            .unwrap();

        let thumbnail = opened(&make(&path).unwrap()).to_rgb8();

        // Tall now, with what was on the left at the top.
        assert_eq!((thumbnail.width(), thumbnail.height()), (128, 256));
        let top = thumbnail.get_pixel(64, 40).0;
        let bottom = thumbnail.get_pixel(64, 216).0;
        assert!(top[0] > 150 && top[2] < 90, "{top:?}");
        assert!(bottom[2] > 150 && bottom[0] < 90, "{bottom:?}");
    }

    /// A picture that would take more memory to open than is allowed has
    /// no thumbnail. The file is attached all the same.
    #[test]
    fn a_picture_too_large_to_open_has_no_thumbnail() {
        let folder = tempfile::tempdir().unwrap();
        // 600 by 600 at four bytes a pixel: 1,440,000 bytes to open.
        let path = saved(
            &folder,
            &RgbaImage::from_pixel(600, 600, Rgba([9, 9, 9, 255])),
            ImageFormat::Png,
        );
        assert_eq!(make_within(&path, 1_000_000), None);
        assert!(make_within(&path, 8_000_000).is_some());
        assert!(make(&path).is_some());
    }

    /// A few hundred bytes that claim to be a picture of four hundred
    /// million pixels. It is refused by what it claims, before anything
    /// that size is set aside for it.
    #[test]
    fn a_small_file_that_claims_to_be_enormous_is_not_opened() {
        let folder = tempfile::tempdir().unwrap();
        let path = saved(
            &folder,
            &RgbaImage::from_pixel(8, 8, Rgba([1, 2, 3, 255])),
            ImageFormat::Png,
        );
        let mut bytes = std::fs::read(&path).unwrap();
        // The width and height are the eight bytes after the header's
        // name, and the four after them are its checksum.
        let header = 16;
        bytes[header..header + 4].copy_from_slice(&19_000u32.to_be_bytes());
        bytes[header + 4..header + 8].copy_from_slice(&19_000u32.to_be_bytes());
        let checksum = crc32(&bytes[12..29]);
        bytes[29..33].copy_from_slice(&checksum.to_be_bytes());
        std::fs::write(&path, &bytes).unwrap();
        // It is a picture as far as its header goes, of that size.
        let claimed = ImageReader::open(&path)
            .unwrap()
            .with_guessed_format()
            .unwrap()
            .into_dimensions()
            .unwrap();
        assert_eq!(claimed, (19_000, 19_000));

        assert_eq!(make(&path), None);
    }

    /// The checksum a PNG keeps for each of its parts.
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = u32::MAX;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }
}
