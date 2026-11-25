//! Generación de la base de terreno: imagen importada o máscara (oscuro = tierra, claro = agua).

use std::path::Path;

use egui::Color32;
use image::{Rgba, RgbaImage};

use crate::map::document::{BaseImage, MapStyle};
use crate::project::{self, MEDIA_MAPS};

const MAX_SIDE: f32 = 4096.0;

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn mix(a: Color32, b: Color32, t: f32) -> Rgba<u8> {
    let l = |x: u8, y: u8| (x as f32 * (1.0 - t) + y as f32 * t).round() as u8;
    Rgba([l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), 255])
}

/// Convierte un boceto en terreno: oscuro → tierra, claro → agua, con línea de costa nítida.
pub fn mask_to_terrain(src: &RgbaImage, style: MapStyle) -> RgbaImage {
    let (w, h) = src.dimensions();
    let (land, water, coast) = (style.land(), style.water(), style.coast());

    // Luminancia efectiva (el alfa transparente cuenta como agua/blanco).
    let lum: Vec<f32> = src
        .pixels()
        .map(|p| {
            let a = p[3] as f32 / 255.0;
            let l = (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) / 255.0;
            l * a + (1.0 - a)
        })
        .collect();
    let is_land = |x: u32, y: u32| lum[(y * w + x) as usize] < 0.5;

    let mut out = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let t = smoothstep(0.43, 0.57, lum[(y * w + x) as usize]);
            let mut px = mix(land, water, t);
            if is_land(x, y) {
                let edge = (x > 0 && !is_land(x - 1, y))
                    || (x + 1 < w && !is_land(x + 1, y))
                    || (y > 0 && !is_land(x, y - 1))
                    || (y + 1 < h && !is_land(x, y + 1));
                if edge {
                    px = Rgba([coast.r(), coast.g(), coast.b(), 255]);
                }
            }
            out.put_pixel(x, y, px);
        }
    }
    out
}

/// Dimensiones destino: ajustadas a `fixed` (conservando proporción) o limitadas a 4096 px.
pub fn target_size(iw: u32, ih: u32, fixed: Option<[f32; 2]>) -> (u32, u32) {
    let (iw, ih) = (iw as f32, ih as f32);
    let s = match fixed {
        Some([w, h]) => (w / iw).min(h / ih),
        None => (MAX_SIDE / iw.max(ih)).min(1.0),
    };
    (
        (iw * s).round().max(1.0) as u32,
        (ih * s).round().max(1.0) as u32,
    )
}

/// Importa una imagen (o máscara) como base del mapa y la guarda en `05 - Medias/Mapas/`.
pub fn make_base(
    story: &Path,
    slug: &str,
    src: &Path,
    style: MapStyle,
    fixed: Option<[f32; 2]>,
    mask: bool,
) -> Result<BaseImage, String> {
    let img = image::open(src)
        .map_err(|e| format!("No se pudo leer la imagen '{}': {e}", src.display()))?;
    let (tw, th) = target_size(img.width(), img.height(), fixed);
    let rgba = img
        .resize_exact(tw, th, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    let out = if mask {
        mask_to_terrain(&rgba, style)
    } else {
        rgba
    };

    let dir = project::media_dir(story, MEDIA_MAPS);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("No se pudo crear la carpeta de medios: {e}"))?;
    let file = format!("{slug}_base.png");
    out.save_with_format(dir.join(&file), image::ImageFormat::Png)
        .map_err(|e| format!("No se pudo guardar la base del mapa: {e}"))?;
    Ok(BaseImage {
        file,
        size: [tw as f32, th as f32],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_splits_land_and_water_with_coast() {
        let mut m = RgbaImage::from_pixel(20, 20, Rgba([255, 255, 255, 255]));
        for y in 5..15 {
            for x in 5..15 {
                m.put_pixel(x, y, Rgba([10, 10, 10, 255]));
            }
        }
        let out = mask_to_terrain(&m, MapStyle::Atlas);
        let water = MapStyle::Atlas.water();
        let land = MapStyle::Atlas.land();
        let coast = MapStyle::Atlas.coast();
        assert_eq!(
            out.get_pixel(1, 1).0,
            [water.r(), water.g(), water.b(), 255]
        );
        assert_eq!(out.get_pixel(10, 10).0, [land.r(), land.g(), land.b(), 255]);
        assert_eq!(
            out.get_pixel(5, 10).0,
            [coast.r(), coast.g(), coast.b(), 255]
        );
    }

    #[test]
    fn target_sizes() {
        assert_eq!(target_size(1000, 500, Some([2000.0, 1400.0])), (2000, 1000));
        assert_eq!(target_size(8000, 4000, None), (4096, 2048));
        assert_eq!(target_size(800, 600, None), (800, 600));
    }
}
