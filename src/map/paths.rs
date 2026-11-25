// src/map/paths.rs
//! Trazos vectoriales (caminos, ríos y murallas): polilíneas por nodos —suavizadas con Catmull-Rom
//! salvo las murallas, que son rectas—, dibujo a mano alzada con simplificación, *snap* y
//! conexiones entre trazos (bifurcaciones) y dibujo estilizado.

use crate::i18n::tr;
use egui::epaint::{Vertex, WHITE_UV};
use egui::{pos2, vec2, Color32, Mesh, Painter, Pos2, Rect, Shape, Stroke, Vec2};
use serde::{Deserialize, Serialize};

use crate::map::document::MapStyle;

/// Radio de *snap* (px del mundo) para anclar un punto a otro trazo.
pub const SNAP_RADIUS: f32 = 12.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum PathKind {
    Camino,
    Rio,
    Muralla,
}

impl PathKind {
    pub const ALL: [PathKind; 3] = [PathKind::Camino, PathKind::Rio, PathKind::Muralla];

    pub fn label(self) -> &'static str {
        match self {
            PathKind::Camino => tr("Camino"),
            PathKind::Rio => tr("Río"),
            PathKind::Muralla => tr("Muralla"),
        }
    }

    pub fn default_width(self) -> f32 {
        match self {
            PathKind::Camino => 8.0,
            PathKind::Rio => 14.0,
            PathKind::Muralla => 10.0,
        }
    }

    /// Caminos y ríos se suavizan; las murallas van en tramos rectos.
    pub fn smooth(self) -> bool {
        !matches!(self, PathKind::Muralla)
    }
}

/// Material de un camino.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Material {
    /// Sendero de tierra: borde suave y desvanecido en los extremos libres.
    #[default]
    Tierra,
    /// Calzada de piedra: borde marcado y juntas a lo largo del trazo.
    Piedra,
}

impl Material {
    pub const ALL: [Material; 2] = [Material::Tierra, Material::Piedra];

    pub fn label(self) -> &'static str {
        match self {
            Material::Tierra => tr("Tierra"),
            Material::Piedra => tr("Piedra"),
        }
    }
}

/// Forma de las torres de una muralla.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum TowerStyle {
    Circular,
    Cuadrada,
    #[default]
    Alternada,
}

impl TowerStyle {
    pub const ALL: [TowerStyle; 3] = [
        TowerStyle::Circular,
        TowerStyle::Cuadrada,
        TowerStyle::Alternada,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TowerStyle::Circular => tr("Circular"),
            TowerStyle::Cuadrada => tr("Cuadrada"),
            TowerStyle::Alternada => tr("Alternada"),
        }
    }
}

/// Separación por defecto de las torres periódicas (px del mundo).
pub const DEFAULT_TOWER_EVERY: f32 = 240.0;
pub const TOWER_EVERY_RANGE: std::ops::RangeInclusive<f32> = 120.0..=400.0;
/// Caché de curvas (en coordenadas del mundo) y de extremos conectados de todos los trazos.
/// Se reconstruye solo cuando cambian los trazos, no en cada fotograma.
#[derive(Default)]
pub struct PathCache {
    sig: u64,
    valid: bool,
    curves: std::collections::HashMap<u64, Vec<Pos2>>,
    ends: std::collections::HashMap<u64, [bool; 2]>,
}

fn signature(paths: &[VPath]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    paths.len().hash(&mut h);
    for p in paths {
        p.id.hash(&mut h);
        (p.kind as u8).hash(&mut h);
        for n in &p.nodes {
            n[0].to_bits().hash(&mut h);
            n[1].to_bits().hash(&mut h);
        }
    }
    h.finish()
}

impl PathCache {
    pub fn refresh(&mut self, paths: &[VPath]) {
        let sig = signature(paths);
        if self.valid && sig == self.sig {
            return;
        }
        self.sig = sig;
        self.valid = true;
        self.curves.clear();
        self.ends.clear();
        for p in paths {
            let pts = curve(p.kind, &p.points())
                .into_iter()
                .map(|(q, _)| q)
                .collect();
            self.curves.insert(p.id, pts);
            self.ends.insert(p.id, connected_ends(paths, p));
        }
    }

    pub fn curve(&self, id: u64) -> &[Pos2] {
        self.curves.get(&id).map_or(&[], |v| v.as_slice())
    }

    pub fn ends(&self, id: u64) -> [bool; 2] {
        self.ends.get(&id).copied().unwrap_or([false; 2])
    }
}

/// Longitud (px del mundo) del desvanecido en los extremos libres de un camino de tierra.
pub const FADE_LEN: f32 = 60.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VPath {
    pub id: u64,
    pub kind: PathKind,
    /// Nodos en coordenadas del mundo.
    pub nodes: Vec<[f32; 2]>,
    pub width: f32,
    #[serde(default)]
    pub dashed: bool,
    /// Capa a la que pertenece (id de capa; 0 = sin asignar, se resuelve al cargar).
    #[serde(default)]
    pub layer: u64,
    /// Material (solo caminos).
    #[serde(default)]
    pub material: Material,
    /// Forma de las torres (solo murallas).
    #[serde(default)]
    pub tower_style: TowerStyle,
    /// Separación de torres periódicas en px del mundo; 0 = solo en nodos y esquinas.
    #[serde(default)]
    pub tower_every: f32,
    /// Sprite PNG (clave de la biblioteca) con el que se texturiza el trazo (caminos de piedra y
    /// murallas). `None` = dibujo vectorial.
    #[serde(default)]
    pub sprite: Option<String>,
}

impl VPath {
    pub fn points(&self) -> Vec<Pos2> {
        self.nodes.iter().map(|n| Pos2::new(n[0], n[1])).collect()
    }
}

