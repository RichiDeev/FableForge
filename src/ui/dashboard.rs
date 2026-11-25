// src/ui/dashboard.rs
//! Dashboard de Inicio (Sub-etapa 2.1): Crear, Cargar e Importar historia.

use std::path::{Path, PathBuf};

use egui::{
    Align, Align2, Button, Color32, Context, CornerRadius, FontId, Id, Key, Layout, Modal,
    Response, RichText, ScrollArea, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2,
};

use crate::i18n::{tr, trf};
use crate::icons::{self, Icon};
use crate::story::{self, Story, StorySummary};
use crate::theme;

const CARD_H: f32 = 300.0;
const GAP: f32 = 24.0;

#[derive(Default)]
pub struct Dashboard {
    create: Option<CreateForm>,
    load: Option<Vec<StorySummary>>,
    notice: Option<String>,
    /// Autor por defecto de las historias nuevas (perfil de autor).
    pub author: String,
}

struct CreateForm {
    title: String,
    subtitle: String,
    error: Option<String>,
    first_frame: bool,
}

impl Dashboard {
    /// Dibuja las tarjetas y modales. Devuelve la historia a abrir, si la hay.
    pub fn show(&mut self, ui: &mut Ui, root: &Path) -> Option<Story> {
        let ctx = ui.ctx().clone();
        let mut opened: Option<Story> = None;

        // --- Tarjetas ---
        let card_w = ((ui.available_width() - 2.0 * GAP) / 3.0).clamp(220.0, 300.0);
        let size = Vec2::new(card_w, CARD_H);
        let mut clicked = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(GAP);
            let cards = [
                (Icon::Plus, tr("Crear Historia"), tr("Comienza un universo nuevo: FableForge prepara la carpeta con capítulos, lore, entidades y mapas.")),
                (Icon::FolderOpen, tr("Cargar Historia"), tr("Retoma una de las historias guardadas en tu carpeta de historias.")),
                (Icon::Import, tr("Importar Historia"), tr("Abre una carpeta de proyecto existente desde cualquier ubicación.")),
            ];
            for (i, (icon, title, desc)) in cards.into_iter().enumerate() {
                if card(ui, size, icon, title, desc).clicked() {
                    clicked = Some(i);
                }
            }
        });

        match clicked {
            Some(0) => {
                self.notice = None;
                self.create = Some(CreateForm {
                    title: String::new(),
                    subtitle: String::new(),
                    error: None,
                    first_frame: true,
                });
            }
            Some(1) => {
                self.notice = None;
                self.load = Some(story::scan(root));
            }
            Some(2) => {
                self.notice = None;
                let mut dialog = rfd::FileDialog::new().set_title("Importar carpeta de historia");
                if root.is_dir() {
                    dialog = dialog.set_directory(root);
                }
                if let Some(dir) = dialog.pick_folder() {
                    match Story::open(&dir) {
                        Ok(s) => opened = Some(s),
                        Err(e) => self.notice = Some(e),
                    }
                }
            }
            _ => {}
        }

        if let Some(msg) = &self.notice {
            ui.add_space(12.0);
            ui.label(
                RichText::new(msg)
                    .size(13.0)
                    .color(Color32::from_rgb(0xB0, 0x4A, 0x3A)),
            );
        }

        if let Some(s) = self.show_create(&ctx, root) {
            opened = Some(s);
        }
        if let Some(s) = self.show_load(&ctx, root) {
            opened = Some(s);
        }
        opened
    }

    fn show_create(&mut self, ctx: &Context, root: &Path) -> Option<Story> {
        let form = self.create.as_mut()?;
        let mut submit = false;
        let mut cancel = false;

        let modal = Modal::new(Id::unique("create_story_modal")).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(
                RichText::new(tr("Crear historia"))
                    .family(theme::serif())
                    .size(26.0),
            );
            ui.add_space(12.0);
            let weak = ui.visuals().weak_text_color();

            ui.label(
                RichText::new(tr("Título de la historia"))
                    .size(12.5)
                    .color(weak),
            );
            let title = ui.add(
                TextEdit::singleline(&mut form.title)
                    .desired_width(f32::INFINITY)
                    .margin(Vec2::new(8.0, 6.0)),
            );
            if form.first_frame {
                title.request_focus();
                form.first_frame = false;
            }
            ui.add_space(8.0);

            ui.label(
                RichText::new(tr("Subtítulo / Género (opcional)"))
                    .size(12.5)
                    .color(weak),
            );
            let sub = ui.add(
                TextEdit::singleline(&mut form.subtitle)
                    .desired_width(f32::INFINITY)
                    .margin(Vec2::new(8.0, 6.0)),
            );
            let enter = ctx.input(|i| i.key_pressed(Key::Enter));
            if enter && (title.lost_focus() || sub.lost_focus()) {
                submit = true;
            }

            ui.add_space(8.0);
            ui.label(
                RichText::new(trf("Se creará en: {}", &[&root.display()]))
                    .size(12.0)
                    .color(weak),
            );

            if let Some(e) = &form.error {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(e)
                        .size(12.5)
                        .color(Color32::from_rgb(0xB0, 0x4A, 0x3A)),
                );
            }

            ui.add_space(16.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let accent = ui.visuals().selection.bg_fill;
                let on_accent = ui.visuals().selection.stroke.color;
                let can = !form.title.trim().is_empty();
                let create = Button::new(
                    RichText::new(tr("Crear historia"))
                        .color(on_accent)
                        .size(14.0),
                )
                .fill(accent)
                .stroke(Stroke::new(1.0, accent))
                .min_size(Vec2::new(130.0, 32.0));
                if ui.add_enabled(can, create).clicked() {
                    submit = true;
                }
                if ui
                    .add(Button::new(tr("Cancelar")).min_size(Vec2::new(90.0, 32.0)))
                    .clicked()
                {
                    cancel = true;
                }
            });
        });
        if modal.should_close() {
            cancel = true;
        }

        if submit && !form.title.trim().is_empty() {
            match Story::create(root, &form.title, &form.subtitle, &self.author) {
                Ok(s) => {
                    self.create = None;
                    return Some(s);
                }
                Err(e) => form.error = Some(e),
            }
        }
        if cancel {
            self.create = None;
        }
        None
    }

    fn show_load(&mut self, ctx: &Context, root: &Path) -> Option<Story> {
        let items = self.load.as_ref()?;
        let mut chosen: Option<PathBuf> = None;
        let mut cancel = false;

        let modal = Modal::new(Id::unique("load_story_modal")).show(ctx, |ui| {
            ui.set_width(500.0);
            ui.label(
                RichText::new(tr("Cargar historia"))
                    .family(theme::serif())
                    .size(26.0),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new(root.display().to_string())
                    .size(12.0)
                    .color(ui.visuals().weak_text_color()),
            );
            ui.add_space(12.0);

            if items.is_empty() {
                ui.add_space(12.0);
                ui.label(
                    RichText::new(tr("No se encontraron historias en esta carpeta."))
                        .color(ui.visuals().weak_text_color()),
                );
                ui.add_space(12.0);
            } else {
                ScrollArea::vertical()
                    .max_height(340.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for item in items {
                            if story_row(ui, item).clicked() {
                                chosen = Some(item.path.clone());
                            }
                            ui.add_space(6.0);
                        }
                    });
            }

            ui.add_space(10.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(Button::new(tr("Cerrar")).min_size(Vec2::new(90.0, 32.0)))
                    .clicked()
                {
                    cancel = true;
                }
            });
        });
        if modal.should_close() {
            cancel = true;
        }

        if let Some(path) = chosen {
            match Story::open(&path) {
                Ok(s) => {
                    self.load = None;
                    return Some(s);
                }
                Err(e) => {
                    self.notice = Some(e);
                    self.load = None;
                }
            }
        } else if cancel {
            self.load = None;
        }
        None
    }
}

