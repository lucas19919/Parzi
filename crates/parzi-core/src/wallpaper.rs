use std::path::{Path, PathBuf};

use image::{imageops, ImageReader};

use crate::error::{ParziError, Result};

pub const MAX_SOURCE_EDGE: u32 = 4096;
pub const CAP_W: u32 = 2560;
pub const CAP_H: u32 = 1600;

pub fn limits() -> image::Limits {
    let mut l = image::Limits::default();
    l.max_image_width = Some(MAX_SOURCE_EDGE);
    l.max_image_height = Some(MAX_SOURCE_EDGE);
    l.max_alloc = Some(256 * 1024 * 1024);
    l
}

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

pub fn source_size(path: &Path) -> Result<(u32, u32)> {
    open(path, header_limits())?
        .into_dimensions()
        .map_err(|e| ParziError::Config(format!("unreadable image: {e}")))
}

pub fn check_source(path: &Path) -> Result<()> {
    let (w, h) = source_size(path)?;
    if w.max(h) > MAX_SOURCE_EDGE {
        return Err(ParziError::Config(format!(
            "{w}×{h} is too big: wallpapers are at most {MAX_SOURCE_EDGE} px on the long edge"
        )));
    }
    Ok(())
}

const IMPORT_MAX_EDGE: u32 = 12_000;
const IMPORT_TARGET_EDGE: u32 = 3840;

pub fn shrink_for_import(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut r = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| ParziError::Config(format!("unreadable image: {e}")))?;
    let mut l = image::Limits::default();
    l.max_image_width = Some(IMPORT_MAX_EDGE);
    l.max_image_height = Some(IMPORT_MAX_EDGE);
    l.max_alloc = Some(768 * 1024 * 1024);
    r.limits(l);
    let img = r.decode().map_err(|e| {
        ParziError::Config(format!(
            "could not read that picture (at most {IMPORT_MAX_EDGE} px on the long edge): {e}"
        ))
    })?;
    let (w, h) = (img.width().max(1), img.height().max(1));
    let s = (IMPORT_TARGET_EDGE as f64 / w.max(h) as f64).min(1.0);
    let (nw, nh) = (
        ((w as f64 * s).round() as u32).max(1),
        ((h as f64 * s).round() as u32).max(1),
    );
    let img = if (nw, nh) == (w, h) {
        img
    } else {
        img.resize_exact(nw, nh, imageops::FilterType::CatmullRom)
    };
    let rgb = img.into_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
        .encode(rgb.as_raw(), nw, nh, image::ExtendedColorType::Rgb8)
        .map_err(|e| ParziError::Config(format!("cannot encode wallpaper: {e}")))?;
    Ok(out)
}

pub fn bytes_size(bytes: &[u8]) -> Result<(u32, u32)> {
    let mut r = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| ParziError::Config(format!("unreadable image: {e}")))?;
    r.limits(header_limits());
    r.into_dimensions().map_err(|e| {
        ParziError::Config(format!(
            "not a picture Parzi can read (png, jpg or webp): {e}"
        ))
    })
}

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

pub fn blur_sigma(blur: f64, scale: f64, out_w: u32, screen_w: u32) -> f32 {
    if blur <= 0.0 || out_w == 0 || screen_w == 0 {
        return 0.0;
    }
    let device = blur * scale.clamp(0.5, 4.0);
    (device * out_w as f64 / screen_w as f64).max(0.0) as f32
}

fn fnv1a(bytes: &[u8], mut h: u64) -> u64 {
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

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

const TMP_GRACE_SECS: u64 = 600;

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
        assert_eq!(fit(3840, 2160, 1920, 1200), (1920, 1080));
        assert_eq!(fit(1280, 720, 1920, 1200), (1280, 720));
        assert_eq!(fit(2000, 4000, 1920, 1200), (600, 1200));
        assert_eq!(fit(4000, 4000, 7680, 4320), (1600, 1600));
        assert_eq!(fit(0, 0, 0, 0), (1, 1));
    }

    #[test]
    fn sigma_follows_dpi_and_the_downscale() {
        assert_eq!(blur_sigma(0.0, 1.5, 1920, 1920), 0.0);
        assert_eq!(blur_sigma(8.0, 1.5, 1920, 1920), 12.0);
        assert_eq!(blur_sigma(8.0, 1.5, 960, 1920), 6.0);
        assert_eq!(blur_sigma(8.0, 1.5, 0, 1920), 0.0);
    }

    #[test]
    fn cache_key_moves_with_every_render_input() {
        let p = Path::new("/home/x/.parzi/backgrounds/a.jpg");
        let base = cache_key(p, 100, 2000, 3.0, 1920, 1080);
        assert_eq!(base, cache_key(p, 100, 2000, 3.0, 1920, 1080));
        assert_ne!(base, cache_key(p, 101, 2000, 3.0, 1920, 1080));
        assert_ne!(base, cache_key(p, 100, 2001, 3.0, 1920, 1080));
        assert_ne!(base, cache_key(p, 100, 2000, 4.0, 1920, 1080));
        assert_ne!(base, cache_key(p, 100, 2000, 3.0, 2560, 1080));
        assert_ne!(
            base,
            cache_key(Path::new("/home/x/b.jpg"), 100, 2000, 3.0, 1920, 1080)
        );
        assert_eq!(base.len(), 16);
        assert_eq!(cache_name(&base, 3.0), format!("bg-{base}.webp"));
        assert_eq!(cache_name(&base, 0.0), format!("bg-{base}.jpg"));
    }

    #[test]
    fn import_shrinks_an_oversized_picture_instead_of_refusing_it() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("wide.png");
        png(&src, 6000, 100);
        let bytes = std::fs::read(&src).unwrap();
        assert_eq!(bytes_size(&bytes).unwrap(), (6000, 100));
        let out = shrink_for_import(&bytes).unwrap();
        assert_eq!(bytes_size(&out).unwrap(), (IMPORT_TARGET_EDGE, 64));
        assert!(bytes_size(b"not a picture").is_err());
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

        let before = std::fs::metadata(&a).unwrap().len();
        let b = prepare(&src, &cache, 6.0, 1200, 800, 1.0).unwrap();
        assert_eq!(a, b);
        assert_eq!(std::fs::metadata(&b).unwrap().len(), before);

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
        let ok = dir.path().join("edge.png");
        png(&ok, MAX_SOURCE_EDGE, 16);
        assert!(check_source(&ok).is_ok());
    }
}
