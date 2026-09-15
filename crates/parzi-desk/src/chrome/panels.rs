//! The three panels over the wallpaper: sidebar (248 px), stage, composer.
//! Hand layout; each panel is a translucent rounded frame. M0 fills them with
//! sample content so the type and the glass can be judged.

use egui::{
    Align, Color32, CornerRadius, Frame, Layout, Margin, Pos2, Rect, RichText, TextStyle, Ui,
    UiBuilder, Vec2,
};

use super::titlebar;
use crate::theme::{self, Tokens};

pub const SIDEBAR_W: f32 = 248.0;
/// Space between the window edge and the panels, and between panels.
const GUTTER: f32 = 10.0;
const COMPOSER_H: f32 = 108.0;
/// Reading measure for the stage column and the composer.
const MEASURE: f32 = 700.0;
const PANEL_PADDING: i8 = 14;

pub struct PanelLayout {
    pub titlebar: Rect,
    pub sidebar: Rect,
    pub stage: Rect,
    pub composer: Rect,
}

pub fn layout(outer: Rect) -> PanelLayout {
    let titlebar = Rect::from_min_size(outer.min, Vec2::new(outer.width(), titlebar::HEIGHT));
    let body = Rect::from_min_max(
        Pos2::new(outer.min.x + GUTTER, titlebar.max.y),
        Pos2::new(outer.max.x - GUTTER, outer.max.y - GUTTER),
    );
    let sidebar = Rect::from_min_size(body.min, Vec2::new(SIDEBAR_W, body.height()));
    let stage = Rect::from_min_max(Pos2::new(sidebar.max.x + GUTTER, body.min.y), body.max);
    let composer_w = (stage.width() - 2.0 * GUTTER).min(MEASURE + 2.0 * GUTTER);
    let composer = Rect::from_center_size(
        Pos2::new(stage.center().x, stage.max.y - GUTTER - COMPOSER_H / 2.0),
        Vec2::new(composer_w, COMPOSER_H),
    );
    PanelLayout {
        titlebar,
        sidebar,
        stage,
        composer,
    }
}

/// A translucent panel frame in the given base colour.
pub fn glass(t: &Tokens, base: Color32) -> Frame {
    Frame::new()
        .fill(t.glass_fill(base))
        .stroke(t.hairline())
        .corner_radius(CornerRadius::same(t.glass_radius))
        .inner_margin(Margin::same(PANEL_PADDING))
        .shadow(t.shadow())
}

pub fn show(ui: &mut Ui, l: &PanelLayout, t: &Tokens) {
    panel(ui, l.sidebar, glass(t, t.sidebar), |ui| sidebar(ui, t));
    panel(ui, l.stage, glass(t, t.stage), |ui| stage(ui, t));
    panel(ui, l.composer, glass(t, t.bar), |ui| composer(ui, t));
}

/// Lay a frame over exactly `rect`, whatever its content needs.
fn panel(ui: &mut Ui, rect: Rect, frame: Frame, add: impl FnOnce(&mut Ui)) {
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::top_down(Align::Min)),
    );
    frame.show(&mut child, |ui| {
        ui.set_min_size(ui.available_size());
        ui.set_clip_rect(ui.max_rect());
        add(ui);
    });
}

fn sidebar(ui: &mut Ui, t: &Tokens) {
    ui.label(RichText::new("Threads").text_style(TextStyle::Name(theme::LABEL.into())));
    ui.add_space(6.0);
    for (title, when) in [
        ("Native UI on egui", "today"),
        ("Approval gate audit", "yesterday"),
        ("Release 0.1.11", "3 days"),
    ] {
        ui.horizontal(|ui| {
            ui.label(title);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(when).small().color(t.text_dim));
            });
        });
    }
}

fn stage(ui: &mut Ui, t: &Tokens) {
    let column = Rect::from_center_size(
        Pos2::new(ui.max_rect().center().x, ui.max_rect().min.y),
        Vec2::new(MEASURE.min(ui.available_width()), 0.0),
    );
    let column = Rect::from_min_size(
        Pos2::new(column.min.x, ui.max_rect().min.y + 24.0),
        Vec2::new(column.width(), ui.available_height()),
    );
    let mut col = ui.new_child(
        UiBuilder::new()
            .max_rect(column)
            .layout(Layout::top_down(Align::Min)),
    );
    let ui = &mut col;
    ui.spacing_mut().item_spacing.y = 10.0;

    ui.label(RichText::new("Good evening.").text_style(TextStyle::Name(theme::HERO.into())));
    ui.label(RichText::new("Skeleton").heading());
    ui.label(
        "This is the reading surface. Body text is Inter, hinted and subpixel-binned, \
         set at the size from theme.toml and laid out at a fixed measure so wider windows \
         add margin rather than line length. The panels are translucent frames over a \
         wallpaper that was blurred once at load and never again.",
    );
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.label("Inline code such as ");
        ui.label(RichText::new("ctx.request_repaint()").code());
        ui.label(" sits in JetBrains Mono on a tinted background.");
    });
    Frame::new()
        .fill(theme::with_alpha(t.bar, 0.9))
        .stroke(t.hairline())
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                RichText::new(
                    "fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {\n\
                     \x20   self.wallpaper.poll(ctx);\n\
                     \x20   let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));\n\
                     }",
                )
                .monospace(),
            );
        });
    ui.label(RichText::new("Dim text at the theme's secondary colour.").color(t.text_dim));
}

fn composer(ui: &mut Ui, t: &Tokens) {
    ui.label(RichText::new("Message Parzi").color(t.text_dim));
    ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Enter to send").small().color(t.text_dim));
            ui.label(RichText::new("·").small().color(t.text_dim));
            ui.label(
                RichText::new("Shift+Enter for a new line")
                    .small()
                    .color(t.text_dim),
            );
        });
    });
}
