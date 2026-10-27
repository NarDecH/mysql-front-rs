//! Application colour identity and themes.
//!
//! The accent is Rust orange (`#F76707`) on a navy chrome — a nod to the
//! project name. Both the light and dark themes are configured once at
//! startup; the active theme can be switched at runtime.

use egui::{Color32, Context, Stroke, Theme, Visuals};

pub const ACCENT: Color32 = Color32::from_rgb(232, 89, 12); // #E8590C
pub const ACCENT_BRIGHT: Color32 = Color32::from_rgb(255, 146, 43); // #FF922B
pub const NAVY: Color32 = Color32::from_rgb(27, 42, 65); // #1B2A41

/// Object-browser icon glyphs, kept as one consistent set.
pub struct ObjIcon {
    pub glyph: &'static str,
    pub color: Color32,
}

pub fn table_icon() -> ObjIcon {
    ObjIcon { glyph: "▤", color: Color32::from_rgb(76, 110, 245) }
}

pub fn view_icon() -> ObjIcon {
    ObjIcon { glyph: "◫", color: Color32::from_rgb(12, 166, 170) }
}

pub fn routine_icon() -> ObjIcon {
    ObjIcon { glyph: "ƒ", color: Color32::from_rgb(150, 90, 220) }
}

pub fn trigger_icon() -> ObjIcon {
    ObjIcon { glyph: "⚡", color: Color32::from_rgb(230, 145, 10) }
}

pub fn event_icon() -> ObjIcon {
    ObjIcon { glyph: "⏰", color: Color32::from_rgb(120, 130, 145) }
}

/// The theme-appropriate accent colour.
pub fn accent(ctx: &Context) -> Color32 {
    match ctx.theme() {
        Theme::Dark => ACCENT_BRIGHT,
        _ => ACCENT,
    }
}

/// Configure how both themes look; call once at startup.
pub fn install(ctx: &Context) {
    let mut light = Visuals::light();
    light.selection.bg_fill = ACCENT.gamma_multiply(0.28);
    light.selection.stroke = Stroke::new(1.2, ACCENT);
    light.hyperlink_color = ACCENT;
    light.widgets.hovered.weak_bg_fill = Color32::from_rgb(232, 236, 242);
    light.panel_fill = Color32::from_rgb(243, 245, 248);
    light.extreme_bg_color = Color32::WHITE;
    light.window_fill = Color32::from_rgb(248, 249, 252);

    let mut dark = Visuals::dark();
    dark.selection.bg_fill = ACCENT_BRIGHT.gamma_multiply(0.30);
    dark.selection.stroke = Stroke::new(1.2, ACCENT_BRIGHT);
    dark.hyperlink_color = ACCENT_BRIGHT;
    dark.panel_fill = Color32::from_rgb(26, 30, 38);
    dark.extreme_bg_color = Color32::from_rgb(18, 21, 27);
    dark.window_fill = Color32::from_rgb(32, 37, 46);

    ctx.set_visuals_of(Theme::Light, light);
    ctx.set_visuals_of(Theme::Dark, dark);
    ctx.set_theme(Theme::Light);
}

/// A filled primary-action button in the accent colour.
pub fn primary_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let c = accent(ui.ctx());
    ui.add(
        egui::Button::new(egui::RichText::new(label).strong().color(Color32::WHITE))
            .fill(c)
            .min_size(egui::vec2(0.0, 24.0)),
    )
}

/// Draw one object-browser entry: coloured glyph + normal text.
pub fn icon_label(ui: &mut egui::Ui, icon: &ObjIcon, text: &str) -> egui::Response {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let fg = ui.visuals().text_color();
    let mut job = egui::text::LayoutJob::default();
    job.append(
        icon.glyph,
        6.0,
        egui::text::TextFormat::simple(font.clone(), icon.color),
    );
    job.append(text, 0.0, egui::text::TextFormat::simple(font, fg));
    ui.add(egui::Label::new(job))
}
