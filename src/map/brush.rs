// src/map/brush.rs
//! Motor raster del terreno: buffer por *chunks* de 256×256 px (sirve para mapas de tamaño fijo y
//! para lienzos infinitos), pincel con interpolación continua, borrador, parches para deshacer y
//! guardado/carga en un PNG hermano (`<mapa>.terrain.png`).
//!
//! Los píxeles se guardan con alfa premultiplicado (el formato nativo de `egui::Color32`).

use std::collections::HashMap;
use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use egui::{
    pos2, Color32, ColorImage, Context, Painter, Pos2, Rect, TextureHandle, TextureOptions,
};
use serde::{Deserialize, Serialize};

use crate::map::camera::Camera;
use crate::map::document::MAP_EXT;

pub const CHUNK: usize = 256;
/// Supersampling: el buffer tiene el doble de resolución que el mapa (1 px del mundo = SS px).
pub const SS: f32 = 2.0;
/// En lienzo infinito se pinta como máximo a ±LIMIT chunks del origen (±16 384 px del mundo).
const LIMIT: i32 = 128;

type Key = (i32, i32);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Referencia al PNG de terreno guardada dentro del `.ffmap.json`.
///
/// El PNG es un atlas: los chunks no vacíos, ordenados por `chunks`, empaquetados en una grilla.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TerrainRef {
    pub file: String,
    pub chunks: Vec<[i32; 2]>,
    /// Resolución del buffer respecto al mundo (los PNG de la 3.2 no lo traen: 1×).
    #[serde(default = "legacy_scale")]
    pub scale: u32,
}

fn legacy_scale() -> u32 {
    1
}

#[derive(Clone, Copy, Debug)]
pub struct BrushParams {
    pub radius: f32,
    /// 0 = muy suave, 1 = borde duro.
    pub hardness: f32,
    /// Techo de opacidad de todo el trazo.
    pub opacity: f32,
    pub color: Color32,
    /// `true` = borrar (resta alfa).
    pub erase: bool,
}

#[derive(Clone)]
struct ChunkPatch {
    key: Key,
    /// `None` si el chunk no existía antes del trazo.
    before: Option<Vec<Color32>>,
    after: Vec<Color32>,
}

/// Cambios de un trazo, para deshacer/rehacer.
#[derive(Clone, Default)]
pub struct TerrainPatch {
    chunks: Vec<ChunkPatch>,
}

struct Chunk {
    px: Vec<Color32>,
    dirty: bool,
    tex: Option<TextureHandle>,
}

impl Chunk {
    fn new() -> Self {
        Self {
            px: vec![Color32::TRANSPARENT; CHUNK * CHUNK],
            dirty: true,
            tex: None,
        }
    }

    fn from_pixels(px: Vec<Color32>) -> Self {
        Self {
            px,
            dirty: true,
            tex: None,
        }
    }
}

struct StrokeChunk {
    existed: bool,
    base: Vec<Color32>,
    /// Cobertura acumulada del trazo (0..1) por píxel.
    cover: Vec<f32>,
}

struct Stroke {
    chunks: HashMap<Key, StrokeChunk>,
    last: Option<Pos2>,
}

pub struct TerrainBuffer {
    id: u64,
    chunks: HashMap<Key, Chunk>,
    bounds: Option<[f32; 2]>,
    stroke: Option<Stroke>,
    /// Hay cambios sin guardar en el PNG.
    pub dirty: bool,
}

/// Ruta del PNG de terreno de un mapa: `x.ffmap.json` → `x.terrain.png`.
pub fn terrain_path(map: &Path) -> PathBuf {
    let name = map
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = name.strip_suffix(MAP_EXT).unwrap_or(&name);
    map.with_file_name(format!("{stem}.terrain.png"))
}

fn chunk_of(v: i32) -> i32 {
    v.div_euclid(CHUNK as i32)
}

/// Ancho (px del buffer) de la transición suave del borde exterior (≈0.6 px del mundo): bordes
/// nítidos pero sin escalones.
const AA: f32 = 1.25;

/// Tolerancia (por canal, 0–255) para considerar «del mismo color» a un píxel al rellenar.
const FILL_TOL: i32 = 12;
/// Radio de la ventana de relleno en lienzo infinito (px del buffer).
const FILL_WINDOW: i32 = 2048;
/// Máximo de píxeles del buffer que puede analizar un relleno.
const FILL_MAX_PIXELS: usize = 30_000_000;

