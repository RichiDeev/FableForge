// src/map/zones.rs
//! Zonas poligonales / territorios (reinos, distritos, zonas de peligro): geometría, edición de
//! vértices y dibujo (relleno translúcido, borde punteado y etiqueta centrada).

use egui::epaint::{Vertex, WHITE_UV};
use egui::{pos2, Align2, Color32, FontFamily, FontId, Mesh, Painter, Pos2, Shape, Stroke, Vec2};
use serde::{Deserialize, Serialize};

/// Opacidad del relleno de una zona.
pub const FILL_ALPHA: f32 = 0.22;

/// Colores apagados que se asignan a las zonas nuevas en rotación.
pub const PALETTE: [[u8; 3]; 8] = [
    [0x8E, 0x5B, 0x4B], // arcilla
    [0x5D, 0x7A, 0x47], // musgo
    [0x4C, 0x6A, 0x8C], // azul pizarra
    [0xB0, 0x8A, 0x3E], // ocre
    [0x7A, 0x56, 0x8C], // ciruela
    [0x3F, 0x7F, 0x7A], // turquesa
    [0xA6, 0x4B, 0x5B], // granate
    [0x6B, 0x6B, 0x6B], // grafito
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Zone {
    pub id: u64,
    pub name: String,
    /// Vértices en coordenadas del mundo (polígono cerrado implícito).
    pub points: Vec<[f32; 2]>,
    pub color: [u8; 3],
    /// Mapa interior vinculado (nombre de archivo dentro de `03 - Mapas/`).
    #[serde(default)]
    pub link: Option<String>,
    /// Altura / relieve (0–1): colinas y montañas proyectan sombra según la hora del día.
    #[serde(default)]
    pub relief: f32,
}

impl Zone {
    pub fn pts(&self) -> Vec<Pos2> {
        self.points.iter().map(|p| pos2(p[0], p[1])).collect()
    }

    pub fn color32(&self) -> Color32 {
        Color32::from_rgb(self.color[0], self.color[1], self.color[2])
    }
}

/// Área con signo (fórmula del zapatero).
pub fn signed_area(poly: &[Pos2]) -> f32 {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum::<f32>()
        * 0.5
}

/// Centroide por área; si el polígono es degenerado, el promedio de los vértices.
pub fn centroid(poly: &[Pos2]) -> Pos2 {
    let n = poly.len();
    if n == 0 {
        return Pos2::ZERO;
    }
    let a = signed_area(poly);
    if a.abs() < 1e-3 {
        let s = poly
            .iter()
            .fold(Pos2::ZERO.to_vec2(), |s, p| s + p.to_vec2());
        return (s / n as f32).to_pos2();
    }
    let (mut cx, mut cy) = (0.0, 0.0);
    for i in 0..n {
        let (p, q) = (poly[i], poly[(i + 1) % n]);
        let cross = p.x * q.y - q.x * p.y;
        cx += (p.x + q.x) * cross;
        cy += (p.y + q.y) * cross;
    }
    pos2(cx / (6.0 * a), cy / (6.0 * a))
}

/// ¿`p` está dentro del polígono? (regla par-impar)
pub fn contains(poly: &[Pos2], p: Pos2) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Arista más cercana a `p`: `(índice donde insertar el nuevo vértice, punto, distancia)`.
pub fn nearest_edge(poly: &[Pos2], p: Pos2) -> Option<(usize, Pos2, f32)> {
    let n = poly.len();
    let mut best: Option<(usize, Pos2, f32)> = None;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let ab = b - a;
        let len2 = ab.length_sq();
        let t = if len2 > 0.0 {
            ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let q = a + ab * t;
        let d = (p - q).length();
        if best.is_none_or(|(_, _, bd)| d < bd) {
            best = Some((i + 1, q, d));
        }
    }
    best
}

fn cross(o: Pos2, a: Pos2, b: Pos2) -> f32 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

fn in_triangle(p: Pos2, a: Pos2, b: Pos2, c: Pos2) -> bool {
    let (d1, d2, d3) = (cross(a, b, p), cross(b, c, p), cross(c, a, p));
    d1 >= 0.0 && d2 >= 0.0 && d3 >= 0.0
}

/// Triangula un polígono simple (cóncavo o convexo) por *ear clipping*.
pub fn triangulate(poly: &[Pos2]) -> Vec<[usize; 3]> {
    let n = poly.len();
    if n < 3 {
        return Vec::new();
    }
    let mut idx: Vec<usize> = (0..n).collect();
    if signed_area(poly) < 0.0 {
        idx.reverse();
    }
    let mut out = Vec::with_capacity(n - 2);
    while idx.len() > 3 {
        let m = idx.len();
        let ear = (0..m).find(|&i| {
            let (ia, ib, ic) = (idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]);
            let (a, b, c) = (poly[ia], poly[ib], poly[ic]);
            if cross(a, b, c) <= 1e-6 {
                return false; // vértice reflejo o colineal
            }
            !idx.iter()
                .filter(|&&k| k != ia && k != ib && k != ic)
                .any(|&k| in_triangle(poly[k], a, b, c))
        });
        match ear {
            Some(i) => {
                let (ia, ib, ic) = (idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]);
                out.push([ia, ib, ic]);
                idx.remove(i);
            }
            None => {
                // Polígono degenerado o autointersectado: abanico con lo que queda.
                for k in 1..idx.len() - 1 {
                    out.push([idx[0], idx[k], idx[k + 1]]);
                }
                return out;
            }
        }
    }
    out.push([idx[0], idx[1], idx[2]]);
    out
}

