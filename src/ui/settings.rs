// src/ui/settings.rs
//! Modal de Ajustes Generales: perfil, apariencia y almacenamiento.

use egui::{
    Align, Align2, Button, Color32, Context, CornerRadius, FontId, Id, Layout, Modal, RichText,
    Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2,
};

use crate::i18n::{tr, Language};
use crate::storage::{self, CustomColors, Settings, ThemeMode};
use crate::textures::TextureCache;
use crate::theme::{self, Palette};

#[derive(Default)]
pub struct SettingsDialog {
    open: bool,
    draft: Option<Settings>,
    error: Option<String>,
}

enum Outcome {
    None,
    Save,
    Cancel,
}

impl SettingsDialog {
    /// Abre el modal con una copia editable de los ajustes actuales.
    pub fn open(&mut self, current: &Settings) {
        self.draft = Some(current.clone());
        self.error = None;
        self.open = true;
    }

    pub fn show(&mut self, ctx: &Context, settings: &mut Settings, textures: &mut TextureCache) {
        if !self.open {
            return;
        }
        let Some(draft) = self.draft.as_mut() else {
            self.open = false;
            return;
        };

        let mut outcome = Outcome::None;
        let error = &mut self.error;

        let modal = Modal::new(Id::unique("settings_modal")).show(ctx, |ui| {
            ui.set_width(460.0);
            let max_h = (ctx.content_rect().height() - 120.0).max(240.0);
            egui::ScrollArea::vertical()
                .max_height(max_h)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    outcome = content(ui, ctx, draft, textures, error);
                });
        });
        if modal.should_close() && matches!(outcome, Outcome::None) {
            outcome = Outcome::Cancel;
        }

        match outcome {
            Outcome::None => {}
            Outcome::Cancel => {
                theme::apply(ctx, settings.theme, settings.custom_colors.as_ref()); // revierte la vista previa
                crate::i18n::set_language(settings.language);
                self.close();
            }
            Outcome::Save => {
                let mut new = draft.clone();
                new.user_name = new.user_name.trim().to_owned();
                if new.user_name.is_empty() {
                    new.user_name = "Autor".to_owned();
                }
                match new.save() {
                    Ok(()) => {
                        *settings = new;
                        crate::i18n::set_language(settings.language);
                        theme::apply(ctx, settings.theme, settings.custom_colors.as_ref());
                        self.close();
                    }
                    Err(e) => self.error = Some(e),
                }
            }
        }
    }

    fn close(&mut self) {
        self.open = false;
        self.draft = None;
        self.error = None;
    }
}

fn section_title(ui: &mut Ui, text: &str) {
    ui.add_space(14.0);
    ui.label(RichText::new(text).family(theme::serif()).size(18.0));
    ui.add_space(2.0);
    ui.separator();
    ui.add_space(6.0);
}

