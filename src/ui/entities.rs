// src/ui/entities.rs
//! Módulo Entidades: fichas de Personajes, Objetos, Varios y categorías propias, con nombre,
//! propiedades y rasgos editables.

use std::path::{Path, PathBuf};

use egui::{
    Align, Align2, Button, Color32, CornerRadius, FontId, Frame, Id, Key, Layout, Margin, Modal,
    Panel, Rect, Response, RichText, ScrollArea, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2,
};

use crate::i18n::{tr, tr_dyn, trf};
use crate::project::{self, Category, Entity, EntitySummary, Prop, Trait};
use crate::story::Story;
use crate::textures::TextureCache;
use crate::theme;
use crate::ui::doc::Autosave;

const PANEL_W: f32 = 280.0;
const SHEET_W: f32 = 860.0;
const PHOTO: f32 = 110.0;
const ERROR_COLOR: Color32 = Color32::from_rgb(0xB0, 0x4A, 0x3A);

enum DialogKind {
    New,
    Delete,
    NewCategory,
    DeleteCategory,
}

struct Dialog {
    kind: DialogKind,
    name: String,
    error: Option<String>,
    first: bool,
}

pub struct EntitiesView {
    root: Option<PathBuf>,
    cats: Vec<Category>,
    tab: Category,
    list: Vec<EntitySummary>,
    selected: Option<String>,
    ent: Option<Entity>,
    auto: Autosave,
    dialog: Option<Dialog>,
    error: Option<String>,
}

impl Default for EntitiesView {
    fn default() -> Self {
        Self {
            root: None,
            cats: Category::builtins(),
            tab: Category::builtins().remove(0),
            list: Vec::new(),
            selected: None,
            ent: None,
            auto: Autosave::default(),
            dialog: None,
            error: None,
        }
    }
}

impl EntitiesView {
    // ------------------------------------------------------------------ estado

    fn refresh(&mut self) {
        if let Some(root) = &self.root {
            self.list = project::list_entities(root, &self.tab.key);
        }
    }

    fn reset(&mut self, story: &Story) {
        self.flush();
        self.root = Some(story.path.clone());
        self.cats = project::load_categories(&story.path);
        self.tab = self.cats[0].clone();
        self.clear_selection();
        self.refresh();
        self.select_first();
    }

    fn clear_selection(&mut self) {
        self.selected = None;
        self.ent = None;
        self.auto = Autosave::default();
    }

    fn select_first(&mut self) {
        if let Some(id) = self.list.first().map(|s| s.id.clone()) {
            self.select(&id);
        }
    }

    fn set_tab(&mut self, cat: Category) {
        if cat == self.tab {
            return;
        }
        self.flush();
        self.tab = cat;
        self.clear_selection();
        self.refresh();
        self.select_first();
    }

    pub fn flush(&mut self) {
        if !self.auto.dirty {
            return;
        }
        if let (Some(root), Some(id), Some(ent)) = (&self.root, &self.selected, &self.ent) {
            match project::save_entity(root, &self.tab.key, id, ent) {
                Ok(()) => {
                    self.auto.dirty = false;
                    self.auto.error = None;
                }
                Err(e) => self.auto.error = Some(e),
            }
        }
    }