/// Segmento Catmull-Rom uniforme entre `p1` y `p2`.
fn catmull_rom(p0: Pos2, p1: Pos2, p2: Pos2, p3: Pos2, t: f32) -> Pos2 {
    let (t2, t3) = (t * t, t * t * t);
    let (a, b, c, d) = (p0.to_vec2(), p1.to_vec2(), p2.to_vec2(), p3.to_vec2());
    let v = ((b * 2.0)
        + (c - a) * t
        + (a * 2.0 - b * 5.0 + c * 4.0 - d) * t2
        + (-a + b * 3.0 - c * 3.0 + d) * t3)
        * 0.5;
    v.to_pos2()
}

/// Muestrea la curva suavizada. Cada punto lleva el índice del segmento de control al que pertenece.
pub fn spline(nodes: &[Pos2]) -> Vec<(Pos2, usize)> {
    let n = nodes.len();
    match n {
        0 => Vec::new(),
        1 => vec![(nodes[0], 0)],
        _ => {
            let mut out = Vec::new();
            for i in 0..n - 1 {
                let p0 = nodes[i.saturating_sub(1)];
                let (p1, p2) = (nodes[i], nodes[i + 1]);
                let p3 = nodes[(i + 2).min(n - 1)];
                let steps = (((p2 - p1).length() / 8.0).ceil() as usize).clamp(4, 48);
                for s in 0..steps {
                    out.push((catmull_rom(p0, p1, p2, p3, s as f32 / steps as f32), i));
                }
            }
            out.push((nodes[n - 1], n - 2));
            out
        }
    }
}

/// Curva que se dibuja para un tipo de trazo: suavizada o, en murallas, la poligonal recta.
pub fn curve(kind: PathKind, nodes: &[Pos2]) -> Vec<(Pos2, usize)> {
    if kind.smooth() {
        return spline(nodes);
    }
    let n = nodes.len();
    nodes
        .iter()
        .enumerate()
        .map(|(i, p)| (*p, i.min(n.saturating_sub(2))))
        .collect()
}

pub struct Nearest {
    pub dist: f32,
    /// Segmento de control (entre `seg` y `seg + 1`).
    pub seg: usize,
    pub point: Pos2,
}

/// Punto de la curva más cercano a `p`.
pub fn nearest(kind: PathKind, nodes: &[Pos2], p: Pos2) -> Option<Nearest> {
    let pts = curve(kind, nodes);
    let mut best: Option<Nearest> = None;
    for w in pts.windows(2) {
        let (a, b) = (w[0].0, w[1].0);
        let ab = b - a;
        let len2 = ab.length_sq();
        let t = if len2 > 0.0 {
            ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let q = a + ab * t;
        let dist = (p - q).length();
        if best.as_ref().is_none_or(|b| dist < b.dist) {
            best = Some(Nearest {
                dist,
                seg: w[0].1,
                point: q,
            });
        }
    }
    best
}

// ---------------------------------------------------------------------- snap y bifurcaciones

pub struct Snap {
    pub point: Pos2,
    /// `true` si se ancló a un nodo existente; `false` si a un punto de un segmento.
    pub on_node: bool,
}

/// Punto de anclaje para `p`: el nodo más cercano dentro de `radius` (de los trazos dados o de
/// `extra`, p. ej. los nodos del propio trazo en construcción) o, si no hay, el punto más cercano
/// de algún segmento.
pub fn snap<'a>(
    paths: impl Iterator<Item = &'a VPath>,
    extra: &[Pos2],
    p: Pos2,
    radius: f32,
) -> Option<Snap> {
    let mut node: Option<(f32, Pos2)> = None;
    let mut seg: Option<(f32, Pos2)> = None;
    let mut consider_node = |q: Pos2| {
        let d = (q - p).length();
        if d <= radius && node.is_none_or(|(bd, _)| d < bd) {
            node = Some((d, q));
        }
    };
    for q in extra {
        consider_node(*q);
    }
    for path in paths {
        let pts = path.points();
        for q in &pts {
            consider_node(*q);
        }
        if let Some(n) = nearest(path.kind, &pts, p) {
            if n.dist <= radius && seg.is_none_or(|(bd, _)| n.dist < bd) {
                seg = Some((n.dist, n.point));
            }
        }
    }
    match (node, seg) {
        (Some((_, q)), _) => Some(Snap {
            point: q,
            on_node: true,
        }),
        (None, Some((_, q))) => Some(Snap {
            point: q,
            on_node: false,
        }),
        _ => None,
    }
}

/// Tras crear el trazo `id`: cada uno de sus nodos que cae sobre el segmento de otro trazo le
/// añade ahí un nodo, de modo que la bifurcación comparte un punto exacto con el trazo anfitrión.
pub fn attach_to_hosts(paths: &mut [VPath], id: u64) {
    let Some(nodes) = paths.iter().find(|p| p.id == id).map(|p| p.points()) else {
        return;
    };
    for n in nodes {
        for host in paths.iter_mut().filter(|h| h.id != id) {
            let pts = host.points();
            if pts.iter().any(|q| (*q - n).length() < 0.05) {
                continue;
            }
            if let Some(nr) = nearest(host.kind, &pts, n) {
                if nr.dist < 0.05 {
                    host.nodes.insert(nr.seg + 1, [n.x, n.y]);
                }
            }
        }
    }
}

/// Mueve el nodo `idx` del trazo `pid` y arrastra consigo los nodos de otros trazos que están
/// conectados (coinciden con su posición anterior).
pub fn move_connected(paths: &mut [VPath], pid: u64, idx: usize, new: Pos2) {
    let Some(old) = paths
        .iter()
        .find(|p| p.id == pid)
        .and_then(|p| p.nodes.get(idx).copied())
    else {
        return;
    };
    let old = pos2(old[0], old[1]);
    for p in paths.iter_mut() {
        let own = p.id == pid;
        for (i, n) in p.nodes.iter_mut().enumerate() {
            if (own && i == idx) || (pos2(n[0], n[1]) - old).length() < 0.5 {
                *n = [new.x, new.y];
            }
        }
    }
}

