use eframe::egui::{self, Color32, FontFamily, FontId, TextStyle};

pub const BACKGROUND: Color32 = Color32::from_rgb(33, 33, 33);
pub const SURFACE: Color32 = Color32::from_rgb(24, 24, 24);
pub const SURFACE_RAISED: Color32 = Color32::from_rgb(47, 47, 47);
pub const ACCENT: Color32 = Color32::from_rgb(16, 163, 127);
pub const TEXT: Color32 = Color32::from_rgb(226, 226, 226);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(158, 158, 158);

pub fn install_theme(context: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Inter".into(),
        egui::FontData::from_static(include_bytes!("../../assets/fonts/Inter.ttf")).into(),
    );
    fonts
        .families
        .get_mut(&FontFamily::Proportional)
        .unwrap()
        .insert(0, "Inter".into());
    context.set_fonts(fonts);
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BACKGROUND;
    visuals.window_fill = SURFACE;
    visuals.extreme_bg_color = SURFACE;
    visuals.faint_bg_color = SURFACE_RAISED;
    visuals.override_text_color = Some(Color32::from_rgb(236, 236, 236));
    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.5);
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::NONE;
    visuals.widgets.inactive.bg_fill = SURFACE_RAISED;
    // A restrained outline keeps checkboxes and inputs visible on raised cards.
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, Color32::from_gray(68));
    visuals.widgets.hovered.bg_fill = Color32::from_gray(62);
    visuals.widgets.active.bg_fill = Color32::from_gray(72);
    visuals.window_corner_radius = egui::CornerRadius::same(14);
    visuals.menu_corner_radius = egui::CornerRadius::same(10);
    let mut style = (*context.style()).clone();
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(15.0));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::proportional(14.0));
    style
        .text_styles
        .insert(TextStyle::Small, FontId::proportional(12.0));
    style
        .text_styles
        .insert(TextStyle::Heading, FontId::proportional(22.0));
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.spacing.interact_size.y = 32.0;
    context.set_style(style);
    context.set_visuals(visuals);
}