    fn select(&mut self, id: &str) {
        self.flush();
        let Some(root) = &self.root else { return };
        match project::load_entity(root, &self.tab.key, id) {
            Ok(e) => {
                self.ent = Some(e);
                self.selected = Some(id.to_owned());
                self.auto = Autosave::default();
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    // ------------------------------------------------------------------ vista

    pub fn show(&mut self, ui: &mut Ui, story: &Story, tex: &mut TextureCache) {
        let ctx = ui.ctx().clone();
        if self.root.as_deref() != Some(story.path.as_path()) {
            self.reset(story);
        }
        if self.auto.due(&ctx) {
            if let (Some(root), Some(id), Some(ent)) = (&self.root, &self.selected, &self.ent) {
                let r = project::save_entity(root, &self.tab.key, id, ent);
                self.auto.finish(&ctx, r);
            }
        }

        let (side, border, canvas) = {
            let v = ui.visuals();
            (v.faint_bg_color, v.window_stroke.color, v.panel_fill)
        };

        let mut clicked: Option<String> = None;
        Panel::right("entities_panel")
            .exact_size(PANEL_W)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                Frame::new()
                    .fill(side)
                    .stroke(Stroke::new(1.0, border))
                    .inner_margin(Margin::same(12)),
            )
            .show(ui, |ui| {
                clicked = self.side_panel(ui, &story.path, tex);
            });
        if let Some(id) = clicked {
            self.select(&id);
        }

        egui::CentralPanel::default()
            .frame(Frame::new().fill(canvas))
            .show(ui, |ui| {
                self.center(ui, &ctx, &story.path, tex);
            });

        self.show_dialog(&ctx, &story.path);
    }

    // ------------------------------------------------------------------ panel derecho

    fn side_panel(&mut self, ui: &mut Ui, root: &Path, tex: &mut TextureCache) -> Option<String> {
        let mut clicked = None;
        let mut new_tab = None;

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(5.0, 5.0);
            for c in &self.cats {
                if ui
                    .add(Button::new(tr_dyn(&c.label)).selected(self.tab == *c))
                    .clicked()
                {
                    new_tab = Some(c.clone());
                }
            }
            if ui
                .add(Button::new(tr("+ Nueva categoría")))
                .on_hover_text(tr("Crea una categoría propia para esta historia"))
                .clicked()
            {
                self.dialog = Some(Dialog {
                    kind: DialogKind::NewCategory,
                    name: String::new(),
                    error: None,
                    first: true,
                });
            }
        });
        // Categoría propia seleccionada: eliminarla desde la cabecera, siempre visible.
        if !self.tab.builtin {
            ui.add_space(6.0);
            let del = Button::new(
                RichText::new(trf("Eliminar categoría «{}»", &[&self.tab.label]))
                    .color(Color32::from_rgb(0xB0, 0x4A, 0x3E)),
            );
            if ui
                .add_sized([ui.available_width(), 28.0], del.truncate())
                .on_hover_text(tr("Quita esta categoría (debe estar vacía)"))
                .clicked()
            {
                self.dialog = Some(Dialog {
                    kind: DialogKind::DeleteCategory,
                    name: self.tab.label.clone(),
                    error: None,
                    first: false,
                });
            }
        }
        ui.add_space(8.0);
        if ui
            .add_sized(
                [ui.available_width(), 30.0],
                Button::new(tr("+ Nueva ficha")),
            )
            .clicked()
        {
            self.dialog = Some(Dialog {
                kind: DialogKind::New,
                name: String::new(),
                error: None,
                first: true,
            });
        }
        ui.add_space(10.0);

        if self.list.is_empty() {
            ui.label(
                RichText::new(tr("No hay fichas en esta pestaña."))
                    .size(13.0)
                    .color(ui.visuals().weak_text_color()),
            );
        }
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for item in &self.list {
                    let selected = self.selected.as_deref() == Some(item.id.as_str());
                    let path = item
                        .image
                        .as_ref()
                        .map(|f| project::media_file(root, project::MEDIA_CHARACTERS, f));
                    if entity_card(ui, tex, path.as_deref(), &item.name, &item.role, selected)
                        .clicked()
                    {
                        clicked = Some(item.id.clone());
                    }
                    ui.add_space(6.0);
                }
            });

        if let Some(k) = new_tab {
            self.set_tab(k);
        }
        clicked
    }

    // ------------------------------------------------------------------ ficha

    fn center(&mut self, ui: &mut Ui, ctx: &egui::Context, root: &Path, tex: &mut TextureCache) {
        let weak = ui.visuals().weak_text_color();
        let cat = self.tab.clone();

        if self.ent.is_none() {
            ui.add_space(80.0);
            ui.vertical_centered(|ui| {
                let msg = if self.list.is_empty() {
                    tr("Crea la primera ficha")
                } else {
                    tr("Selecciona una ficha")
                };
                ui.label(
                    RichText::new(msg)
                        .family(theme::serif())
                        .size(22.0)
                        .color(weak),
                );
                if let Some(e) = &self.error {
                    ui.add_space(8.0);
                    ui.label(RichText::new(e).size(13.0).color(ERROR_COLOR));
                }
            });
            return;
        }

        // Barra superior: estado y eliminar.
        Frame::new()
            .inner_margin(Margin::symmetric(24, 10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&cat.label).size(13.5).color(weak));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button(tr("Eliminar ficha")).clicked() {
                            let name = self
                                .ent
                                .as_ref()
                                .map(|e| e.name.clone())
                                .unwrap_or_default();
                            self.dialog = Some(Dialog {
                                kind: DialogKind::Delete,
                                name,
                                error: None,
                                first: false,
                            });
                        }
                        ui.add_space(8.0);
                        let (txt, is_err) = self.auto.status();
                        ui.label(RichText::new(txt).size(12.5).color(if is_err {
                            ERROR_COLOR
                        } else {
                            weak
                        }));
                    });
                });
            });

        let (card, border) = (
            ui.visuals().extreme_bg_color,
            ui.visuals().window_stroke.color,
        );
        let mut changed = false;
        let mut img_error: Option<String> = None;
        let id = self.selected.clone().unwrap_or_default();

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let w = SHEET_W.min(ui.available_width() - 48.0).max(360.0);
                let Some(ent) = self.ent.as_mut() else { return };
                ui.vertical_centered(|ui| {
                    ui.add_space(8.0);
                    Frame::new()
                        .fill(card)
                        .stroke(Stroke::new(1.0, border))
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(Margin::symmetric(40, 36))
                        .show(ui, |ui| {
                            ui.set_width(w - 80.0);
                            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                                changed |= ficha(ui, &cat, ent, root, tex, &mut img_error);
                            });
                        });
                    ui.add_space(40.0);
                });
            });

        if let Some(e) = img_error {
            self.error = Some(e);
        }
        if changed {
            self.auto.mark(ctx);
            // Mantener la tarjeta del panel sincronizada con la ficha.
            if let Some(ent) = &self.ent {
                if let Some(item) = self.list.iter_mut().find(|s| s.id == id) {
                    *item = EntitySummary::from_entity(&id, ent);
                }
            }
        }
        if ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                Key::S,
            ))
        }) {
            self.flush();
        }
    }

    // ------------------------------------------------------------------ diálogos

    fn show_dialog(&mut self, ctx: &egui::Context, root: &Path) {
        let Some(mut d) = self.dialog.take() else {
            return;
        };
        let mut submit = false;
        let mut cancel = false;
        let is_delete = matches!(d.kind, DialogKind::Delete | DialogKind::DeleteCategory);
        let is_cat = matches!(d.kind, DialogKind::NewCategory | DialogKind::DeleteCategory);
        let title = match d.kind {
            DialogKind::Delete => tr("Eliminar ficha"),
            DialogKind::DeleteCategory => tr("Quitar categoría"),
            DialogKind::NewCategory => tr("Nueva categoría"),
            DialogKind::New => self.tab.new_title(),
        };
        let confirm = match d.kind {
            DialogKind::Delete => tr("Eliminar"),
            DialogKind::DeleteCategory => tr("Quitar"),
            _ => tr("Crear"),
        };

        let modal = Modal::new(Id::unique("entities_dialog")).show(ctx, |ui| {
            ui.set_width(400.0);
            ui.label(RichText::new(title).family(theme::serif()).size(24.0));
            ui.add_space(10.0);
            if is_delete {
                let msg = if is_cat {
                    trf(
                        "Se quitará la categoría «{}» (debe estar vacía).",
                        &[&d.name],
                    )
                } else {
                    trf(
                        "Se eliminará la ficha «{}» y su imagen. Esta acción no se puede deshacer.",
                        &[&d.name],
                    )
                };
                ui.label(RichText::new(msg).size(14.0));
            } else {
                ui.label(
                    RichText::new(tr("Nombre"))
                        .size(12.5)
                        .color(ui.visuals().weak_text_color()),
                );
                let r = ui.add(
                    TextEdit::singleline(&mut d.name)
                        .desired_width(f32::INFINITY)
                        .margin(Vec2::new(8.0, 6.0)),
                );
                if d.first {
                    r.request_focus();
                    d.first = false;
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    submit = true;
                }
            }
            if let Some(e) = &d.error {
                ui.add_space(6.0);
                ui.label(RichText::new(e).size(12.5).color(ERROR_COLOR));
            }
            ui.add_space(16.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let accent = ui.visuals().selection.bg_fill;
                let on_accent = ui.visuals().selection.stroke.color;
                let fill = if is_delete { ERROR_COLOR } else { accent };
                let fg = if is_delete { Color32::WHITE } else { on_accent };
                let b = Button::new(RichText::new(confirm).color(fg).size(14.0))
                    .fill(fill)
                    .stroke(Stroke::new(1.0, fill))
                    .min_size(Vec2::new(110.0, 32.0));
                if ui.add(b).clicked() {
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

        if submit {
            match self.apply_dialog(&d, root) {
                Ok(()) => return,
                Err(e) => d.error = Some(e),
            }
        }
        if !cancel {
            self.dialog = Some(d);
        }
    }

    fn apply_dialog(&mut self, d: &Dialog, root: &Path) -> Result<(), String> {
        match d.kind {
            DialogKind::New => {
                self.flush();
                let id = project::create_entity(root, &self.tab.key, &d.name)?;
                self.refresh();
                self.select(&id);
            }
            DialogKind::NewCategory => {
                self.flush();
                let cat = project::add_category(root, &d.name)?;
                self.cats = project::load_categories(root);
                self.set_tab(cat);
            }
            DialogKind::DeleteCategory => {
                self.flush();
                project::remove_category(root, &self.tab.key)?;
                self.cats = project::load_categories(root);
                let first = self.cats[0].clone();
                self.set_tab(first);
            }
            DialogKind::Delete => {
                if let Some(id) = self.selected.clone() {
                    self.auto = Autosave::default();
                    project::delete_entity(root, &self.tab.key, &id)?;
                    self.clear_selection();
                    self.refresh();
                    self.select_first();
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------- ficha

/// Dibuja la ficha. Todas las categorías comparten el mismo sistema: nombre editable,
/// propiedades clave-valor, rasgos (bloques) y texto largo. Devuelve `true` si algo cambió.
fn ficha(
    ui: &mut Ui,
    cat: &Category,
    ent: &mut Entity,
    root: &Path,
    tex: &mut TextureCache,
    img_error: &mut Option<String>,
) -> bool {
    let mut changed = false;

    ui.horizontal_top(|ui| {
        changed |= photo(ui, ent, root, tex, img_error);
        ui.add_space(20.0);
        ui.vertical(|ui| {
            changed |= ui
                .add(
                    TextEdit::singleline(&mut ent.name)
                        .font(FontId::new(34.0, theme::serif()))
                        .frame(Frame::NONE)
                        .hint_text(tr("Nombre"))
                        .desired_width(f32::INFINITY),
                )
                .changed();
            ui.add_space(8.0);
            // La edad es el campo base fijo de los personajes.
            if cat.is_character() {
                changed |= labeled_edit(ui, tr("Edad"), &mut ent.age);
                ui.add_space(6.0);
            }
            changed |= props_editor(ui, &mut ent.props);
        });
    });

    ui.add_space(22.0);
    section_title(
        ui,
        if cat.is_character() {
            tr("Rasgos")
        } else {
            tr("Detalles")
        },
    );
    changed |= traits_editor(ui, &mut ent.traits);
    ui.add_space(20.0);
    section_title(ui, cat.body_title());
    changed |= field_box(
        ui,
        "",
        &mut ent.bio,
        if cat.is_character() { 14 } else { 12 },
    );
    changed
}

/// Lista de propiedades `Propiedad: Valor`: la clave se renombra, cada fila se quita y se
/// pueden agregar más.
fn props_editor(ui: &mut Ui, props: &mut Vec<Prop>) -> bool {
    let weak = ui.visuals().weak_text_color();
    let mut changed = false;
    let mut remove: Option<usize> = None;
    for (i, p) in props.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let w = ui.available_width();
            changed |= ui
                .add(
                    TextEdit::singleline(&mut p.key)
                        .hint_text(tr("Propiedad"))
                        .text_color(weak)
                        .desired_width((w * 0.32).clamp(90.0, 200.0))
                        .margin(Vec2::new(8.0, 5.0)),
                )
                .changed();
            changed |= ui
                .add(
                    TextEdit::singleline(&mut p.value)
                        .hint_text(tr("Valor"))
                        .desired_width((ui.available_width() - 34.0).max(60.0))
                        .margin(Vec2::new(8.0, 5.0)),
                )
                .changed();
            if ui
                .add(Button::new("✕").frame(false))
                .on_hover_text(tr("Quitar propiedad"))
                .clicked()
            {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        props.remove(i);
        changed = true;
    }
    if ui.button(tr("+ Propiedad")).clicked() {
        props.push(Prop::default());
        changed = true;
    }
    changed
}

/// Bloques de rasgos: título editable, texto libre y botón para quitarlos; se agregan sin límite.
fn traits_editor(ui: &mut Ui, traits: &mut Vec<Trait>) -> bool {
    let (fill, border) = {
        let v = ui.visuals();
        (v.panel_fill, v.window_stroke.color)
    };
    let mut changed = false;
    let mut remove: Option<usize> = None;
    for (i, t) in traits.iter_mut().enumerate() {
        Frame::new()
            .fill(fill)
            .stroke(Stroke::new(1.0, border))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    changed |= ui
                        .add(
                            TextEdit::singleline(&mut t.title)
                                .font(FontId::new(15.5, theme::serif()))
                                .frame(Frame::NONE)
                                .hint_text(tr("Título del rasgo"))
                                .desired_width((ui.available_width() - 30.0).max(60.0)),
                        )
                        .changed();
                    if ui
                        .add(Button::new("✕").frame(false))
                        .on_hover_text(tr("Quitar este rasgo"))
                        .clicked()
                    {
                        remove = Some(i);
                    }
                });
                ui.add_space(4.0);
                changed |= ui
                    .add(
                        TextEdit::multiline(&mut t.text)
                            .font(FontId::new(15.5, theme::serif()))
                            .frame(Frame::NONE)
                            .desired_width(f32::INFINITY)
                            .desired_rows(4),
                    )
                    .changed();
            });
        ui.add_space(8.0);
    }
    if let Some(i) = remove {
        traits.remove(i);
        changed = true;
    }
    if ui.button(tr("+ Rasgo")).clicked() {
        traits.push(Trait::default());
        changed = true;
    }
    changed
}

fn section_title(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).family(theme::serif()).size(21.0));
    ui.add_space(6.0);
}

fn labeled_edit(ui: &mut Ui, label: &str, text: &mut String) -> bool {
    let weak = ui.visuals().weak_text_color();
    let mut changed = false;
    ui.vertical(|ui| {
        ui.label(RichText::new(label).size(12.0).color(weak));
        changed = ui
            .add(
                TextEdit::singleline(text)
                    .desired_width(f32::INFINITY)
                    .margin(Vec2::new(8.0, 5.0)),
            )
            .changed();
    });
    changed
}

/// Bloque con borde y texto multilínea (cuadrícula de rasgos y campos largos).
fn field_box(ui: &mut Ui, title: &str, text: &mut String, rows: usize) -> bool {
    let (fill, border) = {
        let v = ui.visuals();
        (v.panel_fill, v.window_stroke.color)
    };
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, border))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !title.is_empty() {
                ui.label(RichText::new(title).family(theme::serif()).size(15.5));
                ui.add_space(4.0);
            }
            ui.add(
                TextEdit::multiline(text)
                    .font(FontId::new(15.5, theme::serif()))
                    .frame(Frame::NONE)
                    .desired_width(f32::INFINITY)
                    .desired_rows(rows),
            )
            .changed()
        })
        .inner
}

