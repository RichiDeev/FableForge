// src/ui/maps.rs
//! Módulo Mapas (Etapas 3.1 y 3.2): lista de mapas, creación, lienzo con cámara, grid, capas,
//! biblioteca de assets, pincel/borrador de terreno, dispersión de assets, caminos y ríos
//! vectoriales, y deshacer/rehacer.

use std::path::{Path, PathBuf};

use egui::{
    epaint::Vertex, pos2, vec2, Align, Align2, Button, Color32, ComboBox, Context, CornerRadius,
    FontId, Frame, Id, Key, Layout, Margin, Mesh, Modal, Panel, PointerButton, Pos2, Rect,
    Response, RichText, ScrollArea, Sense, Shape, Stroke, StrokeKind, TextEdit, Ui, Vec2,
};

use crate::i18n::{tr, trf};
use crate::icons::{self, Icon};
use crate::map::assets::{self, AssetLibrary};
use crate::map::brush::{self, BrushParams, TerrainBuffer};
use crate::map::camera::{Camera, MAX_ZOOM, MIN_ZOOM};
use crate::map::document::{
    self, GridKind, GridSettings, MapDoc, MapStyle, MapSummary, UndoStack, MAP_EXT,
};
use crate::map::paths::{self, Look, Material, PathKind, TowerStyle};
use crate::map::terrain;
use crate::map::zones::{self, Zone};
use crate::project::{self, MEDIA_MAPS};
use crate::story::{self, Story};
use crate::textures::TextureCache;
use crate::theme;
use crate::ui::doc::Autosave;

const TOOLBAR_W: f32 = 52.0;
const SIDE_W: f32 = 290.0;
const STATUS_H: f32 = 32.0;
const ERROR_COLOR: Color32 = Color32::from_rgb(0xB0, 0x4A, 0x3A);
const ZOOM_STEPS: [u32; 10] = [10, 25, 33, 50, 75, 100, 150, 200, 400, 800];
/// Tamaño (en px de mundo) al que se ajusta un sprite grande al estamparlo.
const STAMP_TARGET: f32 = 96.0;
/// Radio (px de pantalla) para atrapar el nodo de un camino.
const NODE_HIT: f32 = 9.0;
const TOPBAR_H: f32 = 36.0;

const TERRAIN_PRESETS: [(&str, Color32); 6] = [
    ("Césped", Color32::from_rgb(0x5D, 0x7A, 0x47)),
    ("Barro", Color32::from_rgb(0x8E, 0x7A, 0x55)),
    ("Arena", Color32::from_rgb(0xE3, 0xD1, 0xA6)),
    ("Agua profunda", Color32::from_rgb(0x2B, 0x4C, 0x5E)),
    ("Roca", Color32::from_rgb(0x5E, 0x5B, 0x56)),
    ("Nieve", Color32::from_rgb(0xF2, 0xF4, 0xF7)),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Select,
    Place,
    Scatter,
    Brush,
    Eraser,
    Fill,
    Path,
    Zone,
    Region,
    Pan,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SideTab {
    Mapa,
    Herramienta,
    Assets,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CreateMode {
    Blank,
    Image,
    Mask,
}

struct CreateDialog {
    name: String,
    style: MapStyle,
    fixed: bool,
    w: f32,
    h: f32,
    mode: CreateMode,
    src: Option<PathBuf>,
    error: Option<String>,
    first: bool,
}

struct BrushCfg {
    color: Color32,
    /// Diámetro en px del mundo.
    size: f32,
    opacity: f32,
    hardness: f32,
}

impl Default for BrushCfg {
    fn default() -> Self {
        Self {
            color: TERRAIN_PRESETS[0].1,
            size: 40.0,
            opacity: 1.0,
            hardness: 0.5,
        }
    }
}

struct ScatterCfg {
    radius: f32,
    spacing: f32,
}

/// Cómo se dibuja un trazo nuevo.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DrawMode {
    /// Clic a clic, nodo por nodo.
    Nodes,
    /// Presionar y arrastrar; al soltar se simplifica.
    Freehand,
}

impl DrawMode {
    const ALL: [DrawMode; 2] = [DrawMode::Nodes, DrawMode::Freehand];

    fn label(self) -> &'static str {
        match self {
            DrawMode::Nodes => tr("Nodos"),
            DrawMode::Freehand => tr("Mano alzada"),
        }
    }
}

/// Distancia mínima entre puntos (px de pantalla) al simplificar un trazo a mano alzada.
const MIN_POINT_DIST: f32 = 14.0;

struct PathCfg {
    kind: PathKind,
    width: f32,
    dashed: bool,
    mode: DrawMode,
    material: Material,
    tower_style: TowerStyle,
    tower_every: f32,
    sprite: Option<String>,
}

/// Generador pseudoaleatorio simple (xorshift64*) para la dispersión.
struct Rng(u64);

impl Rng {
    fn new() -> Self {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        Self(t | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniforme en [0, 1).
    fn f32(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

enum Hit {
    Asset(u64),
    Path(u64),
    Zone(u64),
    Region(u64),
}

/// Navegación pedida desde la UI (se ejecuta al final del cuadro).
enum Nav {
    /// Entrar al mapa vinculado (nombre de archivo en `03 - Mapas/`).
    Enter(String),
    /// Volver al ancestro `i` de la ruta.
    Goto(usize),
}

/// Animación suave de cámara (pan + zoom).
#[derive(Clone, Copy)]
struct Focus {
    from: Pos2,
    from_zoom: f32,
    to: Pos2,
    to_zoom: f32,
    t0: f64,
}

/// Exportación del mapa a PNG: encuadra todo el mapa, captura la pantalla y recorta.
struct Export {
    path: PathBuf,
    /// Cámara previa, que se restaura al terminar.
    cam: Camera,
    step: u8,
    shot: std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<egui::ColorImage>>>>,
}

/// Renombrado de capa en curso.
struct Rename {
    id: u64,
    text: String,
    fresh: bool,
}

struct OpenMap {
    path: PathBuf,
    root: PathBuf,
    doc: MapDoc,
    terrain: TerrainBuffer,
    hist: UndoStack,
    cam: Camera,
    fit: bool,
    tool: Tool,
    /// Capa activa (id).
    active: u64,
    selected: Option<u64>,
    sel_path: Option<u64>,
    sel_zone: Option<u64>,
    sel_region: Option<u64>,
    /// Selección múltiple de assets (2 o más); con un solo asset se usa `selected`.
    multi: Vec<u64>,
    /// Arrastre de grupo: puntero inicial (mundo) y posición original de cada asset.
    group_drag: Option<(Pos2, Vec<(u64, [f32; 2])>)>,
    /// Selección por recuadro: esquina inicial (mundo) y selección previa (con Ctrl).
    marquee: Option<(Pos2, Vec<u64>)>,
    region_drag: Option<(u64, usize)>,
    region_draft: Vec<Pos2>,
    /// Assets elegidos en la biblioteca (el último es el de «Colocar»).
    brushes: Vec<String>,
    brush: BrushCfg,
    scatter: ScatterCfg,
    path_cfg: PathCfg,
    /// Desplazamiento entre el puntero y el objeto/nodo/vértice durante un arrastre.
    drag: Option<Vec2>,
    node_drag: Option<(u64, usize)>,
    zone_drag: Option<(u64, usize)>,
    draft: Vec<Pos2>,
    /// Exportación a PNG en curso / pedida desde la barra superior.
    export: Option<Export>,
    path_cache: std::cell::RefCell<paths::PathCache>,
    want_export: bool,
    /// Al crear un submapa, copiar como referencia lo cercano del mapa general.
    submap_context: bool,
    /// Puntos crudos del trazo a mano alzada en curso (mundo).
    freehand: Vec<Pos2>,
    /// Anclaje visible bajo el cursor con la herramienta de trazos: (posición, es nodo).
    snap_hint: Option<(Pos2, bool)>,
    zone_draft: Vec<Pos2>,
    stroking: bool,
    scatter_last: Option<Pos2>,
    rng: Rng,
    /// La escena cambió y falta confirmarla en el historial.
    touched: bool,
    auto: Autosave,
    cursor: Option<Pos2>,
    status: Option<String>,
    rename: Option<Rename>,
    nav: Option<Nav>,
    /// Otros mapas de la historia: (nombre de archivo, nombre visible).
    others: Vec<(String, String)>,
    /// Pedido de enfoque (rectángulo del mundo) y animación en curso.
    focus_req: Option<Rect>,
    focus: Option<Focus>,
}

impl OpenMap {
    /// Marca un cambio de escena (autoguardado + historial).
    fn touch(&mut self, ctx: &Context) {
        self.auto.mark(ctx);
        self.touched = true;
    }

    fn brush_params(&self, erase: bool) -> BrushParams {
        BrushParams {
            radius: self.brush.size / 2.0,
            hardness: self.brush.hardness,
            opacity: self.brush.opacity,
            color: self.brush.color,
            erase,
        }
    }

    fn layer_name(&self, id: u64) -> String {
        self.doc
            .layer(id)
            .map_or_else(|| "?".to_owned(), |l| l.name.clone())
    }

    /// La capa admite edición (visible y sin bloquear).
    fn layer_ok(&self, id: u64) -> Result<(), String> {
        if !self.doc.is_visible(id) {
            Err(format!("La capa «{}» está oculta.", self.layer_name(id)))
        } else if self.doc.is_locked(id) {
            Err(format!("La capa «{}» está bloqueada.", self.layer_name(id)))
        } else {
            Ok(())
        }
    }

    /// Como `layer_ok`, pero además la capa debe ser de objetos (no la de terreno).
    fn object_layer_ok(&self, id: u64) -> Result<(), String> {
        if self.doc.is_terrain_layer(id) {
            Err(format!(
                "La capa «{}» es solo para pintar terreno; elige otra capa.",
                self.layer_name(id)
            ))
        } else {
            self.layer_ok(id)
        }
    }

    /// Capa donde van los caminos nuevos.
    fn path_layer(&self) -> u64 {
        if self.doc.is_terrain_layer(self.active) {
            self.doc.object_layer_named("camino")
        } else {
            self.active
        }
    }

    /// Si la capa activa desapareció (deshacer, borrado), cae a una válida.
    fn ensure_active(&mut self) {
        if self.doc.layer(self.active).is_none() {
            self.active = self.doc.first_object_layer();
        }
    }

    /// Assets seleccionados (uno o varios).
    fn asset_set(&self) -> Vec<u64> {
        if self.multi.is_empty() {
            self.selected.into_iter().collect()
        } else {
            self.multi.clone()
        }
    }

    fn set_assets(&mut self, ids: Vec<u64>) {
        self.select(None);
        match ids.len() {
            0 => {}
            1 => self.selected = Some(ids[0]),
            _ => self.multi = ids,
        }
    }

    fn select(&mut self, hit: Option<Hit>) {
        self.selected = None;
        self.sel_path = None;
        self.sel_zone = None;
        self.sel_region = None;
        self.multi.clear();
        match hit {
            Some(Hit::Region(id)) => self.sel_region = Some(id),
            Some(Hit::Asset(id)) => self.selected = Some(id),
            Some(Hit::Path(id)) => self.sel_path = Some(id),
            Some(Hit::Zone(id)) => self.sel_zone = Some(id),
            None => {}
        }
    }
}

enum LibAction {
    ImportFiles,
    ImportFolder,
}

pub struct MapsView {
    root: Option<PathBuf>,
    maps: Vec<MapSummary>,
    open: Option<OpenMap>,
    lib: AssetLibrary,
    side_tab: SideTab,
    create: Option<CreateDialog>,
    delete: Option<(PathBuf, String)>,
    error: Option<String>,
    /// Ruta de mapas anidados recorrida hasta el actual (ancestros, del más lejano al más cercano).
    trail: Vec<(PathBuf, String)>,
}

impl Default for MapsView {
    fn default() -> Self {
        Self {
            root: None,
            maps: Vec::new(),
            open: None,
            lib: AssetLibrary::default(),
            side_tab: SideTab::Mapa,
            create: None,
            delete: None,
            error: None,
            trail: Vec::new(),
        }
    }
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Guarda terreno (si cambió) y documento.
fn save_open(om: &mut OpenMap) -> Result<(), String> {
    if om.terrain.dirty {
        om.doc.terrain = om.terrain.save(&brush::terrain_path(&om.path))?;
        om.terrain.dirty = false;
    }
    om.doc.save(&om.path)
}

impl MapsView {
    // ------------------------------------------------------------------ estado

    fn refresh(&mut self) {
        if let Some(root) = &self.root {
            self.maps = document::list_maps(root);
        }
    }

    fn reset(&mut self, story: &Story) {
        self.flush();
        self.root = Some(story.path.clone());
        self.open = None;
        self.trail.clear();
        self.lib = AssetLibrary::scan(&story.path);
        self.refresh();
    }

    pub fn flush(&mut self) {
        if let Some(om) = self.open.as_mut() {
            if om.auto.dirty {
                match save_open(om) {
                    Ok(()) => {
                        om.auto.dirty = false;
                        om.auto.error = None;
                    }
                    Err(e) => om.auto.error = Some(e),
                }
            }
        }
    }

    fn open_map(&mut self, path: &Path) {
        self.flush();
        let Some(root) = self.root.clone() else {
            return;
        };
        match MapDoc::load(path) {
            Ok(doc) => {
                let mut status = None;
                let terrain = match &doc.terrain {
                    Some(r) => TerrainBuffer::load(&brush::terrain_path(path), r, doc.size)
                        .unwrap_or_else(|e| {
                            status = Some(e);
                            TerrainBuffer::new(doc.size)
                        }),
                    None => TerrainBuffer::new(doc.size),
                };
                let me = file_name(path);
                let others = document::list_maps(&root)
                    .into_iter()
                    .map(|m| (file_name(&m.path), m.name))
                    .filter(|(f, _)| *f != me)
                    .collect();
                self.open = Some(OpenMap {
                    path: path.to_path_buf(),
                    root,
                    hist: UndoStack::new(&doc),
                    terrain,
                    cam: Camera::default(),
                    fit: true,
                    tool: Tool::Select,
                    active: doc.object_layer_named("naturaleza"),
                    doc,
                    selected: None,
                    sel_path: None,
                    sel_zone: None,
                    sel_region: None,
                    multi: Vec::new(),
                    group_drag: None,
                    marquee: None,
                    region_drag: None,
                    region_draft: Vec::new(),
                    brushes: Vec::new(),
                    brush: BrushCfg::default(),
                    scatter: ScatterCfg {
                        radius: 90.0,
                        spacing: 36.0,
                    },
                    path_cfg: PathCfg {
                        kind: PathKind::Camino,
                        width: PathKind::Camino.default_width(),
                        dashed: false,
                        mode: DrawMode::Nodes,
                        material: Material::Tierra,
                        tower_style: TowerStyle::Alternada,
                        tower_every: paths::DEFAULT_TOWER_EVERY,
                        sprite: None,
                    },
                    drag: None,
                    node_drag: None,
                    zone_drag: None,
                    draft: Vec::new(),
                    freehand: Vec::new(),
                    export: None,
                    path_cache: Default::default(),
                    want_export: false,
                    submap_context: true,
                    snap_hint: None,
                    zone_draft: Vec::new(),
                    stroking: false,
                    scatter_last: None,
                    rng: Rng::new(),
                    touched: false,
                    auto: Autosave::default(),
                    cursor: None,
                    status,
                    rename: None,
                    nav: None,
                    others,
                    focus_req: None,
                    focus: None,
                });
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// Vuelve a la lista de mapas.
    fn close_map(&mut self) {
        self.flush();
        self.open = None;
        self.trail.clear();
        self.refresh();
    }

    fn handle_nav(&mut self, nav: Nav, root: &Path) {
        match nav {
            Nav::Enter(file) => {
                let target = document::maps_dir(root).join(&file);
                let Some(cur) = self
                    .open
                    .as_ref()
                    .map(|o| (o.path.clone(), o.doc.name.clone()))
                else {
                    return;
                };
                if !target.exists() {
                    if let Some(om) = self.open.as_mut() {
                        om.status = Some(trf("No se encontró el mapa vinculado «{}».", &[&file]));
                    }
                    return;
                }
                self.trail.push(cur);
                self.open_map(&target);
                if self.open.as_ref().is_none_or(|o| o.path != target) {
                    // No se pudo abrir: se queda en el mapa actual.
                    self.trail.pop();
                    let err = self.error.take();
                    if let (Some(om), Some(e)) = (self.open.as_mut(), err) {
                        om.status = Some(e);
                    }
                }
            }
            Nav::Goto(i) => {
                if let Some((p, _)) = self.trail.get(i).cloned() {
                    self.trail.truncate(i);
                    self.open_map(&p);
                }
            }
        }
    }

    // ------------------------------------------------------------------ vista

    pub fn show(&mut self, ui: &mut Ui, story: &Story, tex: &mut TextureCache) {
        let ctx = ui.ctx().clone();
        if self.root.as_deref() != Some(story.path.as_path()) {
            self.reset(story);
        }
        if let Some(om) = self.open.as_mut() {
            if om.auto.due(&ctx) {
                let r = save_open(om);
                om.auto.finish(&ctx, r);
            }
        }

        if self.open.is_some() {
            self.editor(ui, &ctx, &story.path, tex);
        } else {
            self.list_view(ui, &story.path);
        }

        self.show_create(&ctx, &story.path);
        self.show_delete(&ctx, &story.path);
    }

    // ------------------------------------------------------------------ lista de mapas

    fn list_view(&mut self, ui: &mut Ui, _root: &Path) {
        let fill = ui.visuals().panel_fill;
        let mut to_open: Option<PathBuf> = None;
        let mut to_delete: Option<(PathBuf, String)> = None;
        let mut new_map = false;

        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(fill)
                    .inner_margin(Margin::symmetric(56, 40)),
            )
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.label(RichText::new(tr("Mapas")).family(theme::serif()).size(46.0));
                        ui.add_space(14.0);
                        let accent = ui.visuals().selection.bg_fill;
                        let on_accent = ui.visuals().selection.stroke.color;
                        let btn = Button::new(
                            RichText::new(tr("+ Crear Nuevo Mapa"))
                                .color(on_accent)
                                .size(14.5),
                        )
                        .fill(accent)
                        .stroke(Stroke::new(1.0, accent))
                        .min_size(vec2(170.0, 34.0));
                        if ui.add(btn).clicked() {
                            new_map = true;
                        }
                        if let Some(e) = &self.error {
                            ui.add_space(8.0);
                            ui.label(RichText::new(e).size(13.0).color(ERROR_COLOR));
                        }
                        ui.add_space(24.0);

                        if self.maps.is_empty() {
                            ui.label(
                                RichText::new(tr("Todavía no hay mapas en esta historia."))
                                    .size(14.0)
                                    .color(ui.visuals().weak_text_color()),
                            );
                        }
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::splat(20.0);
                            for m in &self.maps {
                                let (resp, trash) = map_card(ui, m);
                                if trash {
                                    to_delete = Some((m.path.clone(), m.name.clone()));
                                } else if resp.clicked() {
                                    to_open = Some(m.path.clone());
                                }
                                resp.context_menu(|ui| {
                                    if ui.button(tr("Eliminar")).clicked() {
                                        to_delete = Some((m.path.clone(), m.name.clone()));
                                        ui.close();
                                    }
                                });
                            }
                        });
                    });
            });

        if new_map {
            let n = self.maps.len() + 1;
            self.create = Some(CreateDialog {
                name: format!("Mapa {n}"),
                style: MapStyle::Atlas,
                fixed: true,
                w: 2000.0,
                h: 1400.0,
                mode: CreateMode::Blank,
                src: None,
                error: None,
                first: true,
            });
        }
        if let Some(p) = to_open {
            self.trail.clear();
            self.open_map(&p);
        }
        if to_delete.is_some() {
            self.delete = to_delete;
        }
    }

    // ------------------------------------------------------------------ editor

    fn editor(&mut self, ui: &mut Ui, ctx: &Context, root: &Path, tex: &mut TextureCache) {
        let (side, border, canvas) = {
            let v = ui.visuals();
            (v.faint_bg_color, v.window_stroke.color, v.panel_fill)
        };
        let frame = |margin: i8| {
            Frame::new()
                .fill(side)
                .stroke(Stroke::new(1.0, border))
                .inner_margin(Margin::same(margin))
        };

        let Self {
            open,
            lib,
            side_tab,
            trail,
            ..
        } = self;
        let Some(om) = open.as_mut() else { return };
        om.ensure_active();
        let prev_tool = om.tool;
        let prev_kind = om.path_cfg.kind;
        let prev_sel = (
            om.selected,
            om.sel_path,
            om.sel_zone,
            om.sel_region,
            om.multi.len(),
        );
        let mut close = false;
        let mut lib_action: Option<LibAction> = None;

        let toolbar_rect = Panel::left("map_toolbar")
            .exact_size(TOOLBAR_W)
            .resizable(false)
            .show_separator_line(false)
            .frame(frame(6))
            .show(ui, |ui| toolbar(ui, om))
            .response
            .rect;

        // Panel derecho de ancho fijo: el lienzo recibe solo el espacio restante.
        let side_rect = Panel::right("map_side")
            .exact_size(SIDE_W)
            .resizable(false)
            .show_separator_line(false)
            .frame(frame(12))
            .show(ui, |ui| lib_action = side_panel(ui, om, lib, side_tab, tex))
            .response
            .rect;

        let status_rect = Panel::bottom("map_status")
            .exact_size(STATUS_H)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                Frame::new()
                    .fill(side)
                    .stroke(Stroke::new(1.0, border))
                    .inner_margin(Margin::symmetric(12, 4)),
            )
            .show(ui, |ui| status_bar(ui, om))
            .response
            .rect;

        let top_rect = Panel::top("map_top")
            .exact_size(TOPBAR_H)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                Frame::new()
                    .fill(side)
                    .stroke(Stroke::new(1.0, border))
                    .inner_margin(Margin::symmetric(12, 4)),
            )
            .show(ui, |ui| close = top_bar(ui, om, &trail[..]))
            .response
            .rect;

