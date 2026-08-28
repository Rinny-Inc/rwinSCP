use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Visuals};

pub const BG_BASE: Color32 = Color32::from_rgb(0x07, 0x0A, 0x10);
pub const BG_SURFACE: Color32 = Color32::from_rgb(0x10, 0x14, 0x1B);
pub const BG_OVERLAY: Color32 = Color32::from_rgb(0x19, 0x1D, 0x24);
pub const BG_SUBTLE: Color32 = Color32::from_rgb(0x23, 0x26, 0x2E);
pub const BG_MUTED: Color32 = Color32::from_rgb(0x30, 0x33, 0x39);

pub const TEXT: Color32 = Color32::from_rgb(0xE9, 0xED, 0xF4);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0xA0, 0xA5, 0xAE);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x7A, 0x7F, 0x88);

pub const ACCENT: Color32 = Color32::from_rgb(0xEC, 0x48, 0x99);
// FIXME: reserved for a hover effect
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0xF0, 0x6C, 0xAD);

pub const OK: Color32 = Color32::from_rgb(0x00, 0xC4, 0x70);
pub const PENDING: Color32 = Color32::from_rgb(0xE2, 0xA0, 0x00);
pub const DANGER: Color32 = Color32::from_rgb(0xF1, 0x4D, 0x4C);

pub const BORDER: Color32 = Color32::from_rgb(0x30, 0x33, 0x39);

pub const HOST_COLORS: [Color32; 8] = [
    Color32::from_rgb(0xEF, 0x44, 0x44),
    Color32::from_rgb(0xF9, 0x73, 0x16),
    Color32::from_rgb(0xEA, 0xB3, 0x08),
    Color32::from_rgb(0x22, 0xC5, 0x5E),
    Color32::from_rgb(0x06, 0xB6, 0xD4),
    Color32::from_rgb(0x3B, 0x82, 0xF6),
    Color32::from_rgb(0x8B, 0x5C, 0xF6),
    Color32::from_rgb(0xEC, 0x48, 0x99),
];

pub const R_SM: u8 = 6;
pub const R_MD: u8 = 8;
pub const R_LG: u8 = 12;
pub const R_XL: u8 = 16;
pub const R_PILL: u8 = 20;

pub const S1: f32 = 4.0;
pub const S2: f32 = 8.0;
pub const S3: f32 = 12.0;
pub const S4: f32 = 16.0;
pub const S5: f32 = 24.0;
pub const S6: f32 = 32.0;

pub fn tint(c: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha)
}

pub fn host_color(name: &str) -> Color32 {
    let mut hash: i32 = 0;
    for c in name.chars() {
        hash = hash.wrapping_mul(31).wrapping_add(c as i32);
    }
    HOST_COLORS[(hash.unsigned_abs() as usize) % HOST_COLORS.len()]
}

pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.set_visuals(visuals());

    ctx.all_styles_mut(|style| {
        use FontFamily::{Monospace, Proportional};
        style.text_styles = [
            (TextStyle::Heading, FontId::new(26.0, Proportional)),
            (TextStyle::Body, FontId::new(14.0, Proportional)),
            (TextStyle::Button, FontId::new(14.0, Proportional)),
            (TextStyle::Small, FontId::new(11.5, Proportional)),
            (TextStyle::Monospace, FontId::new(12.5, Monospace)),
        ]
        .into();

        style.spacing.item_spacing = egui::vec2(S2, S2);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style.spacing.menu_margin = egui::Margin::same(6);
        style.spacing.interact_size.y = 28.0;

        style.interaction.selectable_labels = false;
    });
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    fonts.font_data.insert(
        "phosphor".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/Phosphor.ttf"
        ))),
    );

    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("phosphor".to_owned());
    }

    ctx.set_fonts(fonts);
}

fn visuals() -> Visuals {
    let mut v = Visuals::dark();

    v.override_text_color = Some(TEXT);
    v.panel_fill = BG_BASE;
    v.window_fill = BG_OVERLAY;
    v.extreme_bg_color = BG_BASE;
    v.faint_bg_color = BG_SUBTLE;
    v.hyperlink_color = ACCENT;

    v.selection.bg_fill = tint(ACCENT, 28);
    v.selection.stroke = Stroke::new(1.0, ACCENT);

    let radius = CornerRadius::same(R_MD);

    v.widgets.noninteractive.bg_fill = BG_SURFACE;
    v.widgets.noninteractive.weak_bg_fill = BG_SURFACE;
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_FAINT);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.noninteractive.corner_radius = radius;

    v.widgets.inactive.bg_fill = BG_SURFACE;
    v.widgets.inactive.weak_bg_fill = BG_SURFACE;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_DIM);
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.inactive.corner_radius = radius;

    v.widgets.hovered.bg_fill = BG_SUBTLE;
    v.widgets.hovered.weak_bg_fill = BG_SUBTLE;
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, BG_MUTED);
    v.widgets.hovered.corner_radius = radius;

    v.widgets.active.bg_fill = BG_MUTED;
    v.widgets.active.weak_bg_fill = BG_MUTED;
    v.widgets.active.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.active.corner_radius = radius;

    v.widgets.open.bg_fill = BG_OVERLAY;
    v.widgets.open.weak_bg_fill = BG_OVERLAY;
    v.widgets.open.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.open.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.open.corner_radius = radius;

    v.window_corner_radius = CornerRadius::same(R_LG);
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.popup_shadow = egui::epaint::Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: Color32::from_black_alpha(120),
    };

    v
}
