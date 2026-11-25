// src/ui/lore.rs
//! Módulo Lore y Cronología (Etapa 2.3): lector/editor de eventos + línea de tiempo vertical.

use std::path::{Path, PathBuf};

use egui::{
    Align, Align2, Button, Color32, CornerRadius, FontId, Frame, Id, Key, Layout, Margin, Modal,
    Panel, Rect, Response, RichText, ScrollArea, Sense, Shape, Stroke, StrokeKind, TextEdit, Ui,
    Vec2,
};

use crate::i18n::{tr, trf};
use crate::icons::{self, Icon};
use crate::project::{self, LoreEvent};
use crate::story::Story;
use crate::textures::TextureCache;
use crate::theme;
use crate::ui::doc::Autosave;
use crate::ui::markdown::{self, FontChoice};

const TIMELINE_W: f32 = 280.0;
const SHEET_W: f32 = 760.0;
const ROW_H: f32 = 46.0;
const BODY_SIZE: f32 = 17.0;
const ERROR_COLOR: Color32 = Color32::from_rgb(0xB0, 0x4A, 0x3A);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Edit,
    Preview,
}

enum DialogKind {
    New,
    Edit(String),
    Delete(String),
}

struct Dialog {
    kind: DialogKind,
    title: String,
    year: String,
    tag: String,
    current_image: Option<String>,
    new_image: Option<PathBuf>,
    remove_image: bool,
    error: Option<String>,
    first: bool,
}

pub struct LoreView {
    root: Option<PathBuf>,
    events: Vec<LoreEvent>,
    selected: Option<String>,
    body: String,
    words: usize,
    auto: Autosave,
    mode: Mode,
    /// `true`: lo más reciente arriba (flecha hacia arriba).
    newest_top: bool,
    dialog: Option<Dialog>,
    error: Option<String>,
}

impl Default for LoreView {
    fn default() -> Self {
        Self {
            root: None,
            events: Vec::new(),
            selected: None,
            body: String::new(),
            words: 0,
            auto: Autosave::default(),
            mode: Mode::Edit,
            newest_top: true,
            dialog: None,
            error: None,
        }
    }
}

impl LoreView {
    // ------------------------------------------------------------------ estado

    fn refresh(&mut self) {
        if let Some(root) = &self.root {
            self.events = project::load_timeline(root);
        }
        if let Some(sel) = &self.selected {
            if !self.events.iter().any(|e| &e.file == sel) {
                self.selected = None;
                self.body.clear();
                self.auto = Autosave::default();
            }
        }
    }

    fn reset(&mut self, story: &Story) {
        self.flush();
        self.root = Some(story.path.clone());
        self.selected = None;
        self.body.clear();
        self.auto = Autosave::default();
        self.refresh();
        if let Some(i) = self.order().first() {
            let file = self.events[*i].file.clone();
            self.select(&file);
        }
    }

    /// Índices de eventos en el orden en que se dibujan en la línea de tiempo.
    fn order(&self) -> Vec<usize> {
        let mut idx: Vec<usize> = (0..self.events.len()).collect();
        idx.sort_by(|&a, &b| {
            self.events[a]
                .year
                .cmp(&self.events[b].year)
                .then_with(|| project::natural_cmp(&self.events[a].title, &self.events[b].title))
        });
        if self.newest_top {
            idx.reverse();
        }
        idx
    }

    pub fn flush(&mut self) {
        if !self.auto.dirty {
            return;
        }
        if let (Some(root), Some(file)) = (&self.root, &self.selected) {
            let r = project::write_chapter(&project::lore_dir(root).join(file), &self.body);
            match r {
                Ok(()) => {
                    self.auto.dirty = false;
                    self.auto.error = None;
                }
                Err(e) => self.auto.error = Some(e),
            }
        }
    }

