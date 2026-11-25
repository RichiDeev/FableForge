// src/map/document.rs
//! Modelo de datos del mapa y su serialización (`03 - Mapas/<nombre>.ffmap.json`).

use crate::i18n::tr;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use egui::{pos2, vec2, Color32, Pos2, Vec2};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::map::brush::{TerrainBuffer, TerrainPatch, TerrainRef};
use crate::map::paths::{PathKind, VPath};
use crate::map::zones::Zone;
use crate::story::MAPS_DIR;

pub const MAP_EXT: &str = ".ffmap.json";
pub const DEFAULT_WORLD: [f32; 2] = [2000.0, 1400.0];
/// Versión actual del formato (2 = capas dinámicas, z-order, zonas, enlaces y hora).
pub const FORMAT_VERSION: u32 = 2;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum MapStyle {
    Atlas,
    Pergamino,
}

impl MapStyle {
    pub const ALL: [MapStyle; 2] = [MapStyle::Atlas, MapStyle::Pergamino];

    pub fn label(self) -> &'static str {
        match self {
            MapStyle::Atlas => "Atlas (Tinta Viva a Color)",
            MapStyle::Pergamino => "Pergamino (Clásico Sepia/Café)",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            MapStyle::Atlas => tr("Atlas"),
            MapStyle::Pergamino => tr("Pergamino"),
        }
    }

    /// Fondo de un lienzo en blanco.
    pub fn background(self) -> Color32 {
        match self {
            MapStyle::Atlas => Color32::from_rgb(0x8D, 0xBB, 0xD0),
            MapStyle::Pergamino => Color32::from_rgb(0xE9, 0xD9, 0xB4),
        }
    }

    pub fn land(self) -> Color32 {
        match self {
            MapStyle::Atlas => Color32::from_rgb(0xE6, 0xD9, 0xB0),
            MapStyle::Pergamino => Color32::from_rgb(0xEB, 0xDD, 0xB8),
        }
    }

    pub fn water(self) -> Color32 {
        match self {
            MapStyle::Atlas => Color32::from_rgb(0x8D, 0xBB, 0xD0),
            MapStyle::Pergamino => Color32::from_rgb(0xCD, 0xB9, 0x8F),
        }
    }

    pub fn coast(self) -> Color32 {
        match self {
            MapStyle::Atlas => Color32::from_rgb(0x4C, 0x7F, 0x94),
            MapStyle::Pergamino => Color32::from_rgb(0x6B, 0x4F, 0x34),
        }
    }
}

/// Ids de las capas por defecto (también sirven para migrar mapas del formato 1).
pub const ID_CAMINOS: u64 = 1;
pub const ID_ESTRUCTURAS: u64 = 2;
pub const ID_NATURALEZA: u64 = 3;
pub const ID_TERRENO: u64 = 4;

/// Una capa del mapa. La lista `MapDoc::layers` va de arriba hacia abajo.
///
/// La capa marcada como `terrain` contiene la imagen base y el terreno pintado (solo pintura);
/// las demás contienen assets y caminos.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: u64,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    #[serde(default)]
    pub terrain: bool,
}

/// Capas de un mapa nuevo, arriba → abajo. Naturaleza queda sobre Caminos para que las copas
/// de los árboles cubran riberas y bordes de caminos.
pub fn default_layers() -> Vec<Layer> {
    [
        (ID_NATURALEZA, "Naturaleza", false),
        (ID_ESTRUCTURAS, "Estructuras", false),
        (ID_CAMINOS, "Caminos y Puentes", false),
        (ID_TERRENO, "Terreno", true),
    ]
    .into_iter()
    .map(|(id, name, terrain)| Layer {
        id,
        name: name.to_owned(),
        visible: true,
        locked: false,
        terrain,
    })
    .collect()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum GridKind {
    Square,
    Hex,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GridSettings {
    pub enabled: bool,
    pub kind: GridKind,
    pub size: f32,
    pub opacity: f32,
}

impl Default for GridSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            kind: GridKind::Square,
            size: 50.0,
            opacity: 0.35,
        }
    }
}

/// Imagen base de la capa Terreno (en `05 - Medias/Mapas/`), colocada en (0,0).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BaseImage {
    pub file: String,
    pub size: [f32; 2],
}

/// Instancia de un asset colocado en el mapa.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placed {
    pub id: u64,
    /// Clave de la biblioteca (`g:...` global, `s:...` de la historia).
    pub asset: String,
    /// Id de la capa a la que pertenece.
    pub layer: u64,
    pub pos: [f32; 2],
    /// Grados, sentido horario en pantalla.
    pub rotation: f32,
    pub scale: f32,
    /// Orden de apilado dentro de la capa (mayor = más al frente).
    #[serde(default)]
    pub z: i32,
    /// Mapa interior vinculado (nombre de archivo dentro de `03 - Mapas/`).
    #[serde(default)]
    pub link: Option<String>,
    /// Altura / relieve (0–1): montañas y colinas proyectan sombras más largas y oscuras.
    #[serde(default)]
    pub relief: f32,
}

impl Placed {
    pub fn pos2(&self) -> Pos2 {
        pos2(self.pos[0], self.pos[1])
    }

    fn rot(v: Vec2, deg: f32) -> Vec2 {
        let (s, c) = deg.to_radians().sin_cos();
        vec2(v.x * c - v.y * s, v.x * s + v.y * c)
    }

    /// Esquinas en coordenadas del mundo para un sprite de `size` px naturales.
    pub fn corners(&self, size: Vec2) -> [Pos2; 4] {
        let h = size * self.scale / 2.0;
        [
            vec2(-h.x, -h.y),
            vec2(h.x, -h.y),
            vec2(h.x, h.y),
            vec2(-h.x, h.y),
        ]
        .map(|c| self.pos2() + Self::rot(c, self.rotation))
    }