/// Foto tipo carnet (110×110) clicable para subir o cambiar la imagen.
fn photo(
    ui: &mut Ui,
    ent: &mut Entity,
    root: &Path,
    tex: &mut TextureCache,
    img_error: &mut Option<String>,
) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(PHOTO), Sense::click());
    let (side, border, accent, weak) = {
        let v = ui.visuals();
        (
            v.faint_bg_color,
            v.window_stroke.color,
            v.selection.bg_fill,
            v.weak_text_color(),
        )
    };
    let r = CornerRadius::same(8);
    ui.painter().rect_filled(rect, r, side);
    match &ent.image {
        Some(file) => {
            tex.paint(
                ui,
                &project::media_file(root, project::MEDIA_CHARACTERS, file),
                rect,
                r,
            );
        }
        None => {
            let ch = ent
                .name
                .chars()
                .find(|c| !c.is_whitespace())
                .map(|c| c.to_uppercase().to_string());
            ui.painter().text(
                rect.center() - Vec2::new(0.0, 8.0),
                Align2::CENTER_CENTER,
                ch.unwrap_or_else(|| "?".into()),
                FontId::new(34.0, theme::serif()),
                weak,
            );
            ui.painter().text(
                rect.center() + Vec2::new(0.0, 26.0),
                Align2::CENTER_CENTER,
                tr("Añadir foto"),
                FontId::proportional(11.5),
                weak,
            );
        }
    }
    ui.painter().rect_stroke(
        rect,
        r,
        Stroke::new(1.0, if resp.hovered() { accent } else { border }),
        StrokeKind::Inside,
    );
    if resp.hovered() && ent.image.is_some() {
        ui.painter().rect_filled(
            Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 24.0), rect.max),
            CornerRadius {
                nw: 0,
                ne: 0,
                sw: 8,
                se: 8,
            },
            Color32::from_black_alpha(150),
        );
        ui.painter().text(
            egui::pos2(rect.center().x, rect.bottom() - 12.0),
            Align2::CENTER_CENTER,
            tr("Cambiar foto"),
            FontId::proportional(11.5),
            Color32::WHITE,
        );
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    if resp.clicked() {
        if let Some(src) = rfd::FileDialog::new()
            .set_title(tr("Elegir imagen de la ficha"))
            .add_filter("Imágenes", &["png", "jpg", "jpeg", "webp"])
            .pick_file()
        {
            match project::import_image(root, project::MEDIA_CHARACTERS, &src, true) {
                Ok(name) => {
                    if let Some(old) = ent.image.replace(name) {
                        tex.invalidate(&project::media_file(root, project::MEDIA_CHARACTERS, &old));
                        project::remove_media(root, project::MEDIA_CHARACTERS, &old);
                    }
                    return true;
                }
                Err(e) => *img_error = Some(e),
            }
        }
    }
    false
}

