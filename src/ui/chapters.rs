// src/ui/chapters.rs
//! Módulo Capítulos (Etapa 2.2): explorador de temporadas, hoja de escritura con autoguardado,
//! vista previa Markdown y barra flotante de formato.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use egui::{
    text::{CCursor, CCursorRange},
    text_edit::TextEditState,
    Align, Align2, Area, Button, Color32, ComboBox, Context, CornerRadius, DragValue, FontId,
    Frame, Id, Key, KeyboardShortcut, Layout, Margin, Modal, Modifiers, Order, Panel, Rect,
    Response, RichText, ScrollArea, Sense, Stroke, TextEdit, Ui, Vec2,
};

use crate::i18n::{tr, trf};
use crate::icons::{self, Icon};
use crate::project::{self, Season};
use crate::story::Story;
use crate::theme;
use crate::ui::markdown::{self, Edit, FontChoice, TextAlign};

const EXPLORER_W: f32 = 240.0;
const SHEET_W: f32 = 760.0;
const AUTOSAVE_SECS: f64 = 0.8;
const ERROR_COLOR: Color32 = Color32::from_rgb(0xB0, 0x4A, 0x3A);

fn editor_id() -> Id {
    Id::unique("chapter_editor")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Edit,
    Preview,
}

#[derive(Clone, Copy)]
enum Fmt {
    Bold,
    Italic,
    Underline,
    Heading(u8),
    Align(TextAlign),
    Indent(i8),
}

struct Open {
    path: PathBuf,
    season: String,
    name: String,
    text: String,
    dirty: bool,
    last_edit: f64,
    words: usize,
    save_error: Option<String>,
}

enum DialogKind {
    NewSeason,
    NewChapter(PathBuf),
    Rename(PathBuf),
    DeleteSeason(PathBuf),
    DeleteChapter(PathBuf),
}

struct Dialog {
    kind: DialogKind,
    text: String,
    error: Option<String>,
    first: bool,
}

enum Act {
    ToggleSeason(PathBuf),
    OpenChapter(PathBuf),
    Ask(DialogKind, String),
    Duplicate(PathBuf),
}

pub struct ChaptersView {
    root: Option<PathBuf>,
    seasons: Vec<Season>,
    expanded: HashSet<PathBuf>,
    explorer_open: bool,
    current: Option<Open>,
    mode: Mode,
    font: FontChoice,
    size: u32,
    dialog: Option<Dialog>,
    error: Option<String>,
}

impl Default for ChaptersView {
    fn default() -> Self {
        Self {
            root: None,
            seasons: Vec::new(),
            expanded: HashSet::new(),
            explorer_open: true,
            current: None,
            mode: Mode::Edit,
            font: FontChoice::Serif,
            size: 16,
            dialog: None,
            error: None,
        }
    }
}

impl ChaptersView {
    // ------------------------------------------------------------------ estado

    fn refresh(&mut self) {
        if let Some(root) = &self.root {
            self.seasons = project::list_seasons(root);
        }
    }

    fn reset(&mut self, story: &Story) {
        self.flush();
        self.root = Some(story.path.clone());
        self.current = None;
        self.expanded.clear();
        self.refresh();
        if let Some(ch) = self
            .seasons
            .iter()
            .find_map(|s| s.chapters.first().map(|c| (s.path.clone(), c.path.clone())))
        {
            self.expanded.insert(ch.0);
            self.open_chapter(ch.1);
        }
    }

    /// Guarda de inmediato si hay cambios pendientes.
    pub fn flush(&mut self) {
        if let Some(o) = self.current.as_mut() {
            if o.dirty {
                match project::write_chapter(&o.path, &o.text) {
                    Ok(()) => {
                        o.dirty = false;
                        o.save_error = None;
                    }
                    Err(e) => o.save_error = Some(e),
                }
            }
        }
    }