        // Rectángulo exacto del lienzo: entre los paneles, sin invadir ninguno.
        let canvas_limit = Rect::from_min_max(
            pos2(toolbar_rect.max.x, top_rect.max.y),
            pos2(side_rect.min.x, status_rect.min.y),
        );
        egui::CentralPanel::default()
            .frame(Frame::new().fill(canvas))
            .show(ui, |ui| viewport(ui, ctx, om, lib, tex, root, canvas_limit));

        if std::mem::take(&mut om.want_export) {
            start_export(om, root);
        }

        // Historial: confirma cambios de escena cuando no hay un arrastre ni una edición de texto
        // en curso (así escribir un nombre es un solo paso de deshacer).
        if om.touched && !ctx.input(|i| i.pointer.any_down()) && !ctx.egui_wants_keyboard_input() {
            om.hist.sync(&om.doc);
            om.touched = false;
        }

        // Cambio de herramienta: capa y pestaña acordes.
        if om.tool != prev_tool || om.path_cfg.kind != prev_kind {
            if om.tool != Tool::Path || om.path_cfg.kind != prev_kind {
                om.draft.clear();
            }
            if om.tool != Tool::Zone {
                om.zone_draft.clear();
            }
            if om.tool != Tool::Region {
                om.region_draft.clear();
            }
            match om.tool {
                Tool::Brush | Tool::Eraser | Tool::Fill => {
                    om.active = om.doc.terrain_layer();
                    *side_tab = SideTab::Herramienta;
                }
                Tool::Path => {
                    if om.doc.is_terrain_layer(om.active) {
                        om.active = om.doc.object_layer_named("camino");
                    }
                    *side_tab = SideTab::Herramienta;
                }
                Tool::Place => {
                    if om.doc.is_terrain_layer(om.active) {
                        om.active = om.doc.object_layer_named("naturaleza");
                    }
                    *side_tab = SideTab::Assets;
                }
                Tool::Scatter => {
                    if om.doc.is_terrain_layer(om.active) {
                        om.active = om.doc.object_layer_named("naturaleza");
                    }
                    *side_tab = SideTab::Herramienta;
                }
                Tool::Zone | Tool::Region => *side_tab = SideTab::Herramienta,
                _ => {}
            }
        }
        // Al seleccionar un objeto, sus ajustes pasan al frente.
        let sel = (
            om.selected,
            om.sel_path,
            om.sel_zone,
            om.sel_region,
            om.multi.len(),
        );
        if sel != prev_sel
            && (sel.0.is_some()
                || sel.1.is_some()
                || sel.2.is_some()
                || sel.3.is_some()
                || sel.4 > 0)
        {
            *side_tab = SideTab::Herramienta;
        }

        let nav = om.nav.take();
        if close {
            self.close_map();
        } else if let Some(n) = nav {
            self.handle_nav(n, root);
        }
        match lib_action {
            Some(LibAction::ImportFiles) => {
                if let Some(files) = rfd::FileDialog::new()
                    .set_title(tr("Importar PNG a la biblioteca de la historia"))
                    .add_filter("PNG", &["png"])
                    .pick_files()
                {
                    self.finish_import(root, assets::import_files(root, &files));
                }
            }
            Some(LibAction::ImportFolder) => {
                if let Some(dir) = rfd::FileDialog::new()
                    .set_title(tr("Importar carpeta de assets"))
                    .pick_folder()
                {
                    self.finish_import(root, assets::import_folder(root, &dir));
                }
            }
            None => {}
        }
    }

    fn finish_import(&mut self, root: &Path, result: Result<usize, String>) {
        let msg = match result {
            Ok(0) => tr("No se encontraron PNG para importar.").to_owned(),
            Ok(n) => trf("Se importaron {} asset(s).", &[&n]),
            Err(e) => e,
        };
        self.lib = AssetLibrary::scan(root);
        if let Some(om) = self.open.as_mut() {
            om.status = Some(msg);
        }
    }

    // ------------------------------------------------------------------ diálogos

    fn show_create(&mut self, ctx: &Context, root: &Path) {
        let Some(mut d) = self.create.take() else {
            return;
        };
        let mut submit = false;
        let mut cancel = false;

        let modal = Modal::new(Id::unique("map_create_modal")).show(ctx, |ui| {
            ui.set_width(460.0);
            let weak = ui.visuals().weak_text_color();
            ui.label(
                RichText::new(tr("Crear nuevo mapa"))
                    .family(theme::serif())
                    .size(26.0),
            );
            ui.add_space(10.0);

            ui.label(RichText::new(tr("Nombre del mapa")).size(12.5).color(weak));
            let r = ui.add(
                TextEdit::singleline(&mut d.name)
                    .desired_width(f32::INFINITY)
                    .margin(vec2(8.0, 6.0)),
            );
            if d.first {
                r.request_focus();
                d.first = false;
            }

            ui.add_space(10.0);
            ui.label(RichText::new(tr("Estilo visual")).size(12.5).color(weak));
            ui.horizontal(|ui| {
                for s in MapStyle::ALL {
                    ui.selectable_value(&mut d.style, s, s.label());
                }
            });

            ui.add_space(10.0);
            ui.label(RichText::new(tr("Dimensiones")).size(12.5).color(weak));
            ui.horizontal(|ui| {
                ui.selectable_value(&mut d.fixed, true, tr("Tamaño fijo"));
                ui.selectable_value(&mut d.fixed, false, tr("Lienzo infinito"));
            });
            if d.fixed {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut d.w)
                            .range(500.0..=8000.0)
                            .speed(10.0)
                            .suffix(" px"),
                    );
                    ui.label("×");
                    ui.add(
                        egui::DragValue::new(&mut d.h)
                            .range(500.0..=8000.0)
                            .speed(10.0)
                            .suffix(" px"),
                    );
                });
            }

            ui.add_space(10.0);
            ui.label(RichText::new(tr("Modo de creación")).size(12.5).color(weak));
            ui.horizontal(|ui| {
                ui.selectable_value(&mut d.mode, CreateMode::Blank, tr("Lienzo en blanco"));
                ui.selectable_value(&mut d.mode, CreateMode::Image, tr("Imagen base"));
                ui.selectable_value(&mut d.mode, CreateMode::Mask, tr("Desde contorno"));
            });
            match d.mode {
                CreateMode::Blank => {
                    ui.label(
                        RichText::new(
                            tr("Fondo base de agua (Atlas) o papel pergamino (Pergamino)."),
                        )
                        .size(12.0)
                        .color(weak),
                    );
                }
                CreateMode::Image | CreateMode::Mask => {
                    let hint = if d.mode == CreateMode::Image {
                        tr("Se carga completa en la capa Terreno como fondo o guía.")
                    } else {
                        tr("Píxeles oscuros → tierra, claros → agua; se genera una textura base nítida.")
                    };
                    ui.label(RichText::new(hint).size(12.0).color(weak));
                    ui.horizontal(|ui| {
                        if ui.button(tr("Elegir imagen…")).clicked() {
                            if let Some(p) = rfd::FileDialog::new()
                                .set_title(tr("Elegir imagen"))
                                .add_filter("Imágenes", &["png", "jpg", "jpeg", "webp", "bmp"])
                                .pick_file()
                            {
                                d.src = Some(p);
                            }
                        }
                        let name = d
                            .src
                            .as_ref()
                            .and_then(|p| p.file_name())
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| tr("Ninguna seleccionada").to_owned());
                        ui.label(RichText::new(name).size(12.5).color(weak));
                    });
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
                let b = Button::new(RichText::new(tr("Crear mapa")).color(on_accent).size(14.0))
                    .fill(accent)
                    .stroke(Stroke::new(1.0, accent))
                    .min_size(vec2(120.0, 32.0));
                if ui.add(b).clicked() {
                    submit = true;
                }
                if ui
                    .add(Button::new(tr("Cancelar")).min_size(vec2(90.0, 32.0)))
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
            match self.apply_create(&d, root) {
                Ok(()) => return,
                Err(e) => d.error = Some(e),
            }
        }
        if !cancel {
            self.create = Some(d);
        }
    }

    fn apply_create(&mut self, d: &CreateDialog, root: &Path) -> Result<(), String> {
        let name = d.name.trim();
        if name.is_empty() {
            return Err(tr("El nombre no puede estar vacío.").to_owned());
        }
        let slug = story::sanitize_folder_name(name);
        let path = document::maps_dir(root).join(format!("{slug}{MAP_EXT}"));
        if path.exists() {
            return Err(tr("Ya existe un mapa con ese nombre.").to_owned());
        }
        let size = d.fixed.then_some([d.w, d.h]);
        let mut doc = MapDoc::new(name, d.style, size);
        match d.mode {
            CreateMode::Blank => {}
            CreateMode::Image | CreateMode::Mask => {
                let src = d
                    .src
                    .as_ref()
                    .ok_or(tr("Elige una imagen para este modo de creación."))?;
                let base = terrain::make_base(
                    root,
                    &slug,
                    src,
                    d.style,
                    size,
                    d.mode == CreateMode::Mask,
                )?;
                if d.fixed {
                    doc.size = Some(base.size); // el mapa toma las proporciones de la imagen
                }
                doc.base = Some(base);
            }
        }
        doc.save(&path)?;
        self.refresh();
        self.open_map(&path);
        Ok(())
    }

    fn show_delete(&mut self, ctx: &Context, root: &Path) {
        let Some((path, name)) = self.delete.take() else {
            return;
        };
        let mut confirm = false;
        let mut cancel = false;
        let modal = Modal::new(Id::unique("map_delete_modal")).show(ctx, |ui| {
            ui.set_width(400.0);
            ui.label(
                RichText::new(tr("Eliminar mapa"))
                    .family(theme::serif())
                    .size(24.0),
            );
            ui.add_space(10.0);
            ui.label(
                RichText::new(trf(
                    "Se eliminará «{}». Esta acción no se puede deshacer.",
                    &[&name],
                ))
                .size(14.0),
            );
            ui.add_space(16.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let b = Button::new(
                    RichText::new(tr("Eliminar"))
                        .color(Color32::WHITE)
                        .size(14.0),
                )
                .fill(ERROR_COLOR)
                .stroke(Stroke::new(1.0, ERROR_COLOR))
                .min_size(vec2(110.0, 32.0));
                if ui.add(b).clicked() {
                    confirm = true;
                }
                if ui
                    .add(Button::new(tr("Cancelar")).min_size(vec2(90.0, 32.0)))
                    .clicked()
                {
                    cancel = true;
                }
            });
        });
        if modal.should_close() {
            cancel = true;
        }
        if confirm {
            if let Ok(doc) = MapDoc::load(&path) {
                if let Some(b) = doc.base {
                    project::remove_media(root, MEDIA_MAPS, &b.file);
                }
            }
            let _ = std::fs::remove_file(brush::terrain_path(&path));
            if let Err(e) = std::fs::remove_file(&path) {
                self.error = Some(format!("No se pudo eliminar el mapa: {e}"));
            }
            self.refresh();
        } else if !cancel {
            self.delete = Some((path, name));
        }
    }
}

// ---------------------------------------------------------------------- tarjeta de mapa

/// Devuelve la respuesta de la tarjeta y si se pulsó la papelera.
fn map_card(ui: &mut Ui, m: &MapSummary) -> (Response, bool) {
    let (rect, resp) = ui.allocate_exact_size(vec2(280.0, 120.0), Sense::click());
    let v = ui.visuals();
    let (fill, stroke) = if resp.hovered() {
        (v.widgets.hovered.weak_bg_fill, v.selection.bg_fill)
    } else {
        (v.extreme_bg_color, v.window_stroke.color)
    };
    let (text, weak) = (
        v.widgets.noninteractive.fg_stroke.color,
        v.weak_text_color(),
    );
    let r = CornerRadius::same(8);
    ui.painter().rect_filled(rect, r, fill);
    ui.painter()
        .rect_stroke(rect, r, Stroke::new(1.0, stroke), StrokeKind::Inside);

    // Muestra del estilo.
    let sw = Rect::from_min_size(rect.min + vec2(16.0, 16.0), vec2(48.0, 48.0));
    ui.painter()
        .rect_filled(sw, CornerRadius::same(6), m.style.background());
    ui.painter().rect_stroke(
        sw,
        CornerRadius::same(6),
        Stroke::new(1.0, m.style.coast()),
        StrokeKind::Inside,
    );
    icons::paint(
        ui,
        Icon::Map,
        Rect::from_center_size(sw.center(), Vec2::splat(24.0)),
        m.style.coast(),
    );

    let mut clip_rect = rect.shrink(8.0);
    clip_rect.max.x -= 30.0; // deja libre la papelera
    let clip = ui.painter().with_clip_rect(clip_rect);
    clip.text(
        pos2(sw.right() + 14.0, rect.top() + 24.0),
        Align2::LEFT_CENTER,
        &m.name,
        FontId::new(19.0, theme::serif()),
        text,
    );
    let dims = match m.size {
        Some([w, h]) => format!("{} · {}×{} px", m.style.short(), w as u32, h as u32),
        None => trf("{} · Lienzo infinito", &[&m.style.short()]),
    };
    clip.text(
        pos2(sw.right() + 14.0, rect.top() + 48.0),
        Align2::LEFT_CENTER,
        dims,
        FontId::proportional(12.5),
        weak,
    );
    if let Some(t) = m.modified {
        ui.painter().text(
            pos2(rect.left() + 16.0, rect.bottom() - 18.0),
            Align2::LEFT_CENTER,
            trf("Modificado {}", &[&story::format_date(t)]),
            FontId::proportional(12.0),
            weak,
        );
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    // Papelera (esquina superior derecha). Se crea después de la tarjeta, así recibe el clic.
    let trash_rect = Rect::from_min_size(
        pos2(rect.right() - 34.0, rect.top() + 8.0),
        Vec2::splat(26.0),
    );
    let tid = Id::unique(("map_trash", m.path.as_path()));
    let trash_resp = ui.interact(trash_rect, tid, Sense::click());
    let color = if trash_resp.hovered() {
        ui.painter().rect_filled(
            trash_rect,
            CornerRadius::same(6),
            ERROR_COLOR.gamma_multiply(0.15),
        );
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        ERROR_COLOR
    } else {
        weak
    };
    icons::paint(
        ui,
        Icon::Trash,
        Rect::from_center_size(trash_rect.center(), Vec2::splat(15.0)),
        color,
    );
    let clicked = trash_resp.on_hover_text(tr("Eliminar mapa")).clicked();
    (resp, clicked)
}

// ---------------------------------------------------------------------- barra de herramientas

/// Cambia el tipo de trazo y ajusta su ancho por defecto.
fn set_path_kind(om: &mut OpenMap, kind: PathKind) {
    if om.path_cfg.kind != kind {
        om.path_cfg.kind = kind;
        om.path_cfg.width = kind.default_width();
        if kind == PathKind::Muralla {
            om.path_cfg.mode = DrawMode::Nodes;
        }
    }
}

fn tool_group_gap(ui: &mut Ui) {
    ui.add_space(4.0);
    let w = ui.available_width() * 0.6;
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    let color = ui.visuals().window_stroke.color;
    ui.painter().hline(
        egui::Rangef::new(r.center().x - w / 2.0, r.center().x + w / 2.0),
        r.center().y,
        Stroke::new(1.0, color),
    );
    ui.add_space(5.0);
}

/// Herramientas agrupadas: Selección · Terreno · Trazos · Mundo.
fn toolbar(ui: &mut Ui, om: &mut OpenMap) {
    ui.shrink_clip_rect(ui.max_rect());
    ui.vertical_centered(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        let groups: [&[(Tool, Icon, &str)]; 4] = [
            &[
                (Tool::Select, Icon::MousePointer, tr("Seleccionar (V)")),
                (
                    Tool::Pan,
                    Icon::Hand,
                    tr("Mano: mover vista (H) · o clic central / derecho"),
                ),
            ],
            &[
                (Tool::Brush, Icon::Brush, tr("Pincel de terreno (B)")),
                (Tool::Eraser, Icon::Eraser, tr("Borrador de terreno (E)")),
                (Tool::Fill, Icon::Bucket, tr("Relleno / cubeta (G)")),
                (
                    Tool::Region,
                    Icon::Region,
                    tr("Región vectorial: costas, lagos, plataformas nítidas (T)"),
                ),
            ],
            &[],
            &[
                (Tool::Place, Icon::Stamp, tr("Estampar assets (P)")),
                (Tool::Scatter, Icon::Scatter, tr("Dispersar assets (D)")),
                (Tool::Zone, Icon::Zone, tr("Zonas / Territorios (Z)")),
            ],
        ];
        for (gi, group) in groups.iter().enumerate() {
            if gi > 0 {
                tool_group_gap(ui);
            }
            if gi == 2 {
                let wall = om.path_cfg.kind == PathKind::Muralla;
                if tool_button(
                    ui,
                    Icon::Route,
                    tr("Caminos / Ríos (C)"),
                    om.tool == Tool::Path && !wall,
                )
                .clicked()
                {
                    if wall {
                        set_path_kind(om, PathKind::Camino);
                    }
                    om.tool = Tool::Path;
                }
                if tool_button(
                    ui,
                    Icon::Wall,
                    tr("Murallas (W)"),
                    om.tool == Tool::Path && wall,
                )
                .clicked()
                {
                    set_path_kind(om, PathKind::Muralla);
                    om.tool = Tool::Path;
                }
                continue;
            }
            for &(tool, icon, tip) in group.iter() {
                if tool_button(ui, icon, tip, om.tool == tool).clicked() {
                    om.tool = tool;
                }
            }
        }
    });
}

