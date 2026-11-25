// src/theme.rs
//! Paleta Café Pastel (Claro / Oscuro) y tipografía editorial.

use egui::{
    Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, Shadow, Stroke, Visuals,
};

use crate::storage::{CustomColors, ThemeMode};

pub struct Palette {
    pub canvas: Color32,
    pub side: Color32,
    pub card: Color32,
    pub hover: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_weak: Color32,
}

impl Palette {
    pub const fn of(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Light => Self {
                canvas: Color32::from_rgb(0xF5, 0xEF, 0xE6),
                side: Color32::from_rgb(0xEA, 0xE0, 0xD5),
                card: Color32::from_rgb(0xFA, 0xF6, 0xF0),
                hover: Color32::from_rgb(0xE2, 0xD4, 0xC5),
                accent: Color32::from_rgb(0xA6, 0x7B, 0x5B),
                on_accent: Color32::from_rgb(0xFA, 0xF6, 0xF0),
                border: Color32::from_rgb(0xD5, 0xC7, 0xB8),
                text: Color32::from_rgb(0x2C, 0x22, 0x1E),
                text_weak: Color32::from_rgb(0x6E, 0x5D, 0x54),
            },
            ThemeMode::Black => Self {
                canvas: Color32::from_rgb(0x0A, 0x0A, 0x0B),
                side: Color32::from_rgb(0x11, 0x11, 0x13),
                card: Color32::from_rgb(0x17, 0x17, 0x1A),
                hover: Color32::from_rgb(0x20, 0x20, 0x24),
                accent: Color32::from_rgb(0xC8, 0xB5, 0x98),
                on_accent: Color32::from_rgb(0x0A, 0x0A, 0x0B),
                border: Color32::from_rgb(0x2A, 0x2A, 0x2F),
                text: Color32::from_rgb(0xEC, 0xE8, 0xE1),
                text_weak: Color32::from_rgb(0x8E, 0x8A, 0x84),
            },
            ThemeMode::Matrix => Self {
                canvas: Color32::from_rgb(0x05, 0x0B, 0x07),
                side: Color32::from_rgb(0x0A, 0x14, 0x0D),
                card: Color32::from_rgb(0x10, 0x1C, 0x14),
                hover: Color32::from_rgb(0x17, 0x28, 0x1D),
                accent: Color32::from_rgb(0x5F, 0xBF, 0x7A),
                on_accent: Color32::from_rgb(0x04, 0x12, 0x0A),
                border: Color32::from_rgb(0x1E, 0x33, 0x25),
                text: Color32::from_rgb(0xCF, 0xE8, 0xD5),
                text_weak: Color32::from_rgb(0x6F, 0x9A, 0x7B),
            },
            ThemeMode::Dark => Self {
                canvas: Color32::from_rgb(0x1E, 0x19, 0x16),
                side: Color32::from_rgb(0x26, 0x20, 0x1C),
                card: Color32::from_rgb(0x2F, 0x27, 0x22),
                hover: Color32::from_rgb(0x3A, 0x30, 0x2A),
                accent: Color32::from_rgb(0xC6, 0x9C, 0x72),
                on_accent: Color32::from_rgb(0x1E, 0x19, 0x16),
                border: Color32::from_rgb(0x3D, 0x33, 0x2C),
                text: Color32::from_rgb(0xEF, 0xE6, 0xDD),
                text_weak: Color32::from_rgb(0xA8, 0x98, 0x8E),
            },
        }
    }
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

fn luminance(c: Color32) -> f32 {
    (0.2126 * c.r() as f32 + 0.7152 * c.g() as f32 + 0.0722 * c.b() as f32) / 255.0
}

