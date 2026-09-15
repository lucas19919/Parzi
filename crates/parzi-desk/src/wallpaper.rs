//! The wallpaper: decoded off the UI thread, sized to the display, blurred
//! once, uploaded once as a mipmapped texture. Painting it is one textured
//! rounded rect plus a dim overlay and a vignette mesh; nothing per frame.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, TryRecvError};
use std::time::Instant;

use egui::epaint::RectShape;
use egui::{
    Color32, ColorImage, Context, CornerRadius, Mesh, Painter, Rect, Shape, TextureFilter,
    TextureHandle, TextureOptions, Vec2,
};
use image::imageops::{self, FilterType};

use crate::theme::Tokens;

pub struct Wallpaper {
    path: Option<PathBuf>,
    blur: f32,
    state: State,
}

enum State {
    /// Waiting for the display size before decoding.
    Pending,
    Loading(mpsc::Receiver<Option<ColorImage>>),
    Ready(TextureHandle),
    /// No wallpaper configured or the file could not be decoded.
    Unavailable,
}

impl Wallpaper {
    pub fn new(t: &Tokens) -> Self {
        let state = if t.bg_image.is_some() {
            State::Pending
        } else {
            State::Unavailable
        };
        Self {
            path: t.bg_image.clone(),
            blur: t.bg_blur,
            state,
        }
    }

    /// Start decoding once the display size is known; adopt the result when
    /// the worker delivers it. Cheap to call every frame.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // sizes are positive
    pub fn poll(&mut self, ctx: &Context) {
        match &mut self.state {
            State::Pending => {
                let Some(path) = self.path.clone() else {
                    self.state = State::Unavailable;
                    return;
                };
                let ppp = ctx.pixels_per_point();
                let target = ctx.input(|i| {
                    let v = i.viewport();
                    v.monitor_size.or(v.inner_rect.map(|r| r.size()))
                });
                let Some(target) = target else {
                    return; // not known yet; try again next frame
                };
                let target_px = [
                    (target.x * ppp).ceil() as u32,
                    (target.y * ppp).ceil() as u32,
                ];
                let sigma = self.blur * ppp;
                let (tx, rx) = mpsc::channel();
                let repaint = ctx.clone();
                let spawned =
                    std::thread::Builder::new()
                        .name("wallpaper".into())
                        .spawn(move || {
                            let image = decode(&path, target_px, sigma);
                            // The UI may have gone away; nothing to do then.
                            let _ = tx.send(image);
                            repaint.request_repaint();
                        });
                self.state = match spawned {
                    Ok(_) => State::Loading(rx),
                    Err(e) => {
                        tracing::warn!(error = %e, "wallpaper thread failed to start");
                        State::Unavailable
                    }
                };
            }
            State::Loading(rx) => match rx.try_recv() {
                Ok(Some(image)) => {
                    let options =
                        TextureOptions::LINEAR.with_mipmap_mode(Some(TextureFilter::Linear));
                    let handle = ctx.load_texture("wallpaper", image, options);
                    self.state = State::Ready(handle);
                }
                Ok(None) | Err(TryRecvError::Disconnected) => self.state = State::Unavailable,
                Err(TryRecvError::Empty) => {}
            },
            State::Ready(_) | State::Unavailable => {}
        }
    }

    /// Paint the window body: flat stage colour, then (when ready) the
    /// wallpaper cover-fitted into `rect`, dimmed and vignetted.
    pub fn paint(&self, painter: &Painter, rect: Rect, radius: CornerRadius, t: &Tokens) {
        painter.rect_filled(rect, radius, t.stage);
        let State::Ready(texture) = &self.state else {
            return;
        };
        let uv = cover_uv(texture.size_vec2(), rect.size());
        painter.add(RectShape::filled(rect, radius, Color32::WHITE).with_texture(texture.id(), uv));
        if t.bg_dim > 0.0 {
            painter.rect_filled(rect, radius, Color32::from_black_alpha(alpha_u8(t.bg_dim)));
        }
        if t.bg_vignette > 0.0 {
            // Inset by the radius so the mesh never lands on a transparent corner.
            let inset = f32::from(radius.nw.max(radius.ne).max(radius.sw).max(radius.se));
            vignette(painter, rect.shrink(inset), t.bg_vignette);
        }
    }
}