fn content(
    ui: &mut Ui,
    ctx: &Context,
    draft: &mut Settings,
    textures: &mut TextureCache,
    error: &mut Option<String>,
) -> Outcome {
    let mut outcome = Outcome::None;
    let weak = ui.visuals().weak_text_color();

    // Encabezado
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(tr("Ajustes generales"))
                .family(theme::serif())
                .size(26.0),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add(Button::new(RichText::new("✕").size(15.0)).frame(false))
                .on_hover_text(tr("Cerrar"))
                .clicked()
            {
                outcome = Outcome::Cancel;
            }
        });
    });

    // --- Perfil ---
    section_title(ui, tr("Perfil de Autor (opcional)"));
    ui.horizontal(|ui| {
        avatar_preview(ui, draft, textures);
        ui.add_space(10.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(tr("Nombre")).size(12.5).color(weak));
            ui.add(
                TextEdit::singleline(&mut draft.user_name)
                    .desired_width(240.0)
                    .margin(Vec2::new(8.0, 6.0)),
            );
            ui.add_space(4.0);
            if ui.button(tr("Cambiar foto")).clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title(tr("Elegir foto de perfil"))
                    .add_filter("Imágenes", &["png", "jpg", "jpeg", "webp"])
                    .pick_file()
                {
                    match storage::import_avatar(&path) {
                        Ok(()) => {
                            textures.invalidate(&storage::avatar_path());
                            *error = None;
                        }
                        Err(e) => *error = Some(e),
                    }
                }
            }
        });
    });

    // --- Idioma ---
    section_title(ui, tr("Idioma / Language"));
    ui.horizontal(|ui| {
        for lang in Language::ALL {
            if ui
                .selectable_label(draft.language == lang, lang.native_name())
                .clicked()
            {
                draft.language = lang;
                crate::i18n::set_language(lang); // vista previa inmediata
            }
        }
    });

    // --- Apariencia ---
    section_title(ui, tr("Apariencia"));
    ui.label(RichText::new(tr("Temas rápidos")).size(12.5).color(weak));
    ui.add_space(4.0);
    let w = (ui.available_width() - 10.0) / 2.0;
    for pair in ThemeMode::ALL.chunks(2) {
        ui.horizontal(|ui| {
            for &mode in pair {
                let selected = draft.theme == mode && draft.custom_colors.is_none();
                if theme_card(ui, mode, mode.label(), selected, w).clicked() && !selected {
                    draft.theme = mode;
                    draft.custom_colors = None; // un preset reemplaza los colores propios
                    theme::apply(ctx, mode, None); // vista previa inmediata
                }
            }
        });
        ui.add_space(4.0);
    }

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(tr("Colores personalizados"))
                .size(12.5)
                .color(weak),
        );
        if draft.custom_colors.is_some() {
            ui.label(RichText::new(tr("· activos")).size(12.5).color(weak));
            if ui.small_button(tr("Restablecer al tema")).clicked() {
                draft.custom_colors = None;
                theme::apply(ctx, draft.theme, None);
            }
        }
    });
    ui.add_space(4.0);
    let mut cc: CustomColors = draft
        .custom_colors
        .unwrap_or_else(|| Palette::of(draft.theme).to_custom());
    let mut edited = false;
    let rows: [(&str, &mut [u8; 4]); 5] = [
        (tr("Fondo principal"), &mut cc.background),
        (tr("Paneles y barras"), &mut cc.panels),
        (tr("Tarjetas e inputs"), &mut cc.cards),
        (tr("Acento"), &mut cc.accent),
        ("Texto", &mut cc.text),
    ];
    for (label, value) in rows {
        ui.horizontal(|ui| {
            let mut c = Color32::from_rgb(value[0], value[1], value[2]);
            if ui.color_edit_button_srgba(&mut c).changed() {
                *value = [c.r(), c.g(), c.b(), 255];
                edited = true;
            }
            ui.label(label);
        });
    }
    if edited {
        draft.custom_colors = Some(cc);
        theme::apply(ctx, draft.theme, Some(&cc)); // vista previa inmediata
    }

    // --- Almacenamiento ---
    section_title(ui, tr("Almacenamiento"));
    ui.label(
        RichText::new(tr("Carpeta donde se guardan tus historias"))
            .size(12.5)
            .color(weak),
    );
    ui.add_space(2.0);
    ui.label(
        RichText::new(draft.root_folder.display().to_string())
            .monospace()
            .size(12.5),
    );
    ui.add_space(6.0);
    if ui.button(tr("Cambiar carpeta")).clicked() {
        if let Some(dir) = rfd::FileDialog::new()
            .set_title(tr("Elegir carpeta de historias"))
            .set_directory(&draft.root_folder)
            .pick_folder()
        {
            draft.root_folder = dir;
        }
    }

    if let Some(e) = error.as_deref() {
        ui.add_space(8.0);
        ui.label(
            RichText::new(e)
                .size(12.5)
                .color(Color32::from_rgb(0xB0, 0x4A, 0x3A)),
        );
    }

    // --- Pie ---
    ui.add_space(18.0);
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let accent = ui.visuals().selection.bg_fill;
        let on_accent = ui.visuals().selection.stroke.color;
        let save = Button::new(
            RichText::new(tr("Guardar y cerrar"))
                .color(on_accent)
                .size(14.0),
        )
        .fill(accent)
        .stroke(Stroke::new(1.0, accent))
        .min_size(Vec2::new(150.0, 32.0));
        if ui.add(save).clicked() {
            outcome = Outcome::Save;
        }
    });

    // Versión de la aplicación (tomada de Cargo.toml).
    ui.add_space(14.0);
    ui.vertical_centered(|ui| {
        ui.label(
            RichText::new(format!("FableForge v{}", env!("CARGO_PKG_VERSION")))
                .size(11.5)
                .color(weak),
        );
    });

    outcome
}