    pub fn contains(&self, p: Pos2, size: Vec2) -> bool {
        let local = Self::rot(p - self.pos2(), -self.rotation);
        let h = size * self.scale / 2.0;
        local.x.abs() <= h.x && local.y.abs() <= h.y
    }
}

/// Región de terreno vectorial: polígono cerrado de color sólido con borde nítido opcional,
/// editable por vértices. Se ve limpio a cualquier zoom (costas, lagos, plataformas).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub id: u64,
    pub name: String,
    /// Vértices en coordenadas del mundo (polígono cerrado implícito).
    pub points: Vec<[f32; 2]>,
    pub color: [u8; 3],
    #[serde(default = "default_true")]
    pub border: bool,
    #[serde(default = "default_border_color")]
    pub border_color: [u8; 3],
    /// Grosor del borde en px del mundo.
    #[serde(default = "default_border_width")]
    pub border_width: f32,
}

fn default_true() -> bool {
    true
}

fn default_border_color() -> [u8; 3] {
    [0x4A, 0x3A, 0x2A]
}

fn default_border_width() -> f32 {
    3.0
}

impl Region {
    pub fn pts(&self) -> Vec<Pos2> {
        self.points.iter().map(|p| pos2(p[0], p[1])).collect()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct MapDoc {
    pub version: u32,
    pub name: String,
    pub style: MapStyle,
    /// `None` = lienzo infinito.
    pub size: Option<[f32; 2]>,
    pub grid: GridSettings,
    /// Orden de arriba hacia abajo.
    pub layers: Vec<Layer>,
    pub base: Option<BaseImage>,
    pub assets: Vec<Placed>,
    /// Caminos y ríos vectoriales.
    pub paths: Vec<VPath>,
    /// Zonas / territorios (se dibujan sobre todas las capas).
    pub zones: Vec<Zone>,
    pub zones_visible: bool,
    /// Regiones de terreno vectoriales (sobre el terreno pintado, bajo caminos y assets).
    pub regions: Vec<Region>,
    /// Terreno pintado: atlas PNG hermano del `.ffmap.json`.
    pub terrain: Option<TerrainRef>,
    /// Hora del día (0–24) para iluminación y sombras.
    pub hour: f32,
    pub shadows: bool,
    /// Texturas activas: imagen base del terreno y sprites PNG de caminos/murallas.
    pub textures: bool,
    pub next_id: u64,
}

impl Default for MapDoc {
    fn default() -> Self {
        Self::new("Mapa", MapStyle::Atlas, Some(DEFAULT_WORLD))
    }
}

/// Convierte un JSON del formato 1 (capas fijas por tipo) al formato 2.
fn migrate_v1(v: &mut Value) {
    let id_of = |k: &str| match k {
        "Caminos" => ID_CAMINOS,
        "Estructuras" => ID_ESTRUCTURAS,
        "Naturaleza" => ID_NATURALEZA,
        _ => ID_TERRENO,
    };
    let name_of = |k: &str| match k {
        "Caminos" => "Caminos y Puentes",
        "Estructuras" => "Estructuras",
        "Naturaleza" => "Naturaleza",
        _ => "Terreno",
    };
    if let Some(layers) = v.get_mut("layers").and_then(Value::as_array_mut) {
        for l in layers {
            let kind = l
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("Terreno")
                .to_owned();
            l["id"] = json!(id_of(&kind));
            l["name"] = json!(name_of(&kind));
            l["terrain"] = json!(kind == "Terreno");
            if let Some(o) = l.as_object_mut() {
                o.remove("kind");
            }
        }
    }
    if let Some(assets) = v.get_mut("assets").and_then(Value::as_array_mut) {
        for a in assets {
            if let Some(k) = a.get("layer").and_then(Value::as_str).map(str::to_owned) {
                a["layer"] = json!(id_of(&k));
            }
        }
    }
    v["version"] = json!(FORMAT_VERSION);
}

impl MapDoc {
    pub fn new(name: &str, style: MapStyle, size: Option<[f32; 2]>) -> Self {
        Self {
            version: FORMAT_VERSION,
            name: name.to_owned(),
            style,
            size,
            grid: GridSettings::default(),
            layers: default_layers(),
            base: None,
            assets: Vec::new(),
            paths: Vec::new(),
            zones: Vec::new(),
            zones_visible: true,
            regions: Vec::new(),
            terrain: None,
            hour: 12.0,
            shadows: true,
            textures: true,
            next_id: 1,
        }
    }

    // ------------------------------------------------------------------ capas

    pub fn layer(&self, id: u64) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    pub fn layer_mut(&mut self, id: u64) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }

    pub fn is_visible(&self, id: u64) -> bool {
        self.layer(id).is_none_or(|l| l.visible)
    }

    pub fn is_locked(&self, id: u64) -> bool {
        self.layer(id).is_some_and(|l| l.locked)
    }

    pub fn is_terrain_layer(&self, id: u64) -> bool {
        self.layer(id).is_some_and(|l| l.terrain)
    }

    pub fn terrain_layer(&self) -> u64 {
        self.layers
            .iter()
            .find(|l| l.terrain)
            .map_or(ID_TERRENO, |l| l.id)
    }

    pub fn first_object_layer(&self) -> u64 {
        self.layers
            .iter()
            .find(|l| !l.terrain)
            .map_or(ID_NATURALEZA, |l| l.id)
    }

    /// Primera capa de objetos cuyo nombre contiene `needle` (sin distinguir mayúsculas);
    /// si no hay, la primera capa de objetos.
    pub fn object_layer_named(&self, needle: &str) -> u64 {
        let n = needle.to_lowercase();
        self.layers
            .iter()
            .find(|l| !l.terrain && l.name.to_lowercase().contains(&n))
            .map_or_else(|| self.first_object_layer(), |l| l.id)
    }

    pub fn layer_count(&self, id: u64) -> usize {
        self.assets.iter().filter(|a| a.layer == id).count()
            + self.paths.iter().filter(|p| p.layer == id).count()
    }

    /// Crea una capa de objetos justo encima de `above` (o arriba del todo).
    pub fn add_layer(&mut self, name: &str, above: Option<u64>) -> u64 {
        let id = self.layers.iter().map(|l| l.id).max().unwrap_or(0) + 1;
        let at = above
            .and_then(|a| self.layers.iter().position(|l| l.id == a))
            .unwrap_or(0);
        self.layers.insert(
            at,
            Layer {
                id,
                name: name.to_owned(),
                visible: true,
                locked: false,
                terrain: false,
            },
        );
        id
    }

    /// Elimina una capa de objetos. Su contenido pasa a la capa de objetos vecina (primero la de
    /// abajo). No se puede eliminar la capa de terreno ni la última capa de objetos.
    pub fn remove_layer(&mut self, id: u64) -> bool {
        let Some(idx) = self.layers.iter().position(|l| l.id == id) else {
            return false;
        };
        if self.layers[idx].terrain || self.layers.iter().filter(|l| !l.terrain).count() < 2 {
            return false;
        }
        let dest = self.layers[idx + 1..]
            .iter()
            .chain(self.layers[..idx].iter().rev())
            .find(|l| !l.terrain)
            .map(|l| l.id)
            .unwrap_or_else(|| self.first_object_layer());
        for a in self.assets.iter_mut().filter(|a| a.layer == id) {
            a.layer = dest;
        }
        for p in self.paths.iter_mut().filter(|p| p.layer == id) {
            p.layer = dest;
        }
        self.layers.remove(idx);
        true
    }

    /// Sube o baja una capa en la lista. `false` si ya está en el extremo.
    pub fn move_layer(&mut self, id: u64, up: bool) -> bool {
        let Some(idx) = self.layers.iter().position(|l| l.id == id) else {
            return false;
        };
        let to = if up {
            match idx.checked_sub(1) {
                Some(t) => t,
                None => return false,
            }
        } else {
            idx + 1
        };
        if to >= self.layers.len() {
            return false;
        }
        self.layers.swap(idx, to);
        true
    }

    // ------------------------------------------------------------------ assets

    /// Copia como referencia lo que hay en `region` de `src` (el mapa general): ríos, caminos,
    /// murallas y assets. Se reescala (proporcional) para que `region` llene este mapa y todo va a
    /// una capa nueva y bloqueada. Devuelve `(trazos, assets)` copiados.
    pub fn import_context(&mut self, src: &MapDoc, region: egui::Rect) -> (usize, usize) {
        let target = self.size.unwrap_or(DEFAULT_WORLD);
        let k = (target[0] / region.width().max(1.0)).min(target[1] / region.height().max(1.0));
        let off = pos2(target[0] / 2.0, target[1] / 2.0) - region.center().to_vec2() * k;
        let map = |p: Pos2| pos2(p.x * k + off.x, p.y * k + off.y);

        let mut runs: Vec<(&VPath, Vec<Pos2>)> = Vec::new();
        for p in &src.paths {
            let pts = p.points();
            let mut cur: Vec<Pos2> = Vec::new();
            for w in pts.windows(2) {
                match clip_segment(w[0], w[1], region) {
                    Some((a, b)) => {
                        if cur.last().is_none_or(|l| (*l - a).length() > 0.01) {
                            if cur.len() >= 2 {
                                runs.push((p, std::mem::take(&mut cur)));
                            }
                            cur.clear();
                            cur.push(a);
                        }
                        cur.push(b);
                    }
                    None => {
                        if cur.len() >= 2 {
                            runs.push((p, std::mem::take(&mut cur)));
                        }
                        cur.clear();
                    }
                }
            }
            if cur.len() >= 2 {
                runs.push((p, cur));
            }
        }
        let near: Vec<&Placed> = src
            .assets
            .iter()
            .filter(|a| region.contains(a.pos2()))
            .collect();
        if runs.is_empty() && near.is_empty() {
            return (0, 0);
        }
        let layer = self.add_layer("Referencia (mapa general)", None);
        if let Some(l) = self.layer_mut(layer) {
            l.locked = true;
        }
        let n_paths = runs.len();
        for (p, nodes) in runs {
            let nodes: Vec<Pos2> = nodes.into_iter().map(map).collect();
            let id = self.add_path(p.kind, &nodes, (p.width * k).max(2.0), p.dashed, layer);
            if let Some(n) = self.path_mut(id) {
                n.material = p.material;
                n.tower_style = p.tower_style;
                n.tower_every = p.tower_every * k;
                n.sprite = p.sprite.clone();
            }
        }
        let n_assets = near.len();
        for a in near {
            let id = self.add_asset(&a.asset, layer, map(a.pos2()), a.scale * k);
            if let Some(n) = self.asset_mut(id) {
                n.rotation = a.rotation;
                n.z = a.z;
                n.relief = a.relief;
            }
        }
        (n_paths, n_assets)
    }

    /// Duplica un asset (desplazado) y devuelve el id de la copia.
    pub fn duplicate_asset(&mut self, id: u64) -> Option<u64> {
        let mut copy = self.assets.iter().find(|a| a.id == id)?.clone();
        copy.id = self.next_id;
        self.next_id += 1;
        copy.pos[0] += 28.0;
        copy.pos[1] += 28.0;
        copy.z += 1;
        let new = copy.id;
        self.assets.push(copy);
        Some(new)
    }

    pub fn add_asset(&mut self, asset: &str, layer: u64, pos: Pos2, scale: f32) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.assets.push(Placed {
            id,
            asset: asset.to_owned(),
            layer,
            pos: [pos.x, pos.y],
            rotation: 0.0,
            scale,
            z: 0,
            link: None,
            relief: 0.0,
        });
        id
    }