// ---------------------------------------------------------------------- mano alzada

fn perp_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len = ab.length();
    if len < 1e-6 {
        return (p - a).length();
    }
    ((p - a).x * ab.y - (p - a).y * ab.x).abs() / len
}

/// Ramer-Douglas-Peucker iterativo.
fn rdp(points: &[Pos2], eps: f32) -> Vec<Pos2> {
    let n = points.len();
    if n < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    let mut stack = vec![(0usize, n - 1)];
    while let Some((a, b)) = stack.pop() {
        let mut far = (0.0, a);
        for i in a + 1..b {
            let d = perp_distance(points[i], points[a], points[b]);
            if d > far.0 {
                far = (d, i);
            }
        }
        if far.0 > eps {
            keep[far.1] = true;
            stack.push((a, far.1));
            stack.push((far.1, b));
        }
    }
    points
        .iter()
        .zip(keep)
        .filter_map(|(p, k)| k.then_some(*p))
        .collect()
}

/// Simplifica un trazo a mano alzada: conserva un punto cada `min_dist` como mínimo y quita los
/// que quedan alineados, dejando una poligonal limpia que luego se suaviza con Catmull-Rom.
pub fn simplify(points: &[Pos2], min_dist: f32) -> Vec<Pos2> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let last = points[points.len() - 1];
    let mut out = vec![points[0]];
    for &p in &points[1..points.len() - 1] {
        if (p - out[out.len() - 1]).length() >= min_dist {
            out.push(p);
        }
    }
    if out.len() > 1 && (last - out[out.len() - 1]).length() < min_dist * 0.5 {
        let n = out.len();
        out[n - 1] = last;
    } else {
        out.push(last);
    }
    rdp(&out, min_dist * 0.15)
}

// ---------------------------------------------------------------------- dibujo

/// ¿Los extremos (inicio, fin) del trazo están conectados a otro trazo?
pub fn connected_ends(paths: &[VPath], p: &VPath) -> [bool; 2] {
    let (Some(a), Some(b)) = (p.nodes.first(), p.nodes.last()) else {
        return [false; 2];
    };
    let ends = [pos2(a[0], a[1]), pos2(b[0], b[1])];
    let mut out = [false; 2];
    for (i, e) in ends.iter().enumerate() {
        out[i] = paths.iter().filter(|o| o.id != p.id).any(|o| {
            let pts = o.points();
            Rect::from_points(&pts).expand(30.0).contains(*e)
                && nearest(o.kind, &pts, *e).is_some_and(|n| n.dist < 0.6)
        });
    }
    out
}

/// Color (tinta de borde, relleno) según el tipo, material y estilo del mapa.
pub fn colors(kind: PathKind, material: Material, style: MapStyle) -> (Color32, Color32) {
    let c = Color32::from_rgb;
    match (kind, material, style) {
        (PathKind::Camino, Material::Tierra, MapStyle::Atlas) => {
            (c(0x7A, 0x5C, 0x3C), c(0xC9, 0xA8, 0x78))
        }
        (PathKind::Camino, Material::Tierra, MapStyle::Pergamino) => {
            (c(0x6B, 0x4A, 0x2A), c(0xB8, 0x9B, 0x6A))
        }
        (PathKind::Camino, Material::Piedra, MapStyle::Atlas) => {
            (c(0x38, 0x36, 0x33), c(0x9D, 0x9A, 0x93))
        }
        (PathKind::Camino, Material::Piedra, MapStyle::Pergamino) => {
            (c(0x3C, 0x30, 0x24), c(0xA8, 0x9F, 0x8C))
        }
        (PathKind::Rio, _, MapStyle::Atlas) => (c(0x2F, 0x5F, 0x78), c(0x5E, 0x9C, 0xBF)),
        (PathKind::Rio, _, MapStyle::Pergamino) => (c(0x5A, 0x44, 0x2A), c(0x8F, 0xA9, 0xA0)),
        (PathKind::Muralla, _, MapStyle::Atlas) => (c(0x2B, 0x26, 0x22), c(0xA2, 0x9D, 0x93)),
        (PathKind::Muralla, _, MapStyle::Pergamino) => (c(0x3A, 0x2C, 0x1E), c(0xB3, 0xA4, 0x8A)),
    }
}

/// Aspecto de un trazo al dibujarlo.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    pub kind: PathKind,
    pub material: Material,
    pub width: f32,
    pub dashed: bool,
    /// Desvanecer el inicio / el final (solo caminos de tierra con extremo libre).
    pub fade: [bool; 2],
}

impl Look {
    pub fn of(p: &VPath, connected: [bool; 2]) -> Self {
        Self::new(p.kind, p.material, p.width, p.dashed, connected)
    }

    pub fn new(
        kind: PathKind,
        material: Material,
        width: f32,
        dashed: bool,
        connected: [bool; 2],
    ) -> Self {
        let soft = kind == PathKind::Camino && material == Material::Tierra && !dashed;
        Self {
            kind,
            material,
            width,
            dashed,
            fade: [soft && !connected[0], soft && !connected[1]],
        }
    }

    fn soft(&self) -> bool {
        self.kind == PathKind::Camino && self.material == Material::Tierra && !self.dashed
    }
}