    fn select(&mut self, file: &str) {
        self.flush();
        let Some(root) = &self.root else { return };
        match project::read_chapter(&project::lore_dir(root).join(file)) {
            Ok(text) => {
                self.words = project::count_words(&text);
                self.body = text;
                self.selected = Some(file.to_owned());
                self.auto = Autosave::default();
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn current_event(&self) -> Option<&LoreEvent> {
        let sel = self.selected.as_ref()?;
        self.events.iter().find(|e| &e.file == sel)
    }

    // ------------------------------------------------------------------ vista

    pub fn show(&mut self, ui: &mut Ui, story: &Story, tex: &mut TextureCache) {
        let ctx = ui.ctx().clone();
        if self.root.as_deref() != Some(story.path.as_path()) {
            self.reset(story);
        }
        if self.auto.due(&ctx) {
            self.flush_with(&ctx);
        }

        let (side, border, canvas) = {
            let v = ui.visuals();
            (v.faint_bg_color, v.window_stroke.color, v.panel_fill)
        };

        let mut clicked: Option<String> = None;
        Panel::right("lore_timeline")
            .exact_size(TIMELINE_W)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                Frame::new()
                    .fill(side)
                    .stroke(Stroke::new(1.0, border))
                    .inner_margin(Margin::same(12)),
            )
            .show(ui, |ui| {
                clicked = self.timeline(ui, &story.path, tex);
            });
        if let Some(file) = clicked {
            self.select(&file);
        }

        egui::CentralPanel::default()
            .frame(Frame::new().fill(canvas))
            .show(ui, |ui| {
                self.center(ui, &ctx, &story.path, tex);
            });

        self.show_dialog(&ctx, &story.path);
    }

    fn flush_with(&mut self, ctx: &egui::Context) {
        if let (Some(root), Some(file)) = (&self.root, &self.selected) {
            let r = project::write_chapter(&project::lore_dir(root).join(file), &self.body);
            self.auto.finish(ctx, r);
        }
    }

    // ------------------------------------------------------------------ área central

    fn center(&mut self, ui: &mut Ui, ctx: &egui::Context, root: &Path, tex: &mut TextureCache) {
        let weak = ui.visuals().weak_text_color();

        Frame::new()
            .inner_margin(Margin::symmetric(24, 10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let accent = ui.visuals().selection.bg_fill;
                    let on_accent = ui.visuals().selection.stroke.color;
                    let new_btn = Button::new(
                        RichText::new(tr("+ Nuevo evento de Lore"))
                            .color(on_accent)
                            .size(13.5),
                    )
                    .fill(accent)
                    .stroke(Stroke::new(1.0, accent));
                    if ui.add(new_btn).clicked() {
                        self.dialog = Some(Dialog {
                            kind: DialogKind::New,
                            title: String::new(),
                            year: String::new(),
                            tag: String::new(),
                            current_image: None,
                            new_image: None,
                            remove_image: false,
                            error: None,
                            first: true,
                        });
                    }
                    if let Some(ev) = self.current_event().cloned() {
                        if ui.button(tr("Editar datos")).clicked() {
                            self.dialog = Some(Dialog {
                                kind: DialogKind::Edit(ev.file.clone()),
                                title: ev.title.clone(),
                                year: ev.year.to_string(),
                                tag: ev.tag.clone(),
                                current_image: ev.image.clone(),
                                new_image: None,
                                remove_image: false,
                                error: None,
                                first: true,
                            });
                        }
                        if ui.button(tr("Eliminar")).clicked() {
                            self.dialog = Some(Dialog {
                                kind: DialogKind::Delete(ev.file.clone()),
                                title: ev.title.clone(),
                                year: String::new(),
                                tag: String::new(),
                                current_image: None,
                                new_image: None,
                                remove_image: false,
                                error: None,
                                first: false,
                            });
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add(
                                    Button::new(tr("Vista previa"))
                                        .selected(self.mode == Mode::Preview),
                                )
                                .clicked()
                            {
                                self.mode = Mode::Preview;
                            }
                            if ui
                                .add(Button::new(tr("Editar")).selected(self.mode == Mode::Edit))
                                .clicked()
                            {
                                self.mode = Mode::Edit;
                            }
                            ui.add_space(10.0);
                            let words = if self.words == 1 {
                                tr("1 palabra").to_owned()
                            } else {
                                trf("{} palabras", &[&self.words])
                            };
                            ui.label(RichText::new(words).size(12.5).color(weak));
                            ui.add_space(6.0);
                            let (txt, is_err) = self.auto.status();
                            ui.label(RichText::new(txt).size(12.5).color(if is_err {
                                ERROR_COLOR
                            } else {
                                weak
                            }));
                        });
                    }
                });
            });

