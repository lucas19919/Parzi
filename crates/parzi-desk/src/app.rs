//! The frame loop: window body, chrome, panels and the resize edges.

use std::time::Instant;

use egui::{
    Context, CornerRadius, CursorIcon, Id, Rect, ResizeDirection, Sense, Stroke, StrokeKind, Ui,
    Vec2, ViewportCommand,
};

use crate::chrome::{panels, titlebar};
use crate::fonts;
use crate::theme::{self, Tokens};
use crate::wallpaper::Wallpaper;

/// Width of the invisible resize zones along the window edges (points).
const RESIZE_EDGE: f32 = 6.0;
/// Window corner radius while not maximized (points). Corners are transparent.
const WINDOW_RADIUS: u8 = 10;

pub struct DeskApp {
    tokens: Tokens,
    wallpaper: Wallpaper,
    started: Instant,
    first_frame_logged: bool,
}

impl DeskApp {
    pub fn new(cc: &eframe::CreationContext<'_>, started: Instant) -> Self {
        let tokens = theme::load();
        cc.egui_ctx.set_fonts(fonts::definitions());
        theme::apply(&cc.egui_ctx, &tokens);
        let wallpaper = Wallpaper::new(&tokens);
        Self {
            tokens,
            wallpaper,
            started,
            first_frame_logged: false,
        }
    }

    /// Corner radius of the window body: square when maximized so nothing
    /// shows through at the screen edges.
    fn window_radius(maximized: bool) -> CornerRadius {
        if maximized {
            CornerRadius::ZERO
        } else {
            CornerRadius::same(WINDOW_RADIUS)
        }
    }
}

impl eframe::App for DeskApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // Fully transparent: the rounded window body is painted by us, the
        // corners show the desktop behind.
        [0.0, 0.0, 0.0, 0.0]
    }

    /// The root `Ui` covers the whole window with no margin and no
    /// background: we paint the body ourselves.
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        if !self.first_frame_logged {
            self.first_frame_logged = true;
            tracing::info!(
                cold_start_ms = self.started.elapsed().as_millis(),
                "first frame"
            );
        }
        let ctx: Context = ui.ctx().clone();
        self.wallpaper.poll(&ctx);

        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        let radius = Self::window_radius(maximized);
        let outer = ui.max_rect();

        self.wallpaper
            .paint(ui.painter(), outer, radius, &self.tokens);

        let layout = panels::layout(outer);
        titlebar::show(ui, layout.titlebar, &self.tokens, maximized, WINDOW_RADIUS);
        panels::show(ui, &layout, &self.tokens);

        ui.painter().rect_stroke(
            outer,
            radius,
            Stroke::new(1.0, self.tokens.border),
            StrokeKind::Inside,
        );

        // Last, so the edge zones win over the drag region and panels.
        if !maximized {
            resize_edges(ui, outer);
        }
    }
}

/// Eight invisible zones along the window border. Dragging one hands the
/// gesture to the OS (`BeginResize`), which then owns snapping and limits.
fn resize_edges(ui: &mut Ui, outer: Rect) {
    let e = RESIZE_EDGE;
    let (min, max) = (outer.min, outer.max);
    let inner_w = outer.width() - 2.0 * e;
    let inner_h = outer.height() - 2.0 * e;
    let zones = [
        (
            ResizeDirection::NorthWest,
            Rect::from_min_size(min, Vec2::splat(e)),
            CursorIcon::ResizeNorthWest,
        ),
        (
            ResizeDirection::NorthEast,
            Rect::from_min_size(egui::pos2(max.x - e, min.y), Vec2::splat(e)),
            CursorIcon::ResizeNorthEast,
        ),
        (
            ResizeDirection::SouthWest,
            Rect::from_min_size(egui::pos2(min.x, max.y - e), Vec2::splat(e)),
            CursorIcon::ResizeSouthWest,
        ),
        (
            ResizeDirection::SouthEast,
            Rect::from_min_size(egui::pos2(max.x - e, max.y - e), Vec2::splat(e)),
            CursorIcon::ResizeSouthEast,
        ),
        (
            ResizeDirection::North,
            Rect::from_min_size(egui::pos2(min.x + e, min.y), egui::vec2(inner_w, e)),
            CursorIcon::ResizeNorth,
        ),
        (
            ResizeDirection::South,
            Rect::from_min_size(egui::pos2(min.x + e, max.y - e), egui::vec2(inner_w, e)),
            CursorIcon::ResizeSouth,
        ),
        (
            ResizeDirection::West,
            Rect::from_min_size(egui::pos2(min.x, min.y + e), egui::vec2(e, inner_h)),
            CursorIcon::ResizeWest,
        ),
        (
            ResizeDirection::East,
            Rect::from_min_size(egui::pos2(max.x - e, min.y + e), egui::vec2(e, inner_h)),
            CursorIcon::ResizeEast,
        ),
    ];
    for (dir, rect, cursor) in zones {
        let resp = ui.interact(rect, Id::new(("resize-edge", dir as u8)), Sense::drag());
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(cursor);
        }
        if resp.drag_started() {
            ui.ctx()
                .send_viewport_cmd(ViewportCommand::BeginResize(dir));
        }
    }
}