fn tool_button(ui: &mut Ui, icon: Icon, tip: &str, selected: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(38.0), Sense::click());
    let v = ui.visuals();
    let text = v.widgets.noninteractive.fg_stroke.color;
    let (bg, fg) = if selected {
        (v.selection.bg_fill, v.selection.stroke.color)
    } else if resp.hovered() {
        (v.widgets.hovered.weak_bg_fill, text)
    } else {
        (Color32::TRANSPARENT, text)
    };
    ui.painter().rect_filled(rect, CornerRadius::same(8), bg);
    icons::paint(
        ui,
        icon,
        Rect::from_center_size(rect.center(), Vec2::splat(20.0)),
        fg,
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.on_hover_text(tip)
}

fn small_icon(ui: &mut Ui, icon: Icon, tip: &str, active: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
    let v = ui.visuals();
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(6), v.widgets.hovered.weak_bg_fill);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let color = if active {
        v.widgets.noninteractive.fg_stroke.color
    } else {
        v.weak_text_color()
    };
    icons::paint(
        ui,
        icon,
        Rect::from_center_size(rect.center(), Vec2::splat(16.0)),
        color,
    );
    resp.on_hover_text(tip)
}

// ---------------------------------------------------------------------- historial

fn run_history(om: &mut OpenMap, ctx: &Context, redo: bool) {
    if om.stroking {
        return;
    }
    let ok = if redo {
        om.hist.redo(&mut om.doc, &mut om.terrain)
    } else {
        om.hist.undo(&mut om.doc, &mut om.terrain)
    };
    if ok {
        om.touched = false;
        om.draft.clear();
        om.zone_draft.clear();
        om.region_draft.clear();
        om.group_drag = None;
        om.marquee = None;
        let existing: Vec<u64> = om
            .multi
            .iter()
            .copied()
            .filter(|id| om.doc.assets.iter().any(|a| a.id == *id))
            .collect();
        om.multi = existing;
        if om.multi.len() < 2 {
            let one = om.multi.pop();
            om.selected = om.selected.or(one);
        }
        if om.sel_region.is_some_and(|id| om.doc.region(id).is_none()) {
            om.sel_region = None;
        }
        let sel = om.selected;
        if sel.is_some_and(|id| !om.doc.assets.iter().any(|a| a.id == id)) {
            om.selected = None;
        }
        let sp = om.sel_path;
        if sp.is_some_and(|id| om.doc.path(id).is_none()) {
            om.sel_path = None;
        }
        let sz = om.sel_zone;
        if sz.is_some_and(|id| om.doc.zone(id).is_none()) {
            om.sel_zone = None;
        }
        om.ensure_active();
        om.auto.mark(ctx);
    }
}

// ---------------------------------------------------------------------- barra de estado

fn status_bar(ui: &mut Ui, om: &mut OpenMap) {
    ui.shrink_clip_rect(ui.max_rect());
    let weak = ui.visuals().weak_text_color();
    let ctx = ui.ctx().clone();
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if small_icon(ui, Icon::Undo, tr("Deshacer (Ctrl+Z)"), om.hist.can_undo()).clicked() {
            run_history(om, &ctx, false);
        }
        if small_icon(ui, Icon::Redo, tr("Rehacer (Ctrl+Y)"), om.hist.can_redo()).clicked() {
            run_history(om, &ctx, true);
        }
        ui.add_space(8.0);

        let pct = (om.cam.zoom * 100.0).round() as u32;
        ComboBox::from_id_salt("map_zoom")
            .width(72.0)
            .selected_text(format!("{pct}%"))
            .show_ui(ui, |ui| {
                for z in ZOOM_STEPS {
                    if ui.selectable_label(pct == z, format!("{z}%")).clicked() {
                        om.cam.set_zoom(z as f32 / 100.0);
                    }
                }
            });
        if ui
            .button(tr("Ajustar"))
            .on_hover_text(tr("Centrar y encuadrar el mapa completo"))
            .clicked()
        {
            om.fit = true;
        }
        ui.add_space(10.0);
        let coords = match om.cursor {
            Some(p) => format!("X: {:.0}   Y: {:.0}", p.x, p.y),
            None => "X: —   Y: —".to_owned(),
        };
        ui.label(RichText::new(coords).monospace().size(12.5).color(weak));

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let (txt, err) = om.auto.status();
            ui.label(
                RichText::new(txt)
                    .size(12.5)
                    .color(if err { ERROR_COLOR } else { weak }),
            );
            ui.add_space(8.0);
            if let Some(s) = &om.status {
                ui.label(RichText::new(s).size(12.5).color(weak));
            }
            ui.label(
                RichText::new(trf(
                    "{} · Capa: {}",
                    &[
                        &om.doc.name,
                        &om.doc.layer(om.active).map_or("—", |l| l.name.as_str()),
                    ],
                ))
                .size(12.5)
                .color(weak),
            );
        });
    });
}

// ---------------------------------------------------------------------- barra superior

fn fmt_hour(v: f64) -> String {
    let total = (v * 60.0).round() as u32;
    format!("{:02}:{:02}", total / 60, total % 60)
}

/// Izquierda: volver, ruta de mapas anidados y nombre. Derecha: hora, sombras, texturas y exportar.
/// Devuelve `true` si se pidió volver a la lista de mapas.
fn top_bar(ui: &mut Ui, om: &mut OpenMap, trail: &[(PathBuf, String)]) -> bool {
    // Los paneles hijos heredan el recorte de toda la pantalla: sin esto, lo que no cabe
    // desborda sobre el panel derecho.
    ui.shrink_clip_rect(ui.max_rect());
    let narrow = ui.available_width() < 900.0;
    let weak = ui.visuals().weak_text_color();
    let ctx = ui.ctx().clone();
    let mut close = false;
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if small_icon(ui, Icon::ArrowLeft, tr("Volver a Mapas"), true).clicked() {
            close = true;
        }
        for (i, (_, name)) in trail.iter().enumerate() {
            let b = Button::new(RichText::new(name).size(13.5).color(weak)).frame(false);
            if ui
                .add(b)
                .on_hover_text(tr("Volver a este mapa"))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                om.nav = Some(Nav::Goto(i));
            }
            ui.label(RichText::new("›").size(14.0).color(weak));
        }
        ui.label(
            RichText::new(&om.doc.name)
                .family(theme::serif())
                .size(16.0),
        );

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let mut changed = false;
            if ui
                .button(tr("Exportar PNG"))
                .on_hover_text(tr("Guarda el mapa completo como imagen"))
                .clicked()
            {
                om.want_export = true;
            }
            changed |= ui
                .checkbox(&mut om.doc.textures, tr("Texturas"))
                .on_hover_text(tr("Activa o desactiva la textura base del terreno y los PNG de caminos y murallas"))
                .changed();
            changed |= ui.checkbox(&mut om.doc.shadows, tr("Sombras")).changed();
            ui.spacing_mut().slider_width = if narrow { 90.0 } else { 180.0 };
            let mut h = om.doc.hour;
            let resp = ui
                .add(
                    egui::Slider::new(&mut h, 0.0..=24.0)
                        .step_by(0.25)
                        .custom_formatter(|v, _| fmt_hour(v)),
                )
                .on_hover_text(tr("Hora del día: sombras (06:00–19:00) y noche (19:30–05:30)"));
            if resp.changed() {
                om.doc.hour = h;
                changed = true;
            }
            let (r, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
            icons::paint(ui, Icon::Sun, r, weak);
            if changed {
                om.auto.mark(&ctx);
            }
        });
    });
    close
}

// ---------------------------------------------------------------------- panel derecho

fn section_title(ui: &mut Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text).family(theme::serif()).size(19.0));
    ui.add_space(2.0);
    ui.separator();
    ui.add_space(4.0);
}

fn side_panel(
    ui: &mut Ui,
    om: &mut OpenMap,
    lib: &AssetLibrary,
    side_tab: &mut SideTab,
    tex: &mut TextureCache,
) -> Option<LibAction> {
    ui.shrink_clip_rect(ui.max_rect());
    // Ancho fijo del contenido: nada del panel puede crecer hacia el lienzo.
    let w = ui.available_width();
    ui.set_min_width(w);
    ui.set_max_width(w);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let bw = ((w - 8.0) / 3.0).floor();
        for (t, label) in [
            (SideTab::Mapa, tr("Mapa")),
            (SideTab::Herramienta, tr("Herramienta")),
            (SideTab::Assets, tr("Assets")),
        ] {
            let b = Button::new(RichText::new(label).size(13.0))
                .selected(*side_tab == t)
                .min_size(vec2(bw, 28.0));
            if ui.add_sized([bw, 28.0], b).clicked() {
                *side_tab = t;
            }
        }
    });
    ui.add_space(4.0);

    let mut action = None;
    ScrollArea::vertical()
        .id_salt(("map_side_scroll", *side_tab as u8))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_max_width(ui.available_width());
            ui.spacing_mut().slider_width = (ui.available_width() - 130.0).clamp(60.0, 140.0);
            match *side_tab {
                SideTab::Mapa => {
                    section_title(ui, tr("Mapa"));
                    map_tab(ui, om);
                    ui.add_space(10.0);
                    section_title(ui, tr("Capas"));
                    layers(ui, om);
                }
                SideTab::Herramienta => {
                    section_title(ui, tr("Herramienta"));
                    tool_tab(ui, om, lib);
                }
                SideTab::Assets => {
                    section_title(ui, tr("Biblioteca"));
                    action = library(ui, om, lib, tex);
                    ui.add_space(10.0);
                    section_title(ui, tr("Zonas"));
                    zones_tab(ui, om);
                }
            }
        });
    action
}

enum LinkAction {
    Enter,
    Create,
}

/// Selector de mapa interior vinculado con «Entrar» y «Crear sub-mapa».
fn link_section(
    ui: &mut Ui,
    others: &[(String, String)],
    link: &mut Option<String>,
    copy_context: &mut bool,
) -> (bool, Option<LinkAction>) {
    let mut changed = false;
    let mut act = None;
    ui.add_space(6.0);
    ui.label(RichText::new(tr("Mapa interior vinculado")).size(13.0));
    let current = match link.as_deref() {
        Some(f) => others
            .iter()
            .find(|(file, _)| file == f)
            .map(|(_, n)| n.clone())
            .unwrap_or_else(|| trf("{} (no encontrado)", &[&f])),
        None => tr("Sin enlace").to_owned(),
    };
    ComboBox::from_id_salt("map_link")
        .width((ui.available_width() - 8.0).max(80.0))
        .selected_text(current)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(link.is_none(), tr("Sin enlace"))
                .clicked()
            {
                *link = None;
                changed = true;
            }
            for (file, name) in others {
                let sel = link.as_deref() == Some(file.as_str());
                if ui.selectable_label(sel, name).clicked() {
                    *link = Some(file.clone());
                    changed = true;
                }
            }
        });
    ui.checkbox(copy_context, tr("Copiar contexto cercano al crear"))
        .on_hover_text(tr("El sub-mapa recibe, como referencia bloqueada, los ríos, caminos y assets del mapa general que están alrededor"));
    ui.horizontal_wrapped(|ui| {
        if link.is_some() && ui.button(tr("Entrar al mapa interior")).clicked() {
            act = Some(LinkAction::Enter);
        }
        if ui.button(tr("Crear sub-mapa interior")).clicked() {
            act = Some(LinkAction::Create);
        }
    });
    (changed, act)
}

/// Crea un mapa en blanco con el estilo del actual y devuelve su nombre de archivo.
fn create_submap(om: &mut OpenMap, name: &str, region: Rect) -> Result<String, String> {
    let dir = document::maps_dir(&om.root);
    let name = if name.trim().is_empty() {
        tr("Mapa interior")
    } else {
        name.trim()
    };
    let mut n = 1;
    let (display, file) = loop {
        let display = if n == 1 {
            name.to_owned()
        } else {
            format!("{name} ({n})")
        };
        let file = format!("{}{MAP_EXT}", story::sanitize_folder_name(&display));
        if !dir.join(&file).exists() {
            break (display, file);
        }
        n += 1;
    };
    let mut sub = MapDoc::new(&display, om.doc.style, Some(document::DEFAULT_WORLD));
    let (paths_n, assets_n) = if om.submap_context && region.width() > 1.0 {
        sub.import_context(&om.doc, region)
    } else {
        (0, 0)
    };
    sub.save(&dir.join(&file))?;
    om.others.push((file.clone(), display));
    om.status = Some(if paths_n + assets_n > 0 {
        format!(
            "Sub-mapa creado con {paths_n} trazos y {assets_n} assets de referencia (capa bloqueada)."
        )
    } else {
        tr("Sub-mapa creado; usa «Entrar al mapa interior».").to_owned()
    });
    Ok(file)
}

fn layer_combo(ui: &mut Ui, opts: &[(u64, String)], cur: &mut u64) -> bool {
    let mut changed = false;
    let text = opts
        .iter()
        .find(|(id, _)| id == cur)
        .map_or_else(|| "—".to_owned(), |(_, n)| n.clone());
    ui.horizontal(|ui| {
        ui.label(tr("Capa"));
        ComboBox::from_id_salt("obj_layer")
            .selected_text(text)
            .show_ui(ui, |ui| {
                for (id, n) in opts {
                    if ui.selectable_label(*cur == *id, n).clicked() {
                        *cur = *id;
                        changed = true;
                    }
                }
            });
    });
    changed
}

fn zone_rect(z: &Zone) -> Rect {
    Rect::from_points(&z.pts())
}

/// Pestaña «Mapa»: estilo, dimensiones y cuadrícula.
fn map_tab(ui: &mut Ui, om: &mut OpenMap) {
    let weak = ui.visuals().weak_text_color();
    let ctx = ui.ctx().clone();
    let mut changed = false;

    ui.label(RichText::new(tr("Estilo")).size(13.5));
    ui.horizontal_wrapped(|ui| {
        for st in MapStyle::ALL {
            changed |= ui
                .selectable_value(&mut om.doc.style, st, st.short())
                .changed();
        }
    });
    let dims = match om.doc.size {
        Some([w, h]) => format!("{}×{} px", w as u32, h as u32),
        None => tr("Lienzo infinito").to_owned(),
    };
    ui.label(
        RichText::new(trf("Dimensiones: {}", &[&dims]))
            .size(12.5)
            .color(weak),
    );
    ui.add_space(8.0);

    ui.label(RichText::new(tr("Cuadrícula")).size(13.5));
    let g: &mut GridSettings = &mut om.doc.grid;
    changed |= ui
        .checkbox(&mut g.enabled, tr("Mostrar cuadrícula"))
        .changed();
    ui.horizontal_wrapped(|ui| {
        changed |= ui
            .selectable_value(&mut g.kind, GridKind::Square, tr("Cuadrada"))
            .changed();
        changed |= ui
            .selectable_value(&mut g.kind, GridKind::Hex, tr("Hexagonal"))
            .changed();
    });
    changed |= ui
        .add(
            egui::Slider::new(&mut g.size, 10.0..=100.0)
                .text(tr("Tamaño"))
                .suffix(" px"),
        )
        .changed();
    changed |= ui
        .add(egui::Slider::new(&mut g.opacity, 0.05..=1.0).text(tr("Opacidad")))
        .changed();
    if changed {
        om.touch(&ctx);
    }
}