    pub fn remove_asset(&mut self, id: u64) {
        self.assets.retain(|a| a.id != id);
    }

    pub fn asset_mut(&mut self, id: u64) -> Option<&mut Placed> {
        self.assets.iter_mut().find(|a| a.id == id)
    }

    /// Assets de una capa en orden de dibujo (de atrás hacia adelante; estable por `z`).
    pub fn sorted_assets(&self, layer: u64) -> Vec<&Placed> {
        let mut v: Vec<&Placed> = self.assets.iter().filter(|a| a.layer == layer).collect();
        v.sort_by_key(|a| a.z);
        v
    }

    /// Lleva el asset al frente de su capa.
    pub fn bring_front(&mut self, id: u64) {
        let Some(layer) = self.assets.iter().find(|a| a.id == id).map(|a| a.layer) else {
            return;
        };
        let top = self
            .assets
            .iter()
            .filter(|a| a.layer == layer && a.id != id)
            .map(|a| a.z)
            .max()
            .unwrap_or(0);
        if let Some(a) = self.asset_mut(id) {
            a.z = a.z.max(top + 1);
        }
    }

    /// Envía el asset al fondo de su capa.
    pub fn send_back(&mut self, id: u64) {
        let Some(layer) = self.assets.iter().find(|a| a.id == id).map(|a| a.layer) else {
            return;
        };
        let bottom = self
            .assets
            .iter()
            .filter(|a| a.layer == layer && a.id != id)
            .map(|a| a.z)
            .min()
            .unwrap_or(0);
        if let Some(a) = self.asset_mut(id) {
            a.z = a.z.min(bottom - 1);
        }
    }