fn story_row(ui: &mut Ui, item: &StorySummary) -> Response {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 58.0), Sense::click());
    let v = ui.visuals();
    let fill = if resp.hovered() {
        v.widgets.hovered.weak_bg_fill
    } else {
        v.extreme_bg_color
    };
    let stroke = if resp.hovered() {
        v.selection.bg_fill
    } else {
        v.window_stroke.color
    };
    let text = v.widgets.noninteractive.fg_stroke.color;
    let weak = v.weak_text_color();

    let p = ui.painter();
    let r = CornerRadius::same(6);
    p.rect_filled(rect, r, fill);
    p.rect_stroke(rect, r, Stroke::new(1.0, stroke), StrokeKind::Inside);

    let clip = p.with_clip_rect(rect.shrink2(Vec2::new(12.0, 0.0)));
    clip.text(
        rect.left_top() + Vec2::new(14.0, 11.0),
        Align2::LEFT_TOP,
        &item.title,
        FontId::new(17.0, theme::serif()),
        text,
    );
    let sub = if item.subtitle.is_empty() {
        item.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        item.subtitle.clone()
    };
    clip.text(
        rect.left_top() + Vec2::new(14.0, 34.0),
        Align2::LEFT_TOP,
        sub,
        FontId::proportional(12.5),
        weak,
    );
    if let Some(t) = item.modified {
        p.text(
            rect.right_center() - Vec2::new(14.0, 0.0),
            Align2::RIGHT_CENTER,
            story::format_date(t),
            FontId::proportional(12.5),
            weak,
        );
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Tarjeta editorial: zona de icono, título centrado y descripción.
fn card(ui: &mut Ui, size: Vec2, icon: Icon, title: &str, desc: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let hovered = resp.hovered();
    let v = ui.visuals();
    let fill = if hovered {
        v.widgets.hovered.weak_bg_fill
    } else {
        v.extreme_bg_color
    };
    let accent = v.selection.bg_fill;
    let border = if hovered {
        accent
    } else {
        v.window_stroke.color
    };
    let text = v.widgets.noninteractive.fg_stroke.color;
    let weak = v.weak_text_color();
    let well = v.faint_bg_color;

    let p = ui.painter();
    let r = CornerRadius::same(8);
    p.rect_filled(rect, r, fill);
    p.rect_stroke(rect, r, Stroke::new(1.0, border), StrokeKind::Inside);

    let well_rect = egui::Rect::from_min_size(
        rect.min + Vec2::splat(16.0),
        Vec2::new(rect.width() - 32.0, 130.0),
    );
    p.rect_filled(well_rect, CornerRadius::same(6), well);
    p.rect_stroke(
        well_rect,
        CornerRadius::same(6),
        Stroke::new(1.0, v.window_stroke.color),
        StrokeKind::Inside,
    );
    let icon_rect = egui::Rect::from_center_size(well_rect.center(), Vec2::splat(44.0));
    icons::paint(ui, icon, icon_rect, if hovered { accent } else { weak });

    let p = ui.painter();
    p.text(
        egui::pos2(rect.center().x, well_rect.bottom() + 28.0),
        Align2::CENTER_CENTER,
        title,
        FontId::new(21.0, theme::serif()),
        text,
    );
    let galley = p.layout(
        desc.to_owned(),
        FontId::proportional(13.0),
        weak,
        rect.width() - 40.0,
    );
    p.galley(
        egui::pos2(rect.left() + 20.0, well_rect.bottom() + 54.0),
        galley,
        weak,
    );

    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}