fn opaque(c: [u8; 4]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

impl Palette {
    /// Paleta efectiva: el preset, o los cinco colores elegidos a mano (el resto se deriva).
    pub fn resolve(mode: ThemeMode, custom: Option<&CustomColors>) -> Self {
        let Some(c) = custom else {
            return Self::of(mode);
        };
        let (canvas, side, card) = (opaque(c.background), opaque(c.panels), opaque(c.cards));
        let (accent, text) = (opaque(c.accent), opaque(c.text));
        Self {
            canvas,
            side,
            card,
            hover: mix(card, text, 0.12),
            accent,
            on_accent: if luminance(accent) > 0.55 {
                Color32::from_rgb(0x14, 0x10, 0x0E)
            } else {
                Color32::from_rgb(0xFA, 0xF6, 0xF0)
            },
            border: mix(canvas, text, 0.2),
            text,
            text_weak: mix(text, canvas, 0.38),
        }
    }

    /// Los cinco colores editables de esta paleta (punto de partida para personalizar).
    pub fn to_custom(&self) -> CustomColors {
        let a = |c: Color32| [c.r(), c.g(), c.b(), 255];
        CustomColors {
            background: a(self.canvas),
            panels: a(self.side),
            cards: a(self.card),
            accent: a(self.accent),
            text: a(self.text),
        }
    }

    pub fn is_dark(&self) -> bool {
        luminance(self.canvas) < 0.5
    }
}

/// Familia tipográfica editorial (serif). Siempre registrada; cae a la proporcional si no hay serif en el sistema.
pub fn serif() -> FontFamily {
    FontFamily::Name("Serif".into())
}

pub fn serif_bold() -> FontFamily {
    FontFamily::Name("SerifBold".into())
}

pub fn sans_bold() -> FontFamily {
    FontFamily::Name("SansBold".into())
}

fn read_first(paths: &[&str]) -> Option<Vec<u8>> {
    paths.iter().find_map(|p| std::fs::read(p).ok())
}

/// Registra las fuentes del sistema (Georgia / Segoe UI en Windows). Llamar una vez al arrancar.
/// Si alguna no existe, cada familia cae a la proporcional por defecto.
pub fn install_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::default();
    let base = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();

    let sets: [(FontFamily, &str, &[&str]); 3] = [
        (
            serif(),
            "serif_system",
            &[
                "C:\\Windows\\Fonts\\georgia.ttf",
                "C:\\Windows\\Fonts\\times.ttf",
                "/System/Library/Fonts/Supplemental/Georgia.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSerif.ttf",
                "/usr/share/fonts/TTF/DejaVuSerif.ttf",
            ],
        ),
        (
            serif_bold(),
            "serif_system_bold",
            &[
                "C:\\Windows\\Fonts\\georgiab.ttf",
                "C:\\Windows\\Fonts\\timesbd.ttf",
                "/System/Library/Fonts/Supplemental/Georgia Bold.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSerif-Bold.ttf",
                "/usr/share/fonts/TTF/DejaVuSerif-Bold.ttf",
            ],
        ),
        (
            sans_bold(),
            "sans_system_bold",
            &[
                "C:\\Windows\\Fonts\\segoeuib.ttf",
                "C:\\Windows\\Fonts\\arialbd.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
                "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
            ],
        ),
    ];

    for (family, key, paths) in sets {
        let mut list = base.clone();
        if let Some(bytes) = read_first(paths) {
            fonts.font_data.insert(
                key.to_owned(),
                std::sync::Arc::new(FontData::from_owned(bytes)),
            );
            list.insert(0, key.to_owned());
        }
        fonts.families.insert(family, list);
    }
    ctx.set_fonts(fonts);
}

/// Aplica el tema completo al contexto.
pub fn apply(ctx: &Context, mode: ThemeMode, custom: Option<&CustomColors>) {
    let p = Palette::resolve(mode, custom);
    let mut v = if p.is_dark() {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let r = CornerRadius::same(6);

    v.panel_fill = p.canvas;
    v.window_fill = p.canvas;
    v.faint_bg_color = p.side;
    v.extreme_bg_color = p.card;
    v.code_bg_color = p.side;
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent;
    v.selection.stroke = Stroke::new(1.0, p.on_accent);
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(8);
    v.menu_corner_radius = r;
    v.window_shadow = Shadow::NONE;
    v.popup_shadow = Shadow::NONE;

    let border = Stroke::new(1.0, p.border);
    let w = &mut v.widgets;

    w.noninteractive.bg_fill = p.canvas;
    w.noninteractive.weak_bg_fill = p.canvas;
    w.noninteractive.bg_stroke = border;
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = r;

    w.inactive.bg_fill = p.card;
    w.inactive.weak_bg_fill = p.card;
    w.inactive.bg_stroke = border;
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);
    w.inactive.corner_radius = r;

    w.hovered.bg_fill = p.hover;
    w.hovered.weak_bg_fill = p.hover;
    w.hovered.bg_stroke = Stroke::new(1.0, p.accent);
    w.hovered.fg_stroke = Stroke::new(1.0, p.text);
    w.hovered.corner_radius = r;

    w.active.bg_fill = p.accent;
    w.active.weak_bg_fill = p.accent;
    w.active.bg_stroke = Stroke::new(1.0, p.accent);
    w.active.fg_stroke = Stroke::new(1.0, p.on_accent);
    w.active.corner_radius = r;

    w.open.bg_fill = p.hover;
    w.open.weak_bg_fill = p.hover;
    w.open.bg_stroke = border;
    w.open.fg_stroke = Stroke::new(1.0, p.text);
    w.open.corner_radius = r;

    ctx.set_visuals(v);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_and_custom_resolve() {
        assert!(!Palette::of(ThemeMode::Light).is_dark());
        for m in [ThemeMode::Dark, ThemeMode::Black, ThemeMode::Matrix] {
            assert!(Palette::of(m).is_dark(), "{m:?}");
        }
        // Sin colores propios devuelve el preset; con ellos, los usa y deriva el resto.
        let base = Palette::of(ThemeMode::Matrix);
        assert_eq!(
            Palette::resolve(ThemeMode::Matrix, None).accent,
            base.accent
        );
        let mut c = base.to_custom();
        c.accent = [250, 220, 40, 255];
        c.background = [250, 250, 250, 255];
        c.text = [20, 20, 20, 255];
        let p = Palette::resolve(ThemeMode::Matrix, Some(&c));
        assert_eq!(p.accent, Color32::from_rgb(250, 220, 40));
        assert!(!p.is_dark()); // el fondo claro manda sobre el preset
        assert!(luminance(p.on_accent) < 0.3); // texto oscuro sobre acento claro
        assert_ne!(p.border, p.canvas);
    }
}
