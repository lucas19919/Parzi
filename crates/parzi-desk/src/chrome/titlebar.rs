//! Frameless title bar: a drag region that also maximizes on double-click,
//! and minimize / maximize / close drawn as shapes, no icon font.

use egui::{
    Align2, Color32, CornerRadius, FontId, Id, PointerButton, Pos2, Rect, Sense, Stroke,
    StrokeKind, Ui, Vec2, ViewportCommand,
};

use crate::fonts;
use crate::theme::{self, Tokens};

pub const HEIGHT: f32 = 40.0;
const BUTTON_W: f32 = 46.0;
/// Side of the square the glyphs are drawn in (points).
const ICON: f32 = 10.0;
const WORDMARK_PAD: f32 = 18.0;
const WORDMARK_SIZE: f32 = 21.0;
/// Diameter of the Parzi mark next to the wordmark (points).
const MARK_D: f32 = 20.0;
const MARK_GAP: f32 = 8.0;
/// Brand colours from the `src-tauri/icons/icon.svg` master.
const MARK_DISC: Color32 = Color32::from_rgb(0x07, 0x07, 0x0B);
const MARK_INK: Color32 = Color32::from_rgb(0xED, 0xED, 0xF2);
/// Windows 11 close-button red.
const CLOSE_HOVER: Color32 = Color32::from_rgb(196, 43, 28);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Control {
    Minimize,
    Maximize,
    Close,
}

/// `radius` is the window corner radius, so the close button hover can
/// follow the top-right corner.
pub fn show(ui: &mut Ui, rect: Rect, t: &Tokens, maximized: bool, radius: u8) {
    let corner = if maximized { 0 } else { radius };

    // Controls, right to left.
    let mut x = rect.max.x;
    for control in [Control::Close, Control::Maximize, Control::Minimize] {
        x -= BUTTON_W;
        let r = Rect::from_min_size(Pos2::new(x, rect.min.y), Vec2::new(BUTTON_W, HEIGHT));
        let cr = if control == Control::Close {
            CornerRadius {
                ne: corner,
                ..CornerRadius::ZERO
            }
        } else {
            CornerRadius::ZERO
        };
        button(ui, r, control, t, maximized, cr);
    }
    let controls_left = x;

    // The official mark, then the wordmark to its right.
    let mark_r = MARK_D / 2.0;
    let mark_center = Pos2::new(
        ui.painter()
            .round_to_pixel_center(rect.min.x + WORDMARK_PAD + mark_r),
        ui.painter().round_to_pixel_center(rect.center().y),
    );
    paint_mark(ui.painter(), mark_center, mark_r);
    ui.painter().text(
        Pos2::new(mark_center.x + mark_r + MARK_GAP, rect.center().y),
        Align2::LEFT_CENTER,
        "Parzi",
        FontId::new(WORDMARK_SIZE, fonts::family(fonts::SERIF_ITALIC)),
        t.text,
    );

    // Everything left of the controls moves the window.
    let drag = Rect::from_min_max(rect.min, Pos2::new(controls_left, rect.max.y));
    let resp = ui.interact(drag, Id::new("titlebar-drag"), Sense::click_and_drag());
    if resp.double_clicked() {
        ui.ctx()
            .send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    } else if resp.drag_started_by(PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
}

fn button(
    ui: &mut Ui,
    rect: Rect,
    control: Control,
    t: &Tokens,
    maximized: bool,
    cr: CornerRadius,
) {
    let resp = ui.interact(
        rect,
        Id::new(("titlebar-control", control as u8)),
        Sense::click(),
    );
    let hovered = resp.hovered() || resp.is_pointer_button_down_on();
    let painter = ui.painter();

    if hovered {
        let fill = if control == Control::Close {
            CLOSE_HOVER
        } else {
            theme::with_alpha(t.text, 0.08)
        };
        painter.rect_filled(rect, cr, fill);
    }
    let ink = if hovered && control == Control::Close {
        Color32::WHITE
    } else {
        t.text
    };
    let stroke = Stroke::new(1.0, ink);

    // Snap the glyph to the pixel grid so 1 px strokes stay 1 px.
    let c = Pos2::new(
        painter.round_to_pixel_center(rect.center().x),
        painter.round_to_pixel_center(rect.center().y),
    );
    let h = ICON / 2.0;
    match control {
        Control::Minimize => {
            painter.line_segment([Pos2::new(c.x - h, c.y), Pos2::new(c.x + h, c.y)], stroke);
        }
        Control::Maximize if maximized => {
            // Restore: front square with the back one peeking out top-right.
            let front =
                Rect::from_min_size(Pos2::new(c.x - h, c.y - h + 2.0), Vec2::splat(ICON - 2.0));
            painter.rect_stroke(front, CornerRadius::ZERO, stroke, StrokeKind::Middle);
            let (bx, by) = (front.max.x + 2.0, front.min.y - 2.0);
            painter.line_segment(
                [Pos2::new(front.min.x + 2.0, by), Pos2::new(bx, by)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(bx, by), Pos2::new(bx, front.max.y - 2.0)],
                stroke,
            );
        }
        Control::Maximize => {
            painter.rect_stroke(
                Rect::from_center_size(c, Vec2::splat(ICON)),
                CornerRadius::ZERO,
                stroke,
                StrokeKind::Middle,
            );
        }
        Control::Close => {
            painter.line_segment(
                [Pos2::new(c.x - h, c.y - h), Pos2::new(c.x + h, c.y + h)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(c.x - h, c.y + h), Pos2::new(c.x + h, c.y - h)],
                stroke,
            );
        }
    }

    if resp.clicked() {
        let cmd = match control {
            Control::Minimize => ViewportCommand::Minimized(true),
            Control::Maximize => ViewportCommand::Maximized(!maximized),
            Control::Close => ViewportCommand::Close,
        };
        ui.ctx().send_viewport_cmd(cmd);
    }
}

/// The Parzi mark at `center` with outer-disc radius `r`: a dark disc, a
/// light ring, and three 45° stripes clipped to the inner circle.
///
/// Geometry follows the `src-tauri/icons/icon.svg` master (1024 viewBox:
/// disc r385, ring r340/w84, clip r256, stripes w80 through the centre and
/// offset ±160 perpendicular), expressed here as fractions of `r` so the
/// mark scales. Stripes are drawn as chords of the inner circle, which is
/// the clip analytically solved: half-chord `sqrt(inner² − d²)`.
fn paint_mark(painter: &egui::Painter, center: Pos2, r: f32) {
    painter.circle_filled(center, r, MARK_DISC);
    painter.circle_stroke(
        center,
        r * 340.0 / 385.0,
        Stroke::new((r * 84.0 / 385.0).max(1.0), MARK_INK),
    );
    let inner = r * 256.0 / 385.0;
    let width = (r * 80.0 / 385.0).max(1.0);
    let off = r * 160.0 / 385.0;
    let u = Vec2::new(
        std::f32::consts::FRAC_1_SQRT_2,
        std::f32::consts::FRAC_1_SQRT_2,
    );
    let n = Vec2::new(
        std::f32::consts::FRAC_1_SQRT_2,
        -std::f32::consts::FRAC_1_SQRT_2,
    );
    for d in [-off, 0.0, off] {
        let half = (inner * inner - d * d).max(0.0).sqrt();
        let mid = center + n * d;
        painter.line_segment(
            [mid - u * half, mid + u * half],
            Stroke::new(width, MARK_INK),
        );
    }
}