/// Pestaña «Herramienta»: solo los ajustes de la herramienta activa o de la selección.
fn tool_tab(ui: &mut Ui, om: &mut OpenMap, lib: &AssetLibrary) {
    let weak = ui.visuals().weak_text_color();
    let ctx = ui.ctx().clone();
    let mut changed = false;

    if om.selected.is_none()
        && om.sel_path.is_none()
        && om.sel_zone.is_none()
        && om.sel_region.is_none()
        && om.multi.is_empty()
        && matches!(om.tool, Tool::Select | Tool::Pan)
    {
        ui.label(
            RichText::new(
                tr("Selecciona un asset, trazo, zona o región del mapa, o elige una herramienta de la barra izquierda. Ctrl + clic suma o quita assets; arrastra sobre un espacio vacío (o Mayús + arrastrar) para seleccionar por recuadro."),
            )
            .size(12.5)
            .color(weak),
        );
    }

    // Ajustes de la herramienta activa.
    match om.tool {
        Tool::Scatter => {
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(4.0);
            ui.label(RichText::new(tr("Dispersión")).size(13.5));
            ui.add(
                egui::Slider::new(&mut om.scatter.radius, 10.0..=400.0)
                    .text(tr("Radio"))
                    .suffix(" px"),
            );
            ui.add(
                egui::Slider::new(&mut om.scatter.spacing, 6.0..=200.0)
                    .text(tr("Separación"))
                    .suffix(" px"),
            );
            ui.label(
                RichText::new(
                    tr("Mantén pulsado y arrastra. Rotación 0–360° y escala ×0.85–1.15 al azar. Con varios assets elegidos se mezclan."),
                )
                .size(12.0)
                .color(weak),
            );
        }
        Tool::Brush | Tool::Eraser | Tool::Fill => colors_tab(ui, om),
        Tool::Region if om.sel_region.is_none() => {
            ui.label(RichText::new(tr("Región vectorial")).size(13.5));
            ui.label(
                RichText::new(
                    tr("Clic para añadir vértices; doble clic, clic derecho o Enter cierran la región. Color sólido y borde nítido a cualquier zoom; edita los vértices con Seleccionar."),
                )
                .size(12.0)
                .color(weak),
            );
        }
        Tool::Place => {
            ui.label(
                RichText::new(
                    tr("Elige uno o varios assets en la pestaña Assets y haz clic en el mapa para estamparlos."),
                )
                .size(12.0)
                .color(weak),
            );
        }
        Tool::Path if om.sel_path.is_none() => {
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(4.0);
            if om.path_cfg.kind == PathKind::Muralla {
                ui.label(RichText::new(tr("Nueva muralla")).size(13.5));
            } else {
                ui.label(RichText::new(tr("Nuevo camino / río")).size(13.5));
                let prev = om.path_cfg.kind;
                ui.horizontal_wrapped(|ui| {
                    for k in [PathKind::Camino, PathKind::Rio] {
                        ui.selectable_value(&mut om.path_cfg.kind, k, k.label());
                    }
                });
                if om.path_cfg.kind != prev {
                    om.path_cfg.width = om.path_cfg.kind.default_width();
                }
            }
            ui.add(
                egui::Slider::new(&mut om.path_cfg.width, 2.0..=40.0)
                    .text(tr("Ancho"))
                    .suffix(" px"),
            );
            if om.path_cfg.kind != PathKind::Muralla {
                ui.checkbox(&mut om.path_cfg.dashed, tr("Línea discontinua (sendero)"));
            }
            {
                let c = &mut om.path_cfg;
                path_style_controls(
                    ui,
                    lib,
                    c.kind,
                    &mut c.material,
                    &mut c.tower_style,
                    &mut c.tower_every,
                    &mut c.sprite,
                    weak,
                );
            }
            ui.add_space(2.0);
            ui.label(RichText::new(tr("Modo")).size(12.5).color(weak));
            ui.horizontal_wrapped(|ui| {
                for m in DrawMode::ALL {
                    if om.path_cfg.kind == PathKind::Muralla && m != DrawMode::Nodes {
                        continue;
                    }
                    ui.selectable_value(&mut om.path_cfg.mode, m, m.label());
                }
            });
            let help = match om.path_cfg.mode {
                DrawMode::Nodes => {
                    tr("Clic añade nodos · Enter o doble clic finaliza · Retroceso quita el último · Esc cancela.")
                }
                DrawMode::Freehand => {
                    tr("Mantén presionado y arrastra para dibujar; al soltar se limpia el trazo.")
                }
            };
            ui.label(RichText::new(help).size(12.0).color(weak));
            ui.label(
                RichText::new(
                    tr("Acércate a otro trazo para anclarte a él y crear bifurcaciones (aro de conexión)."),
                )
                .size(12.0)
                .color(weak),
            );
        }
        Tool::Zone if om.sel_zone.is_none() => {
            ui.add_space(8.0);
            ui.label(
                RichText::new(
                    tr("Clic añade vértices · clic derecho, Enter o doble clic cierra la zona · Retroceso quita el último · Esc cancela."),
                )
                .size(12.0)
                .color(weak),
            );
        }
        _ => {}
    }

    let obj_layers: Vec<(u64, String)> = om
        .doc
        .layers
        .iter()
        .filter(|l| !l.terrain)
        .map(|l| (l.id, l.name.clone()))
        .collect();

    // Objeto seleccionado: asset.
    if let Some(id) = om.selected {
        ui.add_space(10.0);
        ui.separator();
        ui.add_space(4.0);
        let (mut delete, mut front, mut back, mut dup) = (false, false, false, false);
        let mut link_act = None;
        let mut sub_name = String::new();
        let mut sub_region = Rect::ZERO;
        if let Some(a) = om.doc.asset_mut(id) {
            let name = lib
                .get(&a.asset)
                .map(|i| i.name.clone())
                .unwrap_or_else(|| a.asset.clone());
            ui.label(RichText::new(tr("Objeto seleccionado")).size(13.5));
            ui.label(
                RichText::new(format!("{name} · orden z {}", a.z))
                    .size(12.5)
                    .color(weak),
            );
            changed |= ui
                .add(
                    egui::Slider::new(&mut a.rotation, 0.0..=360.0)
                        .text(tr("Rotación"))
                        .suffix("°"),
                )
                .changed();
            changed |= ui
                .add(
                    egui::Slider::new(&mut a.relief, 0.0..=1.0)
                        .text(tr("Relieve"))
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
                )
                .on_hover_text(tr("Altura (montañas, colinas): sombra más larga según la hora. Requiere Sombras activas."))
                .changed();
            let mut pct = a.scale * 100.0;
            if ui
                .add(
                    egui::Slider::new(&mut pct, 5.0..=500.0)
                        .text(tr("Escala"))
                        .suffix("%")
                        .logarithmic(true),
                )
                .changed()
            {
                a.scale = pct / 100.0;
                changed = true;
            }
            ui.horizontal_wrapped(|ui| {
                if ui.button(tr("Duplicar")).clicked() {
                    dup = true;
                }
                if ui.button(tr("Rotar +45°")).clicked() {
                    a.rotation = (a.rotation + 45.0).rem_euclid(360.0);
                    changed = true;
                }
                if ui.button(tr("Borrar")).on_hover_text(tr("Supr")).clicked() {
                    delete = true;
                }
                if ui.button(tr("Girar 15° (R)")).clicked() {
                    a.rotation = (a.rotation + 15.0).rem_euclid(360.0);
                    changed = true;
                }
            });
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button(tr("Traer al frente (RePág)"))
                    .on_hover_text(tr("Sobre el resto de assets de su capa"))
                    .clicked()
                {
                    front = true;
                }
                if ui
                    .button(tr("Enviar atrás (AvPág)"))
                    .on_hover_text(tr("Bajo el resto de assets de su capa"))
                    .clicked()
                {
                    back = true;
                }
            });
            changed |= layer_combo(ui, &obj_layers, &mut a.layer);
            let (c, act) = link_section(ui, &om.others, &mut a.link, &mut om.submap_context);
            changed |= c;
            link_act = act;
            sub_name = name;
            sub_region = Rect::from_center_size(a.pos2(), Vec2::splat(900.0));
        }
        if delete {
            om.doc.remove_asset(id);
            om.selected = None;
            changed = true;
        } else {
            if dup {
                if let Some(n) = om.doc.duplicate_asset(id) {
                    om.select(Some(Hit::Asset(n)));
                    changed = true;
                }
            }
            if front {
                om.doc.bring_front(id);
                changed = true;
            }
            if back {
                om.doc.send_back(id);
                changed = true;
            }
            match link_act {
                Some(LinkAction::Enter) => {
                    let l = om.doc.assets.iter().find(|a| a.id == id);
                    if let Some(l) = l.and_then(|a| a.link.clone()) {
                        om.nav = Some(Nav::Enter(l));
                    }
                }
                Some(LinkAction::Create) => match create_submap(om, &sub_name, sub_region) {
                    Ok(file) => {
                        if let Some(a) = om.doc.asset_mut(id) {
                            a.link = Some(file);
                        }
                        changed = true;
                    }
                    Err(e) => om.status = Some(e),
                },
                None => {}
            }
        }
    }

    // Objeto seleccionado: camino / río.
    if let Some(id) = om.sel_path {
        ui.add_space(10.0);
        ui.separator();
        ui.add_space(4.0);
        let mut delete = false;
        if let Some(p) = om.doc.path_mut(id) {
            ui.label(RichText::new(tr("Trazo seleccionado")).size(13.5));
            ui.horizontal_wrapped(|ui| {
                for k in PathKind::ALL {
                    changed |= ui.selectable_value(&mut p.kind, k, k.label()).changed();
                }
            });
            changed |= ui
                .add(
                    egui::Slider::new(&mut p.width, 2.0..=40.0)
                        .text(tr("Ancho"))
                        .suffix(" px"),
                )
                .changed();
            if p.kind != PathKind::Muralla {
                changed |= ui
                    .checkbox(&mut p.dashed, tr("Línea discontinua"))
                    .changed();
            }
            changed |= path_style_controls(
                ui,
                lib,
                p.kind,
                &mut p.material,
                &mut p.tower_style,
                &mut p.tower_every,
                &mut p.sprite,
                weak,
            );
            changed |= layer_combo(ui, &obj_layers, &mut p.layer);
            ui.label(
                RichText::new(trf(
                    "{} nodos · arrastra un nodo para moverlo · doble clic en un nodo lo quita · doble clic en el trazo añade uno",
                    &[&p.nodes.len()],
                ))
                .size(12.0)
                .color(weak),
            );
            if ui.button(tr("Eliminar trazo (Supr)")).clicked() {
                delete = true;
            }
        }
        if delete {
            om.doc.remove_path(id);
            om.sel_path = None;
            changed = true;
        }
    }

    // Selección múltiple: acciones en lote.
    if om.multi.len() > 1 {
        ui.add_space(10.0);
        ui.separator();
        ui.add_space(4.0);
        ui.label(RichText::new(trf("{} assets seleccionados", &[&om.multi.len()])).size(13.5));
        ui.label(
            RichText::new(tr(
                "Arrastra uno para mover el grupo · Ctrl + clic suma o quita",
            ))
            .size(12.0)
            .color(weak),
        );
        let (mut rot, mut dup, mut del) = (0.0_f32, false, false);
        ui.horizontal_wrapped(|ui| {
            if ui.button("↺ −15°").clicked() {
                rot = -15.0;
            }
            if ui.button("↻ +15°").clicked() {
                rot = 15.0;
            }
            if ui.button("↻ +90°").clicked() {
                rot = 90.0;
            }
            if ui.button(tr("Duplicar")).clicked() {
                dup = true;
            }
            if ui.button(tr("Eliminar (Supr)")).clicked() {
                del = true;
            }
        });
        if rot != 0.0 {
            rotate_group(om, rot);
            changed = true;
        }
        if dup {
            duplicate_group(om);
            changed = true;
        }
        if del {
            for id in std::mem::take(&mut om.multi) {
                om.doc.remove_asset(id);
            }
            changed = true;
        }
    }

    // Objeto seleccionado: región vectorial.
    if let Some(id) = om.sel_region {
        ui.add_space(10.0);
        ui.separator();
        ui.add_space(4.0);
        let (mut delete, mut center) = (false, false);
        let mut focus = Rect::ZERO;
        if let Some(r) = om.doc.region_mut(id) {
            ui.label(RichText::new(tr("Región seleccionada")).size(13.5));
            changed |= ui
                .add(TextEdit::singleline(&mut r.name).desired_width(f32::INFINITY))
                .changed();
            ui.horizontal(|ui| {
                ui.label(tr("Relleno"));
                changed |= ui.color_edit_button_srgb(&mut r.color).changed();
            });
            changed |= ui.checkbox(&mut r.border, tr("Borde nítido")).changed();
            if r.border {
                ui.horizontal(|ui| {
                    ui.label(tr("Color del borde"));
                    changed |= ui.color_edit_button_srgb(&mut r.border_color).changed();
                });
                changed |= ui
                    .add(
                        egui::Slider::new(&mut r.border_width, 0.5..=20.0)
                            .text(tr("Grosor"))
                            .suffix(" px"),
                    )
                    .changed();
            }
            ui.label(
                RichText::new(trf(
                    "{} vértices · arrastra un vértice · doble clic en un vértice lo quita · doble clic en el borde añade uno",
                    &[&r.points.len()],
                ))
                .size(12.0)
                .color(weak),
            );
            focus = Rect::from_points(&r.pts());
            ui.horizontal(|ui| {
                if ui.button(tr("Centrar")).clicked() {
                    center = true;
                }
                if ui.button(tr("Eliminar región (Supr)")).clicked() {
                    delete = true;
                }
            });
        }
        if center {
            om.focus_req = Some(focus);
        }
        if delete {
            om.doc.remove_region(id);
            om.sel_region = None;
            changed = true;
        }
    }

    // Objeto seleccionado: zona.
    if let Some(id) = om.sel_zone {
        ui.add_space(10.0);
        ui.separator();
        ui.add_space(4.0);
        let (mut delete, mut center) = (false, false);
        let mut link_act = None;
        let mut sub_name = String::new();
        let mut sub_region = Rect::ZERO;
        if let Some(z) = om.doc.zone_mut(id) {
            ui.label(RichText::new(tr("Zona seleccionada")).size(13.5));
            changed |= ui
                .add(TextEdit::singleline(&mut z.name).desired_width(f32::INFINITY))
                .changed();
            ui.horizontal(|ui| {
                ui.label(tr("Color"));
                changed |= ui.color_edit_button_srgb(&mut z.color).changed();
            });
            changed |= ui
                .add(
                    egui::Slider::new(&mut z.relief, 0.0..=1.0)
                        .text(tr("Relieve"))
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
                )
                .on_hover_text(tr("Altura de la zona (colina, meseta): proyecta sombra según la hora. Requiere Sombras activas."))
                .changed();
            ui.label(
                RichText::new(trf(
                    "{} vértices · arrastra un vértice · doble clic en un vértice lo quita · doble clic en el borde añade uno",
                    &[&z.points.len()],
                ))
                .size(12.0)
                .color(weak),
            );
            let (c, act) = link_section(ui, &om.others, &mut z.link, &mut om.submap_context);
            changed |= c;
            link_act = act;
            sub_name = z.name.clone();
            sub_region = zone_rect(z).expand(zone_rect(z).size().max_elem() * 0.1);
            ui.horizontal(|ui| {
                if ui.button(tr("Centrar")).clicked() {
                    center = true;
                }
                if ui.button(tr("Eliminar zona (Supr)")).clicked() {
                    delete = true;
                }
            });
        }
        if center {
            om.focus_req = om.doc.zone(id).map(zone_rect);
        }
        if delete {
            om.doc.remove_zone(id);
            om.sel_zone = None;
            changed = true;
        } else {
            match link_act {
                Some(LinkAction::Enter) => {
                    if let Some(l) = om.doc.zone(id).and_then(|z| z.link.clone()) {
                        om.nav = Some(Nav::Enter(l));
                    }
                }
                Some(LinkAction::Create) => match create_submap(om, &sub_name, sub_region) {
                    Ok(file) => {
                        if let Some(z) = om.doc.zone_mut(id) {
                            z.link = Some(file);
                        }
                        changed = true;
                    }
                    Err(e) => om.status = Some(e),
                },
                None => {}
            }
        }
    }
    if changed {
        om.touch(&ctx);
    }
}

fn layers(ui: &mut Ui, om: &mut OpenMap) {
    let ctx = ui.ctx().clone();
    let weak = ui.visuals().weak_text_color();
    let mut changed = false;

    ui.set_max_width(ui.available_width());
    ui.horizontal_wrapped(|ui| {
        if ui
            .button(tr("+ Nueva capa"))
            .on_hover_text(tr("Crea una capa encima de la activa"))
            .clicked()
        {
            let n = om.doc.layers.iter().filter(|l| !l.terrain).count() + 1;
            let id = om.doc.add_layer(&format!("Capa {n}"), Some(om.active));
            om.active = id;
            changed = true;
        }
        ui.label(
            RichText::new(tr("Doble clic: renombrar"))
                .size(11.5)
                .color(weak),
        );
    });
    ui.add_space(4.0);

    let rows = om.doc.layers.clone();
    for (i, l) in rows.iter().enumerate() {
        let count = om.doc.layer_count(l.id);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let eye = if l.visible { Icon::Eye } else { Icon::EyeOff };
            if small_icon(
                ui,
                eye,
                if l.visible { tr("Ocultar capa") } else { tr("Mostrar capa") },
                l.visible,
            )
            .clicked()
            {
                if let Some(x) = om.doc.layer_mut(l.id) {
                    x.visible = !x.visible;
                    changed = true;
                }
            }
            let lock = if l.locked { Icon::Lock } else { Icon::Unlock };
            if small_icon(
                ui,
                lock,
                if l.locked { tr("Desbloquear capa") } else { tr("Bloquear capa") },
                l.locked,
            )
            .clicked()
            {
                if let Some(x) = om.doc.layer_mut(l.id) {
                    x.locked = !x.locked;
                    changed = true;
                }
            }

            // Quedan 3 iconos de 28 px a la derecha: el nombre usa solo el resto.
            let name_w = (ui.available_width() - 3.0 * 30.0 - 4.0).max(40.0);
            if om.rename.as_ref().is_some_and(|r| r.id == l.id) {
                let esc = ui.input(|i| i.key_pressed(Key::Escape));
                let mut lost = false;
                let mut text = String::new();
                if let Some(r) = om.rename.as_mut() {
                    let resp = ui.add(
                        TextEdit::singleline(&mut r.text)
                            .desired_width(name_w)
                            .margin(vec2(6.0, 4.0)),
                    );
                    if r.fresh {
                        resp.request_focus();
                        r.fresh = false;
                    }
                    lost = resp.lost_focus();
                    text = r.text.trim().to_owned();
                }
                if lost {
                    if !esc && !text.is_empty() {
                        if let Some(x) = om.doc.layer_mut(l.id) {
                            if x.name != text {
                                x.name = text;
                                changed = true;
                            }
                        }
                    }
                    om.rename = None;
                }
            } else {
                let label = if count > 0 {
                    format!("{}  ({count})", l.name)
                } else {
                    l.name.clone()
                };
                let btn = Button::new(label)
                    .selected(om.active == l.id)
                    .truncate();
                let resp = ui
                    .add_sized([name_w, 26.0], btn)
                    .on_hover_text(tr("Clic: capa activa · doble clic: renombrar"));
                if resp.clicked() {
                    om.active = l.id;
                }
                if resp.double_clicked() {
                    om.rename = Some(Rename {
                        id: l.id,
                        text: l.name.clone(),
                        fresh: true,
                    });
                }
            }

            if small_icon(ui, Icon::ArrowUp, tr("Subir capa"), i > 0).clicked()
                && om.doc.move_layer(l.id, true)
            {
                changed = true;
            }
            if small_icon(ui, Icon::ArrowDown, tr("Bajar capa"), i + 1 < rows.len()).clicked()
                && om.doc.move_layer(l.id, false)
            {
                changed = true;
            }
            if small_icon(ui, Icon::Trash, tr("Eliminar capa (su contenido pasa a la capa vecina)"), !l.terrain)
                .clicked()
            {
                if om.doc.remove_layer(l.id) {
                    om.ensure_active();
                    changed = true;
                } else {
                    om.status = Some(
                        tr("No se puede eliminar: la capa de terreno y la última capa de objetos son obligatorias.")
                            .to_owned(),
                    );
                }
            }
        });
    }
    if changed {
        om.touch(&ctx);
    }
}

fn zones_tab(ui: &mut Ui, om: &mut OpenMap) {
    let weak = ui.visuals().weak_text_color();
    let ctx = ui.ctx().clone();
    if ui
        .checkbox(&mut om.doc.zones_visible, tr("Mostrar zonas"))
        .changed()
    {
        om.auto.mark(&ctx);
    }
    if ui
        .add(Button::new(tr("+ Nueva zona (Z)")).selected(om.tool == Tool::Zone))
        .clicked()
    {
        om.tool = Tool::Zone;
    }
    ui.add_space(6.0);
    if om.doc.zones.is_empty() {
        ui.label(
            RichText::new(tr(
                "Todavía no hay zonas. Dibuja reinos, distritos o zonas de peligro.",
            ))
            .size(12.5)
            .color(weak),
        );
        return;
    }
    ui.label(
        RichText::new(tr("Clic en una zona para enfocarla en el mapa."))
            .size(12.0)
            .color(weak),
    );
    let rows: Vec<(u64, String, bool, Rect)> = om
        .doc
        .zones
        .iter()
        .map(|z| (z.id, z.name.clone(), z.link.is_some(), zone_rect(z)))
        .collect();
    for (i, (id, name, linked, bounds)) in rows.into_iter().enumerate() {
        let label = format!(
            "[{:02}] {}{}",
            i + 1,
            name,
            if linked {
                tr("  · con mapa interior")
            } else {
                ""
            }
        );
        let btn = Button::new(label)
            .selected(om.sel_zone == Some(id))
            .min_size(vec2(ui.available_width(), 26.0));
        if ui.add(btn).clicked() {
            om.select(Some(Hit::Zone(id)));
            om.focus_req = Some(bounds);
        }
    }
}