        let Some(ev) = self.current_event().cloned() else {
            ui.add_space(80.0);
            ui.vertical_centered(|ui| {
                let msg = if self.events.is_empty() {
                    tr("Crea el primer evento de lore")
                } else {
                    tr("Selecciona un evento en la línea de tiempo")
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
        };

        let (mode, card, border) = (
            self.mode,
            ui.visuals().extreme_bg_color,
            ui.visuals().window_stroke.color,
        );
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let w = SHEET_W.min(ui.available_width() - 48.0).max(320.0);
                let avail_h = ui.available_height();
                ui.vertical_centered(|ui| {
                    ui.add_space(8.0);
                    Frame::new()
                        .fill(card)
                        .stroke(Stroke::new(1.0, border))
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(Margin::symmetric(48, 40))
                        .show(ui, |ui| {
                            ui.set_width(w - 96.0);
                            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                                ui.label(
                                    RichText::new(&ev.title).family(theme::serif()).size(40.0),
                                );
                                ui.add_space(8.0);
                                ui.horizontal_wrapped(|ui| {
                                    pill(ui, &trf("Año {}", &[&ev.year]), true);
                                    if !ev.tag.is_empty() {
                                        pill(ui, &ev.tag, false);
                                    }
                                });
                                if let Some(img) = &ev.image {
                                    ui.add_space(14.0);
                                    tex.show(
                                        ui,
                                        &project::media_file(root, project::MEDIA_LORE, img),
                                        Vec2::new(w - 96.0, 280.0),
                                        CornerRadius::same(8),
                                    );
                                }
                                ui.add_space(18.0);
                                match mode {
                                    Mode::Edit => {
                                        let min_h = (avail_h - 260.0).max(240.0);
                                        let resp = ui.add(
                                            TextEdit::multiline(&mut self.body)
                                                .id(Id::unique("lore_editor"))
                                                .font(FontId::new(BODY_SIZE, theme::serif()))
                                                .frame(Frame::NONE)
                                                .hint_text(tr(
                                                    "Escribe aquí el lore de este evento…",
                                                ))
                                                .desired_width(f32::INFINITY)
                                                .min_size(Vec2::new(0.0, min_h))
                                                .lock_focus(true),
                                        );
                                        if resp.changed() {
                                            self.words = project::count_words(&self.body);
                                            self.auto.mark(ctx);
                                        }
                                    }
                                    Mode::Preview => {
                                        markdown::render_preview(
                                            ui,
                                            &self.body,
                                            FontChoice::Serif,
                                            BODY_SIZE,
                                        );
                                        ui.add_space(24.0);
                                    }
                                }
                            });
                        });
                    ui.add_space(40.0);
                });
            });

        if ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                Key::S,
            ))
        }) {
            self.flush();
        }
    }

    // ------------------------------------------------------------------ línea de tiempo

    fn timeline(&mut self, ui: &mut Ui, root: &Path, tex: &mut TextureCache) -> Option<String> {
        let mut clicked = None;
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(tr("Cronología"))
                    .family(theme::serif())
                    .size(19.0),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let tip = if self.newest_top {
                    tr("Invertir: más antiguo arriba")
                } else {
                    tr("Invertir: más reciente arriba")
                };
                if icon_button(ui, Icon::ArrowUpDown, tip).clicked() {
                    self.newest_top = !self.newest_top;
                }
            });
        });
        ui.add_space(8.0);

        if self.events.is_empty() {
            ui.label(
                RichText::new(tr(
                    "Aún no hay eventos. Crea uno con «+ Nuevo evento de Lore».",
                ))
                .size(13.0)
                .color(ui.visuals().weak_text_color()),
            );
            return None;
        }

        let order = self.order();
        let selected = self.selected.clone();
        let newest_top = self.newest_top;
        let (axis_col, text_col, weak, accent, on_accent, card, border, hover_bg) = {
            let v = ui.visuals();
            (
                v.widgets.noninteractive.fg_stroke.color,
                v.widgets.noninteractive.fg_stroke.color,
                v.weak_text_color(),
                v.selection.bg_fill,
                v.selection.stroke.color,
                v.extreme_bg_color,
                v.window_stroke.color,
                v.widgets.hovered.weak_bg_fill,
            )
        };

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let width = ui.available_width();
                let axis_x_off = width - 62.0;
                let stroke = Stroke::new(2.0, axis_col);

                let cap = |ui: &mut Ui, arrow: bool, up: bool| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 26.0), Sense::hover());
                    let x = rect.left() + axis_x_off;
                    let p = ui.painter();
                    if arrow {
                        // La flecha apunta hacia donde avanza el tiempo.
                        let (tip_y, base_y, far_y) = if up {
                            (rect.top() + 4.0, rect.top() + 12.0, rect.bottom())
                        } else {
                            (rect.bottom() - 4.0, rect.bottom() - 12.0, rect.top())
                        };
                        p.line_segment([egui::pos2(x, far_y), egui::pos2(x, tip_y)], stroke);
                        p.add(Shape::convex_polygon(
                            vec![
                                egui::pos2(x, tip_y),
                                egui::pos2(x - 5.0, base_y),
                                egui::pos2(x + 5.0, base_y),
                            ],
                            axis_col,
                            Stroke::NONE,
                        ));
                    } else {
                        let (near_y, end_y) = if up {
                            (rect.top(), rect.bottom() - 6.0)
                        } else {
                            (rect.bottom(), rect.top() + 6.0)
                        };
                        p.line_segment([egui::pos2(x, near_y), egui::pos2(x, end_y)], stroke);
                        p.circle_filled(egui::pos2(x, end_y), 3.5, axis_col);
                    }
                };

                // Extremo superior: flecha si el tiempo avanza hacia arriba; si no, extremo redondeado.
                if newest_top {
                    cap(ui, true, true);
                } else {
                    cap(ui, false, false);
                }

                let mut last_year: Option<i64> = None;
                for &i in &order {
                    let ev = &self.events[i];
                    let (rect, resp) =
                        ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::click());
                    let is_sel = selected.as_deref() == Some(ev.file.as_str());
                    let x = rect.left() + axis_x_off;
                    let cy = rect.center().y;
                    let p = ui.painter();

                    if resp.hovered() || is_sel {
                        p.rect_filled(
                            Rect::from_min_max(
                                rect.min + Vec2::new(0.0, 3.0),
                                egui::pos2(x - 12.0, rect.max.y - 3.0),
                            ),
                            CornerRadius::same(6),
                            if is_sel {
                                accent.gamma_multiply(0.18)
                            } else {
                                hover_bg
                            },
                        );
                    }
                    p.line_segment(
                        [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                        stroke,
                    );

                    // Título a la izquierda del eje.
                    let clip = Rect::from_min_max(
                        rect.min + Vec2::new(8.0, 0.0),
                        egui::pos2(x - 18.0, rect.max.y),
                    );
                    p.with_clip_rect(clip).text(
                        egui::pos2(x - 18.0, cy),
                        Align2::RIGHT_CENTER,
                        &ev.title,
                        FontId::new(14.0, theme::serif()),
                        if is_sel { accent } else { text_col },
                    );

                    // Nodo.
                    let first_of_year = last_year != Some(ev.year);
                    let r = if first_of_year { 6.5 } else { 4.5 };
                    p.circle_filled(
                        egui::pos2(x, cy),
                        r + if is_sel { 2.0 } else { 0.0 },
                        if is_sel { accent } else { axis_col },
                    );
                    if is_sel {
                        p.circle_filled(egui::pos2(x, cy), r - 2.0, on_accent);
                    }

                    // Etiqueta de año (una por año).
                    if first_of_year {
                        let galley = p.layout_no_wrap(
                            ev.year.to_string(),
                            FontId::proportional(13.0),
                            text_col,
                        );
                        let chip = Rect::from_min_size(
                            egui::pos2(x + 12.0, cy - 11.0),
                            Vec2::new(galley.size().x + 14.0, 22.0),
                        );
                        p.rect_filled(chip, CornerRadius::same(6), card);
                        p.rect_stroke(
                            chip,
                            CornerRadius::same(6),
                            Stroke::new(1.0, border),
                            StrokeKind::Inside,
                        );
                        p.galley(
                            egui::pos2(chip.left() + 7.0, chip.center().y - galley.size().y / 2.0),
                            galley,
                            text_col,
                        );
                    }
                    last_year = Some(ev.year);

                    let resp = resp.on_hover_ui(|ui| {
                        ui.set_max_width(210.0);
                        if let Some(img) = &ev.image {
                            tex.show(
                                ui,
                                &project::media_file(root, project::MEDIA_LORE, img),
                                Vec2::new(190.0, 120.0),
                                CornerRadius::same(6),
                            );
                            ui.add_space(4.0);
                        }
                        ui.label(RichText::new(&ev.title).family(theme::serif()).size(15.0));
                        ui.label(
                            RichText::new(trf("Año {}", &[&ev.year]))
                                .size(12.0)
                                .color(weak),
                        );
                    });
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        clicked = Some(ev.file.clone());
                    }
                }

                if newest_top {
                    cap(ui, false, true);
                } else {
                    cap(ui, true, false);
                }
            });
        clicked
    }

    // ------------------------------------------------------------------ diálogos

    fn show_dialog(&mut self, ctx: &egui::Context, root: &Path) {
        let Some(mut d) = self.dialog.take() else {
            return;
        };
        let mut submit = false;
        let mut cancel = false;
        let is_delete = matches!(d.kind, DialogKind::Delete(_));
        let (title, confirm) = match &d.kind {
            DialogKind::New => (tr("Nuevo evento de Lore"), tr("Crear")),
            DialogKind::Edit(_) => (tr("Editar evento"), tr("Guardar")),
            DialogKind::Delete(_) => (tr("Eliminar evento"), tr("Eliminar")),
        };

        let modal = Modal::new(Id::unique("lore_dialog")).show(ctx, |ui| {
            ui.set_width(430.0);
            let weak = ui.visuals().weak_text_color();
            ui.label(RichText::new(title).family(theme::serif()).size(24.0));
            ui.add_space(10.0);

            if is_delete {
                ui.label(
                    RichText::new(trf(
                        "Se eliminará «{}» junto con su texto e imagen. Esta acción no se puede deshacer.",
                        &[&d.title],
                    ))
                    .size(14.0),
                );
            } else {
                ui.label(RichText::new(tr("Título")).size(12.5).color(weak));
                let t = ui.add(TextEdit::singleline(&mut d.title).desired_width(f32::INFINITY).margin(Vec2::new(8.0, 6.0)));
                if d.first {
                    t.request_focus();
                    d.first = false;
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(tr("Año (ej. 2021 o -340)")).size(12.5).color(weak));
                        ui.add(TextEdit::singleline(&mut d.year).desired_width(130.0).margin(Vec2::new(8.0, 6.0)));
                    });
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(tr("Etiqueta (opcional)")).size(12.5).color(weak));
                        ui.add(
                            TextEdit::singleline(&mut d.tag)
                                .desired_width(f32::INFINITY)
                                .margin(Vec2::new(8.0, 6.0)),
                        );
                    });
                });
                ui.add_space(8.0);
                ui.label(RichText::new(tr("Imagen (opcional)")).size(12.5).color(weak));
                ui.horizontal(|ui| {
                    if ui.button(tr("Elegir imagen…")).clicked() {
                        if let Some(p) = rfd::FileDialog::new()
                            .set_title(tr("Elegir imagen del evento"))
                            .add_filter("Imágenes", &["png", "jpg", "jpeg", "webp"])
                            .pick_file()
                        {
                            d.new_image = Some(p);
                            d.remove_image = false;
                        }
                    }
                    let label = if let Some(p) = &d.new_image {
                        p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
                    } else if d.remove_image {
                        tr("Se quitará la imagen").to_owned()
                    } else if d.current_image.is_some() {
                        tr("Imagen actual").to_owned()
                    } else {
                        tr("Sin imagen").to_owned()
                    };
                    ui.label(RichText::new(label).size(12.5).color(weak));
                    if (d.new_image.is_some() || (d.current_image.is_some() && !d.remove_image))
                        && ui.button(tr("Quitar")).clicked()
                    {
                        d.new_image = None;
                        d.remove_image = true;
                    }
                });
                if ui.input(|i| i.key_pressed(Key::Enter)) && t.lost_focus() {
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
                if ui.add(Button::new(tr("Cancelar")).min_size(Vec2::new(90.0, 32.0))).clicked() {
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
        match &d.kind {
            DialogKind::New => {
                let year = project::parse_year(&d.year)?;
                self.flush();
                let ev =
                    project::create_event(root, &d.title, year, &d.tag, d.new_image.as_deref())?;
                self.refresh();
                self.select(&ev.file);
            }
            DialogKind::Edit(file) => {
                let year = project::parse_year(&d.year)?;
                project::update_event(
                    root,
                    file,
                    &d.title,
                    year,
                    &d.tag,
                    d.new_image.as_deref(),
                    d.remove_image,
                )?;
                self.refresh();
            }
            DialogKind::Delete(file) => {
                if self.selected.as_deref() == Some(file.as_str()) {
                    self.auto = Autosave::default(); // el archivo se elimina: no guardar
                }
                project::delete_event(root, file)?;
                self.refresh();
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------- widgets

fn pill(ui: &mut Ui, text: &str, strong: bool) {
    let (fill, border, fg) = {
        let v = ui.visuals();
        let fg = if strong {
            v.selection.bg_fill
        } else {
            v.weak_text_color()
        };
        (
            v.faint_bg_color,
            if strong {
                v.selection.bg_fill
            } else {
                v.window_stroke.color
            },
            fg,
        )
    };
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, border))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(10, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(13.0).color(fg));
        });
}

fn icon_button(ui: &mut Ui, icon: Icon, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
    let v = ui.visuals();
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(6), v.widgets.hovered.weak_bg_fill);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let color = v.widgets.noninteractive.fg_stroke.color;
    icons::paint(
        ui,
        icon,
        Rect::from_center_size(rect.center(), Vec2::splat(16.0)),
        color,
    );
    resp.on_hover_text(tip)
}