    // ------------------------------------------------------------------ caminos y zonas

    pub fn add_path(
        &mut self,
        kind: PathKind,
        nodes: &[Pos2],
        width: f32,
        dashed: bool,
        layer: u64,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.paths.push(VPath {
            id,
            kind,
            nodes: nodes.iter().map(|p| [p.x, p.y]).collect(),
            width,
            dashed,
            layer,
            material: Default::default(),
            tower_style: Default::default(),
            tower_every: 0.0,
            sprite: None,
        });
        id
    }

    pub fn remove_path(&mut self, id: u64) {
        self.paths.retain(|p| p.id != id);
    }

    pub fn path(&self, id: u64) -> Option<&VPath> {
        self.paths.iter().find(|p| p.id == id)
    }

    pub fn path_mut(&mut self, id: u64) -> Option<&mut VPath> {
        self.paths.iter_mut().find(|p| p.id == id)
    }

    pub fn add_zone(&mut self, name: &str, points: &[Pos2], color: [u8; 3]) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.zones.push(Zone {
            id,
            name: name.to_owned(),
            points: points.iter().map(|p| [p.x, p.y]).collect(),
            color,
            link: None,
            relief: 0.0,
        });
        id
    }

    pub fn add_region(&mut self, name: &str, points: &[Pos2], color: [u8; 3]) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.regions.push(Region {
            id,
            name: name.to_owned(),
            points: points.iter().map(|p| [p.x, p.y]).collect(),
            color,
            border: true,
            border_color: default_border_color(),
            border_width: default_border_width(),
        });
        id
    }

    pub fn remove_region(&mut self, id: u64) {
        self.regions.retain(|r| r.id != id);
    }

    pub fn region(&self, id: u64) -> Option<&Region> {
        self.regions.iter().find(|r| r.id == id)
    }

    pub fn region_mut(&mut self, id: u64) -> Option<&mut Region> {
        self.regions.iter_mut().find(|r| r.id == id)
    }

    pub fn remove_zone(&mut self, id: u64) {
        self.zones.retain(|z| z.id != id);
    }

    pub fn zone(&self, id: u64) -> Option<&Zone> {
        self.zones.iter().find(|z| z.id == id)
    }

    pub fn zone_mut(&mut self, id: u64) -> Option<&mut Zone> {
        self.zones.iter_mut().find(|z| z.id == id)
    }

    // ------------------------------------------------------------------ historial

    /// Copia de lo editable por deshacer.
    pub fn scene(&self) -> Scene {
        Scene {
            layers: self.layers.clone(),
            assets: self.assets.clone(),
            paths: self.paths.clone(),
            zones: self.zones.clone(),
            regions: self.regions.clone(),
        }
    }

    pub fn apply_scene(&mut self, s: &Scene) {
        self.layers = s.layers.clone();
        self.assets = s.assets.clone();
        self.paths = s.paths.clone();
        self.zones = s.zones.clone();
        self.regions = s.regions.clone();
    }

    // ------------------------------------------------------------------ vista y disco

    /// Área del mundo que se encuadra con «Ajustar».
    pub fn content_rect(&self) -> egui::Rect {
        if let Some([w, h]) = self.size {
            return egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(w, h));
        }
        if let Some(b) = &self.base {
            return egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(b.size[0], b.size[1]));
        }
        let mut r =
            egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(DEFAULT_WORLD[0], DEFAULT_WORLD[1]));
        for a in &self.assets {
            r.extend_with(a.pos2());
        }
        for n in self
            .paths
            .iter()
            .flat_map(|p| p.nodes.iter())
            .chain(self.zones.iter().flat_map(|z| z.points.iter()))
            .chain(self.regions.iter().flat_map(|g| g.points.iter()))
        {
            r.extend_with(pos2(n[0], n[1]));
        }
        r
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)
                .map_err(|e| format!("No se pudo crear la carpeta de mapas: {e}"))?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, json).map_err(|e| format!("No se pudo guardar el mapa: {e}"))?;
        fs::rename(&tmp, path).map_err(|e| format!("No se pudo guardar el mapa: {e}"))
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path).map_err(|e| format!("No se pudo leer el mapa: {e}"))?;
        let invalid = |e: serde_json::Error| format!("El mapa no es válido: {e}");
        let mut value: Value = serde_json::from_str(&text).map_err(invalid)?;
        if value.get("version").and_then(Value::as_u64).unwrap_or(1) < 2 {
            migrate_v1(&mut value);
        }
        let mut doc: MapDoc = serde_json::from_value(value).map_err(invalid)?;
        doc.fix_references();
        Ok(doc)
    }

    /// Garantiza una capa de terreno, al menos una capa de objetos y referencias válidas.
    fn fix_references(&mut self) {
        let next = |layers: &[Layer]| layers.iter().map(|l| l.id).max().unwrap_or(0) + 1;
        if !self.layers.iter().any(|l| l.terrain) {
            let id = next(&self.layers);
            self.layers.push(Layer {
                id,
                name: "Terreno".to_owned(),
                visible: true,
                locked: false,
                terrain: true,
            });
        }
        if !self.layers.iter().any(|l| !l.terrain) {
            let id = next(&self.layers);
            self.layers.insert(
                0,
                Layer {
                    id,
                    name: "Capa 1".to_owned(),
                    visible: true,
                    locked: false,
                    terrain: false,
                },
            );
        }
        let valid: HashSet<u64> = self
            .layers
            .iter()
            .filter(|l| !l.terrain)
            .map(|l| l.id)
            .collect();
        let (first, roads) = (self.first_object_layer(), self.object_layer_named("camino"));
        for a in &mut self.assets {
            if !valid.contains(&a.layer) {
                a.layer = first;
            }
        }
        for p in &mut self.paths {
            if !valid.contains(&p.layer) {
                p.layer = roads;
            }
        }
        let max_id = self
            .assets
            .iter()
            .map(|a| a.id)
            .chain(self.paths.iter().map(|p| p.id))
            .chain(self.zones.iter().map(|z| z.id))
            .chain(self.regions.iter().map(|g| g.id))
            .max()
            .unwrap_or(0);
        self.next_id = self.next_id.max(max_id + 1);
        self.version = FORMAT_VERSION;
    }
}

