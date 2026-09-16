//! Pre-rendered wallpaper textures.
//!
//! The stage used to be handed `theme.background.image` itself, so the
//! compositor held a texture the size of whatever file the user picked (a 4K
//! photo is 33 MB of RGBA for a 1920×1200 screen), and since the compositor
//! diet dropped the CSS `filter` from `.bg-img`, `background.blur` stopped
//! doing anything at all.
//!
//! Both are fixed by rendering once, here: decode with limits → downscale to
//! the display (hard-capped) → blur → write `bg-<key>.{webp,jpg}` into the
//! cache. The key covers everything that changes the picture, so a new blur
//! or a new screen size renders a new file and the old one is swept.

use std::path::{Path, PathBuf};

use image::{imageops, ImageReader};

use crate::error::{ParziError, Result};

/// Long edge we refuse rather than decode: an 8K JPEG is ~130 MB of pixels
/// before we get the chance to shrink it.
pub const MAX_SOURCE_EDGE: u32 = 4096;
/// Nothing on a desk needs more than this, whatever the panel reports.
pub const CAP_W: u32 = 2560;
pub const CAP_H: u32 = 1600;

/// Decoder limits for every wallpaper read (AUDIT C-7: `image::open` ran with
/// defaults, so a crafted file could ask for an arbitrary allocation).
pub fn limits() -> image::Limits {
    let mut l = image::Limits::default();
    l.max_image_width = Some(MAX_SOURCE_EDGE);
    l.max_image_height = Some(MAX_SOURCE_EDGE);
    l.max_alloc = Some(256 * 1024 * 1024);
    l
}

/// Dimensions come out of the header and allocate no pixels, so the size
/// caps stay off while reading them: we want to *name* an oversized picture,
/// not to fail it with "exceeds limit".
fn header_limits() -> image::Limits {
    let mut l = image::Limits::default();
    l.max_alloc = Some(64 * 1024 * 1024);
    l
}

fn open(path: &Path, l: image::Limits) -> Result<ImageReader<std::io::BufReader<std::fs::File>>> {
    let mut r = ImageReader::open(path)
        .map_err(|e| ParziError::Config(format!("unreadable image: {e}")))?
        .with_guessed_format()
        .map_err(|e| ParziError::Config(format!("unreadable image: {e}")))?;
    r.limits(l);
    Ok(r)
}

/// Pixel size from the header alone — no decode, no allocation.
pub fn source_size(path: &Path) -> Result<(u32, u32)> {
    open(path, header_limits())?
        .into_dimensions()
        .map_err(|e| ParziError::Config(format!("unreadable image: {e}")))
}

/// Refuse a source we will not decode. Called when a wallpaper is chosen so
/// the message lands on the person picking it, not on a blank stage.
pub fn check_source(path: &Path) -> Result<()> {
    let (w, h) = source_size(path)?;
    if w.max(h) > MAX_SOURCE_EDGE {
        return Err(ParziError::Config(format!(
            "{w}×{h} is too big: wallpapers are at most {MAX_SOURCE_EDGE} px on the long edge"
        )));
    }
    Ok(())
}

/// Largest size that fits inside `max_w`×`max_h` (itself capped) keeping the
/// aspect ratio. Never upscales: a small picture stays small and CSS `cover`
/// stretches it, which is cheaper than storing the stretch.
pub fn fit(src_w: u32, src_h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    let (sw, sh) = (src_w.max(1), src_h.max(1));
    let (mw, mh) = (max_w.clamp(1, CAP_W), max_h.clamp(1, CAP_H));
    if sw <= mw && sh <= mh {
        return (sw, sh);
    }
    let s = (mw as f64 / sw as f64).min(mh as f64 / sh as f64);
    (
        ((sw as f64 * s).round() as u32).max(1),
        ((sh as f64 * s).round() as u32).max(1),
    )
}

/// CSS `blur(Npx)` is a Gaussian with σ = N *CSS* pixels, so on a 150 %
/// display it was σ = 1.5N device pixels. The cached texture is stretched
/// across `screen_w` device pixels, so one of its pixels covers
/// `screen_w / out_w` of them and σ shrinks by the same ratio.
pub fn blur_sigma(blur: f64, scale: f64, out_w: u32, screen_w: u32) -> f32 {
    if blur <= 0.0 || out_w == 0 || screen_w == 0 {
        return 0.0;
    }
    let device = blur * scale.clamp(0.5, 4.0);
    (device * out_w as f64 / screen_w as f64).max(0.0) as f32
}