fn library(
    ui: &mut Ui,
    om: &mut OpenMap,
    lib: &AssetLibrary,
    tex: &mut TextureCache,
) -> Option<LibAction> {
    let mut action = None;
    let (weak, accent, card, border) = {
        let v = ui.visuals();
        (
            v.weak_text_color(),
            v.selection.bg_fill,
            v.extreme_bg_color,
            v.window_stroke.color,
        )
    };
    let multi = ui.input(|i| i.modifiers.command || i.modifiers.shift);

    ui.horizontal_wrapped(|ui| {
        if ui.button(tr("+ Importar PNG")).clicked() {
            action = Some(LibAction::ImportFiles);
        }
        if ui.button(tr("+ Importar carpeta de assets")).clicked() {
            action = Some(LibAction::ImportFolder);
        }
    });
    ui.add_space(6.0);

    let brush_name = match om.brushes.len() {
        0 => "ninguno".to_owned(),
        1 => lib
            .get(&om.brushes[0])
            .map(|a| a.name.clone())
            .unwrap_or_else(|| "?".to_owned()),
        n => trf("{} assets (mezcla aleatoria al dispersar)", &[&n]),
    };
    ui.label(
        RichText::new(trf(
            "Pincel: {} · clic estampa · Ctrl/Mayús+clic elige varios",
            &[&brush_name],
        ))
        .size(12.0)
        .color(weak),
    );
    ui.add_space(6.0);

    if lib.items.is_empty() {
        ui.label(
            RichText::new(
                tr("No hay assets. Copia tus PNG en assets/map_defaults/ (junto al proyecto) o importa los tuyos con los botones de arriba."),
            )
            .size(12.5)
            .color(weak),
        );
        return action;
    }

    for (cat, items) in lib.categories() {
        egui::CollapsingHeader::new(RichText::new(format!("{cat} ({})", items.len())).size(14.0))
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::splat(6.0);
                    for a in items {
                        let (rect, resp) =
                            ui.allocate_exact_size(Vec2::splat(64.0), Sense::click());
                        let selected = om.brushes.contains(&a.key);
                        let r = CornerRadius::same(6);
                        ui.painter().rect_filled(rect, r, card);
                        if ui.is_rect_visible(rect) {
                            tex.paint(ui, &a.path, rect.shrink(5.0), CornerRadius::ZERO);
                        }
                        let stroke = if selected {
                            Stroke::new(2.0, accent)
                        } else if resp.hovered() {
                            Stroke::new(1.0, accent)
                        } else {
                            Stroke::new(1.0, border)
                        };
                        ui.painter()
                            .rect_stroke(rect, r, stroke, StrokeKind::Inside);
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        let tip = format!(
                            "{} ({})",
                            a.name,
                            if a.global { "global" } else { "historia" }
                        );
                        if resp.on_hover_text(tip).clicked() {
                            if multi {
                                if let Some(pos) = om.brushes.iter().position(|k| *k == a.key) {
                                    om.brushes.remove(pos);
                                } else {
                                    om.brushes.push(a.key.clone());
                                }
                            } else {
                                om.brushes = vec![a.key.clone()];
                            }
                            if !matches!(om.tool, Tool::Place | Tool::Scatter) {
                                om.tool = Tool::Place;
                            }
                        }
                    }
                });
            });
    }
    action
}

fn colors_tab(ui: &mut Ui, om: &mut OpenMap) {
    let (weak, accent, border) = {
        let v = ui.visuals();
        (
            v.weak_text_color(),
            v.selection.bg_fill,
            v.window_stroke.color,
        )
    };
    let mut picked: Option<Color32> = None;

    let swatch = |ui: &mut Ui, c: Color32, name: &str, current: Color32| -> bool {
        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(34.0), Sense::click());
        ui.painter().rect_filled(rect, CornerRadius::same(6), c);
        let stroke = if current == c {
            Stroke::new(2.0, accent)
        } else {
            Stroke::new(1.0, border)
        };
        ui.painter()
            .rect_stroke(rect, CornerRadius::same(6), stroke, StrokeKind::Inside);
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        resp.on_hover_text(name).clicked()
    };

    ui.label(RichText::new(tr("Terreno")).size(13.5));
    ui.add_space(2.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::splat(6.0);
        for (name, c) in TERRAIN_PRESETS {
            if swatch(ui, c, crate::i18n::tr_dyn(name).as_str(), om.brush.color) {
                picked = Some(c);
            }
        }
    });
    ui.add_space(4.0);
    ui.label(
        RichText::new(trf("Paleta del estilo {}", &[&om.doc.style.short()]))
            .size(12.0)
            .color(weak),
    );
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::splat(6.0);
        for (name, c) in [
            (tr("Fondo"), om.doc.style.background()),
            (tr("Tierra"), om.doc.style.land()),
            (tr("Agua"), om.doc.style.water()),
            (tr("Costa / tinta"), om.doc.style.coast()),
        ] {
            if swatch(ui, c, name, om.brush.color) {
                picked = Some(c);
            }
        }
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(tr("Color libre"));
        let mut c = om.brush.color;
        if ui.color_edit_button_srgba(&mut c).changed() {
            picked = Some(Color32::from_rgb(c.r(), c.g(), c.b()));
        }
    });
    if let Some(c) = picked {
        om.brush.color = c;
        if !matches!(om.tool, Tool::Brush | Tool::Eraser | Tool::Fill) {
            om.tool = Tool::Brush;
            om.active = om.doc.terrain_layer();
        }
    }

    ui.add_space(10.0);
    ui.label(RichText::new(tr("Pincel")).size(13.5));
    ui.add(
        egui::Slider::new(&mut om.brush.size, 2.0..=400.0)
            .text(tr("Tamaño"))
            .suffix(" px")
            .logarithmic(true),
    );
    ui.add(
        egui::Slider::new(&mut om.brush.opacity, 0.05..=1.0)
            .text(tr("Opacidad"))
            .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
    );
    ui.add(
        egui::Slider::new(&mut om.brush.hardness, 0.0..=1.0)
            .text(tr("Dureza"))
            .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
    );
    ui.horizontal(|ui| {
        if ui
            .add(Button::new(tr("Pincel (B)")).selected(om.tool == Tool::Brush))
            .clicked()
        {
            om.tool = Tool::Brush;
            om.active = om.doc.terrain_layer();
        }
        if ui
            .add(Button::new(tr("Borrador (E)")).selected(om.tool == Tool::Eraser))
            .clicked()
        {
            om.tool = Tool::Eraser;
            om.active = om.doc.terrain_layer();
        }
        if ui
            .add(Button::new(tr("Relleno (G)")).selected(om.tool == Tool::Fill))
            .on_hover_text(tr(
                "Colorea un área cerrada (o el fondo) con el color elegido",
            ))
            .clicked()
        {
            om.tool = Tool::Fill;
            om.active = om.doc.terrain_layer();
        }
    });
    ui.add_space(4.0);
    ui.label(
        RichText::new(tr(
            "El borrador revela el fondo base (agua o papel) para esculpir costas e islas.",
        ))
        .size(12.0)
        .color(weak),
    );
}

// ---------------------------------------------------------------------- lienzo

/// Tamaño natural del sprite de un asset (64×64 si falta o no carga).
fn sprite_size(lib: &AssetLibrary, tex: &mut TextureCache, ctx: &Context, key: &str) -> Vec2 {
    lib.get(key)
        .and_then(|a| tex.get(ctx, &a.path).ok().map(|t| t.size_vec2()))
        .unwrap_or(vec2(64.0, 64.0))
}

/// Orden z inicial: los árboles quedan sobre arbustos y rocas dentro de la misma capa.
fn default_z(lib: &AssetLibrary, key: &str) -> i32 {
    match lib.get(key).map(|a| a.category.as_str()) {
        Some("Árboles") => 1,
        _ => 0,
    }
}

fn hit_test(
    om: &OpenMap,
    lib: &AssetLibrary,
    tex: &mut TextureCache,
    ctx: &Context,
    p: Pos2,
) -> Option<Hit> {
    for layer in &om.doc.layers {
        if layer.terrain || !layer.visible || layer.locked {
            continue;
        }
        for a in om.doc.sorted_assets(layer.id).into_iter().rev() {
            if a.contains(p, sprite_size(lib, tex, ctx, &a.asset)) {
                return Some(Hit::Asset(a.id));
            }
        }
        for path in om.doc.paths.iter().rev().filter(|x| x.layer == layer.id) {
            let tol = path.width / 2.0 + 5.0 / om.cam.zoom;
            if paths::nearest(path.kind, &path.points(), p).is_some_and(|n| n.dist <= tol) {
                return Some(Hit::Path(path.id));
            }
        }
    }
    if om.doc.zones_visible {
        for z in om.doc.zones.iter().rev() {
            if zones::contains(&z.pts(), p) {
                return Some(Hit::Zone(z.id));
            }
        }
    }
    for r in om.doc.regions.iter().rev() {
        if zones::contains(&r.pts(), p) {
            return Some(Hit::Region(r.id));
        }
    }
    None
}

/// Assets visibles y desbloqueados cuyo centro cae dentro del rectángulo (mundo).
fn assets_in_rect(om: &OpenMap, r: Rect) -> Vec<u64> {
    om.doc
        .assets
        .iter()
        .filter(|a| {
            om.doc
                .layer(a.layer)
                .is_some_and(|l| !l.terrain && l.visible && !l.locked)
                && r.contains(a.pos2())
        })
        .map(|a| a.id)
        .collect()
}

/// Rota el grupo seleccionado alrededor de su centro y suma el ángulo a cada asset.
fn rotate_group(om: &mut OpenMap, deg: f32) {
    let ids = om.asset_set();
    let pts: Vec<Pos2> = om
        .doc
        .assets
        .iter()
        .filter(|a| ids.contains(&a.id))
        .map(|a| a.pos2())
        .collect();
    if pts.is_empty() {
        return;
    }
    let c = zones::centroid(&pts);
    let (s, co) = deg.to_radians().sin_cos();
    for a in om.doc.assets.iter_mut().filter(|a| ids.contains(&a.id)) {
        let d = a.pos2() - c;
        let n = c + vec2(d.x * co - d.y * s, d.x * s + d.y * co);
        a.pos = [n.x, n.y];
        a.rotation = (a.rotation + deg).rem_euclid(360.0);
    }
}

/// Duplica el grupo y selecciona las copias.
fn duplicate_group(om: &mut OpenMap) {
    let ids = om.asset_set();
    let copies: Vec<u64> = ids
        .iter()
        .filter_map(|id| om.doc.duplicate_asset(*id))
        .collect();
    om.set_assets(copies);
}

/// Nodo del camino seleccionado bajo el punto de pantalla `s`.
fn node_hit(om: &OpenMap, view: Rect, s: Pos2) -> Option<(u64, usize)> {
    let id = om.sel_path?;
    let path = om.doc.path(id)?;
    path.nodes
        .iter()
        .position(|n| om.cam.world_to_screen(view, pos2(n[0], n[1])).distance(s) <= NODE_HIT)
        .map(|i| (id, i))
}

/// Vértice de la zona seleccionada bajo el punto de pantalla `s`.
fn zone_vertex_hit(om: &OpenMap, view: Rect, s: Pos2) -> Option<(u64, usize)> {
    let id = om.sel_zone?;
    let zone = om.doc.zone(id)?;
    zone.points
        .iter()
        .position(|n| om.cam.world_to_screen(view, pos2(n[0], n[1])).distance(s) <= NODE_HIT)
        .map(|i| (id, i))
}

/// Vértice de la región seleccionada bajo el punto de pantalla `s`.
fn region_vertex_hit(om: &OpenMap, view: Rect, s: Pos2) -> Option<(u64, usize)> {
    let id = om.sel_region?;
    let reg = om.doc.region(id)?;
    reg.points
        .iter()
        .position(|n| om.cam.world_to_screen(view, pos2(n[0], n[1])).distance(s) <= NODE_HIT)
        .map(|i| (id, i))
}

fn finish_region(om: &mut OpenMap, ctx: &Context) {
    if om.region_draft.len() >= 3 {
        let pts = std::mem::take(&mut om.region_draft);
        let n = om.doc.regions.len() + 1;
        let w = om.doc.style.water();
        let id = om
            .doc
            .add_region(&format!("Región {n}"), &pts, [w.r(), w.g(), w.b()]);
        om.select(Some(Hit::Region(id)));
        om.touch(ctx);
    } else {
        om.region_draft.clear();
    }
}

/// Selector del PNG con el que se texturiza un camino de piedra o una muralla.
fn sprite_combo(
    ui: &mut Ui,
    lib: &AssetLibrary,
    words: &[&str],
    sprite: &mut Option<String>,
    weak: Color32,
) -> bool {
    let mut changed = false;
    let cands: Vec<&assets::AssetInfo> = lib
        .items
        .iter()
        .filter(|i| {
            let n = format!("{} {}", i.name, i.key).to_lowercase();
            words.iter().any(|w| n.contains(w)) && !n.contains("tower") && !n.contains("torre")
        })
        .collect();
    ui.add_space(2.0);
    ui.label(RichText::new(tr("Textura PNG")).size(12.5).color(weak));
    let cur = sprite
        .as_deref()
        .and_then(|k| lib.get(k))
        .map_or_else(|| tr("Vectorial (sin PNG)").to_owned(), |i| i.name.clone());
    ComboBox::from_id_salt("path_sprite")
        .width((ui.available_width() - 8.0).max(80.0))
        .selected_text(cur)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(sprite.is_none(), tr("Vectorial (sin PNG)"))
                .clicked()
            {
                *sprite = None;
                changed = true;
            }
            for i in &cands {
                let sel = sprite.as_deref() == Some(i.key.as_str());
                if ui.selectable_label(sel, &i.name).clicked() {
                    *sprite = Some(i.key.clone());
                    changed = true;
                }
            }
        });
    if cands.is_empty() {
        ui.label(
            RichText::new(tr(
                "Importa un PNG con «Camino» o «Muralla» en el nombre para usarlo aquí.",
            ))
            .size(12.0)
            .color(weak),
        );
    }
    changed
}

/// Selector de material (caminos) o de torres (murallas). Devuelve `true` si algo cambió.
#[allow(clippy::too_many_arguments)]
fn path_style_controls(
    ui: &mut Ui,
    lib: &AssetLibrary,
    kind: PathKind,
    material: &mut Material,
    tower_style: &mut TowerStyle,
    every: &mut f32,
    sprite: &mut Option<String>,
    weak: Color32,
) -> bool {
    let mut changed = false;
    match kind {
        PathKind::Camino => {
            ui.add_space(2.0);
            ui.label(RichText::new(tr("Material")).size(12.5).color(weak));
            ui.horizontal(|ui| {
                for m in Material::ALL {
                    changed |= ui.selectable_value(material, m, m.label()).changed();
                }
            });
            if *material == Material::Piedra {
                changed |= sprite_combo(
                    ui,
                    lib,
                    &[
                        "camino", "calzada", "road", "path", "cobble", "piedra", "stone",
                    ],
                    sprite,
                    weak,
                );
            }
        }
        PathKind::Muralla => {
            ui.add_space(2.0);
            ui.label(RichText::new(tr("Torres")).size(12.5).color(weak));
            ui.horizontal(|ui| {
                for t in TowerStyle::ALL {
                    changed |= ui.selectable_value(tower_style, t, t.label()).changed();
                }
            });
            let mut periodic = *every > 0.0;
            if ui
                .checkbox(&mut periodic, tr("Torres periódicas"))
                .changed()
            {
                *every = if periodic {
                    paths::DEFAULT_TOWER_EVERY
                } else {
                    0.0
                };
                changed = true;
            }
            if *every > 0.0 {
                changed |= ui
                    .add(
                        egui::Slider::new(every, paths::TOWER_EVERY_RANGE)
                            .text(tr("Cada"))
                            .suffix(" px"),
                    )
                    .changed();
            }
            changed |= sprite_combo(
                ui,
                lib,
                &["muralla", "muro", "wall", "rampart"],
                sprite,
                weak,
            );
        }
        PathKind::Rio => {}
    }
    changed
}

/// Crea un trazo con la configuración actual y lo conecta a los trazos anfitriones.
fn commit_path(om: &mut OpenMap, ctx: &Context, nodes: &[Pos2]) {
    let layer = om.path_layer();
    let cfg = &om.path_cfg;
    let (kind, width, dashed) = (cfg.kind, cfg.width, cfg.dashed);
    let (material, tower_style, every) = (cfg.material, cfg.tower_style, cfg.tower_every);
    let sprite = cfg.sprite.clone();
    let id = om.doc.add_path(
        kind,
        nodes,
        width,
        dashed && kind != PathKind::Muralla,
        layer,
    );
    if let Some(p) = om.doc.path_mut(id) {
        p.material = material;
        p.tower_style = tower_style;
        p.tower_every = if kind == PathKind::Muralla {
            every
        } else {
            0.0
        };
        let textured =
            kind == PathKind::Muralla || (kind == PathKind::Camino && material == Material::Piedra);
        p.sprite = if textured { sprite } else { None };
    }
    paths::attach_to_hosts(&mut om.doc.paths, id);
    om.touch(ctx);
}

fn finish_draft(om: &mut OpenMap, ctx: &Context) {
    if om.draft.len() >= 2 {
        let nodes = std::mem::take(&mut om.draft);
        commit_path(om, ctx, &nodes);
    } else {
        om.draft.clear();
    }
}

/// Radio de anclaje en el mundo; nunca menor a ~10 px de pantalla para que sea usable al alejar.
fn snap_radius(om: &OpenMap) -> f32 {
    paths::SNAP_RADIUS.max(10.0 / om.cam.zoom)
}

/// Punto de anclaje para `p` sobre los demás trazos (visibles) y los nodos previos del borrador.
fn snap_at(om: &OpenMap, p: Pos2) -> Option<paths::Snap> {
    let hidden = |layer: u64| {
        om.doc
            .layers
            .iter()
            .find(|l| l.id == layer)
            .is_none_or(|l| !l.visible)
    };
    let own: &[Pos2] = om.draft.split_last().map_or(&[], |(_, rest)| rest);
    paths::snap(
        om.doc.paths.iter().filter(|x| !hidden(x.layer)),
        own,
        p,
        snap_radius(om),
    )
}

/// Termina el trazo a mano alzada: simplifica, ancla los extremos y crea el trazo.
fn finish_freehand(om: &mut OpenMap, ctx: &Context) {
    let raw = std::mem::take(&mut om.freehand);
    if raw.len() < 2 {
        return;
    }
    let mut pts = paths::simplify(&raw, MIN_POINT_DIST / om.cam.zoom);
    if pts.len() < 2 {
        return;
    }
    let last = pts.len() - 1;
    for i in [0, last] {
        if let Some(s) = paths::snap(om.doc.paths.iter(), &[], pts[i], snap_radius(om)) {
            pts[i] = s.point;
        }
    }
    if (pts[0] - pts[last]).length() < 0.5 && pts.len() == 2 {
        return;
    }
    commit_path(om, ctx, &pts);
}

// ---------------------------------------------------------------------- exportar PNG

fn start_export(om: &mut OpenMap, root: &Path) {
    let dir = project::media_file(root, MEDIA_MAPS, "x")
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.to_path_buf());
    let _ = std::fs::create_dir_all(&dir);
    let name = format!("{}.png", story::sanitize_folder_name(&om.doc.name));
    let Some(path) = rfd::FileDialog::new()
        .set_title(tr("Exportar mapa a PNG"))
        .set_directory(&dir)
        .set_file_name(name)
        .add_filter("Imagen PNG", &["png"])
        .save_file()
    else {
        return;
    };
    om.export = Some(Export {
        path,
        cam: om.cam,
        step: 0,
        shot: Default::default(),
    });
}