/// Estado editable por deshacer: capas, assets, caminos y zonas.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    pub layers: Vec<Layer>,
    pub assets: Vec<Placed>,
    pub paths: Vec<VPath>,
    pub zones: Vec<Zone>,
    pub regions: Vec<Region>,
}

enum Command {
    Scene { before: Scene, after: Scene },
    Terrain(TerrainPatch),
}

/// Pila de deshacer/rehacer (Ctrl+Z / Ctrl+Y).
///
/// Los cambios de capas, assets, caminos y zonas se detectan comparando la escena con la última
/// confirmada (`sync`), así cualquier edición queda cubierta sin registrar comandos a mano; los
/// trazos de pincel entran como parches de chunks.
pub struct UndoStack {
    undo: Vec<Command>,
    redo: Vec<Command>,
    last: Scene,
}

impl UndoStack {
    const LIMIT: usize = 80;

    pub fn new(doc: &MapDoc) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            last: doc.scene(),
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    fn push(&mut self, c: Command) {
        self.undo.push(c);
        if self.undo.len() > Self::LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Confirma como un paso de historial lo que cambió en la escena desde la última vez.
    pub fn sync(&mut self, doc: &MapDoc) -> bool {
        if doc.assets == self.last.assets
            && doc.paths == self.last.paths
            && doc.zones == self.last.zones
            && doc.regions == self.last.regions
            && doc.layers == self.last.layers
        {
            return false;
        }
        let after = doc.scene();
        let before = std::mem::replace(&mut self.last, after.clone());
        self.push(Command::Scene { before, after });
        true
    }

    pub fn push_terrain(&mut self, patch: TerrainPatch) {
        self.push(Command::Terrain(patch));
    }

    pub fn undo(&mut self, doc: &mut MapDoc, terrain: &mut TerrainBuffer) -> bool {
        self.sync(doc);
        let Some(cmd) = self.undo.pop() else {
            return false;
        };
        match &cmd {
            Command::Scene { before, .. } => {
                doc.apply_scene(before);
                self.last = before.clone();
            }
            Command::Terrain(p) => terrain.apply_patch(p, true),
        }
        self.redo.push(cmd);
        true
    }

    pub fn redo(&mut self, doc: &mut MapDoc, terrain: &mut TerrainBuffer) -> bool {
        self.sync(doc);
        let Some(cmd) = self.redo.pop() else {
            return false;
        };
        match &cmd {
            Command::Scene { after, .. } => {
                doc.apply_scene(after);
                self.last = after.clone();
            }
            Command::Terrain(p) => terrain.apply_patch(p, false),
        }
        self.undo.push(cmd);
        true
    }
}

pub fn maps_dir(story: &Path) -> PathBuf {
    story.join(MAPS_DIR)
}

#[derive(Clone, Debug)]
pub struct MapSummary {
    pub name: String,
    pub path: PathBuf,
    pub style: MapStyle,
    pub size: Option<[f32; 2]>,
    pub modified: Option<SystemTime>,
}

/// Lista los `*.ffmap.json` de `03 - Mapas/`, más recientes primero.
pub fn list_maps(story: &Path) -> Vec<MapSummary> {
    let Ok(rd) = fs::read_dir(maps_dir(story)) else {
        return Vec::new();
    };
    let mut out: Vec<MapSummary> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .is_some_and(|n| n.to_string_lossy().ends_with(MAP_EXT))
        })
        .filter_map(|p| {
            let doc = MapDoc::load(&p).ok()?;
            let modified = fs::metadata(&p).and_then(|m| m.modified()).ok();
            Some(MapSummary {
                name: doc.name,
                path: p,
                style: doc.style,
                size: doc.size,
                modified,
            })
        })
        .collect();
    out.sort_by(|a, b| b.modified.cmp(&a.modified));
    out
}

