use egui::{Color32, CornerRadius, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, Vec2};

use crate::icon;
use crate::theme;

pub fn section_label(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .color(theme::TEXT_FAINT)
            .small()
            .strong(),
    );
}

pub fn page_heading(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.label(RichText::new(title).color(theme::TEXT).heading().strong());
    ui.add_space(theme::S1);
    ui.label(RichText::new(subtitle).color(theme::TEXT_DIM));
}

pub fn primary_button(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let button = egui::Button::new(RichText::new(text).color(Color32::WHITE).strong())
        .fill(if enabled {
            theme::ACCENT
        } else {
            theme::BG_MUTED
        })
        .corner_radius(theme::R_SM)
        .min_size(Vec2::new(0.0, 30.0));
    ui.add_enabled(enabled, button)
}

pub fn secondary_button(ui: &mut Ui, text: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(theme::TEXT_DIM))
            .fill(theme::BG_SURFACE)
            .stroke(Stroke::new(1.0, theme::BORDER))
            .corner_radius(theme::R_MD)
            .min_size(Vec2::new(0.0, 30.0)),
    )
}

pub fn ghost_button(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let button = egui::Button::new(RichText::new(text).color(theme::TEXT_DIM))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::NONE)
        .corner_radius(theme::R_SM);
    ui.add_enabled(enabled, button)
}

pub fn card_frame(radius: u8) -> egui::Frame {
    egui::Frame::default()
        .fill(theme::BG_SURFACE)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(radius)
        .inner_margin(theme::S3 as i8)
}

pub fn search_field(ui: &mut Ui, value: &mut String, hint: &str) {
    card_frame(theme::R_MD).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(icon::MAGNIFYING_GLASS).color(theme::TEXT_FAINT));
            ui.add(
                egui::TextEdit::singleline(value)
                    .hint_text(RichText::new(hint).color(theme::TEXT_FAINT))
                    .frame(egui::Frame::NONE)
                    .desired_width(f32::INFINITY),
            );
        });
    });
}

pub fn status_dot(ui: &mut Ui, color: Color32, diameter: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), diameter / 2.0, color);
}

pub fn pill(ui: &mut Ui, id_salt: &str, add_contents: impl FnOnce(&mut Ui)) -> Response {
    let inner = egui::Frame::default()
        .fill(theme::BG_SURFACE)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(theme::R_PILL)
        .inner_margin(egui::Margin::symmetric(
            theme::S3 as i8,
            theme::S1 as i8 + 2,
        ))
        .show(ui, |ui| {
            ui.horizontal(|ui| add_contents(ui));
        });

    let response = ui.interact(
        inner.response.rect,
        ui.id().with(("pill", id_salt)),
        Sense::click(),
    );
    if response.hovered() {
        ui.painter().rect_stroke(
            inner.response.rect,
            CornerRadius::same(theme::R_PILL),
            Stroke::new(1.0, theme::ACCENT),
            StrokeKind::Inside,
        );
    }
    response
}

pub fn avatar(ui: &mut Ui, name: &str, color: Color32, diameter: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::hover());
    let initial = name
        .chars()
        .next()
        .unwrap_or('?')
        .to_uppercase()
        .to_string();

    ui.painter()
        .circle_filled(rect.center(), diameter / 2.0, theme::tint(color, 44));
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        initial,
        egui::FontId::proportional(diameter * 0.44),
        color,
    );
}

pub fn fixed_card(
    ui: &mut Ui,
    size: Vec2,
    radius: u8,
    padding: f32,
    add_contents: impl FnOnce(&mut Ui),
) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());

    let hovered = response.hovered();
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(radius), theme::BG_SURFACE);
    painter.rect_stroke(
        rect,
        CornerRadius::same(radius),
        Stroke::new(
            1.0,
            if hovered {
                theme::ACCENT
            } else {
                theme::BORDER
            },
        ),
        StrokeKind::Inside,
    );

    let content_rect = rect.shrink(padding);
    let mut content_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    add_contents(&mut content_ui);

    response
}

pub fn corner_glow(ui: &Ui, rect: Rect, color: Color32) {
    let painter = ui.painter().with_clip_rect(rect);
    let center = rect.left_top() + Vec2::new(rect.width() * 0.18, rect.height() * 0.10);
    for step in 0..4 {
        let radius = 30.0 + step as f32 * 18.0;
        let alpha = 14u8.saturating_sub(step * 3);
        painter.circle_filled(center, radius, theme::tint(color, alpha));
    }
}

pub fn divider(ui: &mut Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0, theme::BORDER);
}

pub fn divider_spacer(ui: &mut Ui) {
    ui.add_space(theme::S1);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(1.0, 18.0), Sense::hover());
    ui.painter().rect_filled(rect, 0, theme::BORDER);
    ui.add_space(theme::S1);
}

pub fn empty_state(ui: &mut Ui, title: &str, hint: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(theme::S6);
        ui.label(RichText::new(title).color(theme::TEXT_DIM).size(15.0));
        ui.add_space(theme::S1);
        ui.label(RichText::new(hint).color(theme::TEXT_FAINT).small());
    });
}

#[derive(Clone, PartialEq, Eq)]
pub enum EditorOutcome {
    Commit,
    Cancel,
}

pub struct InlineEditor<'a> {
    pub glyph: &'a str,
    pub context: Option<&'a str>,
    pub hint: &'a str,
    pub commit_label: Option<&'a str>,
    pub width: Option<f32>,
    pub id_salt: &'a str,
}

pub fn inline_editor(
    ui: &mut Ui,
    cfg: InlineEditor<'_>,
    buffer: &mut String,
) -> Option<EditorOutcome> {
    let InlineEditor {
        glyph,
        context,
        hint,
        commit_label,
        width,
        id_salt,
    } = cfg;

    let mut outcome = None;

    ui.horizontal(|ui| {
        ui.label(RichText::new(glyph).color(theme::ACCENT));

        if let Some(context) = context {
            ui.label(
                RichText::new(context)
                    .color(theme::TEXT_FAINT)
                    .monospace()
                    .small(),
            );
        }

        let desired = width.unwrap_or_else(|| (ui.available_width() - 96.0).max(120.0));
        let field = ui.add(
            egui::TextEdit::singleline(buffer)
                .id_salt(id_salt)
                .font(egui::TextStyle::Monospace)
                .hint_text(RichText::new(hint).color(theme::TEXT_FAINT))
                .desired_width(desired),
        );

        if !field.has_focus() {
            field.request_focus();
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), field.id) {
                let all = egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(buffer.chars().count()),
                );
                state.cursor.set_char_range(Some(all));
                state.store(ui.ctx(), field.id);
            }
        }

        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
        if enter && (field.has_focus() || field.lost_focus()) {
            outcome = Some(EditorOutcome::Commit);
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            outcome = Some(EditorOutcome::Cancel);
        }

        if let Some(commit_label) = commit_label {
            if ui.small_button(commit_label).clicked() {
                outcome = Some(EditorOutcome::Commit);
            }
            if ui.small_button("Cancel").clicked() {
                outcome = Some(EditorOutcome::Cancel);
            }
        }
    });

    outcome
}
