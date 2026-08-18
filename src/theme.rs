use egui::{Color32, CornerRadius, Stroke, Visuals};

pub const BG_BASE: Color32 = Color32::from_rgb(0x07, 0x0A, 0x10);
pub const BG_SURFACE: Color32 = Color32::from_rgb(0x10, 0x14, 0x1B);
pub const BG_OVERLAY: Color32 = Color32::from_rgb(0x19, 0x1D, 0x24);
pub const BG_SUBTLE: Color32 = Color32::from_rgb(0x23, 0x26, 0x2E);
pub const BG_MUTED: Color32 = Color32::from_rgb(0x30, 0x33, 0x39);

pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xE9, 0xED, 0xF4);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(0xA0, 0xA5, 0xAE);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x7A, 0x7F, 0x88);

pub const ACCENT: Color32 = Color32::from_rgb(0xEC, 0x48, 0x99);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0xDB, 0x27, 0x77);

pub const STATUS_CONNECTED: Color32 = Color32::from_rgb(0x00, 0xC4, 0x70);
pub const STATUS_CONNECTING: Color32 = Color32::from_rgb(0xE2, 0xA0, 0x00);
pub const STATUS_ERROR: Color32 = Color32::from_rgb(0xF1, 0x4D, 0x4C);
pub const STATUS_DISCONNECTED: Color32 = Color32::from_rgb(0x70, 0x75, 0x7E);

pub const BORDER: Color32 = Color32::from_rgb(0x30, 0x33, 0x39);

pub fn accent_tint(alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), alpha)
}

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

pub fn visuals() -> Visuals {
    let mut v = Visuals::dark();

    v.override_text_color = Some(TEXT_PRIMARY);
    v.panel_fill = BG_BASE;
    v.window_fill = BG_OVERLAY;
    v.extreme_bg_color = BG_BASE;
    v.faint_bg_color = BG_SUBTLE;
    v.hyperlink_color = ACCENT;

    v.selection.bg_fill = accent_tint(26);
    v.selection.stroke = Stroke::new(1.0, ACCENT);

    v.widgets.noninteractive.bg_fill = BG_SURFACE;
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_MUTED);
    v.widgets.noninteractive.corner_radius = CornerRadius::same(8);

    v.widgets.inactive.bg_fill = BG_SURFACE;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_SECONDARY);
    v.widgets.inactive.corner_radius = CornerRadius::same(8);

    v.widgets.hovered.bg_fill = BG_SUBTLE;
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, BG_SUBTLE);
    v.widgets.hovered.corner_radius = CornerRadius::same(8);

    v.widgets.active.bg_fill = BG_MUTED;
    v.widgets.active.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.active.corner_radius = CornerRadius::same(8);

    v.widgets.open.bg_fill = BG_OVERLAY;
    v.widgets.open.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);

    v.window_corner_radius = CornerRadius::same(10);
    v.window_stroke = Stroke::new(1.0, BORDER);

    v
}

pub fn ok_color() -> Color32 {
    STATUS_CONNECTED
}

pub fn err_color() -> Color32 {
    STATUS_ERROR
}