/// Recorta el segmento `a`-`b` a `r` (Liang–Barsky).
fn clip_segment(a: Pos2, b: Pos2, r: egui::Rect) -> Option<(Pos2, Pos2)> {
    let d = b - a;
    let (mut t0, mut t1) = (0.0_f32, 1.0_f32);
    for (p, q) in [
        (-d.x, a.x - r.min.x),
        (d.x, r.max.x - a.x),
        (-d.y, a.y - r.min.y),
        (d.y, r.max.y - a.y),
    ] {
        if p.abs() < 1e-9 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return None;
            }
        }
    }
    Some((a + d * t0, a + d * t1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_roundtrip_and_layers() {
        let dir = std::env::temp_dir().join(format!("ff_map_{}", std::process::id()));
        let path = dir.join("x.ffmap.json");
        let mut d = MapDoc::new("Reino", MapStyle::Pergamino, None);
        let id = d.add_asset("g:Oak_1.png", ID_NATURALEZA, pos2(10.0, 20.0), 0.5);
        d.asset_mut(id).unwrap().rotation = 45.0;
        d.asset_mut(id).unwrap().link = Some("Capital.ffmap.json".into());
        d.add_zone(
            "Norte",
            &[pos2(0.0, 0.0), pos2(10.0, 0.0), pos2(5.0, 9.0)],
            [1, 2, 3],
        );
        d.hour = 18.5;
        d.save(&path).unwrap();
        let l = MapDoc::load(&path).unwrap();
        assert_eq!(l.name, "Reino");
        assert_eq!(l.assets[0].rotation, 45.0);
        assert_eq!(l.assets[0].link.as_deref(), Some("Capital.ffmap.json"));
        assert_eq!(l.zones.len(), 1);
        assert_eq!(l.hour, 18.5);
        assert_eq!(l.layers.len(), 4);
        // Naturaleza queda por encima de Caminos en los mapas nuevos.
        let pos = |id| l.layers.iter().position(|x| x.id == id).unwrap();
        assert!(pos(ID_NATURALEZA) < pos(ID_CAMINOS));
        assert!(l.layers.last().unwrap().terrain);
        assert_eq!(list_maps(dir.parent().unwrap()).len(), 0); // no está en 03 - Mapas
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn migrates_format_one_maps() {
        let dir = std::env::temp_dir().join(format!("ff_map_v1_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("viejo.ffmap.json");
        fs::write(
            &path,
            r#"{"version":1,"name":"Viejo","style":"Atlas","size":[2000.0,1400.0],
            "layers":[{"kind":"Caminos","visible":true,"locked":false},
                      {"kind":"Estructuras","visible":false,"locked":false},
                      {"kind":"Naturaleza","visible":true,"locked":true},
                      {"kind":"Terreno","visible":true,"locked":false}],
            "assets":[{"id":7,"asset":"g:Oak_1.png","layer":"Naturaleza","pos":[1.0,2.0],"rotation":0.0,"scale":1.0}],
            "paths":[{"id":8,"kind":"Rio","nodes":[[0.0,0.0],[5.0,5.0]],"width":10.0}],
            "next_id":9}"#,
        )
        .unwrap();
        let d = MapDoc::load(&path).unwrap();
        assert_eq!(d.version, FORMAT_VERSION);
        assert_eq!(d.layers[0].name, "Caminos y Puentes"); // se respeta el orden viejo
        assert!(!d.layers[1].visible);
        assert!(d.layers[2].locked && d.layers[2].id == ID_NATURALEZA);
        assert!(d.layers[3].terrain);
        assert_eq!(d.assets[0].layer, ID_NATURALEZA);
        assert_eq!(d.paths[0].layer, ID_CAMINOS);
        assert_eq!(d.hour, 12.0);
        assert!(d.next_id >= 9);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn walls_and_branches_roundtrip() {
        let dir = std::env::temp_dir().join(format!("ff_map_wall_{}", std::process::id()));
        let path = dir.join("m.ffmap.json");
        let mut d = MapDoc::new("Muros", MapStyle::Atlas, None);
        let river = d.add_path(
            PathKind::Rio,
            &[pos2(0.0, 0.0), pos2(100.0, 0.0), pos2(200.0, 0.0)],
            14.0,
            false,
            ID_CAMINOS,
        );
        let wall = d.add_path(
            PathKind::Muralla,
            &[pos2(0.0, 50.0), pos2(80.0, 50.0), pos2(80.0, 120.0)],
            10.0,
            false,
            ID_CAMINOS,
        );
        let branch = d.add_path(
            PathKind::Rio,
            &[pos2(150.0, 0.0), pos2(150.0, 90.0)],
            9.0,
            false,
            ID_CAMINOS,
        );
        crate::map::paths::attach_to_hosts(&mut d.paths, branch);
        d.save(&path).unwrap();
        let l = MapDoc::load(&path).unwrap();
        assert_eq!(l.path(wall).unwrap().kind, PathKind::Muralla);
        // La bifurcación comparte un nodo exacto con el río principal.
        assert!(l.path(river).unwrap().nodes.contains(&[150.0, 0.0]));
        assert_eq!(l.path(branch).unwrap().nodes[0], [150.0, 0.0]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn materials_towers_and_old_files() {
        use crate::map::paths::{Material, TowerStyle};
        let mut d = MapDoc::new("M", MapStyle::Atlas, None);
        let w = d.add_path(
            PathKind::Muralla,
            &[pos2(0.0, 0.0), pos2(300.0, 0.0)],
            10.0,
            false,
            ID_CAMINOS,
        );
        let c = d.add_path(
            PathKind::Camino,
            &[pos2(0.0, 10.0), pos2(300.0, 10.0)],
            8.0,
            false,
            ID_CAMINOS,
        );
        {
            let p = d.path_mut(w).unwrap();
            p.tower_style = TowerStyle::Circular;
            p.tower_every = 150.0;
        }
        d.path_mut(c).unwrap().material = Material::Piedra;
        let json = serde_json::to_string(&d).unwrap();
        let l: MapDoc = serde_json::from_str(&json).unwrap();
        assert_eq!(l.path(w).unwrap().tower_style, TowerStyle::Circular);
        assert_eq!(l.path(w).unwrap().tower_every, 150.0);
        assert_eq!(l.path(c).unwrap().material, Material::Piedra);
        // Un trazo guardado antes de esta etapa carga con valores por defecto.
        let old: crate::map::paths::VPath = serde_json::from_str(
            r#"{"id":1,"kind":"Camino","nodes":[[0.0,0.0],[1.0,1.0]],"width":8.0}"#,
        )
        .unwrap();
        assert_eq!(old.material, Material::Tierra);
        assert_eq!(old.tower_every, 0.0);
    }

    #[test]
    fn submap_receives_nearby_context_as_locked_reference() {
        let mut g = MapDoc::new("General", MapStyle::Atlas, Some([4000.0, 3000.0]));
        // Un río que cruza la región de lado a lado y uno lejano.
        g.add_path(
            PathKind::Rio,
            &[
                pos2(0.0, 1000.0),
                pos2(2000.0, 1000.0),
                pos2(4000.0, 1000.0),
            ],
            20.0,
            false,
            ID_CAMINOS,
        );
        g.add_path(
            PathKind::Camino,
            &[pos2(3000.0, 2500.0), pos2(3500.0, 2600.0)],
            8.0,
            false,
            ID_CAMINOS,
        );
        let a = g.add_asset("g:Oak_1.png", ID_NATURALEZA, pos2(1100.0, 1050.0), 1.0);
        g.asset_mut(a).unwrap().relief = 0.6;
        g.add_asset("g:Oak_1.png", ID_NATURALEZA, pos2(3500.0, 2800.0), 1.0);

        let region = egui::Rect::from_center_size(pos2(1000.0, 1000.0), vec2(800.0, 600.0));
        let mut sub = MapDoc::new("Interior", MapStyle::Atlas, Some([2000.0, 1400.0]));
        let (np, na) = sub.import_context(&g, region);
        assert_eq!((np, na), (1, 1));
        let layer = sub
            .layers
            .iter()
            .find(|l| l.name.starts_with("Referencia"))
            .unwrap();
        assert!(layer.locked);
        // El río recortado cruza el submapa de lado a lado, ya reescalado.
        let r = &sub.paths[0];
        assert_eq!(r.layer, layer.id);
        let (x0, x1) = (r.nodes.first().unwrap()[0], r.nodes.last().unwrap()[0]);
        // 800×600 se ajusta (proporcional, centrado) a 2000×1400: escala 2.33.
        assert!(
            (x0 - 66.7).abs() < 1.0 && (x1 - 1933.3).abs() < 1.0,
            "{x0} {x1}"
        );
        assert!(r.width > 20.0); // escala > 1
        assert_eq!(sub.assets[0].relief, 0.6);
        // Región vacía: no se crea capa.
        let mut empty = MapDoc::new("Vacío", MapStyle::Atlas, Some([2000.0, 1400.0]));
        let far = egui::Rect::from_center_size(pos2(500.0, 2800.0), vec2(100.0, 100.0));
        assert_eq!(empty.import_context(&g, far), (0, 0));
        assert_eq!(empty.layers.len(), 4);
    }

    #[test]
    fn textures_flag_and_sprites_roundtrip() {
        let mut d = MapDoc::new("T", MapStyle::Atlas, None);
        d.textures = false;
        let id = d.add_path(
            PathKind::Muralla,
            &[pos2(0.0, 0.0), pos2(50.0, 0.0)],
            10.0,
            false,
            ID_CAMINOS,
        );
        d.path_mut(id).unwrap().sprite = Some("g:Muralla.png".into());
        let z = d.add_zone(
            "Colina",
            &[pos2(0.0, 0.0), pos2(9.0, 0.0), pos2(4.0, 8.0)],
            [1, 2, 3],
        );
        d.zone_mut(z).unwrap().relief = 0.8;
        let l: MapDoc = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert!(!l.textures);
        assert_eq!(l.path(id).unwrap().sprite.as_deref(), Some("g:Muralla.png"));
        assert_eq!(l.zone(z).unwrap().relief, 0.8);
        // Un archivo anterior no trae el campo: texturas activas por defecto.
        let old: MapDoc = serde_json::from_str(r#"{"name":"Viejo"}"#).unwrap();
        assert!(old.textures);
    }

    #[test]
    fn regions_roundtrip_and_undo() {
        let mut d = MapDoc::new("R", MapStyle::Atlas, None);
        let mut h = UndoStack::new(&d);
        let id = d.add_region(
            "Lago",
            &[pos2(0.0, 0.0), pos2(100.0, 0.0), pos2(50.0, 80.0)],
            [10, 20, 30],
        );
        assert!(h.sync(&d));
        let json = serde_json::to_string(&d).unwrap();
        let back: MapDoc = serde_json::from_str(&json).unwrap();
        assert_eq!(back.regions.len(), 1);
        assert!(back.region(id).unwrap().border);
        // Un JSON antiguo sin regiones carga igual.
        let old: MapDoc = serde_json::from_str("{\"name\":\"x\"}").unwrap();
        assert!(old.regions.is_empty());
        let mut t = TerrainBuffer::new(None);
        assert!(h.undo(&mut d, &mut t));
        assert!(d.regions.is_empty());
        assert!(h.redo(&mut d, &mut t));
        assert_eq!(d.regions.len(), 1);
        d.remove_region(id);
        assert!(d.regions.is_empty());
    }

    #[test]
    fn duplicate_asset_copies_and_offsets() {
        let mut d = MapDoc::new("D", MapStyle::Atlas, None);
        let id = d.add_asset("g:Oak_1.png", ID_NATURALEZA, pos2(10.0, 20.0), 0.5);
        d.asset_mut(id).unwrap().rotation = 45.0;
        let n = d.duplicate_asset(id).unwrap();
        assert_ne!(n, id);
        let a = d.assets.iter().find(|a| a.id == n).unwrap();
        assert_eq!((a.rotation, a.scale), (45.0, 0.5));
        assert!(a.pos[0] > 10.0);
        assert!(d.duplicate_asset(9999).is_none());
    }

    #[test]
    fn layer_management() {
        let mut d = MapDoc::new("L", MapStyle::Atlas, None);
        let new = d.add_layer("Copas", Some(ID_NATURALEZA));
        assert_eq!(d.layers[0].id, new);
        assert_eq!(d.layers[0].name, "Copas");

        assert!(d.move_layer(new, false));
        assert_eq!(d.layers[1].id, new);
        assert!(d.move_layer(new, true));
        assert!(!d.move_layer(new, true)); // ya está arriba
        assert!(!d.move_layer(ID_TERRENO, false)); // ya está abajo

        let a = d.add_asset("g:x.png", new, pos2(0.0, 0.0), 1.0);
        d.add_path(
            PathKind::Camino,
            &[pos2(0.0, 0.0), pos2(9.0, 9.0)],
            8.0,
            false,
            new,
        );
        assert_eq!(d.layer_count(new), 2);
        assert!(d.remove_layer(new));
        assert_eq!(
            d.assets.iter().find(|x| x.id == a).unwrap().layer,
            ID_NATURALEZA
        );
        assert_eq!(d.paths[0].layer, ID_NATURALEZA);

        assert!(!d.remove_layer(ID_TERRENO));
        assert!(d.remove_layer(ID_NATURALEZA));
        assert!(d.remove_layer(ID_ESTRUCTURAS));
        assert!(!d.remove_layer(ID_CAMINOS)); // es la última capa de objetos
    }

    #[test]
    fn z_order_inside_a_layer() {
        let mut d = MapDoc::new("Z", MapStyle::Atlas, None);
        let a = d.add_asset("a", ID_NATURALEZA, pos2(0.0, 0.0), 1.0);
        let b = d.add_asset("b", ID_NATURALEZA, pos2(0.0, 0.0), 1.0);
        let c = d.add_asset("c", ID_NATURALEZA, pos2(0.0, 0.0), 1.0);
        let order = |d: &MapDoc| {
            d.sorted_assets(ID_NATURALEZA)
                .iter()
                .map(|x| x.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(order(&d), vec![a, b, c]);
        d.bring_front(a);
        assert_eq!(order(&d), vec![b, c, a]);
        d.send_back(c);
        assert_eq!(order(&d), vec![c, b, a]);
    }

    #[test]
    fn undo_redo_scene_layers_paths_and_zones() {
        let mut d = MapDoc::new("H", MapStyle::Atlas, None);
        let mut t = TerrainBuffer::new(None);
        let mut h = UndoStack::new(&d);
        assert!(!h.sync(&d));

        let a = d.add_asset("g:Oak_1.png", ID_NATURALEZA, pos2(5.0, 5.0), 1.0);
        assert!(h.sync(&d));
        d.add_path(
            PathKind::Rio,
            &[pos2(0.0, 0.0), pos2(50.0, 20.0)],
            10.0,
            false,
            ID_CAMINOS,
        );
        assert!(h.sync(&d));
        d.add_layer("Extra", None);
        assert!(h.sync(&d));
        d.add_zone(
            "Z",
            &[pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(0.0, 1.0)],
            [0, 0, 0],
        );
        assert!(h.sync(&d));
        d.asset_mut(a).unwrap().rotation = 30.0;
        assert!(h.sync(&d));

        assert!(h.undo(&mut d, &mut t));
        assert_eq!(d.assets[0].rotation, 0.0);
        assert!(h.undo(&mut d, &mut t));
        assert!(d.zones.is_empty());
        assert!(h.undo(&mut d, &mut t));
        assert_eq!(d.layers.len(), 4);
        assert!(h.undo(&mut d, &mut t));
        assert!(d.paths.is_empty());
        assert!(h.undo(&mut d, &mut t));
        assert!(d.assets.is_empty());
        assert!(!h.undo(&mut d, &mut t));

        for _ in 0..4 {
            assert!(h.redo(&mut d, &mut t));
        }
        assert_eq!(d.layers.len(), 5);
        // Una edición nueva invalida el rehacer.
        d.remove_asset(a);
        h.sync(&d);
        assert!(!h.can_redo());
    }

    #[test]
    fn rotated_hit_test() {
        let mut p = Placed {
            id: 1,
            asset: String::new(),
            layer: ID_NATURALEZA,
            pos: [100.0, 100.0],
            rotation: 0.0,
            scale: 1.0,
            z: 0,
            link: None,
            relief: 0.0,
        };
        let size = vec2(100.0, 20.0);
        assert!(p.contains(pos2(140.0, 100.0), size));
        assert!(!p.contains(pos2(100.0, 130.0), size));
        p.rotation = 90.0; // ahora es alto y angosto
        assert!(p.contains(pos2(100.0, 140.0), size));
        assert!(!p.contains(pos2(140.0, 100.0), size));
        let c = p.corners(size);
        assert!((c[0] - pos2(110.0, 50.0)).length() < 1e-3);
    }
}