/// Avanza la exportación: encuadra el mapa, pide la captura y recorta/guarda el PNG.
fn drive_export(om: &mut OpenMap, ctx: &Context, rect: Rect, canvas: Rect) {
    let Some(ex) = om.export.as_mut() else { return };
    ctx.request_repaint();
    match ex.step {
        0 => {
            om.cam.fit(rect, om.doc.content_rect(), 0.0);
            om.focus = None;
            om.fit = false;
            ex.step = 1;
        }
        1 => {
            let shot = ex.shot.clone();
            ctx.request_screenshot(move |img| {
                if let Ok(mut g) = shot.lock() {
                    *g = Some(img);
                }
            });
            ex.step = 2;
        }
        _ => {
            let img = ex.shot.lock().ok().and_then(|mut g| g.take());
            let Some(img) = img else { return };
            let Some(ex) = om.export.take() else { return };
            let cr = om.doc.content_rect();
            let ppp = ctx.pixels_per_point();
            let shown = Rect::from_min_size(
                Pos2::ZERO,
                vec2(img.size[0] as f32 / ppp, img.size[1] as f32 / ppp),
            );
            let m = Rect::from_min_max(
                om.cam.world_to_screen(rect, cr.min),
                om.cam.world_to_screen(rect, cr.max),
            )
            .intersect(canvas)
            .intersect(shown);
            om.cam = ex.cam;
            if m.width() < 1.0 || m.height() < 1.0 {
                om.status = Some("No se pudo exportar: el lienzo es demasiado pequeño.".into());
                return;
            }
            let crop = img.region(&m, Some(ppp));
            let res = image::save_buffer(
                &ex.path,
                crop.as_raw(),
                crop.width() as u32,
                crop.height() as u32,
                image::ColorType::Rgba8,
            );
            om.status = Some(match res {
                Ok(()) => trf("Mapa exportado: {}", &[&ex.path.display()]),
                Err(e) => format!("No se pudo exportar el PNG: {e}"),
            });
        }
    }
}

fn finish_zone(om: &mut OpenMap, ctx: &Context) {
    if om.zone_draft.len() >= 3 {
        let pts = std::mem::take(&mut om.zone_draft);
        let n = om.doc.zones.len() + 1;
        let color = zones::PALETTE[(n - 1) % zones::PALETTE.len()];
        let id = om.doc.add_zone(&format!("Zona {n}"), &pts, color);
        om.select(Some(Hit::Zone(id)));
        om.touch(ctx);
    } else {
        om.zone_draft.clear();
    }
}

/// Estampa varias instancias aleatorias dentro del radio, respetando la separación mínima.
fn scatter_at(
    om: &mut OpenMap,
    lib: &AssetLibrary,
    tex: &mut TextureCache,
    ctx: &Context,
    c: Pos2,
) {
    let kind = om.active;
    if om.brushes.is_empty() {
        om.status = Some("Elige uno o más assets en la Biblioteca para dispersar.".to_owned());
        return;
    }
    if let Err(e) = om.object_layer_ok(kind) {
        om.status = Some(e);
        return;
    }
    om.status = None;
    let (r, spacing) = (om.scatter.radius, om.scatter.spacing.max(1.0));
    let attempts = ((std::f32::consts::PI * r * r) / (spacing * spacing) * 1.5)
        .ceil()
        .clamp(1.0, 40.0) as usize;
    let min2 = spacing * spacing;
    let mut placed = false;
    for _ in 0..attempts {
        let ang = om.rng.f32() * std::f32::consts::TAU;
        let rad = r * om.rng.f32().sqrt();
        let p = c + vec2(ang.cos(), ang.sin()) * rad;
        if let Some([w, h]) = om.doc.size {
            if p.x < 0.0 || p.y < 0.0 || p.x > w || p.y > h {
                continue;
            }
        }
        if om
            .doc
            .assets
            .iter()
            .any(|a| a.layer == kind && (a.pos2() - p).length_sq() < min2)
        {
            continue;
        }
        let key = om.brushes[om.rng.below(om.brushes.len())].clone();
        let size = sprite_size(lib, tex, ctx, &key);
        let base = (STAMP_TARGET / size.x.max(size.y)).min(1.0);
        let scale = base * (0.85 + 0.30 * om.rng.f32());
        let id = om.doc.add_asset(&key, kind, p, scale);
        let rot = om.rng.f32() * 360.0;
        let z = default_z(lib, &key);
        if let Some(a) = om.doc.asset_mut(id) {
            a.rotation = rot;
            a.z = z;
        }
        placed = true;
    }
    if placed {
        om.touch(ctx);
    }
}

fn to_world(om: &OpenMap, rect: Rect, s: Option<Pos2>) -> Option<Pos2> {
    s.map(|p| om.cam.screen_to_world(rect, p))
}

/// Teclas del lienzo de este cuadro.
struct Keys {
    del: bool,
    rot: bool,
    rot_back: bool,
    esc: bool,
    enter: bool,
    back: bool,
    undo: bool,
    redo: bool,
    front: bool,
    behind: bool,
    tool: Option<Tool>,
    wall: bool,
}

fn read_keys(ctx: &Context) -> Keys {
    ctx.input(|i| {
        let cmd = i.modifiers.command;
        let shift = i.modifiers.shift;
        let tool = if cmd {
            None
        } else {
            [
                (Key::V, Tool::Select),
                (Key::P, Tool::Place),
                (Key::D, Tool::Scatter),
                (Key::B, Tool::Brush),
                (Key::E, Tool::Eraser),
                (Key::G, Tool::Fill),
                (Key::C, Tool::Path),
                (Key::Z, Tool::Zone),
                (Key::T, Tool::Region),
                (Key::H, Tool::Pan),
            ]
            .into_iter()
            .find(|(k, _)| i.key_pressed(*k))
            .map(|(_, t)| t)
        };
        Keys {
            del: i.key_pressed(Key::Delete),
            rot: !cmd && i.key_pressed(Key::R) && !shift,
            rot_back: !cmd && i.key_pressed(Key::R) && shift,
            esc: i.key_pressed(Key::Escape),
            enter: i.key_pressed(Key::Enter),
            back: i.key_pressed(Key::Backspace),
            undo: cmd && !shift && i.key_pressed(Key::Z),
            redo: cmd && (i.key_pressed(Key::Y) || (shift && i.key_pressed(Key::Z))),
            front: i.key_pressed(Key::PageUp),
            behind: i.key_pressed(Key::PageDown),
            tool,
            wall: !cmd && i.key_pressed(Key::W),
        }
    })
}