/// FNV-1a over everything that changes the picture. This names a cache file,
/// it is not a security boundary, so no hashing dependency is worth adding.
fn fnv1a(bytes: &[u8], mut h: u64) -> u64 {
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// Cache key: source identity (path + mtime + length) and every render
/// parameter. Change the blur or move to a bigger screen and the key moves.
pub fn cache_key(src: &Path, mtime: u64, len: u64, sigma: f32, out_w: u32, out_h: u32) -> String {
    let mut h = fnv1a(src.to_string_lossy().as_bytes(), 0xcbf2_9ce4_8422_2325);
    for n in [
        mtime,
        len,
        out_w as u64,
        out_h as u64,
        (sigma * 100.0).round().max(0.0) as u64,
    ] {
        h = fnv1a(&n.to_le_bytes(), h);
    }
    format!("{h:016x}")
}

/// Blurred output is flat, so lossless WebP holds it in a couple of hundred
/// KB with no banding. An untouched photo is not flat and `image` ships no
/// *lossy* WebP encoder, so that case goes out as JPEG q85 rather than as a
/// ten-megabyte VP8L file.
pub fn cache_name(key: &str, sigma: f32) -> String {
    if sigma > 0.0 {
        format!("bg-{key}.webp")
    } else {
        format!("bg-{key}.jpg")
    }
}

fn mtime_secs(m: &std::fs::Metadata) -> u64 {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Render `src` for a `screen_w`×`screen_h` display at `scale`, or return the
/// cached file when one already matches. The warm path is two `stat` calls.
pub fn prepare(
    src: &Path,
    cache_dir: &Path,
    blur: f64,
    screen_w: u32,
    screen_h: u32,
    scale: f64,
) -> Result<PathBuf> {
    let meta = std::fs::symlink_metadata(src)?;
    if !meta.is_file() {
        return Err(ParziError::Config("wallpaper is not a file".into()));
    }
    let (sw, sh) = source_size(src)?;
    if sw.max(sh) > MAX_SOURCE_EDGE {
        return Err(ParziError::Config(format!(
            "{sw}×{sh} is too big: wallpapers are at most {MAX_SOURCE_EDGE} px on the long edge"
        )));
    }
    let (out_w, out_h) = fit(sw, sh, screen_w, screen_h);
    let sigma = blur_sigma(blur, scale, out_w, screen_w);
    let key = cache_key(src, mtime_secs(&meta), meta.len(), sigma, out_w, out_h);
    let dest = cache_dir.join(cache_name(&key, sigma));
    if dest.is_file() {
        sweep(cache_dir, &dest);
        return Ok(dest);
    }

    std::fs::create_dir_all(cache_dir)?;
    let img = open(src, limits())?
        .decode()
        .map_err(|e| ParziError::Config(format!("unreadable image: {e}")))?;
    let img = if (out_w, out_h) == (sw, sh) {
        img
    } else {
        img.resize_exact(out_w, out_h, imageops::FilterType::CatmullRom)
    };
    let mut rgb = img.into_rgb8();
    if sigma > 0.0 {
        rgb = imageops::fast_blur(&rgb, sigma);
    }

    // Unique tmp name + rename: a second window asking at the same moment
    // must never read a half-written texture (core report C-5's rule).
    let tmp = cache_dir.join(format!("{key}.{}.tmp", std::process::id()));
    let write = (|| -> Result<()> {
        let mut w = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
        let r = if sigma > 0.0 {
            image::codecs::webp::WebPEncoder::new_lossless(&mut w).encode(
                rgb.as_raw(),
                out_w,
                out_h,
                image::ExtendedColorType::Rgb8,
            )
        } else {
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut w, 85).encode(
                rgb.as_raw(),
                out_w,
                out_h,
                image::ExtendedColorType::Rgb8,
            )
        };
        r.map_err(|e| ParziError::Config(format!("cannot encode wallpaper: {e}")))?;
        // Same rule as `store::cache::atomic_write_sync`: flush the buffer and
        // `sync_all` before the rename, or a crash can leave a cache file whose
        // name promises bytes that never reached the disk.
        w.into_inner()
            .map_err(|e| ParziError::Config(format!("cannot write wallpaper: {e}")))?
            .sync_all()?;
        Ok(())
    })();
    if let Err(e) = write {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, &dest) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    sweep(cache_dir, &dest);
    Ok(dest)
}

/// A `.tmp` younger than this may be another instance rendering right now.
const TMP_GRACE_SECS: u64 = 600;

/// One wallpaper is live at a time, so the cache is one file. Anything else
/// named `bg-*` is a previous blur, screen or picture and goes. A `.tmp` is
/// only swept once it is older than `TMP_GRACE_SECS`: a second window can be
/// half-way through writing one, and deleting it under that writer turns its
/// render into an error.
pub fn sweep(cache_dir: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(cache_dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p == keep || !p.is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        if name.ends_with(".tmp") {
            let abandoned = e
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| std::time::SystemTime::now().duration_since(t).ok())
                .is_some_and(|age| age.as_secs() > TMP_GRACE_SECS);
            if abandoned {
                let _ = std::fs::remove_file(&p);
            }
        } else if name.starts_with("bg-") {
            let _ = std::fs::remove_file(&p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_shrinks_to_the_screen_and_never_grows() {
        // 4K source on a 1920×1200 panel: width-bound.
        assert_eq!(fit(3840, 2160, 1920, 1200), (1920, 1080));
        // Already smaller than the screen: untouched.
        assert_eq!(fit(1280, 720, 1920, 1200), (1280, 720));
        // Tall source on a wide screen: height-bound.
        assert_eq!(fit(2000, 4000, 1920, 1200), (600, 1200));
        // The cap wins over an absurd screen.
        assert_eq!(fit(4000, 4000, 7680, 4320), (1600, 1600));
        // Degenerate inputs stay legal.
        assert_eq!(fit(0, 0, 0, 0), (1, 1));
    }

    #[test]
    fn sigma_follows_dpi_and_the_downscale() {
        // No blur asked for, no blur applied.
        assert_eq!(blur_sigma(0.0, 1.5, 1920, 1920), 0.0);
        // Texture at screen size on a 150 % display: σ = 1.5 × CSS px.
        assert_eq!(blur_sigma(8.0, 1.5, 1920, 1920), 12.0);
        // Texture at half the screen: σ halves with it.
        assert_eq!(blur_sigma(8.0, 1.5, 960, 1920), 6.0);
        // Nothing to scale against.
        assert_eq!(blur_sigma(8.0, 1.5, 0, 1920), 0.0);
    }

    #[test]
    fn cache_key_moves_with_every_render_input() {
        let p = Path::new("/home/x/.parzi/backgrounds/a.jpg");
        let base = cache_key(p, 100, 2000, 3.0, 1920, 1080);
        assert_eq!(base, cache_key(p, 100, 2000, 3.0, 1920, 1080));
        assert_ne!(base, cache_key(p, 101, 2000, 3.0, 1920, 1080)); // touched
        assert_ne!(base, cache_key(p, 100, 2001, 3.0, 1920, 1080)); // rewritten
        assert_ne!(base, cache_key(p, 100, 2000, 4.0, 1920, 1080)); // blur
        assert_ne!(base, cache_key(p, 100, 2000, 3.0, 2560, 1080)); // screen
        assert_ne!(
            base,
            cache_key(Path::new("/home/x/b.jpg"), 100, 2000, 3.0, 1920, 1080)
        );
        assert_eq!(base.len(), 16);
        assert_eq!(cache_name(&base, 3.0), format!("bg-{base}.webp"));
        assert_eq!(cache_name(&base, 0.0), format!("bg-{base}.jpg"));
    }

    fn png(path: &Path, w: u32, h: u32) {
        let mut img = image::RgbImage::new(w, h);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = image::Rgb([(x % 256) as u8, (y % 256) as u8, 90]);
        }
        img.save(path).unwrap();
    }

    #[test]
    fn prepare_downscales_blurs_caches_and_sweeps() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("wall.png");
        png(&src, 2400, 1200);
        let cache = dir.path().join("cache");

        let a = prepare(&src, &cache, 6.0, 1200, 800, 1.0).unwrap();
        assert!(a.is_file());
        assert_eq!(a.extension().unwrap(), "webp");
        assert_eq!(source_size(&a).unwrap(), (1200, 600));

        // Warm call returns the same file without rewriting it.
        let before = std::fs::metadata(&a).unwrap().len();
        let b = prepare(&src, &cache, 6.0, 1200, 800, 1.0).unwrap();
        assert_eq!(a, b);
        assert_eq!(std::fs::metadata(&b).unwrap().len(), before);

        // A different blur is a different file, and the old one is swept.
        let c = prepare(&src, &cache, 0.0, 1200, 800, 1.0).unwrap();
        assert_ne!(a, c);
        assert_eq!(c.extension().unwrap(), "jpg");
        assert!(!a.exists());
        assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 1);
    }

    #[test]
    fn sweep_leaves_a_fresh_tmp_alone() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        std::fs::create_dir_all(&cache).unwrap();
        let keep = cache.join("bg-keep.jpg");
        std::fs::write(&keep, b"x").unwrap();
        std::fs::write(cache.join("bg-old.jpg"), b"x").unwrap();
        // Another instance is writing this one right now.
        let tmp = cache.join("bg-other.99.tmp");
        std::fs::write(&tmp, b"half").unwrap();

        sweep(&cache, &keep);
        assert!(keep.is_file());
        assert!(!cache.join("bg-old.jpg").exists(), "stale render must go");
        assert!(tmp.is_file(), "an in-flight tmp must survive the sweep");
    }

    #[test]
    fn oversized_sources_are_refused_before_decoding() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("huge.png");
        png(&src, MAX_SOURCE_EDGE + 1, 16);
        let err = check_source(&src).unwrap_err().to_string();
        assert!(err.contains("4096"), "{err}");
        assert!(prepare(&src, &dir.path().join("cache"), 0.0, 1920, 1200, 1.0).is_err());
        // A source at the limit is fine.
        let ok = dir.path().join("edge.png");
        png(&ok, MAX_SOURCE_EDGE, 16);
        assert!(check_source(&ok).is_ok());
    }
}
