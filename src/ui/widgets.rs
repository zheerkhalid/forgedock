//! Small reusable pieces of UI.

use super::theme::{pal, regular, semibold, Icon};
use eframe::egui::{
    self, pos2, text::LayoutJob, text::TextWrapping, vec2, Align, Align2, Color32, CornerRadius,
    FontId, Frame, Galley, Layout, Margin, Rect, Response, RichText, Sense, Stroke, TextureHandle,
    Ui,
};
use std::sync::Arc;

/// A rounded card.
pub fn card(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let p = pal(ui);
    Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(1.0_f32, p.border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
}

/// A row of fixed height. Inside it, [`right`] may be used safely: the fixed
/// height keeps right-to-left layouts from stretching in scroll areas.
pub fn row(ui: &mut Ui, height: f32, add: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), height),
        Layout::left_to_right(Align::Center),
        add,
    );
}

/// Right-aligned content; only inside a [`row`].
pub fn right(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.with_layout(Layout::right_to_left(Align::Center), add);
}

fn filled_button(ui: &mut Ui, text: &str, fill: Color32, fg: Color32, enabled: bool) -> Response {
    let p = pal(ui);
    let (fill, fg) = if enabled {
        (fill, fg)
    } else {
        (p.hover, p.faint)
    };
    let button = egui::Button::new(RichText::new(text).font(semibold(14.0)).color(fg))
        .fill(fill)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(8))
        .min_size(vec2(0.0, 34.0));
    let prev = ui.spacing().button_padding;
    ui.spacing_mut().button_padding = vec2(16.0, 7.0);
    let r = ui.add_enabled(enabled, button);
    ui.spacing_mut().button_padding = prev;
    if r.hovered() && enabled {
        ui.painter()
            .rect_filled(r.rect, CornerRadius::same(8), Color32::from_white_alpha(22));
    }
    r
}

/// The main action on a screen.
pub fn primary_button(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let p = pal(ui);
    let fill = if p.dark {
        Color32::from_rgb(0x6d, 0x5d, 0xf5)
    } else {
        p.accent
    };
    filled_button(ui, text, fill, p.on_accent, enabled)
}

/// A destructive action.
pub fn danger_button(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let p = pal(ui);
    filled_button(ui, text, p.danger, Color32::WHITE, enabled)
}

/// A secondary action.
pub fn ghost_button(ui: &mut Ui, text: &str) -> Response {
    let p = pal(ui);
    let button = egui::Button::new(RichText::new(text).color(p.text))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::new(1.0_f32, p.border))
        .corner_radius(CornerRadius::same(8))
        .min_size(vec2(0.0, 34.0));
    ui.add(button)
}

/// A square icon-only button with a tooltip.
pub fn icon_button(ui: &mut Ui, icon: Icon, tooltip: &str) -> Response {
    let p = pal(ui);
    let label = if icon.glyph().is_empty() {
        tooltip.chars().take(1).collect::<String>()
    } else {
        icon.glyph().to_string()
    };
    let button = egui::Button::new(RichText::new(label).size(15.0).color(p.muted))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::new(1.0_f32, p.border))
        .corner_radius(CornerRadius::same(8))
        .min_size(vec2(34.0, 34.0));
    ui.add(button).on_hover_text(tooltip)
}

fn paint_badge_at(ui: &Ui, rect: Rect, galley: Arc<Galley>, color: Color32) {
    ui.painter()
        .rect_filled(rect, CornerRadius::same(10), color.gamma_multiply(0.16));
    let pos = pos2(
        rect.center().x - galley.size().x / 2.0,
        rect.center().y - galley.size().y / 2.0,
    );
    ui.painter().galley(pos, galley, color);
}

/// A small pill label.
pub fn badge(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), semibold(11.5), color);
    let size = vec2(galley.size().x + 16.0, 22.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    paint_badge_at(ui, rect, galley, color);
    resp
}

/// Paints a badge with its left-centre at `at` and returns its width.
pub fn paint_badge(ui: &Ui, at: egui::Pos2, text: &str, color: Color32) -> f32 {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), semibold(11.5), color);
    let w = galley.size().x + 16.0;
    let r = Rect::from_min_size(pos2(at.x, at.y - 11.0), vec2(w, 22.0));
    paint_badge_at(ui, r, galley, color);
    w
}

pub fn chip(ui: &mut Ui, label: &str, active: bool) -> Response {
    let p = pal(ui);
    let (fill, fg, stroke) = if active {
        (
            p.accent.gamma_multiply(0.16),
            p.accent,
            Stroke::new(1.0_f32, p.accent.gamma_multiply(0.6)),
        )
    } else {
        (
            Color32::TRANSPARENT,
            p.muted,
            Stroke::new(1.0_f32, p.border),
        )
    };
    let b = egui::Button::new(RichText::new(label).size(13.0).color(fg))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(15))
        .min_size(vec2(0.0, 30.0));
    ui.add(b)
}

/// Text laid out to `width`, cut off with an ellipsis after `rows` lines.
pub fn galley(
    ui: &Ui,
    text: &str,
    font: FontId,
    color: Color32,
    width: f32,
    rows: usize,
) -> Arc<Galley> {
    let mut job = LayoutJob::simple(text.to_string(), font, color, width);
    job.wrap = TextWrapping {
        max_width: width,
        max_rows: rows,
        break_anywhere: false,
        overflow_character: Some('…'),
    };
    ui.fonts(|f| f.layout_job(job))
}

/// An app icon: the texture when there is one, otherwise a coloured tile
/// with the app's initial.
pub fn paint_icon(ui: &Ui, rect: Rect, name: &str, accent: [u8; 3], tex: Option<&TextureHandle>) {
    match tex {
        Some(t) => {
            egui::Image::new(t).paint_at(ui, rect);
        }
        None => {
            let color = Color32::from_rgb(accent[0], accent[1], accent[2]);
            // The family's icons are rounded squares inset in their canvas.
            let tile = rect.shrink(rect.width() * 0.03);
            ui.painter()
                .rect_filled(tile, CornerRadius::same((rect.width() * 0.22) as u8), color);
            let letter: String = name.chars().take(1).collect();
            ui.painter().text(
                tile.center(),
                Align2::CENTER_CENTER,
                letter,
                semibold(rect.height() * 0.46),
                Color32::WHITE,
            );
        }
    }
}

pub fn empty_state(ui: &mut Ui, icon: Icon, color: Color32, title: &str, text: &str) {
    let p = pal(ui);
    ui.add_space(70.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(icon.glyph()).size(40.0).color(color));
        ui.add_space(4.0);
        ui.label(RichText::new(title).font(semibold(18.0)));
        ui.label(RichText::new(text).color(p.muted));
    });
}

/// A small heading above a block.
pub fn section(ui: &mut Ui, title: &str) {
    let p = pal(ui);
    ui.label(RichText::new(title).font(semibold(13.0)).color(p.muted));
}

/// A label and its value, one above the other.
pub fn stat(ui: &mut Ui, label: &str, value: &str) {
    let p = pal(ui);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(RichText::new(label).font(regular(12.0)).color(p.faint));
        ui.label(RichText::new(value).font(semibold(16.0)));
    });
}