// ---------------------------------------------------------------------- tarjeta

fn entity_card(
    ui: &mut Ui,
    tex: &mut TextureCache,
    image_path: Option<&Path>,
    name: &str,
    role: &str,
    selected: bool,
) -> Response {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 64.0), Sense::click());
    let v = ui.visuals();
    let (fill, stroke) = if selected {
        (
            v.selection.bg_fill.gamma_multiply(0.18),
            v.selection.bg_fill,
        )
    } else if resp.hovered() {
        (v.widgets.hovered.weak_bg_fill, v.selection.bg_fill)
    } else {
        (v.extreme_bg_color, v.window_stroke.color)
    };
    let (text, weak, side) = (
        v.widgets.noninteractive.fg_stroke.color,
        v.weak_text_color(),
        v.faint_bg_color,
    );

    let r = CornerRadius::same(8);
    ui.painter().rect_filled(rect, r, fill);
    ui.painter()
        .rect_stroke(rect, r, Stroke::new(1.0, stroke), StrokeKind::Inside);

    let thumb = Rect::from_min_size(rect.min + Vec2::new(10.0, 10.0), Vec2::splat(44.0));
    let rad = CornerRadius::same(6);
    ui.painter().rect_filled(thumb, rad, side);
    match image_path {
        Some(path) => {
            tex.paint(ui, path, thumb, rad);
        }
        None => {
            let ch = name
                .chars()
                .find(|c| !c.is_whitespace())
                .map(|c| c.to_uppercase().to_string());
            ui.painter().text(
                thumb.center(),
                Align2::CENTER_CENTER,
                ch.unwrap_or_else(|| "?".into()),
                FontId::new(20.0, theme::serif()),
                weak,
            );
        }
    }

    let clip = ui.painter().with_clip_rect(Rect::from_min_max(
        egui::pos2(thumb.right() + 8.0, rect.top()),
        egui::pos2(rect.right() - 8.0, rect.bottom()),
    ));
    let name_pos = egui::pos2(
        thumb.right() + 12.0,
        rect.center().y - if role.is_empty() { 0.0 } else { 9.0 },
    );
    let shown = if name.trim().is_empty() {
        tr("Sin nombre")
    } else {
        name
    };
    clip.text(
        name_pos,
        Align2::LEFT_CENTER,
        shown,
        FontId::new(15.5, theme::serif()),
        text,
    );
    if !role.is_empty() {
        clip.text(
            egui::pos2(thumb.right() + 12.0, rect.center().y + 11.0),
            Align2::LEFT_CENTER,
            role,
            FontId::proportional(12.0),
            weak,
        );
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}