/// Caída radial: núcleo sólido hasta `hardness`, suavizado (smoothstep) hasta el borde y un
/// anti-aliasing subpíxel de `AA` px centrado en el radio.
fn falloff(d: f32, r: f32, hardness: f32) -> f32 {
    let e = ((r - d) / AA + 0.5).clamp(0.0, 1.0);
    let edge = e * e * (3.0 - 2.0 * e);
    if edge <= 0.0 {
        return 0.0;
    }
    let h = hardness.clamp(0.0, 0.99);
    let t = d / r;
    let core = if t <= h {
        1.0
    } else {
        let u = ((t - h) / (1.0 - h)).clamp(0.0, 1.0);
        1.0 - u * u * (3.0 - 2.0 * u)
    };
    core * edge
}

fn over(dst: Color32, src: Color32, a: f32) -> Color32 {
    let k = 1.0 - a;
    let f = |s: u8, d: u8| (s as f32 * a + d as f32 * k).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_premultiplied(
        f(src.r(), dst.r()),
        f(src.g(), dst.g()),
        f(src.b(), dst.b()),
        (255.0 * a + dst.a() as f32 * k).round().clamp(0.0, 255.0) as u8,
    )
}

fn fade(dst: Color32, a: f32) -> Color32 {
    let k = 1.0 - a;
    let f = |d: u8| (d as f32 * k).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_premultiplied(f(dst.r()), f(dst.g()), f(dst.b()), f(dst.a()))
}

