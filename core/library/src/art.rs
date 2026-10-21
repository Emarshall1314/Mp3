//! Artwork cache: originals deduplicated by content hash, thumbnails generated on demand.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use fast_image_resize::images::Image;
use fast_image_resize::{IntoImageView, Resizer};
use image::{ImageEncoder, ImageReader};

use crate::{Error, Result};

/// Thumbnail edge lengths the cache produces; requests round up to the next one.
pub const THUMB_SIZES: &[u32] = &[64, 128, 256, 512, 1024];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredArt {
    pub hash: String,
    pub mime: &'static str,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
}

#[derive(Debug)]
pub(crate) struct ArtCache {
    dir: PathBuf,
}

fn sniff(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(("image/jpeg", "jpg"))
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some(("image/png", "png"))
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(("image/webp", "webp"))
    } else if bytes.starts_with(b"GIF8") {
        Some(("image/gif", "gif"))
    } else if bytes.starts_with(b"BM") {
        Some(("image/bmp", "bmp"))
    } else {
        None
    }
}

/// Writes atomically: temp file in the same directory, then rename.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Io(std::io::Error::other("no parent dir")))?;
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(".{}.tmp", uuid::Uuid::now_v7()));
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// JPEG must contain its end-of-image marker near the end (some writers pad after it); PNG must
/// end with an IEND chunk.
fn looks_complete(bytes: &[u8], mime: &str) -> bool {
    match mime {
        "image/jpeg" => bytes[bytes.len().saturating_sub(1024)..]
            .windows(2)
            .any(|w| w == [0xff, 0xd9]),
        "image/png" => bytes[bytes.len().saturating_sub(64)..]
            .windows(4)
            .any(|w| w == b"IEND"),
        _ => true,
    }
}

impl ArtCache {
    pub(crate) fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn original_path(&self, hash: &str, ext: &str) -> PathBuf {
        self.dir
            .join("originals")
            .join(&hash[..2])
            .join(format!("{hash}.{ext}"))
    }

    /// Stores image bytes; `None` when they are not a readable image (e.g. truncated JPEG).
    pub(crate) fn store(&self, bytes: &[u8]) -> Result<Option<StoredArt>> {
        let Some((mime, ext)) = sniff(bytes) else {
            return Ok(None);
        };
        let Ok((width, height)) = ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .map_err(Error::Io)?
            .into_dimensions()
        else {
            return Ok(None);
        };
        // Header parsing and even lenient decoders accept truncated files; require the
        // format's end marker too.
        if !looks_complete(bytes, mime) || image::load_from_memory(bytes).is_err() {
            return Ok(None);
        }
        let hash = blake3::hash(bytes).to_hex()[..32].to_owned();
        let path = self.original_path(&hash, ext);
        if !path.exists() {
            write_atomic(&path, bytes)?;
        }
        Ok(Some(StoredArt {
            hash,
            mime,
            width,
            height,
            bytes: bytes.len() as u64,
        }))
    }

    pub(crate) fn original(&self, hash: &str) -> Option<PathBuf> {
        ["jpg", "png", "webp", "gif", "bmp"]
            .iter()
            .map(|e| self.original_path(hash, e))
            .find(|p| p.exists())
    }

    /// A JPEG thumbnail no larger than `size` on its longest edge, created on first request.
    pub(crate) fn thumbnail(&self, hash: &str, size: u32) -> Result<PathBuf> {
        if hash.len() < 2 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::NotFound(format!("artwork {hash}")));
        }
        let size = THUMB_SIZES
            .iter()
            .copied()
            .find(|s| *s >= size)
            .unwrap_or(1024);
        let path = self
            .dir
            .join("thumbs")
            .join(size.to_string())
            .join(&hash[..2])
            .join(format!("{hash}.jpg"));
        if path.exists() {
            return Ok(path);
        }
        let original = self
            .original(hash)
            .ok_or_else(|| Error::NotFound(format!("artwork {hash}")))?;
        let img = image::open(&original)
            .map_err(|e| Error::Image(e.to_string()))?
            .into_rgb8();
        let (w, h) = img.dimensions();
        let scale = (size as f64 / w.max(h) as f64).min(1.0);
        let (tw, th) = (
            ((w as f64 * scale).round() as u32).max(1),
            ((h as f64 * scale).round() as u32).max(1),
        );
        let mut dst = Image::new(
            tw,
            th,
            img.pixel_type()
                .ok_or_else(|| Error::Image("pixel type".into()))?,
        );
        Resizer::new()
            .resize(&img, &mut dst, None)
            .map_err(|e| Error::Image(e.to_string()))?;
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
            .write_image(dst.buffer(), tw, th, image::ExtendedColorType::Rgb8)
            .map_err(|e| Error::Image(e.to_string()))?;
        write_atomic(&path, &out)?;
        Ok(path)
    }
}

/// The best folder image for an album directory: cover > folder > front > album, then the only
/// image if there is exactly one.
pub(crate) fn folder_image(dir: &Path) -> Option<PathBuf> {
    let entries: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                matches!(
                    e.to_ascii_lowercase().as_str(),
                    "jpg" | "jpeg" | "png" | "webp"
                )
            })
        })
        .collect();
    for stem in ["cover", "folder", "front", "album"] {
        if let Some(p) = entries.iter().find(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case(stem))
        }) {
            return Some(p.clone());
        }
    }
    (entries.len() == 1).then(|| entries[0].clone())
}
