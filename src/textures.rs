//! Caché de texturas cargadas directamente desde disco (sin loaders `file://` de egui).
//!
//! `image::open` → RGBA8 → `ColorImage` → `ctx.load_texture`. Funciona igual con rutas de Windows
//! (`C:\...`). Si una imagen falla, el error exacto se imprime en consola (una sola vez) y se
//! muestra como tooltip sobre el marcador de error. Base para los sprites de mapas (Etapa 3).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use egui::{
    load::SizedTexture, Align2, Color32, ColorImage, Context, CornerRadius, FontId, Id, Image,
    Rect, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui, Vec2,
};

enum Entry {
    Loaded(TextureHandle),
    Failed(String),
}

#[derive(Default)]
pub struct TextureCache {
    map: HashMap<PathBuf, Entry>,
}

const ERROR_COLOR: Color32 = Color32::from_rgb(0xB0, 0x4A, 0x3A);

fn load(ctx: &Context, path: &Path) -> Entry {
    let result = image::open(path)
        .map_err(|e| format!("No se pudo cargar la imagen '{}': {e}", path.display()))
        .map(|img| {
            let rgba = img.to_rgba8();
            let size = [rgba.width() as usize, rgba.height() as usize];
            let color = ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
            ctx.load_texture(
                format!("img:{}", path.display()),
                color,
                TextureOptions::LINEAR,
            )
        });
    match result {
        Ok(tex) => Entry::Loaded(tex),
        Err(msg) => {
            eprintln!("[FableForge] {msg}");
            Entry::Failed(msg)
        }
    }
}

impl TextureCache {
    /// Textura de `path` (la carga la primera vez). `Err` trae el mensaje exacto del fallo.
    pub fn get(&mut self, ctx: &Context, path: &Path) -> Result<&TextureHandle, &str> {
        let entry = self
            .map
            .entry(path.to_path_buf())
            .or_insert_with(|| load(ctx, path));
        match entry {
            Entry::Loaded(t) => Ok(t),
            Entry::Failed(e) => Err(e.as_str()),
        }
    }

    /// Descarta la textura cacheada (usar cuando el usuario reemplaza un archivo existente).
    pub fn invalidate(&mut self, path: &Path) {
        self.map.remove(path);
    }

    /// Dibuja la imagen ajustada (sin deformar) y centrada dentro de `rect`.
    pub fn paint(&mut self, ui: &mut Ui, path: &Path, rect: Rect, rounding: CornerRadius) {
        match self.get(ui.ctx(), path) {
            Ok(tex) => {
                let size = tex.size_vec2();
                let scale = (rect.width() / size.x).min(rect.height() / size.y);
                let drawn = Rect::from_center_size(rect.center(), size * scale);
                Image::new(SizedTexture::new(tex.id(), size))
                    .corner_radius(rounding)
                    .paint_at(ui, drawn);
            }
            Err(msg) => {
                let msg = msg.to_owned();
                error_marker(ui, path, rect, rounding, &msg);
            }
        }
    }

    /// Reserva espacio y dibuja la imagen a su tamaño natural, limitada a `max` (sin ampliar).
    pub fn show(&mut self, ui: &mut Ui, path: &Path, max: Vec2, rounding: CornerRadius) {
        let size = match self.get(ui.ctx(), path) {
            Ok(tex) => {
                let s = tex.size_vec2();
                let scale = (max.x / s.x).min(max.y / s.y).min(1.0);
                Some(s * scale)
            }
            Err(_) => None,
        };
        match size {
            Some(size) => {
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                self.paint(ui, path, rect, rounding);
            }
            None => {
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::new(max.x.min(220.0), 40.0), Sense::hover());
                self.paint(ui, path, rect, rounding);
            }
        }
    }
}

fn error_marker(ui: &mut Ui, path: &Path, rect: Rect, rounding: CornerRadius, msg: &str) {
    let p = ui.painter();
    p.rect_stroke(
        rect,
        rounding,
        Stroke::new(1.0, ERROR_COLOR),
        StrokeKind::Inside,
    );
    let font = FontId::proportional((rect.height() * 0.5).clamp(11.0, 20.0));
    p.text(rect.center(), Align2::CENTER_CENTER, "!", font, ERROR_COLOR);
    let id = Id::unique(("img_err", path));
    ui.interact(rect, id, Sense::hover()).on_hover_text(msg);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_reports_and_invalidates() {
        let dir = std::env::temp_dir().join(format!("ff_tex_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ok = dir.join("ok.png");
        image::RgbaImage::from_pixel(4, 2, image::Rgba([200, 100, 50, 255]))
            .save(&ok)
            .unwrap();
        let bad = dir.join("bad.png");
        std::fs::write(&bad, b"no es una imagen").unwrap();

        let ctx = Context::default();
        let mut cache = TextureCache::default();
        assert_eq!(cache.get(&ctx, &ok).unwrap().size(), [4, 2]);
        let err = cache.get(&ctx, &bad).err().unwrap().to_owned();
        assert!(err.contains("bad.png"), "{err}");
        assert!(cache.get(&ctx, &dir.join("falta.png")).is_err());

        // Reemplazar el archivo y invalidar recarga la textura nueva.
        image::RgbaImage::from_pixel(8, 8, image::Rgba([0, 0, 0, 255]))
            .save(&ok)
            .unwrap();
        assert_eq!(cache.get(&ctx, &ok).unwrap().size(), [4, 2]);
        cache.invalidate(&ok);
        assert_eq!(cache.get(&ctx, &ok).unwrap().size(), [8, 8]);
        let _ = std::fs::remove_dir_all(dir);
    }
}