/// Decode, cover-fit to the display without upscaling, blur once.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn decode(path: &Path, target: [u32; 2], sigma: f32) -> Option<ColorImage> {
    let started = Instant::now();
    let decoded = match image::open(path) {
        Ok(img) => img.into_rgb8(),
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "wallpaper unreadable");
            return None;
        }
    };
    let (w, h) = decoded.dimensions();
    if w == 0 || h == 0 || target[0] == 0 || target[1] == 0 {
        return None;
    }
    // Scale so the picture covers the display, never larger than needed,
    // never upscaled: a small picture stays small and is stretched at paint.
    let scale = (target[0] as f32 / w as f32)
        .max(target[1] as f32 / h as f32)
        .min(1.0);
    let (nw, nh) = (
        ((w as f32 * scale).round() as u32).max(1),
        ((h as f32 * scale).round() as u32).max(1),
    );
    let resized = if (nw, nh) == (w, h) {
        decoded
    } else {
        imageops::resize(&decoded, nw, nh, FilterType::Triangle)
    };
    let blurred = if sigma > 0.05 {
        imageops::fast_blur(&resized, sigma)
    } else {
        resized
    };
    let (bw, bh) = blurred.dimensions();
    let image = ColorImage::from_rgb([bw as usize, bh as usize], blurred.as_raw());
    tracing::info!(
        path = %path.display(),
        source = format!("{w}x{h}"),
        texture = format!("{bw}x{bh}"),
        sigma,
        ms = started.elapsed().as_millis(),
        "wallpaper ready"
    );
    Some(image)
}

/// UV rect that shows as much of the texture as fits `view` at the texture
/// aspect, centred: the CSS `background-size: cover` rule.
fn cover_uv(texture: Vec2, view: Vec2) -> Rect {
    if texture.x <= 0.0 || texture.y <= 0.0 || view.x <= 0.0 || view.y <= 0.0 {
        return Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    }
    let texture_aspect = texture.x / texture.y;
    let view_aspect = view.x / view.y;
    if view_aspect > texture_aspect {
        // Window is wider than the picture: crop top and bottom.
        let visible = texture_aspect / view_aspect;
        let off = (1.0 - visible) / 2.0;
        Rect::from_min_max(egui::pos2(0.0, off), egui::pos2(1.0, off + visible))
    } else {
        let visible = view_aspect / texture_aspect;
        let off = (1.0 - visible) / 2.0;
        Rect::from_min_max(egui::pos2(off, 0.0), egui::pos2(off + visible, 1.0))
    }
}

/// Four gradient bands darkening toward the edges. One mesh, 16 vertices.
fn vignette(painter: &Painter, rect: Rect, strength: f32) {
    let edge = Color32::from_black_alpha(alpha_u8(strength * 0.85));
    let clear = Color32::TRANSPARENT;
    let band = rect.width().min(rect.height()) * 0.35;
    let mut mesh = Mesh::default();
    let mut quad =
        |outer_a: egui::Pos2, outer_b: egui::Pos2, inner_b: egui::Pos2, inner_a: egui::Pos2| {
            let i = u32::try_from(mesh.vertices.len()).unwrap_or(0);
            mesh.colored_vertex(outer_a, edge);
            mesh.colored_vertex(outer_b, edge);
            mesh.colored_vertex(inner_b, clear);
            mesh.colored_vertex(inner_a, clear);
            mesh.add_triangle(i, i + 1, i + 2);
            mesh.add_triangle(i, i + 2, i + 3);
        };
    let (l, r, t, b) = (rect.min.x, rect.max.x, rect.min.y, rect.max.y);
    // top
    quad(
        egui::pos2(l, t),
        egui::pos2(r, t),
        egui::pos2(r, t + band),
        egui::pos2(l, t + band),
    );
    // bottom
    quad(
        egui::pos2(l, b),
        egui::pos2(r, b),
        egui::pos2(r, b - band),
        egui::pos2(l, b - band),
    );
    // left
    quad(
        egui::pos2(l, t),
        egui::pos2(l, b),
        egui::pos2(l + band, b),
        egui::pos2(l + band, t),
    );
    // right
    quad(
        egui::pos2(r, t),
        egui::pos2(r, b),
        egui::pos2(r - band, b),
        egui::pos2(r - band, t),
    );
    painter.add(Shape::mesh(mesh));
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped
fn alpha_u8(a: f32) -> u8 {
    (a.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_uv_crops_the_long_axis_only() {
        // 16:9 picture in a 4:3 window: full height, cropped width.
        let uv = cover_uv(Vec2::new(1600.0, 900.0), Vec2::new(800.0, 600.0));
        assert!((uv.min.y).abs() < 1e-6 && (uv.max.y - 1.0).abs() < 1e-6);
        assert!((uv.width() - 0.75).abs() < 1e-6);
        assert!((uv.min.x - 0.125).abs() < 1e-6);

        // 4:3 picture in an ultra-wide window: full width, cropped height.
        let uv = cover_uv(Vec2::new(800.0, 600.0), Vec2::new(2000.0, 500.0));
        assert!((uv.min.x).abs() < 1e-6 && (uv.max.x - 1.0).abs() < 1e-6);
        assert!(uv.height() < 1.0 && uv.height() > 0.0);
    }

    #[test]
    fn cover_uv_degenerate_sizes_do_not_panic() {
        let uv = cover_uv(Vec2::ZERO, Vec2::new(10.0, 10.0));
        assert_eq!(
            uv,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0))
        );
    }
}