    fn autosave(&mut self, ctx: &Context, now: f64) {
        if let Some(o) = self.current.as_mut() {
            if !o.dirty {
                return;
            }
            let elapsed = now - o.last_edit;
            if elapsed >= AUTOSAVE_SECS {
                match project::write_chapter(&o.path, &o.text) {
                    Ok(()) => {
                        o.dirty = false;
                        o.save_error = None;
                    }
                    Err(e) => {
                        o.save_error = Some(e);
                        o.last_edit = now; // reintenta en 800 ms
                    }
                }
            }
            ctx.request_repaint_after(Duration::from_secs_f64((AUTOSAVE_SECS - elapsed).max(0.05)));
        }
    }

    fn open_chapter(&mut self, path: PathBuf) {
        self.flush();
        match project::read_chapter(&path) {
            Ok(text) => {
                let season = path
                    .parent()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let name = path
                    .file_stem()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.current = Some(Open {
                    words: project::count_words(&text),
                    path,
                    season,
                    name,
                    text,
                    dirty: false,
                    last_edit: 0.0,
                    save_error: None,
                });
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn touch(ctx: &Context, o: &mut Open) {
        o.dirty = true;
        o.last_edit = ctx.input(|i| i.time);
        o.words = project::count_words(&o.text);
    }

    // ------------------------------------------------------------------ formato

    fn cursor_sel(ctx: &Context, len: usize) -> (usize, usize) {
        TextEditState::load(ctx, editor_id())
            .and_then(|s| s.cursor.char_range())
            .map(|r| (r.secondary.index.0.min(len), r.primary.index.0.min(len)))
            .unwrap_or((len, len))
    }

    fn apply_format(&mut self, ctx: &Context, f: Fmt) {
        let Some(o) = self.current.as_mut() else {
            return;
        };
        let len = o.text.chars().count();
        let sel = Self::cursor_sel(ctx, len);
        let cursor = sel.1;

        let edit: Option<Edit> = match f {
            Fmt::Bold => Some(markdown::wrap_selection(&o.text, sel, "**", "**")),
            Fmt::Italic => Some(markdown::wrap_selection(&o.text, sel, "*", "*")),
            Fmt::Underline => Some(markdown::wrap_selection(&o.text, sel, "<u>", "</u>")),
            Fmt::Heading(l) => Some(markdown::set_heading(&o.text, cursor, l)),
            Fmt::Align(a) => markdown::align_paragraph(&o.text, cursor, a),
            Fmt::Indent(d) => markdown::indent(&o.text, cursor, d),
        };
        if let Some(e) = edit {
            o.text = e.text;
            let mut state = TextEditState::load(ctx, editor_id()).unwrap_or_default();
            state.cursor.set_char_range(Some(CCursorRange::two(
                CCursor::new(e.sel.0),
                CCursor::new(e.sel.1),
            )));
            state.store(ctx, editor_id());
            Self::touch(ctx, o);
            ctx.memory_mut(|m| m.request_focus(editor_id()));
            ctx.request_repaint();
        }
    }

    // ------------------------------------------------------------------ vista

    pub fn show(&mut self, ui: &mut Ui, story: &Story) {
        let ctx = ui.ctx().clone();
        if self.root.as_deref() != Some(story.path.as_path()) {
            self.reset(story);
        }
        let now = ctx.input(|i| i.time);
        self.autosave(&ctx, now);

        let (side, border, canvas) = {
            let v = ui.visuals();
            (v.faint_bg_color, v.window_stroke.color, v.panel_fill)
        };

        if self.explorer_open {
            Panel::left("chapters_explorer")
                .exact_size(EXPLORER_W)
                .resizable(false)
                .show_separator_line(false)
                .frame(
                    Frame::new()
                        .fill(side)
                        .stroke(Stroke::new(1.0, border))
                        .inner_margin(Margin::same(12)),
                )
                .show(ui, |ui| self.explorer(ui));
        }

        egui::CentralPanel::default()
            .frame(Frame::new().fill(canvas))
            .show(ui, |ui| {
                self.editor_area(ui, &ctx);
            });

        self.show_dialog(&ctx);
    }

    // ------------------------------------------------------------------ explorador

    fn explorer(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(tr("Temporadas"))
                    .family(theme::serif())
                    .size(19.0),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icon_button(ui, Icon::FolderPlus, tr("Nueva temporada")).clicked() {
                    let n = self.seasons.len() + 1;
                    self.dialog = Some(Dialog {
                        kind: DialogKind::NewSeason,
                        text: trf("Temporada {}", &[&n]),
                        error: None,
                        first: true,
                    });
                }
            });
        });
        ui.add_space(8.0);

