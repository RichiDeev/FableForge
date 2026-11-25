//! Cámara 2D: transformaciones pantalla <-> mundo, pan, zoom al cursor y ajuste a ventana.

use egui::{pos2, Pos2, Rect, Vec2};

pub const MIN_ZOOM: f32 = 0.10;
pub const MAX_ZOOM: f32 = 8.0;

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    /// Punto del mundo que queda en el centro del viewport.
    pub center: Pos2,
    pub zoom: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            center: pos2(0.0, 0.0),
            zoom: 1.0,
        }
    }
}

impl Camera {
    pub fn world_to_screen(&self, view: Rect, p: Pos2) -> Pos2 {
        view.center() + (p - self.center) * self.zoom
    }

    pub fn screen_to_world(&self, view: Rect, s: Pos2) -> Pos2 {
        self.center + (s - view.center()) / self.zoom
    }

    /// Desplaza la vista arrastrando `delta` píxeles de pantalla.
    pub fn pan_screen(&mut self, delta: Vec2) {
        self.center -= delta / self.zoom;
    }

    /// Zoom multiplicativo manteniendo fijo el punto del mundo bajo `cursor`.
    pub fn zoom_at(&mut self, view: Rect, cursor: Pos2, factor: f32) {
        let before = self.screen_to_world(view, cursor);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let after = self.screen_to_world(view, cursor);
        self.center += before - after;
    }

    /// Fija el zoom absoluto dejando el centro de la vista donde está.
    pub fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
    }

    /// Centra y encuadra `world` dentro de `view` con un margen proporcional.
    pub fn fit(&mut self, view: Rect, world: Rect, margin: f32) {
        let w = world.width().max(1.0);
        let h = world.height().max(1.0);
        let z = (view.width() / w).min(view.height() / h) * (1.0 - margin * 2.0);
        self.zoom = z.clamp(MIN_ZOOM, MAX_ZOOM);
        self.center = world.center();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> Rect {
        Rect::from_min_size(pos2(100.0, 50.0), egui::vec2(800.0, 600.0))
    }

    #[test]
    fn roundtrip() {
        let c = Camera {
            center: pos2(300.0, -40.0),
            zoom: 2.5,
        };
        let p = pos2(123.0, 456.0);
        let s = c.world_to_screen(view(), p);
        let back = c.screen_to_world(view(), s);
        assert!((back - p).length() < 1e-3);
    }

    #[test]
    fn zoom_keeps_point_under_cursor() {
        let mut c = Camera::default();
        let cursor = pos2(250.0, 200.0);
        let before = c.screen_to_world(view(), cursor);
        c.zoom_at(view(), cursor, 1.7);
        let after = c.screen_to_world(view(), cursor);
        assert!((before - after).length() < 1e-3);
        assert!((c.zoom - 1.7).abs() < 1e-5);
    }

    #[test]
    fn zoom_is_clamped() {
        let mut c = Camera::default();
        c.zoom_at(view(), view().center(), 1000.0);
        assert_eq!(c.zoom, MAX_ZOOM);
        c.zoom_at(view(), view().center(), 1e-6);
        assert_eq!(c.zoom, MIN_ZOOM);
    }

    #[test]
    fn fit_centers_world() {
        let mut c = Camera::default();
        let world = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(2000.0, 1400.0));
        c.fit(view(), world, 0.05);
        assert_eq!(c.center, world.center());
        let tl = c.world_to_screen(view(), world.min);
        let br = c.world_to_screen(view(), world.max);
        assert!(view().contains(tl) && view().contains(br));
    }
}