fn avatar_preview(ui: &mut Ui, draft: &Settings, textures: &mut TextureCache) {
    let size = 64.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let accent = ui.visuals().selection.bg_fill;
    let avatar = storage::avatar_path();
    match avatar.is_file() {
        true => {
            textures.paint(ui, &avatar, rect, CornerRadius::same((size / 2.0) as u8));
        }
        false => {
            ui.painter()
                .circle_filled(rect.center(), size / 2.0, accent);
            let initial = draft
                .user_name
                .chars()
                .find(|c| !c.is_whitespace())
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_else(|| "?".into());
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                initial,
                FontId::new(26.0, theme::serif()),
                ui.visuals().selection.stroke.color,
            );
        }
    }
    ui.painter().circle_stroke(
        rect.center(),
        size / 2.0,
        Stroke::new(1.5, ui.visuals().window_stroke.color),
    );
}

/// Tarjeta seleccionable con miniatura de la paleta.
fn theme_card(
    ui: &mut Ui,
    mode: ThemeMode,
    label: &str,
    selected: bool,
    width: f32,
) -> egui::Response {
    let p = Palette::of(mode);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 92.0), Sense::click());
    let accent = ui.visuals().selection.bg_fill;
    let border = if selected {
        Stroke::new(2.0, accent)
    } else if resp.hovered() {
        Stroke::new(1.0, accent)
    } else {
        Stroke::new(1.0, ui.visuals().window_stroke.color)
    };
    let painter = ui.painter();
    let r = CornerRadius::same(8);
    painter.rect_filled(rect, r, ui.visuals().extreme_bg_color);
    painter.rect_stroke(rect, r, border, StrokeKind::Inside);

    // Miniatura: canvas + barra lateral + "líneas de texto"
    let thumb = egui::Rect::from_min_size(
        rect.min + Vec2::new(10.0, 10.0),
        Vec2::new(width - 20.0, 46.0),
    );
    let tr = CornerRadius::same(5);
    painter.rect_filled(thumb, tr, p.canvas);
    painter.rect_stroke(thumb, tr, Stroke::new(1.0, p.border), StrokeKind::Inside);
    let side = egui::Rect::from_min_size(thumb.min, Vec2::new(16.0, thumb.height()));
    painter.rect_filled(
        side,
        CornerRadius {
            nw: 5,
            sw: 5,
            ne: 0,
            se: 0,
        },
        p.side,
    );
    let line = |y: f32, w: f32, _c: Color32| {
        egui::Rect::from_min_size(thumb.min + Vec2::new(26.0, y), Vec2::new(w, 5.0))
    };
    painter.rect_filled(line(10.0, 56.0, p.text), CornerRadius::same(2), p.text);
    painter.rect_filled(
        line(22.0, 86.0, p.text_weak),
        CornerRadius::same(2),
        p.text_weak,
    );
    painter.rect_filled(line(32.0, 30.0, p.accent), CornerRadius::same(2), p.accent);

    painter.text(
        egui::pos2(rect.center().x, rect.bottom() - 17.0),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(13.5),
        ui.visuals().widgets.noninteractive.fg_stroke.color,
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}