fn smooth01(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Cinta de ancho y opacidad variables (malla) a lo largo de `pts`.
fn ribbon(
    painter: &Painter,
    pts: &[Pos2],
    width: f32,
    color: Color32,
    fade: [bool; 2],
    fade_len: f32,
) {
    let n = pts.len();
    if n < 2 {
        return;
    }
    let mut cum = vec![0.0_f32; n];
    for i in 1..n {
        cum[i] = cum[i - 1] + (pts[i] - pts[i - 1]).length();
    }
    let total = cum[n - 1];
    let dir = |i: usize| -> Vec2 {
        let d = pts[i + 1] - pts[i];
        if d.length() < 1e-4 {
            vec2(1.0, 0.0)
        } else {
            d.normalized()
        }
    };
    let mut mesh = Mesh::default();
    for i in 0..n {
        let (d0, d1) = (dir(i.saturating_sub(1).min(n - 2)), dir(i.min(n - 2)));
        let tangent = (d0 + d1).normalized();
        let tangent = if tangent.is_finite() { tangent } else { d1 };
        let normal = vec2(-tangent.y, tangent.x);
        let miter = (1.0 / normal.dot(vec2(-d1.y, d1.x)).abs().max(0.5)).min(2.0);
        let mut t = 1.0_f32;
        if fade[0] {
            t = t.min(cum[i] / fade_len);
        }
        if fade[1] {
            t = t.min((total - cum[i]) / fade_len);
        }
        let t = smooth01(t);
        // Ancho constante: el extremo se desvanece solo en alfa, con corte recto.
        let half = width * 0.5 * miter;
        let col = color.gamma_multiply(t);
        for sign in [-1.0, 1.0] {
            mesh.vertices.push(Vertex {
                pos: pts[i] + normal * half * sign,
                uv: WHITE_UV,
                color: col,
            });
        }
        if i + 1 < n {
            let b = (i * 2) as u32;
            mesh.indices
                .extend_from_slice(&[b, b + 1, b + 2, b + 1, b + 3, b + 2]);
        }
    }
    painter.add(Shape::mesh(mesh));
}

/// Reparte la poligonal en guiones por longitud de arco: cada guion es un tramo continuo que
/// sigue las esquinas, sin cortes bruscos en los vértices.
pub fn dashes(pts: &[Pos2], dash: f32, gap: f32) -> Vec<Vec<Pos2>> {
    let mut out = Vec::new();
    if pts.len() < 2 || dash <= 0.0 {
        return out;
    }
    let mut cum = vec![0.0_f32; pts.len()];
    for i in 1..pts.len() {
        cum[i] = cum[i - 1] + (pts[i] - pts[i - 1]).length();
    }
    let total = cum[pts.len() - 1];
    let at = |s: f32, seg: usize| -> Pos2 {
        let l = (cum[seg + 1] - cum[seg]).max(1e-6);
        pts[seg].lerp(pts[seg + 1], ((s - cum[seg]) / l).clamp(0.0, 1.0))
    };
    let period = dash + gap.max(0.0);
    let mut start = 0.0_f32;
    let mut seg = 0usize;
    while start < total - 1e-3 {
        let end = (start + dash).min(total);
        while seg + 2 < pts.len() && cum[seg + 1] <= start {
            seg += 1;
        }
        let mut d = vec![at(start, seg)];
        let mut k = seg;
        while k + 1 < pts.len() && cum[k + 1] < end {
            d.push(pts[k + 1]);
            k += 1;
        }
        d.push(at(end, k.min(pts.len() - 2)));
        out.push(d);
        start += period;
    }
    out
}

/// Marcas perpendiculares (juntas de piedra) cada `step` px de pantalla.
fn joints(painter: &Painter, pts: &[Pos2], step: f32, len: f32, stroke: Stroke) {
    let mut carry = step * 0.5;
    for w in pts.windows(2) {
        let d = w[1] - w[0];
        let l = d.length();
        if l < 1e-3 {
            continue;
        }
        let dir = d / l;
        let nrm = vec2(-dir.y, dir.x) * len * 0.5;
        let mut at = carry;
        while at < l {
            let c = w[0] + dir * at;
            painter.line_segment([c - nrm, c + nrm], stroke);
            at += step;
        }
        carry = at - l;
    }
}

/// Dibuja una pasada del trazo sobre puntos ya en pantalla.
///
/// `inner = false` dibuja el borde de tinta, `inner = true` el relleno. Se dibujan primero todos
/// los bordes y luego todos los rellenos para que los cruces se fundan.
pub fn paint(
    painter: &Painter,
    pts: &[Pos2],
    look: &Look,
    style: MapStyle,
    zoom: f32,
    inner: bool,
    alpha: f32,
) {
    if pts.len() < 2 {
        return;
    }
    let (ink, fill) = colors(look.kind, look.material, style);
    let width = look.width;
    let border = match (look.kind, look.material) {
        (PathKind::Camino, Material::Tierra) => 2.0,
        (PathKind::Camino, Material::Piedra) => 4.0,
        (PathKind::Muralla, _) => 4.0,
        _ => 3.0,
    };
    let (color, w) = if inner {
        (fill, (width * zoom).max(0.8))
    } else {
        // El borde de tierra es sutil.
        // Opaco y mezclado con el relleno: dos bordes que se cruzan no se oscurecen entre sí.
        let ink = if look.soft() {
            mix(fill, ink, 0.55)
        } else {
            ink
        };
        (ink, ((width + border) * zoom).max(1.5))
    };
    let color = color.gamma_multiply(alpha);

    if look.soft() {
        ribbon(painter, pts, w, color, look.fade, FADE_LEN * zoom);
        // Extremos conectados: un disco une el encuentro con el trazo anfitrión.
        for (i, end) in [pts[0], pts[pts.len() - 1]].into_iter().enumerate() {
            if !look.fade[i] {
                painter.circle_filled(end, w / 2.0, color);
            }
        }
        return;
    }
    let stroke = Stroke::new(w, color);
    if look.dashed && look.kind != PathKind::Muralla {
        let dash = ((width * 1.8 + 6.0) * zoom).max(3.0);
        let gap = ((width * 1.2 + 4.0) * zoom).max(2.0);
        for d in dashes(pts, dash, gap) {
            painter.add(Shape::line(d.clone(), stroke));
            // Codos y puntas redondeados: cada guion queda como un trazo limpio y continuo.
            if w >= 2.0 {
                for q in &d {
                    painter.circle_filled(*q, w / 2.0, color);
                }
            }
        }
    } else {
        painter.add(Shape::line(pts.to_vec(), stroke));
        if look.kind != PathKind::Muralla {
            painter.circle_filled(pts[0], w / 2.0, color);
            painter.circle_filled(pts[pts.len() - 1], w / 2.0, color);
        }
        if inner {
            // Juntas de las losas / sillares.
            let seg = match (look.kind, look.material) {
                (PathKind::Camino, Material::Piedra) => Some((width * 1.1, 0.9, 0.5)),
                (PathKind::Muralla, _) => Some((width * 1.5, 0.95, 0.3)),
                _ => None,
            };
            if let Some((step, len, a)) = seg {
                let step = (step * zoom).max(6.0);
                let j = Stroke::new(1.0, ink.gamma_multiply(a * alpha));
                joints(painter, pts, step, width * zoom * len, j);
            }
        }
    }
}

/// Texturiza el trazo con un sprite PNG repetido a lo largo del recorrido: el alto de la imagen
/// cubre el ancho del trazo y el largo se reparte en baldosas (cada una con `u` de 0 a 1).
pub fn paint_textured(
    painter: &Painter,
    pts: &[Pos2],
    tex: egui::TextureId,
    tex_size: Vec2,
    width: f32,
    tint: Color32,
) {
    let mesh = textured_mesh(pts, tex, tex_size, width, tint);
    if !mesh.is_empty() {
        painter.add(Shape::mesh(mesh));
    }
}

/// Malla del trazo texturizado (ver [`paint_textured`]).
pub fn textured_mesh(
    pts: &[Pos2],
    tex: egui::TextureId,
    tex_size: Vec2,
    width: f32,
    tint: Color32,
) -> Mesh {
    let n = pts.len();
    if n < 2 || width < 0.5 {
        return Mesh::default();
    }
    let tile = (tex_size.x / tex_size.y.max(1.0) * width).max(4.0);
    let mut cum = vec![0.0_f32; n];
    for i in 1..n {
        cum[i] = cum[i - 1] + (pts[i] - pts[i - 1]).length();
    }
    let seg_dir = |i: usize| -> Vec2 {
        let d = pts[i + 1] - pts[i];
        if d.length() < 1e-4 {
            vec2(1.0, 0.0)
        } else {
            d.normalized()
        }
    };
    // Extremos «izquierdo/derecho» por vértice (con inglete acotado).
    let half = width * 0.5;
    let sides: Vec<(Pos2, Pos2)> = (0..n)
        .map(|i| {
            let (d0, d1) = (
                seg_dir(i.saturating_sub(1).min(n - 2)),
                seg_dir(i.min(n - 2)),
            );
            let t = (d0 + d1).normalized();
            let t = if t.is_finite() { t } else { d1 };
            let nrm = vec2(-t.y, t.x);
            let miter = (1.0 / nrm.dot(vec2(-d1.y, d1.x)).abs().max(0.5)).min(2.0);
            (pts[i] - nrm * half * miter, pts[i] + nrm * half * miter)
        })
        .collect();
    let mut mesh = Mesh::with_texture(tex);
    for i in 0..n - 1 {
        let (s0, s1) = (cum[i], cum[i + 1]);
        if s1 - s0 < 1e-4 {
            continue;
        }
        let mut a = s0;
        while a < s1 - 1e-4 {
            let k = (a / tile).floor();
            let b = ((k + 1.0) * tile).min(s1);
            let (ta, tb) = ((a - s0) / (s1 - s0), (b - s0) / (s1 - s0));
            let (ua, ub) = ((a - k * tile) / tile, (b - k * tile) / tile);
            let lerp = |p: Pos2, q: Pos2, t: f32| p + (q - p) * t;
            let base = mesh.vertices.len() as u32;
            for (t, u) in [(ta, ua), (tb, ub)] {
                for (side, v) in [(sides[i].0, sides[i + 1].0), (sides[i].1, sides[i + 1].1)]
                    .into_iter()
                    .zip([0.0_f32, 1.0])
                {
                    mesh.vertices.push(Vertex {
                        pos: lerp(side.0, side.1, t),
                        uv: pos2(u, v),
                        color: tint,
                    });
                }
            }
            mesh.indices.extend_from_slice(&[
                base,
                base + 1,
                base + 2,
                base + 1,
                base + 3,
                base + 2,
            ]);
            a = b;
        }
    }
    mesh
}

/// Una torre de muralla.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tower {
    pub pos: Pos2,
    pub round: bool,
    /// Tamaño relativo (1 = torre de esquina o extremo).
    pub scale: f32,
    /// Dirección del muro en la torre (radianes, `atan2(dy, dx)`); en una esquina, la bisectriz de
    /// los dos tramos. Las torres cuadradas se giran con él para quedar alineadas con el muro.
    pub angle: f32,
}