fn viewport(
    ui: &mut Ui,
    ctx: &Context,
    om: &mut OpenMap,
    lib: &AssetLibrary,
    tex: &mut TextureCache,
    root: &Path,
    limit: Rect,
) {
    // Rectángulo exacto restante del área central: ningún trazo, nodo, etiqueta o indicador
    // puede expandirlo ni moverlo.
    let avail = ui.available_rect_before_wrap();
    let rect = if limit.is_positive() {
        avail.intersect(limit)
    } else {
        avail
    };
    let resp = ui.allocate_rect(rect, Sense::click_and_drag());
    // Todo lo del mapa se recorta a este rectángulo (nunca invade los paneles).
    let canvas_rect = rect.intersect(ui.clip_rect()).intersect(limit);
    ui.shrink_clip_rect(canvas_rect);
    drive_export(om, ctx, rect, canvas_rect);
    if om.fit {
        om.cam.fit(rect, om.doc.content_rect(), 0.06);
        om.fit = false;
        om.focus = None;
    }

    // --- enfoque suave pedido desde la lista de zonas
    if let Some(wr) = om.focus_req.take() {
        let zoom = ((rect.width() / wr.width().max(1.0)).min(rect.height() / wr.height().max(1.0))
            * 0.7)
            .clamp(MIN_ZOOM, MAX_ZOOM);
        om.focus = Some(Focus {
            from: om.cam.center,
            from_zoom: om.cam.zoom,
            to: wr.center(),
            to_zoom: zoom,
            t0: ctx.input(|i| i.time),
        });
    }

    // --- cámara: zoom al cursor
    let hover = resp.hovered();
    if hover {
        if let Some(p) = ctx.input(|i| i.pointer.hover_pos()) {
            let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
            let pinch = ctx.input(|i| i.zoom_delta());
            let factor = (scroll * 0.0015).exp() * pinch;
            if (factor - 1.0).abs() > f32::EPSILON {
                om.cam.zoom_at(rect, p, factor);
                om.focus = None;
            }
        }
    }

    // --- cámara: pan
    let space = ctx.input(|i| i.key_down(Key::Space));
    let pan_tool = om.tool == Tool::Pan || space;
    let panning = resp.dragged_by(PointerButton::Middle)
        || resp.dragged_by(PointerButton::Secondary)
        || (pan_tool && resp.dragged_by(PointerButton::Primary));
    if panning {
        om.cam.pan_screen(resp.drag_delta());
        om.focus = None;
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if hover && pan_tool {
        ctx.set_cursor_icon(egui::CursorIcon::Grab);
    }

    if let Some(f) = om.focus {
        let t = (((ctx.input(|i| i.time) - f.t0) / 0.45) as f32).clamp(0.0, 1.0);
        let e = 1.0 - (1.0 - t).powi(3);
        om.cam.zoom = f.from_zoom * (f.to_zoom / f.from_zoom).powf(e);
        om.cam.center = f.from + (f.to - f.from) * e;
        ctx.request_repaint();
        if t >= 1.0 {
            om.focus = None;
        }
    }

    om.cursor = ctx
        .input(|i| i.pointer.hover_pos())
        .filter(|p| rect.contains(*p))
        .map(|p| om.cam.screen_to_world(rect, p));
    let pointer_world = to_world(om, rect, resp.interact_pointer_pos());
    let latest_world = to_world(om, rect, ctx.input(|i| i.pointer.latest_pos()));
    let primary_down = ctx.input(|i| i.pointer.primary_down());
    let down = primary_down && resp.is_pointer_button_down_on() && !pan_tool;

    // Un trazo de pincel no puede sobrevivir a un cambio de herramienta.
    if om.stroking && (!matches!(om.tool, Tool::Brush | Tool::Eraser) || pan_tool) {
        if let Some(p) = om.terrain.end_stroke() {
            om.hist.push_terrain(p);
        }
        om.stroking = false;
    }

    // --- herramientas
    om.snap_hint = None;
    if om.tool != Tool::Path || pan_tool {
        om.freehand.clear();
    }
    if !pan_tool {
        match om.tool {
            Tool::Select => {
                if resp.drag_started_by(PointerButton::Primary) {
                    if let Some(ps) = ctx.input(|i| i.pointer.press_origin()) {
                        let pw = om.cam.screen_to_world(rect, ps);
                        om.drag = None;
                        om.node_drag = None;
                        om.zone_drag = None;
                        om.region_drag = None;
                        om.group_drag = None;
                        om.marquee = None;
                        let (ctrl, shift) = ctx.input(|i| (i.modifiers.command, i.modifiers.shift));
                        if let Some((pid, idx)) = node_hit(om, rect, ps) {
                            om.node_drag = Some((pid, idx));
                            om.drag = om.doc.path(pid).map(|p| {
                                let n = p.nodes[idx];
                                pw - pos2(n[0], n[1])
                            });
                        } else if let Some((zid, idx)) = zone_vertex_hit(om, rect, ps) {
                            om.zone_drag = Some((zid, idx));
                            om.drag = om.doc.zone(zid).map(|z| {
                                let n = z.points[idx];
                                pw - pos2(n[0], n[1])
                            });
                        } else if let Some((rid, idx)) = region_vertex_hit(om, rect, ps) {
                            om.region_drag = Some((rid, idx));
                            om.drag = om.doc.region(rid).map(|r| {
                                let n = r.points[idx];
                                pw - pos2(n[0], n[1])
                            });
                        } else if {
                            let h = hit_test(om, lib, tex, ctx, pw);
                            matches!(h, Some(Hit::Asset(id)) if om.multi.contains(&id))
                        } {
                            // Arrastre de grupo.
                            let orig: Vec<(u64, [f32; 2])> = om
                                .doc
                                .assets
                                .iter()
                                .filter(|a| om.multi.contains(&a.id))
                                .map(|a| (a.id, a.pos))
                                .collect();
                            om.group_drag = Some((pw, orig));
                        } else if shift || hit_test(om, lib, tex, ctx, pw).is_none() {
                            // Recuadro de selección sobre espacio vacío.
                            let base = if ctrl || shift {
                                om.asset_set()
                            } else {
                                Vec::new()
                            };
                            if !(ctrl || shift) {
                                om.select(None);
                            }
                            om.marquee = Some((pw, base));
                        } else if ctrl {
                            // Ctrl + clic: se resuelve al soltar (sin arrastrar).
                        } else {
                            let hit = hit_test(om, lib, tex, ctx, pw);
                            om.drag = match &hit {
                                Some(Hit::Asset(id)) => om
                                    .doc
                                    .assets
                                    .iter()
                                    .find(|a| a.id == *id)
                                    .map(|a| pw - a.pos2()),
                                _ => None,
                            };
                            om.select(hit);
                        }
                    }
                }
                if resp.dragged_by(PointerButton::Primary) {
                    if let (Some((start, orig)), Some(p)) = (&om.group_drag, pointer_world) {
                        let delta = p - *start;
                        let moves: Vec<(u64, [f32; 2])> = orig
                            .iter()
                            .map(|(id, pos)| (*id, [pos[0] + delta.x, pos[1] + delta.y]))
                            .collect();
                        for (id, pos) in moves {
                            if let Some(a) = om.doc.asset_mut(id) {
                                a.pos = pos;
                            }
                        }
                        om.touch(ctx);
                    }
                    if let (Some(off), Some(p)) = (om.drag, pointer_world) {
                        let np = p - off;
                        if let Some((pid, idx)) = om.node_drag {
                            paths::move_connected(&mut om.doc.paths, pid, idx, np);
                            om.touch(ctx);
                        } else if let Some((zid, idx)) = om.zone_drag {
                            if let Some(z) = om.doc.zone_mut(zid) {
                                z.points[idx] = [np.x, np.y];
                                om.touch(ctx);
                            }
                        } else if let Some((rid, idx)) = om.region_drag {
                            if let Some(r) = om.doc.region_mut(rid) {
                                r.points[idx] = [np.x, np.y];
                                om.touch(ctx);
                            }
                        } else if let Some(id) = om.selected {
                            if let Some(a) = om.doc.asset_mut(id) {
                                a.pos = [np.x, np.y];
                                om.touch(ctx);
                            }
                        }
                    }
                }
                if resp.drag_stopped() {
                    if let (Some((start, base)), Some(end)) = (om.marquee.take(), latest_world) {
                        let r = Rect::from_two_pos(start, end);
                        let mut ids = base;
                        for id in assets_in_rect(om, r) {
                            if !ids.contains(&id) {
                                ids.push(id);
                            }
                        }
                        om.set_assets(ids);
                    }
                    om.drag = None;
                    om.node_drag = None;
                    om.zone_drag = None;
                    om.region_drag = None;
                    om.group_drag = None;
                }

                let click_screen = resp.interact_pointer_pos();
                if resp.clicked_by(PointerButton::Primary) {
                    if let (Some(ps), Some(pw)) = (click_screen, pointer_world) {
                        if node_hit(om, rect, ps).is_none()
                            && zone_vertex_hit(om, rect, ps).is_none()
                            && region_vertex_hit(om, rect, ps).is_none()
                        {
                            let hit = hit_test(om, lib, tex, ctx, pw);
                            let ctrl = ctx.input(|i| i.modifiers.command);
                            match (ctrl, &hit) {
                                (true, Some(Hit::Asset(id))) => {
                                    // Ctrl + clic: añade o quita del conjunto.
                                    let mut set = om.asset_set();
                                    if let Some(pos) = set.iter().position(|x| x == id) {
                                        set.remove(pos);
                                    } else {
                                        set.push(*id);
                                    }
                                    om.set_assets(set);
                                }
                                (true, _) => {}
                                _ => om.select(hit),
                            }
                        }
                    }
                }
                if resp.double_clicked_by(PointerButton::Primary) {
                    if let (Some(ps), Some(pw)) = (click_screen, pointer_world) {
                        if let Some((pid, idx)) = node_hit(om, rect, ps) {
                            // Quitar nodo (un trazo necesita al menos 2).
                            if let Some(path) = om.doc.path_mut(pid) {
                                if path.nodes.len() > 2 {
                                    path.nodes.remove(idx);
                                    om.touch(ctx);
                                }
                            }
                        } else if let Some((rid, idx)) = region_vertex_hit(om, rect, ps) {
                            if let Some(r) = om.doc.region_mut(rid) {
                                if r.points.len() > 3 {
                                    r.points.remove(idx);
                                    om.touch(ctx);
                                }
                            }
                        } else if let Some((zid, idx)) = zone_vertex_hit(om, rect, ps) {
                            // Quitar vértice (una zona necesita al menos 3).
                            if let Some(z) = om.doc.zone_mut(zid) {
                                if z.points.len() > 3 {
                                    z.points.remove(idx);
                                    om.touch(ctx);
                                }
                            }
                        } else {
                            // Elemento vinculado: entrar al mapa interior.
                            let link = hit_test(om, lib, tex, ctx, pw).and_then(|h| match h {
                                Hit::Asset(id) => om
                                    .doc
                                    .assets
                                    .iter()
                                    .find(|a| a.id == id)
                                    .and_then(|a| a.link.clone()),
                                Hit::Zone(id) => om.doc.zone(id).and_then(|z| z.link.clone()),
                                Hit::Path(_) | Hit::Region(_) => None,
                            });
                            if let Some(l) = link {
                                om.nav = Some(Nav::Enter(l));
                            } else {
                                insert_vertex(om, ctx, pw);
                            }
                        }
                    }
                }
            }
            Tool::Place => {
                if resp.clicked_by(PointerButton::Primary) {
                    if let (Some(p), Some(key)) = (pointer_world, om.brushes.last().cloned()) {
                        let kind = om.active;
                        match om.object_layer_ok(kind) {
                            Err(e) => om.status = Some(e),
                            Ok(()) => {
                                let size = sprite_size(lib, tex, ctx, &key);
                                let scale = (STAMP_TARGET / size.x.max(size.y)).min(1.0);
                                let id = om.doc.add_asset(&key, kind, p, scale);
                                let z = default_z(lib, &key);
                                if let Some(a) = om.doc.asset_mut(id) {
                                    a.z = z;
                                }
                                om.select(Some(Hit::Asset(id)));
                                om.status = None;
                                om.touch(ctx);
                            }
                        }
                    } else if om.brushes.is_empty() {
                        om.status =
                            Some("Elige un asset en la Biblioteca para estampar.".to_owned());
                    }
                }
            }
            Tool::Scatter => {
                if down {
                    if let Some(cur) = latest_world {
                        let step = (om.scatter.radius * 0.5).max(6.0);
                        let go = om.scatter_last.is_none_or(|l| (cur - l).length() >= step);
                        if go {
                            scatter_at(om, lib, tex, ctx, cur);
                            om.scatter_last = Some(cur);
                        }
                    }
                } else {
                    om.scatter_last = None;
                }
            }
            Tool::Brush | Tool::Eraser => {
                let erase = om.tool == Tool::Eraser;
                let params = om.brush_params(erase);
                let layer = om.layer_ok(om.doc.terrain_layer());
                if resp.clicked_by(PointerButton::Primary) && !om.stroking {
                    // Clic suelto: un solo sello.
                    match (&layer, pointer_world) {
                        (Ok(()), Some(p)) => {
                            om.terrain.begin_stroke();
                            om.terrain.stroke_to(p, &params);
                            if let Some(patch) = om.terrain.end_stroke() {
                                om.hist.push_terrain(patch);
                                om.auto.mark(ctx);
                            }
                        }
                        (Err(e), _) => om.status = Some(e.clone()),
                        _ => {}
                    }
                } else if down {
                    match &layer {
                        Ok(()) => {
                            if let Some(cur) = latest_world {
                                if !om.stroking {
                                    om.terrain.begin_stroke();
                                    om.stroking = true;
                                    let start = ctx
                                        .input(|i| i.pointer.press_origin())
                                        .map(|s| om.cam.screen_to_world(rect, s))
                                        .unwrap_or(cur);
                                    om.terrain.stroke_to(start, &params);
                                }
                                om.terrain.stroke_to(cur, &params);
                                om.auto.mark(ctx);
                            }
                        }
                        Err(e) => om.status = Some(e.clone()),
                    }
                }
                if !down && om.stroking {
                    if let Some(patch) = om.terrain.end_stroke() {
                        om.hist.push_terrain(patch);
                    }
                    om.stroking = false;
                    om.auto.mark(ctx);
                }
            }
            Tool::Fill => {
                if resp.clicked_by(PointerButton::Primary) {
                    match (om.layer_ok(om.doc.terrain_layer()), pointer_world) {
                        (Err(e), _) => om.status = Some(e),
                        (Ok(()), Some(p)) => {
                            match om.terrain.fill(p, om.brush.color, om.brush.opacity) {
                                Ok(Some(patch)) => {
                                    om.hist.push_terrain(patch);
                                    om.auto.mark(ctx);
                                    om.status = None;
                                }
                                Ok(None) => {}
                                Err(e) => om.status = Some(e),
                            }
                        }
                        _ => {}
                    }
                }
            }
            Tool::Path => {
                // Anclaje visible bajo el cursor.
                if hover || !om.freehand.is_empty() {
                    om.snap_hint = latest_world
                        .and_then(|c| snap_at(om, c))
                        .map(|s| (s.point, s.on_node));
                }
                match om.path_cfg.mode {
                    DrawMode::Nodes => {
                        om.freehand.clear();
                        if resp.double_clicked_by(PointerButton::Primary) {
                            finish_draft(om, ctx);
                        } else if resp.clicked_by(PointerButton::Primary) {
                            if let Some(p) = pointer_world {
                                match om.object_layer_ok(om.path_layer()) {
                                    Ok(()) => {
                                        let p = snap_at(om, p).map_or(p, |s| s.point);
                                        om.draft.push(p);
                                    }
                                    Err(e) => om.status = Some(e),
                                }
                            }
                        }
                    }
                    DrawMode::Freehand => {
                        om.draft.clear();
                        if resp.drag_started_by(PointerButton::Primary) {
                            om.freehand.clear();
                            let start = ctx
                                .input(|i| i.pointer.press_origin())
                                .map(|s| om.cam.screen_to_world(rect, s));
                            match (om.object_layer_ok(om.path_layer()), start) {
                                (Ok(()), Some(p)) => {
                                    let p = snap_at(om, p).map_or(p, |s| s.point);
                                    om.freehand.push(p);
                                }
                                (Err(e), _) => om.status = Some(e),
                                _ => {}
                            }
                        }
                        if down && !om.freehand.is_empty() {
                            if let Some(cur) = latest_world {
                                let far = om
                                    .freehand
                                    .last()
                                    .is_none_or(|l| (cur - *l).length() * om.cam.zoom >= 2.0);
                                if far {
                                    om.freehand.push(cur);
                                }
                            }
                        }
                        if !down && !om.freehand.is_empty() {
                            finish_freehand(om, ctx);
                        }
                    }
                }
            }
            Tool::Zone => {
                if resp.double_clicked_by(PointerButton::Primary)
                    || resp.clicked_by(PointerButton::Secondary)
                {
                    finish_zone(om, ctx);
                } else if resp.clicked_by(PointerButton::Primary) {
                    if let Some(p) = pointer_world {
                        om.zone_draft.push(p);
                    }
                }
            }
            Tool::Region => {
                if resp.double_clicked_by(PointerButton::Primary)
                    || resp.clicked_by(PointerButton::Secondary)
                {
                    finish_region(om, ctx);
                } else if resp.clicked_by(PointerButton::Primary) {
                    if let Some(p) = pointer_world {
                        om.region_draft.push(p);
                    }
                }
            }
            Tool::Pan => {}
        }
    }

    // --- teclado
    if !ctx.egui_wants_keyboard_input() {
        let k = read_keys(ctx);
        if k.undo {
            run_history(om, ctx, false);
        }
        if k.redo {
            run_history(om, ctx, true);
        }
        let path_draft = om.tool == Tool::Path && !om.draft.is_empty();
        let zone_draft = om.tool == Tool::Zone && !om.zone_draft.is_empty();
        let region_draft = om.tool == Tool::Region && !om.region_draft.is_empty();
        if path_draft {
            if k.enter {
                finish_draft(om, ctx);
            }
            if k.back {
                om.draft.pop();
            }
            if k.esc {
                om.draft.clear();
            }
        } else if region_draft {
            if k.enter {
                finish_region(om, ctx);
            }
            if k.back {
                om.region_draft.pop();
            }
            if k.esc {
                om.region_draft.clear();
            }
        } else if zone_draft {
            if k.enter {
                finish_zone(om, ctx);
            }
            if k.back {
                om.zone_draft.pop();
            }
            if k.esc {
                om.zone_draft.clear();
            }
        } else {
            if k.del || k.back {
                if !om.multi.is_empty() {
                    for id in std::mem::take(&mut om.multi) {
                        om.doc.remove_asset(id);
                    }
                    om.touch(ctx);
                } else if let Some(id) = om.sel_region {
                    om.doc.remove_region(id);
                    om.sel_region = None;
                    om.touch(ctx);
                } else if let Some(id) = om.selected {
                    om.doc.remove_asset(id);
                    om.selected = None;
                    om.touch(ctx);
                } else if let Some(id) = om.sel_path {
                    om.doc.remove_path(id);
                    om.sel_path = None;
                    om.touch(ctx);
                } else if let Some(id) = om.sel_zone {
                    om.doc.remove_zone(id);
                    om.sel_zone = None;
                    om.touch(ctx);
                }
            }
            if om.multi.len() > 1 && (k.rot || k.rot_back) {
                rotate_group(om, if k.rot { 15.0 } else { -15.0 });
                om.touch(ctx);
            }
            if let Some(id) = om.selected {
                if k.rot || k.rot_back {
                    if let Some(a) = om.doc.asset_mut(id) {
                        a.rotation =
                            (a.rotation + if k.rot { 15.0 } else { -15.0 }).rem_euclid(360.0);
                        om.touch(ctx);
                    }
                }
                if k.front {
                    om.doc.bring_front(id);
                    om.touch(ctx);
                }
                if k.behind {
                    om.doc.send_back(id);
                    om.touch(ctx);
                }
            }
            if k.esc {
                om.select(None);
                om.tool = Tool::Select;
            }
        }
        if let Some(t) = k.tool {
            if t == Tool::Path && om.path_cfg.kind == PathKind::Muralla {
                set_path_kind(om, PathKind::Camino);
            }
            om.tool = t;
        }
        if k.wall {
            set_path_kind(om, PathKind::Muralla);
            om.tool = Tool::Path;
        }
    }

    // --- dibujo
    om.terrain.sync(ctx);
    paint_scene(
        ui,
        ctx,
        rect,
        canvas_rect,
        om,
        lib,
        tex,
        root,
        hover && !panning,
    );
}

/// Doble clic sobre el borde del camino o la zona seleccionada: añade un nodo/vértice.
fn insert_vertex(om: &mut OpenMap, ctx: &Context, pw: Pos2) {
    if let Some(pid) = om.sel_path {
        let ins = om.doc.path(pid).and_then(|path| {
            let tol = path.width / 2.0 + 6.0 / om.cam.zoom;
            paths::nearest(path.kind, &path.points(), pw)
                .filter(|n| n.dist <= tol)
                .map(|n| (n.seg + 1, n.point))
        });
        if let Some((at, pt)) = ins {
            if let Some(path) = om.doc.path_mut(pid) {
                path.nodes.insert(at, [pt.x, pt.y]);
                om.touch(ctx);
            }
            return;
        }
    }
    if let Some(rid) = om.sel_region {
        let tol = 8.0 / om.cam.zoom;
        let ins = om.doc.region(rid).and_then(|r| {
            zones::nearest_edge(&r.pts(), pw)
                .filter(|(_, _, d)| *d <= tol)
                .map(|(at, pt, _)| (at, pt))
        });
        if let Some((at, pt)) = ins {
            if let Some(r) = om.doc.region_mut(rid) {
                r.points.insert(at, [pt.x, pt.y]);
                om.touch(ctx);
            }
            return;
        }
    }
    if let Some(zid) = om.sel_zone {
        let tol = 8.0 / om.cam.zoom;
        let ins = om.doc.zone(zid).and_then(|z| {
            zones::nearest_edge(&z.pts(), pw)
                .filter(|(_, _, d)| *d <= tol)
                .map(|(at, pt, _)| (at, pt))
        });
        if let Some((at, pt)) = ins {
            if let Some(z) = om.doc.zone_mut(zid) {
                z.points.insert(at, [pt.x, pt.y]);
                om.touch(ctx);
            }
        }
    }
}

// ---------------------------------------------------------------------- iluminación

/// Tinte multiplicativo del filtro Pergamino (sepia/café cálido).
const PARCH_TINT: Color32 = Color32::from_rgb(232, 208, 168);

struct Sun {
    /// Dirección de las sombras (unitaria, en el mundo).
    dir: Vec2,
    /// Largo relativo de la sombra: grande al amanecer y atardecer, corto al mediodía.
    len: f32,
    /// 0 = sol en el horizonte, 1 = mediodía.
    strength: f32,
}

/// Posición del sol para la hora dada (solo de 06:00 a 19:00).
fn sun_at(hour: f32) -> Option<Sun> {
    if !(6.0..=19.0).contains(&hour) {
        return None;
    }
    let u = (hour - 6.0) / 13.0;
    let s = (std::f32::consts::PI * u).sin();
    let elevation = (12.0 + 66.0 * s).to_radians();
    Some(Sun {
        // Mañana: sol al este → sombras a la izquierda; tarde: a la derecha. Siempre algo hacia abajo.
        dir: vec2(-(std::f32::consts::PI * u).cos(), 0.3).normalized(),
        len: (1.0 / elevation.tan()).clamp(0.2, 2.2),
        strength: s.clamp(0.0, 1.0).sqrt(),
    })
}

/// Intensidad de la noche (0–1): plena de 19:30 a 05:30, con 30 min de transición.
fn night_amount(hour: f32) -> f32 {
    let h = hour.rem_euclid(24.0);
    let smooth = |a: f32, b: f32, x: f32| {
        let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    if h >= 12.0 {
        smooth(19.0, 19.5, h)
    } else {
        1.0 - smooth(5.5, 6.0, h)
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_scene(
    ui: &mut Ui,
    ctx: &Context,
    rect: Rect,
    canvas_rect: Rect,
    om: &OpenMap,
    lib: &AssetLibrary,
    tex: &mut TextureCache,
    root: &Path,
    show_ghost: bool,
) {
    let (surround, accent) = {
        let v = ui.visuals();
        (v.faint_bg_color, v.selection.bg_fill)
    };
    let cam = om.cam;
    let doc = &om.doc;
    let parchment = doc.style == MapStyle::Pergamino;
    let tint = if parchment {
        PARCH_TINT
    } else {
        Color32::WHITE
    };
    // Recorte estricto: nada del mapa se pinta fuera del viewport (ni sobre los paneles laterales).
    let viewport_rect = canvas_rect;
    let canvas_painter = ui.painter().with_clip_rect(viewport_rect);
    canvas_painter.rect_filled(viewport_rect, 0.0, surround);

    // Área del mapa en pantalla y recorte del contenido.
    let map_screen: Option<Rect> = doc.size.map(|[w, h]| {
        Rect::from_min_max(
            cam.world_to_screen(rect, pos2(0.0, 0.0)),
            cam.world_to_screen(rect, pos2(w, h)),
        )
    });
    let content_clip = map_screen.map_or(viewport_rect, |m| m.intersect(viewport_rect));
    canvas_painter.rect_filled(content_clip, 0.0, doc.style.background());
    let painter = canvas_painter.with_clip_rect(content_clip);
    let clean = om.export.is_some();
    let show_ghost = show_ghost && !clean;
    om.path_cache.borrow_mut().refresh(&doc.paths);
    let tower_assets = if doc.paths.iter().any(|p| p.kind == PathKind::Muralla) {
        find_tower_assets(lib)
    } else {
        (None, None)
    };
    let sun = if doc.shadows { sun_at(doc.hour) } else { None };

    // Capas de abajo hacia arriba, según el orden de la lista.
    for layer in doc.layers.iter().rev() {
        if !layer.visible {
            continue;
        }
        if layer.terrain {
            if let (Some(b), true) = (&doc.base, doc.textures) {
                let path = project::media_file(root, MEDIA_MAPS, &b.file);
                let dst = Rect::from_min_max(
                    cam.world_to_screen(rect, pos2(0.0, 0.0)),
                    cam.world_to_screen(rect, pos2(b.size[0], b.size[1])),
                );
                match tex.get(ctx, &path) {
                    Ok(t) => {
                        painter.image(
                            t.id(),
                            dst,
                            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                            tint,
                        );
                    }
                    Err(msg) => {
                        painter.text(
                            dst.center(),
                            Align2::CENTER_CENTER,
                            msg,
                            FontId::proportional(13.0),
                            ERROR_COLOR,
                        );
                    }
                }
            }
            om.terrain.draw(&painter, rect, &cam, tint);
            for r in &doc.regions {
                let pts: Vec<Pos2> = r
                    .pts()
                    .into_iter()
                    .map(|p| cam.world_to_screen(rect, p))
                    .collect();
                if pts.len() < 3 || !Rect::from_points(&pts).intersects(viewport_rect) {
                    continue;
                }
                let tinted = |c: [u8; 3]| {
                    Color32::from_rgb(
                        (c[0] as u16 * tint.r() as u16 / 255) as u8,
                        (c[1] as u16 * tint.g() as u16 / 255) as u8,
                        (c[2] as u16 * tint.b() as u16 / 255) as u8,
                    )
                };
                let fill = tinted(r.color);
                let mut mesh = Mesh::default();
                for p in &pts {
                    mesh.vertices.push(Vertex {
                        pos: *p,
                        uv: egui::epaint::WHITE_UV,
                        color: fill,
                    });
                }
                for t in zones::triangulate(&pts) {
                    mesh.indices.extend(t.map(|i| i as u32));
                }
                painter.add(Shape::mesh(mesh));
                let mut ring = pts.clone();
                ring.push(pts[0]);
                if r.border && r.border_width > 0.0 {
                    let w = (r.border_width * cam.zoom).max(1.0);
                    painter.add(Shape::line(ring, Stroke::new(w, tinted(r.border_color))));
                    for p in &pts {
                        painter.circle_filled(*p, w / 2.0, tinted(r.border_color));
                    }
                } else {
                    // Sin borde: un hilo del mismo color suaviza el contorno.
                    painter.add(Shape::line(ring, Stroke::new(1.0, fill)));
                }
            }
            continue;
        }

        // Caminos, ríos y murallas de esta capa (todos los bordes, luego todos los rellenos).
        let screen: Vec<_> = {
            let cache = om.path_cache.borrow();
            doc.paths
                .iter()
                .filter(|p| p.layer == layer.id)
                .map(|p| {
                    let pts: Vec<Pos2> = cache
                        .curve(p.id)
                        .iter()
                        .map(|q| cam.world_to_screen(rect, *q))
                        .collect();
                    (p, pts)
                })
                .filter(|(p, pts)| {
                    !pts.is_empty()
                        && Rect::from_points(pts)
                            .expand((p.width + 4.0) * cam.zoom * 2.0)
                            .intersects(viewport_rect)
                })
                .map(|(p, pts)| {
                    let look = Look::of(p, cache.ends(p.id));
                    let sprite = if doc.textures {
                        path_sprite(p, lib, tex, ctx)
                    } else {
                        None
                    };
                    (p, pts, look, sprite)
                })
                .collect()
        };
        for inner in [false, true] {
            for (p, pts, look, sprite) in &screen {
                match sprite {
                    // Con PNG: el sprite se repite a lo largo del trazo (sin borde vectorial).
                    Some((id, size)) => {
                        if inner {
                            let k = if p.kind == PathKind::Muralla {
                                1.25
                            } else {
                                1.1
                            };
                            let w = p.width * cam.zoom * k;
                            paths::paint_textured(&painter, pts, *id, *size, w, tint);
                        }
                    }
                    None => paths::paint(&painter, pts, look, doc.style, cam.zoom, inner, 1.0),
                }
            }
        }
        // Torres de las murallas, sobre todos los trazos de la capa.
        for (p, ..) in screen.iter().filter(|(p, ..)| p.kind == PathKind::Muralla) {
            let list = paths::towers(&p.points(), p.tower_style, p.tower_every);
            draw_towers(
                &painter,
                ctx,
                rect,
                &cam,
                lib,
                tex,
                &tower_assets,
                &list,
                p.width,
                doc.style,
                tint,
                1.0,
            );
        }

        // Assets: primero todas las sombras de la capa, luego los sprites por orden z.
        let sorted = doc.sorted_assets(layer.id);
        if let Some(s) = &sun {
            for a in &sorted {
                draw_shadow(&painter, ctx, rect, &cam, lib, tex, a, s);
            }
        }
        for a in &sorted {
            draw_asset(
                &painter,
                ctx,
                rect,
                &cam,
                lib,
                tex,
                a,
                tint,
                Vec2::ZERO,
                false,
            );
        }
    }

    // Zonas / territorios sobre todas las capas.
    if doc.zones_visible {
        for z in &doc.zones {
            let pts: Vec<Pos2> = z
                .pts()
                .into_iter()
                .map(|p| cam.world_to_screen(rect, p))
                .collect();
            if pts.len() >= 3 && Rect::from_points(&pts).intersects(rect) {
                if let (Some(s), true) = (&sun, z.relief > 0.0) {
                    let h = Rect::from_points(&pts).height();
                    let reach = s.dir * s.len * z.relief.clamp(0.0, 1.0) * h * 0.25;
                    let a = (0.26 * (0.4 + 0.6 * s.strength) * 255.0) as u8;
                    zones::paint_relief_shadow(&painter, &pts, reach, Color32::from_black_alpha(a));
                }
                zones::paint(
                    &painter,
                    &pts,
                    z,
                    om.sel_zone == Some(z.id),
                    accent,
                    theme::serif(),
                    cam.zoom,
                );
            }
        }
    }

    // Velo de papel envejecido (solo Pergamino).
    if parchment {
        painter.rect_filled(
            content_clip,
            0.0,
            Color32::from_rgba_unmultiplied(200, 165, 100, 30),
        );
        for i in 0..8 {
            let a = (34.0 - i as f32 * 4.0).max(0.0) as u8;
            painter.rect_stroke(
                content_clip.shrink(i as f32 * 5.0),
                0.0,
                Stroke::new(5.0, Color32::from_rgba_unmultiplied(92, 58, 24, a)),
                StrokeKind::Inside,
            );
        }
    }

    // Atmósfera nocturna / crepuscular.
    let night = night_amount(doc.hour);
    if night > 0.0 {
        painter.rect_filled(
            content_clip,
            0.0,
            Color32::from_rgba_unmultiplied(16, 22, 84, (0.42 * night * 255.0) as u8),
        );
    }

    // Cuadrícula.
    if doc.grid.enabled {
        let bounds = doc
            .size
            .map(|[w, h]| Rect::from_min_size(pos2(0.0, 0.0), vec2(w, h)));
        draw_grid(&painter, rect, &cam, &doc.grid, bounds);
    }

    // Trazo en construcción (por nodos o a mano alzada).
    if om.tool == Tool::Path && !clean {
        let cfg = &om.path_cfg;
        let (kind, w) = (cfg.kind, cfg.width);
        let look = Look::new(kind, cfg.material, w, cfg.dashed, [true, true]);
        let to_screen = |nodes: &[Pos2]| -> Vec<Pos2> {
            nodes
                .iter()
                .map(|q| cam.world_to_screen(rect, *q))
                .collect()
        };
        if !om.draft.is_empty() {
            let mut nodes = om.draft.clone();
            if let (Some(c), true) = (om.cursor, show_ghost) {
                nodes.push(om.snap_hint.map_or(c, |(p, _)| p));
            }
            let pts: Vec<Pos2> = paths::curve(kind, &nodes)
                .into_iter()
                .map(|(q, _)| cam.world_to_screen(rect, q))
                .collect();
            for inner in [false, true] {
                paths::paint(&painter, &pts, &look, doc.style, cam.zoom, inner, 0.8);
            }
            if kind == PathKind::Muralla {
                let list = paths::towers(&nodes, cfg.tower_style, cfg.tower_every);
                draw_towers(
                    &painter,
                    ctx,
                    rect,
                    &cam,
                    lib,
                    tex,
                    &tower_assets,
                    &list,
                    w,
                    doc.style,
                    tint,
                    0.8,
                );
            }
            for n in &om.draft {
                let s = cam.world_to_screen(rect, *n);
                canvas_painter.circle_filled(s, 4.0, accent);
                canvas_painter.circle_stroke(s, 4.0, Stroke::new(1.5, Color32::WHITE));
            }
        }
        if om.freehand.len() >= 2 {
            let pts = to_screen(&om.freehand);
            for inner in [false, true] {
                paths::paint(&painter, &pts, &look, doc.style, cam.zoom, inner, 0.8);
            }
        }
        // Indicador de conexión con otro trazo.
        if show_ghost || !om.freehand.is_empty() {
            if let Some((p, on_node)) = om.snap_hint {
                let s = cam.world_to_screen(rect, p);
                paths::paint_snap(&canvas_painter, s, on_node, accent);
            }
        }
    }

    // Región en construcción.
    if om.tool == Tool::Region && !om.region_draft.is_empty() && !clean {
        let mut pts: Vec<Pos2> = om
            .region_draft
            .iter()
            .map(|p| cam.world_to_screen(rect, *p))
            .collect();
        for p in &pts {
            canvas_painter.circle_filled(*p, 4.0, accent);
            canvas_painter.circle_stroke(*p, 4.0, Stroke::new(1.5, Color32::WHITE));
        }
        if let (Some(c), true) = (om.cursor, show_ghost) {
            pts.push(cam.world_to_screen(rect, c));
        }
        if pts.len() >= 3 {
            pts.push(pts[0]);
        }
        canvas_painter.add(Shape::line(pts, Stroke::new(2.0, accent)));
    }

    // Zona en construcción.
    if om.tool == Tool::Zone && !om.zone_draft.is_empty() && !clean {
        let mut pts: Vec<Pos2> = om
            .zone_draft
            .iter()
            .map(|p| cam.world_to_screen(rect, *p))
            .collect();
        for p in &pts {
            canvas_painter.circle_filled(*p, 4.0, accent);
            canvas_painter.circle_stroke(*p, 4.0, Stroke::new(1.5, Color32::WHITE));
        }
        if let (Some(c), true) = (om.cursor, show_ghost) {
            pts.push(cam.world_to_screen(rect, c));
        }
        if pts.len() >= 3 {
            pts.push(pts[0]);
        }
        canvas_painter.extend(Shape::dashed_line(&pts, Stroke::new(2.0, accent), 8.0, 5.0));
    }

    // Selección: asset.
    if let (Some(id), false) = (om.selected, clean) {
        if let Some(a) = doc.assets.iter().find(|a| a.id == id) {
            let size = sprite_size(lib, tex, ctx, &a.asset);
            let pts: Vec<Pos2> = a
                .corners(size)
                .iter()
                .map(|c| cam.world_to_screen(rect, *c))
                .collect();
            canvas_painter.add(Shape::closed_line(pts.clone(), Stroke::new(2.0, accent)));
            for p in pts {
                canvas_painter.rect_filled(
                    Rect::from_center_size(p, Vec2::splat(7.0)),
                    1.0,
                    accent,
                );
            }
        }
    }

    // Selección: camino (curva + nodos editables).
    if let (Some(id), false) = (om.sel_path, clean) {
        if let Some(p) = doc.path(id) {
            let pts: Vec<Pos2> = paths::curve(p.kind, &p.points())
                .into_iter()
                .map(|(q, _)| cam.world_to_screen(rect, q))
                .collect();
            canvas_painter.add(Shape::line(pts, Stroke::new(1.5, accent)));
            for n in &p.nodes {
                let s = cam.world_to_screen(rect, pos2(n[0], n[1]));
                let r = Rect::from_center_size(s, Vec2::splat(9.0));
                canvas_painter.rect_filled(r, 2.0, Color32::WHITE);
                canvas_painter.rect_stroke(r, 2.0, Stroke::new(2.0, accent), StrokeKind::Inside);
            }
        }
    }

    // Selección: región (vértices editables).
    if let (Some(id), false) = (om.sel_region, clean) {
        if let Some(g) = doc.region(id) {
            let mut ring: Vec<Pos2> = g
                .pts()
                .into_iter()
                .map(|p| cam.world_to_screen(rect, p))
                .collect();
            let first = ring[0];
            ring.push(first);
            canvas_painter.add(Shape::line(ring, Stroke::new(1.5, accent)));
            for n in &g.points {
                let s = cam.world_to_screen(rect, pos2(n[0], n[1]));
                let r = Rect::from_center_size(s, Vec2::splat(9.0));
                canvas_painter.rect_filled(r, 2.0, Color32::WHITE);
                canvas_painter.rect_stroke(r, 2.0, Stroke::new(2.0, accent), StrokeKind::Inside);
            }
        }
    }

    // Selección múltiple: contorno de cada asset y recuadro de selección.
    if !clean {
        for id in &om.multi {
            if let Some(a) = doc.assets.iter().find(|a| a.id == *id) {
                let size = sprite_size(lib, tex, ctx, &a.asset);
                let pts: Vec<Pos2> = a
                    .corners(size)
                    .iter()
                    .map(|c| cam.world_to_screen(rect, *c))
                    .collect();
                canvas_painter.add(Shape::closed_line(pts, Stroke::new(2.0, accent)));
            }
        }
        if let (Some((start, _)), Some(cur)) = (&om.marquee, om.cursor) {
            let r = Rect::from_two_pos(
                cam.world_to_screen(rect, *start),
                cam.world_to_screen(rect, cur),
            );
            canvas_painter.rect_filled(r, 0.0, accent.gamma_multiply(0.18));
            canvas_painter.rect_stroke(r, 0.0, Stroke::new(1.0, accent), StrokeKind::Inside);
        }
    }

    // Selección: zona (vértices editables).
    if let (Some(id), false) = (om.sel_zone, clean) {
        if let Some(z) = doc.zone(id) {
            for n in &z.points {
                let s = cam.world_to_screen(rect, pos2(n[0], n[1]));
                let r = Rect::from_center_size(s, Vec2::splat(9.0));
                canvas_painter.rect_filled(r, 2.0, Color32::WHITE);
                canvas_painter.rect_stroke(r, 2.0, Stroke::new(2.0, accent), StrokeKind::Inside);
            }
        }
    }

    // Fantasma del pincel de assets.
    if show_ghost && om.tool == Tool::Place {
        if let (Some(key), Some(p)) = (om.brushes.last(), om.cursor) {
            let size = sprite_size(lib, tex, ctx, key);
            let ghost = document::Placed {
                id: 0,
                asset: key.clone(),
                layer: om.active,
                pos: [p.x, p.y],
                rotation: 0.0,
                scale: (STAMP_TARGET / size.x.max(size.y)).min(1.0),
                z: 0,
                link: None,
                relief: 0.0,
            };
            let blocked = om.object_layer_ok(om.active).is_err();
            let t = if blocked {
                ERROR_COLOR.gamma_multiply(0.5)
            } else {
                tint.gamma_multiply(0.55)
            };
            draw_asset(
                &painter,
                ctx,
                rect,
                &cam,
                lib,
                tex,
                &ghost,
                t,
                Vec2::ZERO,
                false,
            );
        }
    }

    // Cursor de pincel / borrador / dispersión.
    if show_ghost {
        if let Some(c) = om.cursor {
            let s = cam.world_to_screen(rect, c);
            let ring = |r: f32, soft: bool| {
                let a = if soft { 0.35 } else { 1.0 };
                canvas_painter.circle_stroke(
                    s,
                    r,
                    Stroke::new(2.0, Color32::WHITE.gamma_multiply(0.8 * a)),
                );
                canvas_painter.circle_stroke(
                    s,
                    r,
                    Stroke::new(1.0, Color32::BLACK.gamma_multiply(0.7 * a)),
                );
            };
            match om.tool {
                Tool::Brush | Tool::Eraser => {
                    let r = (om.brush.size / 2.0 * cam.zoom).max(2.0);
                    ring(r, false);
                    if om.brush.hardness < 0.95 {
                        ring((r * om.brush.hardness).max(1.0), true);
                    }
                }
                Tool::Scatter => ring((om.scatter.radius * cam.zoom).max(3.0), false),
                _ => {}
            }
        }
    }

    // Borde del mapa.
    if let Some(m) = map_screen {
        canvas_painter.rect_stroke(
            m,
            0.0,
            Stroke::new(1.5, doc.style.coast()),
            StrokeKind::Outside,
        );
    }
}

/// Sprite PNG de un trazo (solo caminos de piedra y murallas), si está elegido y carga.
fn path_sprite(
    p: &paths::VPath,
    lib: &AssetLibrary,
    tex: &mut TextureCache,
    ctx: &Context,
) -> Option<(egui::TextureId, Vec2)> {
    let usable = p.kind == PathKind::Muralla
        || (p.kind == PathKind::Camino && p.material == Material::Piedra);
    if !usable {
        return None;
    }
    let info = lib.get(p.sprite.as_deref()?)?;
    tex.get(ctx, &info.path)
        .ok()
        .map(|t| (t.id(), t.size_vec2()))
}

/// Assets de torre de la biblioteca: (circular, cuadrada), buscando por nombre
/// (`Tower`/`Torre`, `Circular`/`Round`, `Square`/`Cuadrada`).
fn find_tower_assets(lib: &AssetLibrary) -> (Option<String>, Option<String>) {
    let has = |i: &assets::AssetInfo, words: &[&str]| {
        let n = format!("{} {}", i.name, i.key).to_lowercase();
        words.iter().any(|w| n.contains(w))
    };
    let towers: Vec<&assets::AssetInfo> = lib
        .items
        .iter()
        .filter(|i| has(i, &["tower", "torre"]))
        .collect();
    let round_w = ["circular", "round", "redond"];
    let square_w = ["square", "cuadrad"];
    let pick = |own: &[&str], other: &[&str]| {
        towers
            .iter()
            .find(|i| has(i, own))
            .or_else(|| towers.iter().find(|i| !has(i, other)))
            .or(towers.first())
            .map(|i| i.key.clone())
    };
    (pick(&round_w, &square_w), pick(&square_w, &round_w))
}

/// Dibuja las torres: sprite de la biblioteca si existe; si no, la torre cenital de respaldo.
#[allow(clippy::too_many_arguments)]
fn draw_towers(
    painter: &egui::Painter,
    ctx: &Context,
    view: Rect,
    cam: &Camera,
    lib: &AssetLibrary,
    tex: &mut TextureCache,
    found: &(Option<String>, Option<String>),
    list: &[paths::Tower],
    width: f32,
    style: MapStyle,
    tint: Color32,
    alpha: f32,
) {
    for t in list {
        let key = if t.round { &found.0 } else { &found.1 };
        match key {
            Some(k) => {
                let size = sprite_size(lib, tex, ctx, k);
                let diameter = width * 1.9 * t.scale;
                let placed = document::Placed {
                    id: 0,
                    asset: k.clone(),
                    layer: 0,
                    pos: [t.pos.x, t.pos.y],
                    // Las torres cuadradas se alinean con el muro.
                    rotation: if t.round { 0.0 } else { t.angle.to_degrees() },
                    scale: diameter / size.x.max(size.y).max(1.0),
                    z: 0,
                    link: None,
                    relief: 0.0,
                };
                draw_asset(
                    painter,
                    ctx,
                    view,
                    cam,
                    lib,
                    tex,
                    &placed,
                    tint.gamma_multiply(alpha),
                    Vec2::ZERO,
                    false,
                );
            }
            None => {
                let c = cam.world_to_screen(view, t.pos);
                let r = paths::tower_radius(width, cam.zoom, t.scale);
                paths::paint_tower(painter, c, t.round, t.angle, r, style, alpha);
            }
        }
    }
}

/// Sombra proyectada de un asset: su silueta, desplazada según el sol (3 copias para estirarla).
#[allow(clippy::too_many_arguments)]
fn draw_shadow(
    painter: &egui::Painter,
    ctx: &Context,
    view: Rect,
    cam: &Camera,
    lib: &AssetLibrary,
    tex: &mut TextureCache,
    a: &document::Placed,
    sun: &Sun,
) {
    let size = sprite_size(lib, tex, ctx, &a.asset);
    // El relieve (0–1) alarga y oscurece la sombra: montañas y colinas se leen con volumen.
    let relief = a.relief.clamp(0.0, 1.0);
    let reach = sun.dir * sun.len * (size.y * a.scale) * 0.4 * (1.0 + relief * 2.5);
    let alpha = (0.14 * (0.4 + 0.6 * sun.strength) * (1.0 + relief * 0.8)).min(0.4);
    let tint = Color32::from_black_alpha((alpha * 255.0) as u8);
    if relief > 0.05 {
        let steps = 6;
        let tint = Color32::from_black_alpha((alpha * 0.7 * 255.0) as u8);
        for i in 1..=steps {
            let f = i as f32 / steps as f32;
            draw_asset(painter, ctx, view, cam, lib, tex, a, tint, reach * f, true);
        }
    } else {
        for f in [0.45, 0.75, 1.0] {
            draw_asset(painter, ctx, view, cam, lib, tex, a, tint, reach * f, true);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_asset(
    painter: &egui::Painter,
    ctx: &Context,
    view: Rect,
    cam: &Camera,
    lib: &AssetLibrary,
    tex: &mut TextureCache,
    a: &document::Placed,
    tint: Color32,
    offset: Vec2,
    shadow: bool,
) {
    let loaded = lib
        .get(&a.asset)
        .map(|i| tex.get(ctx, &i.path).map(|t| (t.id(), t.size_vec2())));
    let size = match &loaded {
        Some(Ok((_, s))) => *s,
        _ => vec2(64.0, 64.0),
    };
    let pts = a
        .corners(size)
        .map(|c| cam.world_to_screen(view, c + offset));

    // Descarte rápido fuera de la vista.
    let bounds = Rect::from_points(&pts);
    if !bounds.intersects(view) {
        return;
    }

    match loaded {
        Some(Ok((id, _))) => {
            let mut mesh = Mesh::with_texture(id);
            let uvs = [
                pos2(0.0, 0.0),
                pos2(1.0, 0.0),
                pos2(1.0, 1.0),
                pos2(0.0, 1.0),
            ];
            for i in 0..4 {
                mesh.vertices.push(Vertex {
                    pos: pts[i],
                    uv: uvs[i],
                    color: tint,
                });
            }
            mesh.indices.extend([0, 1, 2, 0, 2, 3]);
            painter.add(Shape::mesh(mesh));
        }
        _ if shadow => {}
        _ => {
            // Asset faltante: caja roja con «?».
            painter.add(Shape::closed_line(
                pts.to_vec(),
                Stroke::new(1.5, ERROR_COLOR),
            ));
            painter.text(
                bounds.center(),
                Align2::CENTER_CENTER,
                "?",
                FontId::proportional(16.0),
                ERROR_COLOR,
            );
        }
    }
}

fn draw_grid(
    painter: &egui::Painter,
    view: Rect,
    cam: &Camera,
    g: &GridSettings,
    bounds: Option<Rect>,
) {
    let mut vis = Rect::from_min_max(
        cam.screen_to_world(view, view.min),
        cam.screen_to_world(view, view.max),
    );
    if let Some(b) = bounds {
        vis = vis.intersect(b);
    }
    if vis.width() <= 0.0 || vis.height() <= 0.0 {
        return;
    }
    let alpha = (g.opacity.clamp(0.0, 1.0) * 255.0) as u8;
    let stroke = Stroke::new(1.0, Color32::from_black_alpha(alpha));
    let w2s = |p: Pos2| cam.world_to_screen(view, p);

    match g.kind {
        GridKind::Square => {
            let mut step = g.size.max(1.0);
            while step * cam.zoom < 6.0 {
                step *= 2.0;
            }
            let mut shapes = Vec::new();
            let mut x = (vis.min.x / step).floor() * step;
            while x <= vis.max.x {
                shapes.push(Shape::line_segment(
                    [w2s(pos2(x, vis.min.y)), w2s(pos2(x, vis.max.y))],
                    stroke,
                ));
                x += step;
            }
            let mut y = (vis.min.y / step).floor() * step;
            while y <= vis.max.y {
                shapes.push(Shape::line_segment(
                    [w2s(pos2(vis.min.x, y)), w2s(pos2(vis.max.x, y))],
                    stroke,
                ));
                y += step;
            }
            painter.extend(shapes);
        }
        GridKind::Hex => {
            if g.size * cam.zoom < 10.0 {
                return;
            }
            // Hexágonos de punta arriba cuyo ancho entre lados es `size`.
            let w = g.size;
            let r = w / 3f32.sqrt();
            let row_h = r * 1.5;
            let j0 = (vis.min.y / row_h).floor() as i32 - 1;
            let j1 = (vis.max.y / row_h).ceil() as i32 + 1;
            let i0 = (vis.min.x / w).floor() as i32 - 1;
            let i1 = (vis.max.x / w).ceil() as i32 + 1;
            let mut shapes = Vec::new();
            for j in j0..=j1 {
                let off = if j.rem_euclid(2) == 1 { w / 2.0 } else { 0.0 };
                for i in i0..=i1 {
                    let c = pos2(i as f32 * w + off, j as f32 * row_h);
                    let pts: Vec<Pos2> = (0..6)
                        .map(|k| {
                            let a = (30.0 + 60.0 * k as f32).to_radians();
                            w2s(pos2(c.x + r * a.cos(), c.y + r * a.sin()))
                        })
                        .collect();
                    shapes.push(Shape::closed_line(pts, stroke));
                }
            }
            painter.extend(shapes);
        }
    }
}