        let current_path = self.current.as_ref().map(|o| o.path.clone());
        let acts = ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut acts: Vec<Act> = Vec::new();
                if self.seasons.is_empty() {
                    ui.label(
                        RichText::new(tr("Aún no hay temporadas. Crea una con el botón superior."))
                            .size(13.0)
                            .color(ui.visuals().weak_text_color()),
                    );
                }
                for season in &self.seasons {
                    let open = self.expanded.contains(&season.path);
                    let chevron = if open {
                        Icon::ChevronDown
                    } else {
                        Icon::ChevronRight
                    };
                    let row = tree_row(
                        ui,
                        0.0,
                        &season.name,
                        false,
                        Some(chevron),
                        Some(Icon::FilePlus),
                        true,
                    );
                    if row.trash {
                        acts.push(Act::Ask(
                            DialogKind::DeleteSeason(season.path.clone()),
                            String::new(),
                        ));
                    } else if row.action {
                        let n = season.chapters.len() + 1;
                        acts.push(Act::Ask(
                            DialogKind::NewChapter(season.path.clone()),
                            trf("Capítulo {}", &[&n]),
                        ));
                    } else if row.clicked {
                        acts.push(Act::ToggleSeason(season.path.clone()));
                    }
                    row.resp.context_menu(|ui| {
                        if ui.button(tr("Renombrar")).clicked() {
                            acts.push(Act::Ask(
                                DialogKind::Rename(season.path.clone()),
                                season.name.clone(),
                            ));
                            ui.close();
                        }
                        if ui.button(tr("Eliminar")).clicked() {
                            acts.push(Act::Ask(
                                DialogKind::DeleteSeason(season.path.clone()),
                                String::new(),
                            ));
                            ui.close();
                        }
                    });

                    if open {
                        for ch in &season.chapters {
                            let selected = current_path.as_deref() == Some(ch.path.as_path());
                            let r = tree_row(ui, 1.0, &ch.name, selected, None, None, true);
                            if r.trash {
                                acts.push(Act::Ask(
                                    DialogKind::DeleteChapter(ch.path.clone()),
                                    String::new(),
                                ));
                            } else if r.clicked {
                                acts.push(Act::OpenChapter(ch.path.clone()));
                            }
                            r.resp.context_menu(|ui| {
                                if ui.button(tr("Renombrar")).clicked() {
                                    acts.push(Act::Ask(
                                        DialogKind::Rename(ch.path.clone()),
                                        ch.name.clone(),
                                    ));
                                    ui.close();
                                }
                                if ui.button(tr("Duplicar")).clicked() {
                                    acts.push(Act::Duplicate(ch.path.clone()));
                                    ui.close();
                                }
                                if ui.button(tr("Eliminar")).clicked() {
                                    acts.push(Act::Ask(
                                        DialogKind::DeleteChapter(ch.path.clone()),
                                        String::new(),
                                    ));
                                    ui.close();
                                }
                            });
                        }
                        if season.chapters.is_empty() {
                            ui.horizontal(|ui| {
                                ui.add_space(30.0);
                                ui.label(
                                    RichText::new(tr("Sin capítulos"))
                                        .size(12.5)
                                        .color(ui.visuals().weak_text_color()),
                                );
                            });
                        }
                    }
                    ui.add_space(2.0);
                }
                acts
            })
            .inner;

        for act in acts {
            match act {
                Act::ToggleSeason(p) => {
                    if !self.expanded.remove(&p) {
                        self.expanded.insert(p);
                    }
                }
                Act::OpenChapter(p) => self.open_chapter(p),
                Act::Ask(kind, text) => {
                    self.dialog = Some(Dialog {
                        kind,
                        text,
                        error: None,
                        first: true,
                    });
                }
                Act::Duplicate(p) => {
                    self.flush();
                    match project::duplicate_chapter(&p) {
                        Ok(np) => {
                            self.refresh();
                            self.open_chapter(np);
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------------ editor

    fn editor_area(&mut self, ui: &mut Ui, ctx: &Context) {
        let panel_rect = ui.max_rect();
        self.header(ui);

        // Atajos de teclado (antes del TextEdit para consumirlos).
        if self.current.is_some() && self.dialog.is_none() {
            let mut fmt: Option<Fmt> = None;
            let mut save = false;
            if self.mode == Mode::Edit {
                ctx.input_mut(|i| {
                    if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::B)) {
                        fmt = Some(Fmt::Bold);
                    } else if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::I))
                    {
                        fmt = Some(Fmt::Italic);
                    }
                });
            }
            ctx.input_mut(|i| {
                if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::S)) {
                    save = true;
                }
            });
            if let Some(f) = fmt {
                self.apply_format(ctx, f);
            }
            if save {
                self.flush();
            }
        }

        let Some(open) = self.current.as_mut() else {
            ui.add_space(80.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(tr("Selecciona o crea un capítulo"))
                        .family(theme::serif())
                        .size(22.0)
                        .color(ui.visuals().weak_text_color()),
                );
                if let Some(e) = &self.error {
                    ui.add_space(8.0);
                    ui.label(RichText::new(e).size(13.0).color(ERROR_COLOR));
                }
            });
            return;
        };

        let (mode, font, size) = (self.mode, self.font, self.size as f32);
        let card = ui.visuals().extreme_bg_color;
        let border = ui.visuals().window_stroke.color;

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let avail_w = ui.available_width();
                let avail_h = ui.available_height();
                let w = SHEET_W.min(avail_w - 48.0).max(320.0);
                ui.vertical_centered(|ui| {
                    ui.add_space(8.0);
                    Frame::new()
                        .fill(card)
                        .stroke(Stroke::new(1.0, border))
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(Margin::symmetric(48, 40))
                        .show(ui, |ui| {
                            ui.set_width(w - 96.0);
                            match mode {
                                Mode::Edit => {
                                    let min_h = (avail_h - 80.0 - 110.0).max(320.0);
                                    let resp = ui.add(
                                        TextEdit::multiline(&mut open.text)
                                            .id(editor_id())
                                            .font(FontId::new(size, font.family(false)))
                                            .frame(Frame::NONE)
                                            .desired_width(f32::INFINITY)
                                            .min_size(Vec2::new(0.0, min_h))
                                            .lock_focus(true),
                                    );
                                    if resp.changed() {
                                        Self::touch(ui.ctx(), open);
                                    }
                                }
                                Mode::Preview => {
                                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                                        markdown::render_preview(ui, &open.text, font, size);
                                    });
                                    ui.add_space(24.0);
                                }
                            }
                        });
                    ui.add_space(110.0);
                });
            });

        if self.mode == Mode::Edit && self.dialog.is_none() {
            if let Some(f) = self.toolbar(ctx, panel_rect) {
                self.apply_format(ctx, f);
            }
        }
    }

    fn header(&mut self, ui: &mut Ui) {
        let weak = ui.visuals().weak_text_color();
        Frame::new()
            .inner_margin(Margin::symmetric(24, 10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let tip = if self.explorer_open {
                        tr("Ocultar explorador")
                    } else {
                        tr("Mostrar explorador")
                    };
                    if icon_button(ui, Icon::PanelLeft, tip).clicked() {
                        self.explorer_open = !self.explorer_open;
                    }
                    if let Some(o) = &self.current {
                        ui.label(
                            RichText::new(format!("{} / {}", o.season, o.name))
                                .size(13.5)
                                .color(weak),
                        );
                    }
                    if self.current.is_some() {
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
                            if let Some(o) = &self.current {
                                let words = if o.words == 1 {
                                    tr("1 palabra").to_owned()
                                } else {
                                    trf("{} palabras", &[&o.words])
                                };
                                ui.label(RichText::new(words).size(12.5).color(weak));
                                ui.add_space(6.0);
                                if o.save_error.is_some() {
                                    ui.label(
                                        RichText::new(tr("Error al guardar"))
                                            .size(12.5)
                                            .color(ERROR_COLOR),
                                    );
                                } else if o.dirty {
                                    ui.label(
                                        RichText::new(tr("Guardando…")).size(12.5).color(weak),
                                    );
                                } else {
                                    ui.label(RichText::new(tr("Guardado")).size(12.5).color(weak));
                                }
                            }
                        });
                    }
                });
            });
    }

    // ------------------------------------------------------------------ barra flotante

    fn toolbar(&mut self, ctx: &Context, panel_rect: Rect) -> Option<Fmt> {
        let mut out: Option<Fmt> = None;
        let level = self
            .current
            .as_ref()
            .map(|o| {
                let len = o.text.chars().count();
                markdown::heading_level(&o.text, Self::cursor_sel(ctx, len).1)
            })
            .unwrap_or(0);
        let level_label = match level {
            1 => tr("Título"),
            2 => tr("Subtítulo"),
            _ => tr("Párrafo"),
        };

        Area::new(Id::unique("chapters_toolbar"))
            .order(Order::Middle)
            .fixed_pos(egui::pos2(
                panel_rect.center().x,
                panel_rect.bottom() - 20.0,
            ))
            .pivot(Align2::CENTER_BOTTOM)
            .show(ctx, |ui| {
                let (card, border) = {
                    let v = ui.visuals();
                    (v.extreme_bg_color, v.window_stroke.color)
                };
                Frame::new()
                    .fill(card)
                    .stroke(Stroke::new(1.0, border))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;

                            ComboBox::from_id_salt("fmt_heading")
                                .width(92.0)
                                .selected_text(level_label)
                                .show_ui(ui, |ui| {
                                    for (l, name) in [
                                        (1u8, tr("Título")),
                                        (2, tr("Subtítulo")),
                                        (0, tr("Párrafo")),
                                    ] {
                                        if ui.selectable_label(level == l, name).clicked() {
                                            out = Some(Fmt::Heading(l));
                                        }
                                    }
                                });
                            ComboBox::from_id_salt("fmt_font")
                                .width(170.0)
                                .selected_text(self.font.label())
                                .show_ui(ui, |ui| {
                                    for f in FontChoice::ALL {
                                        ui.selectable_value(&mut self.font, f, f.label());
                                    }
                                });
                            ui.add(
                                DragValue::new(&mut self.size)
                                    .range(12..=28)
                                    .speed(0.1)
                                    .suffix(" px"),
                            )
                            .on_hover_text(tr("Tamaño de texto"));

                            ui.separator();
                            if icon_button(ui, Icon::Bold, tr("Negrita (Ctrl+B)")).clicked() {
                                out = Some(Fmt::Bold);
                            }
                            if icon_button(ui, Icon::Italic, tr("Cursiva (Ctrl+I)")).clicked() {
                                out = Some(Fmt::Italic);
                            }
                            if icon_button(ui, Icon::Underline, tr("Subrayado")).clicked() {
                                out = Some(Fmt::Underline);
                            }

                            ui.separator();
                            for (icon, tip, a) in [
                                (
                                    Icon::AlignLeft,
                                    tr("Alinear a la izquierda"),
                                    TextAlign::Left,
                                ),
                                (Icon::AlignCenter, tr("Centrar"), TextAlign::Center),
                                (
                                    Icon::AlignRight,
                                    tr("Alinear a la derecha"),
                                    TextAlign::Right,
                                ),
                                (Icon::AlignJustify, tr("Justificar"), TextAlign::Justify),
                            ] {
                                if icon_button(ui, icon, tip).clicked() {
                                    out = Some(Fmt::Align(a));
                                }
                            }

                            ui.separator();
                            if icon_button(ui, Icon::IndentDecrease, tr("Disminuir sangría"))
                                .clicked()
                            {
                                out = Some(Fmt::Indent(-1));
                            }
                            if icon_button(ui, Icon::IndentIncrease, tr("Aumentar sangría"))
                                .clicked()
                            {
                                out = Some(Fmt::Indent(1));
                            }
                        });
                    });
            });
        out
    }

    // ------------------------------------------------------------------ diálogos

    fn show_dialog(&mut self, ctx: &Context) {
        let Some(mut d) = self.dialog.take() else {
            return;
        };
        let mut submit = false;
        let mut cancel = false;

        let (title, confirm_label, is_delete) = match &d.kind {
            DialogKind::NewSeason => (tr("Nueva temporada"), tr("Crear"), false),
            DialogKind::NewChapter(_) => (tr("Nuevo capítulo"), tr("Crear"), false),
            DialogKind::Rename(_) => (tr("Renombrar"), tr("Guardar"), false),
            DialogKind::DeleteSeason(_) | DialogKind::DeleteChapter(_) => {
                (tr("Eliminar"), tr("Eliminar"), true)
            }
        };
        let delete_msg = match &d.kind {
            DialogKind::DeleteSeason(p) => {
                let n = self
                    .seasons
                    .iter()
                    .find(|s| &s.path == p)
                    .map(|s| s.chapters.len())
                    .unwrap_or(0);
                trf(
                    "Se eliminará la temporada «{}» con {} capítulo(s). Esta acción no se puede deshacer.",
                    &[
                        &p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                        &n,
                    ],
                )
            }
            DialogKind::DeleteChapter(p) => trf(
                "Se eliminará el capítulo «{}». Esta acción no se puede deshacer.",
                &[&p.file_stem()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()],
            ),
            _ => String::new(),
        };

        let modal = Modal::new(Id::unique("chapters_dialog")).show(ctx, |ui| {
            ui.set_width(400.0);
            ui.label(RichText::new(title).family(theme::serif()).size(24.0));
            ui.add_space(10.0);
            if is_delete {
                ui.label(RichText::new(&delete_msg).size(14.0));
            } else {
                ui.label(
                    RichText::new(tr("Nombre"))
                        .size(12.5)
                        .color(ui.visuals().weak_text_color()),
                );
                let r = ui.add(
                    TextEdit::singleline(&mut d.text)
                        .desired_width(f32::INFINITY)
                        .margin(Vec2::new(8.0, 6.0)),
                );
                if d.first {
                    r.request_focus();
                    d.first = false;
                }
                if r.lost_focus() && ctx.input(|i| i.key_pressed(Key::Enter)) {
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
                let b = Button::new(RichText::new(confirm_label).color(fg).size(14.0))
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
            match self.apply_dialog(&d) {
                Ok(()) => return,
                Err(e) => d.error = Some(e),
            }
        }
        if !cancel {
            self.dialog = Some(d);
        }
    }

    fn apply_dialog(&mut self, d: &Dialog) -> Result<(), String> {
        let root: PathBuf = self.root.clone().ok_or(tr("No hay historia abierta."))?;
        match &d.kind {
            DialogKind::NewSeason => {
                let p = project::create_season(&root, &d.text)?;
                self.expanded.insert(p);
                self.refresh();
            }
            DialogKind::NewChapter(season) => {
                let p = project::create_chapter(season, &d.text)?;
                self.expanded.insert(season.clone());
                self.refresh();
                self.open_chapter(p);
            }
            DialogKind::Rename(path) => {
                self.flush();
                let new = project::rename(path, &d.text)?;
                if self.expanded.remove(path) {
                    self.expanded.insert(new.clone());
                }
                if let Some(o) = self.current.as_mut() {
                    if let Ok(rest) = o.path.strip_prefix(path) {
                        o.path = if rest.as_os_str().is_empty() {
                            new.clone()
                        } else {
                            new.join(rest)
                        };
                        o.season = o
                            .path
                            .parent()
                            .and_then(|p| p.file_name())
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        o.name = o
                            .path
                            .file_stem()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                    }
                }
                self.refresh();
            }
            DialogKind::DeleteSeason(path) | DialogKind::DeleteChapter(path) => {
                if self
                    .current
                    .as_ref()
                    .is_some_and(|o| o.path.starts_with(path))
                {
                    self.current = None;
                }
                project::delete(path)?;
                self.expanded.remove(path);
                self.refresh();
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------- widgets

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

struct Row {
    resp: Response,
    clicked: bool,
    action: bool,
    /// Se pulsó la papelera (siempre visible).
    trash: bool,
}

fn tree_row(
    ui: &mut Ui,
    indent: f32,
    label: &str,
    selected: bool,
    chevron: Option<Icon>,
    action: Option<Icon>,
    trash: bool,
) -> Row {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::click());
    let v = ui.visuals();
    let text = v.widgets.noninteractive.fg_stroke.color;
    let hovered = resp.hovered();
    let (bg, fg) = if selected {
        (v.selection.bg_fill, v.selection.stroke.color)
    } else if hovered {
        (v.widgets.hovered.weak_bg_fill, text)
    } else {
        (Color32::TRANSPARENT, text)
    };
    let hover_bg = v.widgets.hovered.bg_fill;
    ui.painter().rect_filled(rect, CornerRadius::same(5), bg);

    let mut x = rect.left() + indent * 18.0 + 8.0;
    if let Some(ch) = chevron {
        icons::paint(
            ui,
            ch,
            Rect::from_center_size(egui::pos2(x + 7.0, rect.center().y), Vec2::splat(14.0)),
            fg,
        );
        x += 22.0;
    } else {
        x += 8.0;
    }

    // Botones a la derecha: papelera (siempre visible) y, a su izquierda, la acción opcional.
    let mut right = rect.right() - 16.0;
    let trash_rect = Rect::from_center_size(egui::pos2(right, rect.center().y), Vec2::splat(22.0));
    if trash {
        right -= 24.0;
    }
    let action_rect = Rect::from_center_size(egui::pos2(right, rect.center().y), Vec2::splat(22.0));
    let limit = if action.is_some() {
        action_rect.left() - 4.0
    } else if trash {
        trash_rect.left() - 4.0
    } else {
        rect.right() - 6.0
    };
    let clip = Rect::from_min_max(rect.min, egui::pos2(limit, rect.max.y));
    ui.painter().with_clip_rect(clip).text(
        egui::pos2(x, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(14.0),
        fg,
    );

    let press = resp.interact_pointer_pos();
    let pointer_in_action = action.is_some() && press.is_some_and(|p| action_rect.contains(p));
    let pointer_in_trash = trash && press.is_some_and(|p| trash_rect.contains(p));
    let hover_pos = ui.input(|i| i.pointer.hover_pos());
    if let Some(icon) = action {
        if hovered {
            if hover_pos.is_some_and(|p| action_rect.contains(p)) {
                ui.painter()
                    .rect_filled(action_rect, CornerRadius::same(5), hover_bg);
            }
            icons::paint(
                ui,
                icon,
                Rect::from_center_size(action_rect.center(), Vec2::splat(15.0)),
                fg,
            );
        }
    }
    if trash {
        let over = hover_pos.is_some_and(|p| trash_rect.contains(p));
        let danger = Color32::from_rgb(0xB0, 0x4A, 0x3A);
        if over {
            ui.painter().rect_filled(
                trash_rect,
                CornerRadius::same(5),
                danger.gamma_multiply(0.18),
            );
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let color = if over {
            danger
        } else if hovered || selected {
            fg
        } else {
            fg.gamma_multiply(0.6)
        };
        icons::paint(
            ui,
            Icon::Trash,
            Rect::from_center_size(trash_rect.center(), Vec2::splat(15.0)),
            color,
        );
    }
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let trash_clicked = resp.clicked() && pointer_in_trash;
    let action_clicked = resp.clicked() && pointer_in_action;
    let clicked = resp.clicked() && !pointer_in_action && !pointer_in_trash;
    Row {
        resp,
        clicked,
        action: action_clicked,
        trash: trash_clicked,
    }
}