/// Nodos de una muralla que llevan torre: los extremos y las esquinas (giro de 15° o más).
/// Devuelve `(índice, es_extremo)`.
pub fn tower_nodes(nodes: &[Pos2]) -> Vec<(usize, bool)> {
    let n = nodes.len();
    if n < 2 {
        return Vec::new();
    }
    let closed = n > 2 && (nodes[0] - nodes[n - 1]).length() < 0.5;
    let mut out = Vec::new();
    if closed {
        out.push((0, false)); // el cierre del recinto es una esquina más
    } else {
        out.push((0, true));
        out.push((n - 1, true));
    }
    for i in 1..n - 1 {
        let (a, b) = (nodes[i] - nodes[i - 1], nodes[i + 1] - nodes[i]);
        let (la, lb) = (a.length(), b.length());
        if la < 1e-3 || lb < 1e-3 || a.dot(b) / (la * lb) < 15f32.to_radians().cos() {
            out.push((i, false));
        }
    }
    out.sort_by_key(|(i, _)| *i);
    out
}

/// Torres de una muralla: en extremos y esquinas, más una cada ~`every` px a lo largo de cada
/// tramo (repartidas de forma pareja; `every <= 0` desactiva las periódicas).
pub fn towers(nodes: &[Pos2], style: TowerStyle, every: f32) -> Vec<Tower> {
    let n = nodes.len();
    let at_nodes = tower_nodes(nodes);
    let closed = n > 2 && (nodes[0] - nodes[n - 1]).length() < 0.5;
    let unit = |d: Vec2| (d.length() > 1e-3).then(|| d.normalized());
    let mut raw: Vec<(Pos2, f32, f32)> = Vec::new();
    for i in 0..n {
        if at_nodes.iter().any(|(k, _)| *k == i) {
            // Tramo que llega y tramo que sale; en el cierre de un recinto, el último y el primero.
            let prev = if i > 0 {
                unit(nodes[i] - nodes[i - 1])
            } else if closed {
                unit(nodes[0] - nodes[n - 2])
            } else {
                None
            };
            let next = (i + 1 < n).then(|| unit(nodes[i + 1] - nodes[i])).flatten();
            let dir = match (prev, next) {
                (Some(a), Some(b)) if (a + b).length() > 1e-3 => a + b,
                (_, Some(b)) => b,
                (Some(a), None) => a,
                _ => vec2(1.0, 0.0),
            };
            raw.push((nodes[i], 1.0, dir.y.atan2(dir.x)));
        }
        if every > 0.0 && i + 1 < n {
            let seg = nodes[i + 1] - nodes[i];
            let len = seg.length();
            let angle = seg.y.atan2(seg.x);
            let k = ((len / every).round() as usize).saturating_sub(1);
            for j in 1..=k {
                let t = j as f32 / (k + 1) as f32;
                raw.push((nodes[i] + seg * t, 0.8, angle));
            }
        }
    }
    raw.into_iter()
        .enumerate()
        .map(|(idx, (pos, scale, angle))| Tower {
            pos,
            scale,
            angle,
            round: match style {
                TowerStyle::Circular => true,
                TowerStyle::Cuadrada => false,
                TowerStyle::Alternada => idx % 2 == 0,
            },
        })
        .collect()
}