/// Envolvente convexa (cadena monótona de Andrew), en sentido antihorario de pantalla.
pub fn convex_hull(points: &[Pos2]) -> Vec<Pos2> {
    let mut pts: Vec<Pos2> = points.to_vec();
    pts.sort_by(|a, b| {
        a.x.partial_cmp(&b.x)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal))
    });
    pts.dedup_by(|a, b| (*a - *b).length_sq() < 1e-6);
    if pts.len() < 3 {
        return pts;
    }
    let mut hull: Vec<Pos2> = Vec::new();
    for pass in 0..2 {
        let start = hull.len();
        let iter: Box<dyn Iterator<Item = &Pos2>> = if pass == 0 {
            Box::new(pts.iter())
        } else {
            Box::new(pts.iter().rev())
        };
        for &p in iter {
            while hull.len() >= start + 2
                && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0
            {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
    }
    hull
}

/// Sombra de relieve de una zona: la envolvente de su contorno y del contorno desplazado
/// `offset` (en pantalla), con un único color translúcido (sin solapes que oscurezcan).
pub fn paint_relief_shadow(painter: &Painter, pts: &[Pos2], offset: Vec2, color: Color32) {
    if pts.len() < 3 {
        return;
    }
    let mut all = pts.to_vec();
    all.extend(pts.iter().map(|p| *p + offset));
    let hull = convex_hull(&all);
    if hull.len() < 3 {
        return;
    }
    let mut mesh = Mesh::default();
    for p in &hull {
        mesh.vertices.push(Vertex {
            pos: *p,
            uv: WHITE_UV,
            color,
        });
    }
    for i in 1..hull.len() - 1 {
        mesh.indices.extend([0, i as u32, i as u32 + 1]);
    }
    painter.add(Shape::mesh(mesh));
}

/// Dibuja una zona sobre puntos ya en pantalla.
#[allow(clippy::too_many_arguments)]
pub fn paint(
    painter: &Painter,
    pts: &[Pos2],
    zone: &Zone,
    selected: bool,
    accent: Color32,
    family: FontFamily,
    zoom: f32,
) {
    if pts.len() < 3 {
        return;
    }
    let base = zone.color32();
    let fill = base.gamma_multiply(FILL_ALPHA);
    let mut mesh = Mesh::default();
    for p in pts {
        mesh.vertices.push(Vertex {
            pos: *p,
            uv: WHITE_UV,
            color: fill,
        });
    }
    for t in triangulate(pts) {
        mesh.indices.extend(t.map(|i| i as u32));
    }
    painter.add(Shape::mesh(mesh));

    // Borde punteado estilo cartográfico.
    let mut ring = pts.to_vec();
    ring.push(pts[0]);
    let border = if selected {
        Stroke::new(2.5, accent)
    } else {
        let d = base.to_array();
        Stroke::new(
            2.0,
            Color32::from_rgb(
                (d[0] as f32 * 0.65) as u8,
                (d[1] as f32 * 0.65) as u8,
                (d[2] as f32 * 0.65) as u8,
            ),
        )
    };
    painter.extend(Shape::dashed_line(&ring, border, 10.0, 5.0));

    // Etiqueta centrada (solo si la zona se ve lo bastante grande).
    let bb = egui::Rect::from_points(pts);
    if bb.width() >= 70.0 && bb.height() >= 30.0 {
        let c = centroid(pts);
        let size = (13.0 + 3.0 * zoom.min(2.0)).clamp(12.0, 19.0);
        let font = FontId::new(size, family);
        let ink = Color32::from_rgb(
            (base.r() as f32 * 0.35) as u8,
            (base.g() as f32 * 0.35) as u8,
            (base.b() as f32 * 0.35) as u8,
        );
        let halo = Color32::from_white_alpha(170);
        for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
            painter.text(
                c + egui::vec2(dx, dy),
                Align2::CENTER_CENTER,
                &zone.name,
                font.clone(),
                halo,
            );
        }
        painter.text(c, Align2::CENTER_CENTER, &zone.name, font, ink);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l_shape() -> Vec<Pos2> {
        vec![
            pos2(0.0, 0.0),
            pos2(100.0, 0.0),
            pos2(100.0, 40.0),
            pos2(40.0, 40.0),
            pos2(40.0, 100.0),
            pos2(0.0, 100.0),
        ]
    }

    #[test]
    fn triangulation_covers_concave_polygon_exactly() {
        for poly in [l_shape(), l_shape().into_iter().rev().collect::<Vec<_>>()] {
            let tris = triangulate(&poly);
            assert_eq!(tris.len(), poly.len() - 2);
            let sum: f32 = tris
                .iter()
                .map(|t| signed_area(&[poly[t[0]], poly[t[1]], poly[t[2]]]).abs())
                .sum();
            assert!((sum - signed_area(&poly).abs()).abs() < 1e-2, "{sum}");
        }
    }

    #[test]
    fn point_in_polygon_and_centroid() {
        let p = l_shape();
        assert!(contains(&p, pos2(20.0, 80.0)));
        assert!(contains(&p, pos2(80.0, 20.0)));
        assert!(!contains(&p, pos2(80.0, 80.0)));
        let sq = [
            pos2(0.0, 0.0),
            pos2(10.0, 0.0),
            pos2(10.0, 10.0),
            pos2(0.0, 10.0),
        ];
        let c = centroid(&sq);
        assert!((c - pos2(5.0, 5.0)).length() < 1e-4);
    }

    #[test]
    fn nearest_edge_gives_insert_index() {
        let sq = [
            pos2(0.0, 0.0),
            pos2(10.0, 0.0),
            pos2(10.0, 10.0),
            pos2(0.0, 10.0),
        ];
        let (at, pt, d) = nearest_edge(&sq, pos2(5.0, -3.0)).unwrap();
        assert_eq!(at, 1);
        assert!((pt - pos2(5.0, 0.0)).length() < 1e-4 && (d - 3.0).abs() < 1e-4);
        // Arista de cierre (último → primero) inserta al final.
        let (at, _, _) = nearest_edge(&sq, pos2(-2.0, 5.0)).unwrap();
        assert_eq!(at, 4);
    }

    #[test]
    fn hull_of_zone_and_shifted_copy() {
        let sq = [
            pos2(0.0, 0.0),
            pos2(10.0, 0.0),
            pos2(10.0, 10.0),
            pos2(0.0, 10.0),
            pos2(5.0, 5.0), // interior: no está en la envolvente
        ];
        let h = convex_hull(&sq);
        assert_eq!(h.len(), 4);
        let mut all = sq.to_vec();
        all.extend(sq.iter().map(|p| *p + egui::vec2(20.0, 0.0)));
        let h = convex_hull(&all);
        assert!((signed_area(&h).abs() - 300.0).abs() < 1e-3); // 30×10
    }
}