fn unpremultiply(c: Color32) -> [u8; 4] {
    let a = c.a();
    if a == 0 {
        return [0, 0, 0, 0];
    }
    let f = |v: u8| ((v as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8;
    [f(c.r()), f(c.g()), f(c.b()), a]
}

fn grid_cols(n: usize) -> usize {
    ((n as f64).sqrt().ceil() as usize).max(1)
}

impl TerrainBuffer {
    pub fn new(bounds: Option<[f32; 2]>) -> Self {
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            chunks: HashMap::new(),
            bounds,
            stroke: None,
            dirty: false,
        }
    }

    #[cfg(test)]
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    #[cfg(test)]
    pub fn alpha_at(&self, x: i32, y: i32) -> u8 {
        let (x, y) = ((x as f32 * SS) as i32, (y as f32 * SS) as i32);
        let key = (chunk_of(x), chunk_of(y));
        self.chunks.get(&key).map_or(0, |c| {
            let lx = x.rem_euclid(CHUNK as i32) as usize;
            let ly = y.rem_euclid(CHUNK as i32) as usize;
            c.px[ly * CHUNK + lx].a()
        })
    }

    // ------------------------------------------------------------------ trazos

    pub fn begin_stroke(&mut self) {
        self.stroke = Some(Stroke {
            chunks: HashMap::new(),
            last: None,
        });
    }

    /// Continúa el trazo hasta `p`. El recorrido se divide en tramos de `max(radio·0.35, 4)` px y
    /// cada tramo se pinta como una cápsula acotada a su *bounding box*: continuo sin importar la
    /// velocidad del cursor y con un costo proporcional al área realmente pintada.
    pub fn stroke_to(&mut self, p: Pos2, params: &BrushParams) {
        // Coordenadas y radio del mundo → píxeles del buffer supersampleado.
        let p = pos2(p.x * SS, p.y * SS);
        let mut params = *params;
        params.radius *= SS;
        let params = &params;
        let step = (params.radius * 0.35).max(4.0);
        let mut segs: Vec<(Pos2, Pos2)> = Vec::new();
        {
            let Some(st) = self.stroke.as_mut() else {
                return;
            };
            match st.last {
                None => segs.push((p, p)),
                Some(l) => {
                    let d = (p - l).length();
                    if d <= f32::EPSILON {
                        return;
                    }
                    let n = (d / step).ceil().max(1.0) as usize;
                    let mut prev = l;
                    for i in 1..=n {
                        let q = l + (p - l) * (i as f32 / n as f32);
                        segs.push((prev, q));
                        prev = q;
                    }
                }
            }
            st.last = Some(p);
        }
        for (a, b) in segs {
            self.stamp(a, b, params);
        }
    }

    /// Cierra el trazo y devuelve lo que cambió (o `None` si no hubo cambios).
    pub fn end_stroke(&mut self) -> Option<TerrainPatch> {
        let st = self.stroke.take()?;
        let mut out = Vec::new();
        for (key, sc) in st.chunks {
            if let Some(c) = self.chunks.get(&key) {
                if c.px != sc.base {
                    out.push(ChunkPatch {
                        key,
                        before: sc.existed.then_some(sc.base),
                        after: c.px.clone(),
                    });
                }
            }
        }
        if out.is_empty() {
            return None;
        }
        self.dirty = true;
        Some(TerrainPatch { chunks: out })
    }

    /// Pinta la cápsula entre `a` y `b` (en px del buffer) dentro de su caja envolvente.
    fn stamp(&mut self, a: Pos2, b: Pos2, p: &BrushParams) {
        let r = p.radius.max(0.5);
        let ch = CHUNK as i32;
        let (lx0, ly0, lx1, ly1) = match self.bounds {
            Some([w, h]) => (0, 0, (w * SS).ceil() as i32, (h * SS).ceil() as i32),
            None => (-LIMIT * ch, -LIMIT * ch, LIMIT * ch, LIMIT * ch),
        };
        let pad = r + 2.0;
        let x0 = ((a.x.min(b.x) - pad).floor() as i32).max(lx0);
        let y0 = ((a.y.min(b.y) - pad).floor() as i32).max(ly0);
        let x1 = ((a.x.max(b.x) + pad).ceil() as i32).min(lx1);
        let y1 = ((a.y.max(b.y) + pad).ceil() as i32).min(ly1);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let Self { chunks, stroke, .. } = self;
        let Some(stroke) = stroke.as_mut() else {
            return;
        };
        let color = Color32::from_rgb(p.color.r(), p.color.g(), p.color.b());
        let (abx, aby) = (b.x - a.x, b.y - a.y);
        let len2 = abx * abx + aby * aby;
        let reach = (r + AA) * (r + AA);

        for cy in chunk_of(y0)..=chunk_of(y1 - 1) {
            for cx in chunk_of(x0)..=chunk_of(x1 - 1) {
                let key = (cx, cy);
                let existed = chunks.contains_key(&key);
                if p.erase && !existed {
                    continue; // nada que borrar
                }
                let chunk = chunks.entry(key).or_insert_with(Chunk::new);
                let sc = stroke.chunks.entry(key).or_insert_with(|| StrokeChunk {
                    existed,
                    base: chunk.px.clone(),
                    cover: vec![0.0; CHUNK * CHUNK],
                });
                let (ox, oy) = (cx * ch, cy * ch);
                for y in y0.max(oy)..y1.min(oy + ch) {
                    let dy = y as f32 + 0.5 - a.y;
                    for x in x0.max(ox)..x1.min(ox + ch) {
                        let dx = x as f32 + 0.5 - a.x;
                        let t = if len2 > 0.0 {
                            ((dx * abx + dy * aby) / len2).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        let (qx, qy) = (dx - t * abx, dy - t * aby);
                        let d2 = qx * qx + qy * qy;
                        if d2 > reach {
                            continue;
                        }
                        let cov_new = falloff(d2.sqrt(), r, p.hardness);
                        if cov_new <= 0.0 {
                            continue;
                        }
                        let i = (y - oy) as usize * CHUNK + (x - ox) as usize;
                        if cov_new <= sc.cover[i] {
                            continue;
                        }
                        sc.cover[i] = cov_new;
                        let alpha = cov_new * p.opacity;
                        chunk.px[i] = if p.erase {
                            fade(sc.base[i], alpha)
                        } else {
                            over(sc.base[i], color, alpha)
                        };
                    }
                }
                chunk.dirty = true;
            }
        }
    }

    // ------------------------------------------------------------------ relleno

    /// Copia densa (fila a fila) de la región `[x0,x1)×[y0,y1)` del buffer.
    fn read_region(&self, x0: i32, y0: i32, w: usize, h: usize) -> Vec<Color32> {
        let mut out = vec![Color32::TRANSPARENT; w * h];
        let (x1, y1) = (x0 + w as i32, y0 + h as i32);
        let ch = CHUNK as i32;
        for cy in chunk_of(y0)..=chunk_of(y1 - 1) {
            for cx in chunk_of(x0)..=chunk_of(x1 - 1) {
                let Some(c) = self.chunks.get(&(cx, cy)) else {
                    continue;
                };
                let (ox, oy) = (cx * ch, cy * ch);
                let (xa, xb) = (x0.max(ox), x1.min(ox + ch));
                for y in y0.max(oy)..y1.min(oy + ch) {
                    let src = (y - oy) as usize * CHUNK + (xa - ox) as usize;
                    let dst = (y - y0) as usize * w + (xa - x0) as usize;
                    let n = (xb - xa) as usize;
                    out[dst..dst + n].copy_from_slice(&c.px[src..src + n]);
                }
            }
        }
        out
    }

    /// Relleno tipo cubeta desde `seed` (coordenadas del mundo): colorea la región conectada de
    /// píxeles del mismo color, acotada por lo ya dibujado. En mapas de tamaño fijo el fondo se
    /// rellena hasta el borde del mapa; en lienzo infinito (o regiones enormes) la región debe estar
    /// cerrada. Devuelve el parche para deshacer.
    pub fn fill(
        &mut self,
        seed: Pos2,
        color: Color32,
        opacity: f32,
    ) -> Result<Option<TerrainPatch>, String> {
        let (sx, sy) = ((seed.x * SS).floor() as i32, (seed.y * SS).floor() as i32);
        // Ventana de análisis y qué lados son «artificiales» (si el relleno los toca, está abierto).
        let (mut x0, mut y0, mut x1, mut y1, mut art) = match self.bounds {
            Some([w, h]) => {
                let (bw, bh) = ((w * SS).ceil() as i32, (h * SS).ceil() as i32);
                if sx < 0 || sy < 0 || sx >= bw || sy >= bh {
                    return Err("Haz clic dentro del mapa para rellenar.".to_owned());
                }
                (0, 0, bw, bh, [false; 4])
            }
            None => (
                sx - FILL_WINDOW,
                sy - FILL_WINDOW,
                sx + FILL_WINDOW,
                sy + FILL_WINDOW,
                [true; 4],
            ),
        };
        if self.bounds.is_some() && (x1 - x0) as usize * (y1 - y0) as usize > FILL_MAX_PIXELS {
            let half = FILL_WINDOW;
            let (nx0, ny0) = ((sx - half).max(x0), (sy - half).max(y0));
            let (nx1, ny1) = ((sx + half).min(x1), (sy + half).min(y1));
            art = [nx0 > x0, ny0 > y0, nx1 < x1, ny1 < y1];
            (x0, y0, x1, y1) = (nx0, ny0, nx1, ny1);
        }
        let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
        let mut dense = self.read_region(x0, y0, w, h);
        let at = |x: i32, y: i32| (y - y0) as usize * w + (x - x0) as usize;
        let seed_px = dense[at(sx, sy)];
        let same = |c: Color32| {
            (c.r() as i32 - seed_px.r() as i32).abs() <= FILL_TOL
                && (c.g() as i32 - seed_px.g() as i32).abs() <= FILL_TOL
                && (c.b() as i32 - seed_px.b() as i32).abs() <= FILL_TOL
                && (c.a() as i32 - seed_px.a() as i32).abs() <= FILL_TOL
        };

        // Relleno por líneas (4-conexo).
        let mut mask = vec![0u8; w * h];
        let mut open = false;
        let mut stack = vec![(sx, sy)];
        while let Some((x, y)) = stack.pop() {
            let i = at(x, y);
            if mask[i] != 0 || !same(dense[i]) {
                continue;
            }
            let (mut l, mut r) = (x, x);
            while l > x0 && mask[at(l - 1, y)] == 0 && same(dense[at(l - 1, y)]) {
                l -= 1;
            }
            while r < x1 - 1 && mask[at(r + 1, y)] == 0 && same(dense[at(r + 1, y)]) {
                r += 1;
            }
            for xi in l..=r {
                mask[at(xi, y)] = 1;
            }
            open |= (art[0] && l == x0) || (art[2] && r == x1 - 1);
            open |= (art[1] && y == y0) || (art[3] && y == y1 - 1);
            if open {
                return Err(
                    "El área no está cerrada (o es demasiado grande): dibuja un contorno completo."
                        .to_owned(),
                );
            }
            for ny in [y - 1, y + 1] {
                if ny < y0 || ny >= y1 {
                    continue;
                }
                let mut inside = false;
                for xi in l..=r {
                    let j = at(xi, ny);
                    let ok = mask[j] == 0 && same(dense[j]);
                    if ok && !inside {
                        stack.push((xi, ny));
                    }
                    inside = ok;
                }
            }
        }

        // Halo de 2 px alrededor de la región: se rellena *por debajo* de lo dibujado para que el
        // borde suavizado del contorno no deje una línea del color viejo.
        let (mut bx0, mut by0, mut bx1, mut by1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        let mut edge: Vec<(i32, i32)> = Vec::new();
        for y in y0..y1 {
            for x in x0..x1 {
                if mask[at(x, y)] != 1 {
                    continue;
                }
                bx0 = bx0.min(x);
                by0 = by0.min(y);
                bx1 = bx1.max(x + 1);
                by1 = by1.max(y + 1);
                let nb = [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)];
                if nb.iter().any(|&(nx, ny)| {
                    nx < x0 || ny < y0 || nx >= x1 || ny >= y1 || mask[at(nx, ny)] != 1
                }) {
                    edge.push((x, y));
                }
            }
        }
        if bx0 > bx1 {
            return Ok(None);
        }
        for (x, y) in edge {
            for dy in -2..=2 {
                for dx in -2..=2 {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx >= x0 && ny >= y0 && nx < x1 && ny < y1 && mask[at(nx, ny)] == 0 {
                        mask[at(nx, ny)] = 2;
                    }
                }
            }
        }
        let (bx0, by0, bx1, by1) = (
            (bx0 - 2).max(x0),
            (by0 - 2).max(y0),
            (bx1 + 2).min(x1),
            (by1 + 2).min(y1),
        );

        // Aplica por chunks, registrando el estado previo para deshacer.
        let ch = CHUNK as i32;
        let fill_px = Color32::from_rgb(color.r(), color.g(), color.b());
        let fill_pm = [
            fill_px.r() as f32 * opacity,
            fill_px.g() as f32 * opacity,
            fill_px.b() as f32 * opacity,
            255.0 * opacity,
        ];
        self.begin_stroke();
        {
            let Self { chunks, stroke, .. } = self;
            let Some(stroke) = stroke.as_mut() else {
                return Ok(None);
            };
            for cy in chunk_of(by0)..=chunk_of(by1 - 1) {
                for cx in chunk_of(bx0)..=chunk_of(bx1 - 1) {
                    let (ox, oy) = (cx * ch, cy * ch);
                    let (xa, xb) = (bx0.max(ox), bx1.min(ox + ch));
                    let (ya, yb) = (by0.max(oy), by1.min(oy + ch));
                    let any = (ya..yb).any(|y| (xa..xb).any(|x| mask[at(x, y)] != 0));
                    if !any {
                        continue;
                    }
                    let key = (cx, cy);
                    let existed = chunks.contains_key(&key);
                    let chunk = chunks.entry(key).or_insert_with(Chunk::new);
                    stroke.chunks.entry(key).or_insert_with(|| StrokeChunk {
                        existed,
                        base: chunk.px.clone(),
                        cover: Vec::new(),
                    });
                    for y in ya..yb {
                        for x in xa..xb {
                            let i = (y - oy) as usize * CHUNK + (x - ox) as usize;
                            match mask[at(x, y)] {
                                1 => chunk.px[i] = over(chunk.px[i], fill_px, opacity),
                                2 => {
                                    let d = chunk.px[i];
                                    let k = 1.0 - d.a() as f32 / 255.0;
                                    let f = |dv: u8, fv: f32| {
                                        (dv as f32 + fv * k).round().clamp(0.0, 255.0) as u8
                                    };
                                    chunk.px[i] = Color32::from_rgba_premultiplied(
                                        f(d.r(), fill_pm[0]),
                                        f(d.g(), fill_pm[1]),
                                        f(d.b(), fill_pm[2]),
                                        f(d.a(), fill_pm[3]),
                                    );
                                }
                                _ => {}
                            }
                        }
                    }
                    chunk.dirty = true;
                }
            }
        }
        dense.clear();
        Ok(self.end_stroke())
    }

    // ------------------------------------------------------------------ deshacer

    pub fn apply_patch(&mut self, patch: &TerrainPatch, undo: bool) {
        for cp in &patch.chunks {
            let px = if undo {
                match &cp.before {
                    Some(b) => b.clone(),
                    None => {
                        self.chunks.remove(&cp.key);
                        continue;
                    }
                }
            } else {
                cp.after.clone()
            };
            self.chunks.insert(cp.key, Chunk::from_pixels(px));
        }
        self.dirty = true;
    }

    // ------------------------------------------------------------------ GPU

    /// Sube a la GPU los chunks modificados.
    pub fn sync(&mut self, ctx: &Context) {
        let id = self.id;
        for (key, c) in self.chunks.iter_mut().filter(|(_, c)| c.dirty) {
            let img = ColorImage::new([CHUNK, CHUNK], c.px.clone());
            match c.tex.as_mut() {
                Some(t) => t.set(img, TextureOptions::LINEAR),
                None => {
                    let name = format!("terrain_{id}_{}_{}", key.0, key.1);
                    c.tex = Some(ctx.load_texture(name, img, TextureOptions::LINEAR));
                }
            }
            c.dirty = false;
        }
    }

    pub fn draw(&self, painter: &Painter, view: Rect, cam: &Camera, tint: Color32) {
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let clip = painter.clip_rect().intersect(view);
        for (key, c) in &self.chunks {
            let Some(t) = &c.tex else { continue };
            let min = pos2(
                (key.0 * CHUNK as i32) as f32 / SS,
                (key.1 * CHUNK as i32) as f32 / SS,
            );
            let max = pos2(min.x + CHUNK as f32 / SS, min.y + CHUNK as f32 / SS);
            let r = Rect::from_min_max(
                cam.world_to_screen(view, min),
                cam.world_to_screen(view, max),
            );
            if r.intersects(clip) {
                painter.image(t.id(), r, uv, tint);
            }
        }
    }

    // ------------------------------------------------------------------ disco

    /// Guarda el atlas PNG. Devuelve la referencia para el JSON (o `None` si no hay terreno).
    pub fn save(&self, png: &Path) -> Result<Option<TerrainRef>, String> {
        let mut keys: Vec<Key> = self
            .chunks
            .iter()
            .filter(|(_, c)| c.px.iter().any(|p| p.a() > 0))
            .map(|(k, _)| *k)
            .collect();
        keys.sort();
        if keys.is_empty() {
            let _ = fs::remove_file(png);
            return Ok(None);
        }
        let cols = grid_cols(keys.len());
        let rows = keys.len().div_ceil(cols);
        let mut img = image::RgbaImage::new((cols * CHUNK) as u32, (rows * CHUNK) as u32);
        for (n, k) in keys.iter().enumerate() {
            let (gx, gy) = ((n % cols) * CHUNK, (n / cols) * CHUNK);
            let px = &self.chunks[k].px;
            for y in 0..CHUNK {
                for x in 0..CHUNK {
                    img.put_pixel(
                        (gx + x) as u32,
                        (gy + y) as u32,
                        image::Rgba(unpremultiply(px[y * CHUNK + x])),
                    );
                }
            }
        }
        if let Some(dir) = png.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("No se pudo crear la carpeta: {e}"))?;
        }
        let tmp = png.with_extension("png.tmp");
        let file =
            fs::File::create(&tmp).map_err(|e| format!("No se pudo guardar el terreno: {e}"))?;
        let enc = image::codecs::png::PngEncoder::new_with_quality(
            BufWriter::new(file),
            image::codecs::png::CompressionType::Fast,
            image::codecs::png::FilterType::Sub,
        );
        img.write_with_encoder(enc)
            .map_err(|e| format!("No se pudo codificar el terreno: {e}"))?;
        fs::rename(&tmp, png).map_err(|e| format!("No se pudo guardar el terreno: {e}"))?;
        Ok(Some(TerrainRef {
            file: png
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            chunks: keys.iter().map(|k| [k.0, k.1]).collect(),
            scale: SS as u32,
        }))
    }

    pub fn load(png: &Path, r: &TerrainRef, bounds: Option<[f32; 2]>) -> Result<Self, String> {
        let img = image::open(png)
            .map_err(|e| format!("No se pudo leer el terreno '{}': {e}", png.display()))?
            .to_rgba8();
        let cols = grid_cols(r.chunks.len());
        let rows = r.chunks.len().div_ceil(cols);
        if (img.width() as usize) < cols * CHUNK || (img.height() as usize) < rows * CHUNK {
            return Err("El PNG de terreno no coincide con el mapa.".to_owned());
        }
        let mut buf = Self::new(bounds);
        for (n, k) in r.chunks.iter().enumerate() {
            let (gx, gy) = ((n % cols) * CHUNK, (n / cols) * CHUNK);
            let mut px = Vec::with_capacity(CHUNK * CHUNK);
            for y in 0..CHUNK {
                for x in 0..CHUNK {
                    let p = img.get_pixel((gx + x) as u32, (gy + y) as u32).0;
                    px.push(Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]));
                }
            }
            if r.scale >= SS as u32 {
                buf.chunks.insert((k[0], k[1]), Chunk::from_pixels(px));
            } else {
                // PNG de la 3.2 (1×): cada chunk se duplica a 2×2 chunks por vecino más cercano.
                let h = CHUNK / 2;
                for sy in 0..2usize {
                    for sx in 0..2usize {
                        let mut q = Vec::with_capacity(CHUNK * CHUNK);
                        for y in 0..CHUNK {
                            for x in 0..CHUNK {
                                q.push(px[(sy * h + y / 2) * CHUNK + sx * h + x / 2]);
                            }
                        }
                        let key = (k[0] * 2 + sx as i32, k[1] * 2 + sy as i32);
                        buf.chunks.insert(key, Chunk::from_pixels(q));
                    }
                }
            }
        }
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(radius: f32, erase: bool) -> BrushParams {
        BrushParams {
            radius,
            hardness: 0.8,
            opacity: 1.0,
            color: Color32::from_rgb(0x5D, 0x7A, 0x47),
            erase,
        }
    }

    #[test]
    fn fast_stroke_is_continuous() {
        let mut t = TerrainBuffer::new(Some([600.0, 100.0]));
        let p = params(4.0, false);
        t.begin_stroke();
        t.stroke_to(pos2(10.0, 50.0), &p);
        t.stroke_to(pos2(590.0, 50.0), &p); // salto enorme de un solo evento
        assert!(t.end_stroke().is_some());
        for x in 10..590 {
            assert!(t.alpha_at(x, 50) > 200, "hueco en x={x}");
        }
    }

    #[test]
    fn opacity_is_a_ceiling_for_the_whole_stroke() {
        let mut t = TerrainBuffer::new(Some([200.0, 200.0]));
        let mut p = params(10.0, false);
        p.opacity = 0.5;
        t.begin_stroke();
        for _ in 0..6 {
            t.stroke_to(pos2(50.0, 50.0), &p);
            t.stroke_to(pos2(80.0, 50.0), &p);
        }
        t.end_stroke();
        let a = t.alpha_at(65, 50);
        assert!((120..=135).contains(&a), "alfa {a}");
    }

    #[test]
    fn eraser_undo_redo_and_bounds() {
        let mut t = TerrainBuffer::new(Some([300.0, 300.0]));
        t.begin_stroke();
        t.stroke_to(pos2(100.0, 100.0), &params(12.0, false));
        let paint = t.end_stroke().unwrap();
        assert!(t.alpha_at(100, 100) > 250);

        t.begin_stroke();
        t.stroke_to(pos2(100.0, 100.0), &params(12.0, true));
        let erase = t.end_stroke().unwrap();
        assert_eq!(t.alpha_at(100, 100), 0);

        t.apply_patch(&erase, true);
        assert!(t.alpha_at(100, 100) > 250);
        t.apply_patch(&erase, false);
        assert_eq!(t.alpha_at(100, 100), 0);
        t.apply_patch(&erase, true);
        t.apply_patch(&paint, true); // el chunk no existía antes
        assert_eq!(t.chunk_count(), 0);

        // Fuera del mapa de tamaño fijo no se pinta.
        t.begin_stroke();
        t.stroke_to(pos2(-50.0, -50.0), &params(12.0, false));
        assert!(t.end_stroke().is_none());
    }

    #[test]
    fn legacy_one_x_files_are_upscaled() {
        let dir = std::env::temp_dir().join(format!("ff_terrain_legacy_{}", std::process::id()));
        let png = dir.join("viejo.terrain.png");
        let mut t = TerrainBuffer::new(None);
        t.begin_stroke();
        t.stroke_to(pos2(50.0, 50.0), &params(10.0, false));
        t.end_stroke();
        let mut r = t.save(&png).unwrap().unwrap();
        assert_eq!(r.scale, 2);
        r.scale = 1; // como lo escribía la 3.2
        let l = TerrainBuffer::load(&png, &r, None).unwrap();
        assert_eq!(l.chunk_count(), 4 * r.chunks.len());
        assert!(l.alpha_at(100, 100) > 200); // el pixel (100,100) del viejo ahora vive en el mundo (100,100)
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn edge_is_antialiased() {
        let mut t = TerrainBuffer::new(Some([100.0, 100.0]));
        t.begin_stroke();
        t.stroke_to(pos2(50.0, 50.0), &params(20.0, false));
        t.end_stroke();
        // Recorrido horizontal por el borde: debe haber valores intermedios (no escalón).
        let mid = (0..200)
            .filter(|bx| {
                let a = t.alpha_at(*bx / 2, 50);
                a > 5 && a < 250
            })
            .count();
        assert!(mid >= 1);
        assert_eq!(t.alpha_at(50, 50), 255);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("ff_terrain_{}", std::process::id()));
        let json = dir.join("mapa.ffmap.json");
        let png = terrain_path(&json);
        assert!(png.ends_with("mapa.terrain.png"));

        let mut t = TerrainBuffer::new(None);
        t.begin_stroke();
        t.stroke_to(pos2(-20.0, -20.0), &params(10.0, false));
        t.stroke_to(pos2(700.0, 300.0), &params(10.0, false));
        t.end_stroke();
        let r = t.save(&png).unwrap().unwrap();
        assert!(r.chunks.len() >= 3);
        let l = TerrainBuffer::load(&png, &r, None).unwrap();
        assert_eq!(l.chunk_count(), r.chunks.len());
        assert!(l.alpha_at(-20, -20) > 200);
        assert!(l.alpha_at(700, 300) > 200);
        assert_eq!(l.alpha_at(300, 0), 0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn fill_closed_region_and_background() {
        let mut t = TerrainBuffer::new(Some([200.0, 200.0]));
        let ring = params(3.0, false);
        // Contorno cuadrado cerrado.
        t.begin_stroke();
        for p in [
            pos2(60.0, 60.0),
            pos2(140.0, 60.0),
            pos2(140.0, 140.0),
            pos2(60.0, 140.0),
            pos2(60.0, 60.0),
        ] {
            t.stroke_to(p, &ring);
        }
        t.end_stroke().unwrap();
        let blue = Color32::from_rgb(40, 90, 160);
        // Dentro del contorno.
        let patch = t.fill(pos2(100.0, 100.0), blue, 1.0).unwrap().unwrap();
        assert!(t.alpha_at(100, 100) > 250);
        assert!(t.alpha_at(70, 100) > 250); // llega hasta el contorno
        assert_eq!(t.alpha_at(20, 20), 0); // fuera no se toca
                                           // Sin hueco bajo el borde suavizado del contorno.
        assert!(t.alpha_at(61, 100) > 250);
        // Fondo: rellena todo lo de afuera hasta el borde del mapa.
        t.fill(pos2(10.0, 10.0), Color32::from_rgb(200, 190, 150), 1.0)
            .unwrap()
            .unwrap();
        assert!(t.alpha_at(199, 199) > 250 && t.alpha_at(10, 150) > 250);
        // Deshacer el primer relleno vuelve a vaciar el interior.
        let inner_before = t.alpha_at(100, 100);
        t.apply_patch(&patch, true);
        assert!(inner_before > 250 && t.alpha_at(100, 100) == 0);
    }

    #[test]
    fn fill_open_area_on_infinite_canvas_is_refused() {
        let mut t = TerrainBuffer::new(None);
        let r = t.fill(pos2(0.0, 0.0), Color32::RED, 1.0);
        assert!(r.is_err());
        // Un anillo cerrado sí se puede rellenar.
        let ring = params(3.0, false);
        t.begin_stroke();
        for p in [
            pos2(-50.0, -50.0),
            pos2(50.0, -50.0),
            pos2(50.0, 50.0),
            pos2(-50.0, 50.0),
            pos2(-50.0, -50.0),
        ] {
            t.stroke_to(p, &ring);
        }
        t.end_stroke();
        assert!(t.fill(pos2(0.0, 0.0), Color32::RED, 1.0).unwrap().is_some());
        assert!(t.alpha_at(0, 0) > 250);
    }

    #[test]
    fn huge_brush_stays_bounded() {
        // Un sello de 400 px de diámetro solo toca los chunks de su caja envolvente.
        let mut t = TerrainBuffer::new(None);
        t.begin_stroke();
        t.stroke_to(pos2(0.0, 0.0), &params(200.0, false));
        t.stroke_to(pos2(30.0, 0.0), &params(200.0, false));
        t.end_stroke();
        assert!(t.chunk_count() <= 16 * 16);
        assert!(t.alpha_at(0, 0) > 250 && t.alpha_at(190, 0) > 0);
    }
}