/// Vértices de un cuadrado de lado `2·half` centrado en `c` y girado `angle` radianes.
pub fn rotated_square(c: Pos2, half: f32, angle: f32) -> Vec<Pos2> {
    let (s, co) = angle.sin_cos();
    [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
        .iter()
        .map(|(x, y)| c + vec2(x * half * co - y * half * s, x * half * s + y * half * co))
        .collect()
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

/// Radio en pantalla de una torre de respaldo.
pub fn tower_radius(width: f32, zoom: f32, scale: f32) -> f32 {
    (width * 0.95 * zoom * scale).max(3.0)
}

/// Torre cenital de respaldo (cuando no hay un asset de torre): borde oscuro, relleno de piedra,
/// almenas sutiles y techo central.
/// Torre de respaldo; las cuadradas se giran `angle` radianes para alinearse con el muro.
pub fn paint_tower(
    painter: &Painter,
    c: Pos2,
    round: bool,
    angle: f32,
    r: f32,
    style: MapStyle,
    alpha: f32,
) {
    let (ink, stone) = colors(PathKind::Muralla, Material::Piedra, style);
    let (ink, stone) = (ink.gamma_multiply(alpha), stone.gamma_multiply(alpha));
    let roof = mix(stone, ink, 0.35);
    let border = 2.0_f32.min(r * 0.5);
    if round {
        painter.circle_filled(c, r, ink);
        painter.circle_filled(c, r - border, stone);
        painter.circle_stroke(c, r * 0.72, Stroke::new(1.0, ink.gamma_multiply(0.5)));
        painter.circle_filled(c, r * 0.5, roof);
    } else {
        let sq = |half: f32| rotated_square(c, half, angle);
        painter.add(Shape::convex_polygon(sq(r), ink, Stroke::NONE));
        painter.add(Shape::convex_polygon(sq(r - border), stone, Stroke::NONE));
        painter.add(Shape::closed_line(
            sq(r * 0.72),
            Stroke::new(1.0, ink.gamma_multiply(0.5)),
        ));
        painter.add(Shape::convex_polygon(sq(r * 0.5), roof, Stroke::NONE));
    }
}

/// Indicador de conexión (anclaje) en pantalla: aro + punto central.
pub fn paint_snap(painter: &Painter, at: Pos2, on_node: bool, accent: Color32) {
    painter.circle_stroke(at, 9.0, Stroke::new(2.0, accent));
    painter.circle_filled(at, if on_node { 3.5 } else { 2.5 }, accent);
    painter.circle_stroke(at, 9.0, Stroke::new(0.8, Color32::WHITE));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(id: u64, kind: PathKind, pts: &[(f32, f32)]) -> VPath {
        VPath {
            id,
            kind,
            nodes: pts.iter().map(|(x, y)| [*x, *y]).collect(),
            width: 8.0,
            dashed: false,
            layer: 1,
            material: Material::Tierra,
            tower_style: TowerStyle::Alternada,
            tower_every: 0.0,
            sprite: None,
        }
    }

    #[test]
    fn spline_goes_through_nodes() {
        let nodes = [
            pos2(0.0, 0.0),
            pos2(100.0, 50.0),
            pos2(200.0, 0.0),
            pos2(300.0, 80.0),
        ];
        let s = spline(&nodes);
        assert_eq!(s.first().unwrap().0, nodes[0]);
        assert_eq!(s.last().unwrap().0, nodes[3]);
        for (i, n) in nodes.iter().enumerate().take(3) {
            assert!(s
                .iter()
                .any(|(p, seg)| *seg == i && (*p - *n).length() < 1e-3));
        }
    }

    #[test]
    fn dashes_follow_arc_length_through_corners() {
        // Poligonal en L de 100 + 100 px.
        let pts = [pos2(0.0, 0.0), pos2(100.0, 0.0), pos2(100.0, 100.0)];
        let d = dashes(&pts, 30.0, 10.0);
        let len = |v: &Vec<Pos2>| v.windows(2).map(|w| (w[1] - w[0]).length()).sum::<f32>();
        // 200 / 40 = 5 guiones completos.
        assert_eq!(d.len(), 5);
        for g in &d[..4] {
            assert!((len(g) - 30.0).abs() < 0.01, "{}", len(g));
        }
        // El guion que cruza la esquina conserva el vértice.
        assert!(d.iter().any(|g| g.contains(&pos2(100.0, 0.0))));
    }

    #[test]
    fn fade_keeps_constant_width() {
        let _ = Look::new(PathKind::Camino, Material::Tierra, 8.0, false, [false; 2]);
        let cache_paths = vec![VPath {
            id: 1,
            kind: PathKind::Camino,
            nodes: vec![[0.0, 0.0], [100.0, 0.0]],
            width: 8.0,
            dashed: false,
            layer: 0,
            material: Material::Tierra,
            tower_style: TowerStyle::default(),
            tower_every: 0.0,
            sprite: None,
        }];
        let mut c = PathCache::default();
        c.refresh(&cache_paths);
        assert!(!c.curve(1).is_empty());
        assert_eq!(c.ends(1), [false, false]);
    }

    #[test]
    fn walls_are_straight_polylines() {
        let nodes = [pos2(0.0, 0.0), pos2(100.0, 0.0), pos2(100.0, 100.0)];
        let c = curve(PathKind::Muralla, &nodes);
        assert_eq!(c.len(), 3);
        assert_eq!(c[2], (pos2(100.0, 100.0), 1));
        // El camino, en cambio, se suaviza (más muestras).
        assert!(curve(PathKind::Camino, &nodes).len() > 3);
    }

    #[test]
    fn nearest_reports_segment_for_inserting_nodes() {
        let nodes = [pos2(0.0, 0.0), pos2(100.0, 0.0), pos2(200.0, 0.0)];
        let n = nearest(PathKind::Camino, &nodes, pos2(150.0, 7.0)).unwrap();
        assert_eq!(n.seg, 1);
        assert!((n.dist - 7.0).abs() < 0.1);
        let n = nearest(PathKind::Muralla, &nodes, pos2(30.0, -4.0)).unwrap();
        assert_eq!(n.seg, 0);
    }

    #[test]
    fn snap_prefers_nodes_then_segments() {
        let host = p(1, PathKind::Rio, &[(0.0, 0.0), (100.0, 0.0), (200.0, 0.0)]);
        let hosts = [host];
        // Cerca de un nodo: se ancla al nodo.
        let s = snap(hosts.iter(), &[], pos2(103.0, 6.0), SNAP_RADIUS).unwrap();
        assert!(s.on_node && (s.point - pos2(100.0, 0.0)).length() < 1e-4);
        // Entre nodos: se ancla al segmento.
        let s = snap(hosts.iter(), &[], pos2(150.0, 9.0), SNAP_RADIUS).unwrap();
        assert!(!s.on_node && (s.point - pos2(150.0, 0.0)).length() < 1.0);
        // Lejos: nada.
        assert!(snap(hosts.iter(), &[], pos2(150.0, 40.0), SNAP_RADIUS).is_none());
        // Los nodos propios del trazo en construcción también cuentan (cerrar recintos).
        let own = [pos2(500.0, 500.0)];
        assert!(
            snap(hosts.iter(), &own, pos2(505.0, 500.0), SNAP_RADIUS)
                .unwrap()
                .on_node
        );
    }

    #[test]
    fn branch_shares_an_exact_node_with_its_host() {
        let mut v = vec![
            p(1, PathKind::Rio, &[(0.0, 0.0), (100.0, 0.0), (200.0, 0.0)]),
            p(2, PathKind::Rio, &[(150.0, 0.0), (150.0, 80.0)]),
        ];
        attach_to_hosts(&mut v, 2);
        assert_eq!(v[0].nodes.len(), 4);
        assert_eq!(v[0].nodes[2], [150.0, 0.0]);
        // Repetirlo no duplica nodos.
        attach_to_hosts(&mut v, 2);
        assert_eq!(v[0].nodes.len(), 4);

        // Mover el nodo compartido arrastra a ambos trazos.
        move_connected(&mut v, 1, 2, pos2(150.0, -20.0));
        assert_eq!(v[0].nodes[2], [150.0, -20.0]);
        assert_eq!(v[1].nodes[0], [150.0, -20.0]);
        assert_eq!(v[1].nodes[1], [150.0, 80.0]);
    }

    #[test]
    fn freehand_simplification() {
        // Línea casi recta con ruido: queda en 2 puntos.
        let noisy: Vec<Pos2> = (0..=200)
            .map(|i| pos2(i as f32 * 2.0, (i as f32 * 1.7).sin() * 0.8))
            .collect();
        let s = simplify(&noisy, 14.0);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0], noisy[0]);
        assert_eq!(*s.last().unwrap(), *noisy.last().unwrap());

        // Un arco conserva su forma con puntos separados al menos ~min_dist/2.
        let arc: Vec<Pos2> = (0..=180)
            .map(|d| {
                let a = (d as f32).to_radians();
                pos2(100.0 * a.cos(), 100.0 * a.sin())
            })
            .collect();
        let s = simplify(&arc, 14.0);
        assert!(s.len() > 5 && s.len() < arc.len() / 4);
        assert!(s.windows(2).all(|w| (w[1] - w[0]).length() >= 6.0));
        assert!(simplify(&[pos2(0.0, 0.0), pos2(1.0, 1.0)], 14.0).len() == 2);
    }

    #[test]
    fn towers_on_endpoints_and_corners_only() {
        let wall = [
            pos2(0.0, 0.0),
            pos2(100.0, 0.0),
            pos2(200.0, 0.0), // recto: sin torre
            pos2(200.0, 100.0),
        ];
        let t = tower_nodes(&wall);
        let idx: Vec<usize> = t.iter().map(|(i, _)| *i).collect();
        assert!(idx.contains(&0) && idx.contains(&3) && idx.contains(&2));
        assert!(!idx.contains(&1));
        assert!(t.iter().find(|(i, _)| *i == 0).unwrap().1); // extremo
        assert!(!t.iter().find(|(i, _)| *i == 2).unwrap().1); // esquina

        // Recinto cerrado: una sola torre de cierre, sin extremos.
        let ring = [
            pos2(0.0, 0.0),
            pos2(100.0, 0.0),
            pos2(100.0, 100.0),
            pos2(0.0, 0.0),
        ];
        assert!(tower_nodes(&ring).iter().all(|(_, end)| !end));
    }

    #[test]
    fn periodic_towers_and_styles() {
        let wall = [pos2(0.0, 0.0), pos2(600.0, 0.0)];
        // 600 px con una torre cada ~200: extremos + 2 intermedias.
        let t = towers(&wall, TowerStyle::Circular, 200.0);
        assert_eq!(t.len(), 4);
        assert!(t.iter().all(|x| x.round));
        assert!((t[1].pos.x - 200.0).abs() < 1e-3 && (t[2].pos.x - 400.0).abs() < 1e-3);
        assert!(t[1].scale < 1.0);
        let t = towers(&wall, TowerStyle::Cuadrada, 200.0);
        assert!(t.iter().all(|x| !x.round));
        let t = towers(&wall, TowerStyle::Alternada, 200.0);
        assert!(t[0].round && !t[1].round && t[2].round);
        // Sin periódicas: solo extremos.
        assert_eq!(towers(&wall, TowerStyle::Alternada, 0.0).len(), 2);
    }

    #[test]
    fn connected_ends_detect_branches() {
        let host = p(
            1,
            PathKind::Camino,
            &[(0.0, 0.0), (100.0, 0.0), (200.0, 0.0)],
        );
        let branch = p(2, PathKind::Camino, &[(100.0, 0.0), (100.0, 80.0)]);
        let v = vec![host, branch];
        assert_eq!(connected_ends(&v, &v[1]), [true, false]);
        assert_eq!(connected_ends(&v, &v[0]), [false, false]);
        // Solo el extremo libre se desvanece.
        let look = Look::of(&v[1], [true, false]);
        assert_eq!(look.fade, [false, true]);
        let stone = Look::new(PathKind::Camino, Material::Piedra, 8.0, false, [false; 2]);
        assert_eq!(stone.fade, [false, false]);
    }

    #[test]
    fn textured_strip_tiles_along_the_path() {
        // 200 px de largo, alto 20, imagen 2:1 → baldosas de 40 px → 5 baldosas (5 quads).
        let pts = [pos2(0.0, 0.0), pos2(200.0, 0.0)];
        let m = textured_mesh(
            &pts,
            egui::TextureId::default(),
            vec2(64.0, 32.0),
            20.0,
            Color32::WHITE,
        );
        assert_eq!(m.vertices.len(), 5 * 4);
        assert_eq!(m.indices.len(), 5 * 6);
        // `u` recorre 0..1 en cada baldosa y `v` cubre el ancho del trazo.
        assert!(m.vertices.iter().all(|v| (0.0..=1.0).contains(&v.uv.x)));
        let ys: Vec<f32> = m.vertices.iter().map(|v| v.pos.y).collect();
        assert!(ys.iter().cloned().fold(f32::MIN, f32::max) > 9.9);
        assert!(textured_mesh(
            &pts[..1],
            egui::TextureId::default(),
            vec2(1.0, 1.0),
            20.0,
            Color32::WHITE
        )
        .is_empty());
    }

    #[test]
    fn towers_follow_the_wall_direction() {
        use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};
        // Muro horizontal, luego vertical hacia abajo (pantalla): esquina a 90°.
        let wall = [pos2(0.0, 0.0), pos2(400.0, 0.0), pos2(400.0, 400.0)];
        let t = towers(&wall, TowerStyle::Cuadrada, 200.0);
        // Extremo inicial y torre periódica del primer tramo: ángulo 0.
        assert!(t[0].angle.abs() < 1e-4);
        let mid = t
            .iter()
            .find(|x| (x.pos - pos2(200.0, 0.0)).length() < 1e-3)
            .unwrap();
        assert!(mid.angle.abs() < 1e-4);
        // La esquina usa la bisectriz (45° entre 0° y 90°).
        let corner = t
            .iter()
            .find(|x| (x.pos - pos2(400.0, 0.0)).length() < 1e-3)
            .unwrap();
        assert!((corner.angle - FRAC_PI_4).abs() < 1e-4);
        // Extremo final y torre periódica del tramo vertical: 90°.
        let last = t.last().unwrap();
        assert!((last.angle - FRAC_PI_2).abs() < 1e-4);
        let mid2 = t
            .iter()
            .find(|x| (x.pos - pos2(400.0, 200.0)).length() < 1e-3)
            .unwrap();
        assert!((mid2.angle - FRAC_PI_2).abs() < 1e-4);
        // Un cuadrado girado 90° coincide con el original (mismos vértices desplazados).
        let a = rotated_square(pos2(0.0, 0.0), 10.0, 0.0);
        let b = rotated_square(pos2(0.0, 0.0), 10.0, FRAC_PI_2);
        assert!(a
            .iter()
            .all(|p| b.iter().any(|q| (*p - *q).length() < 1e-3)));
    }
}
