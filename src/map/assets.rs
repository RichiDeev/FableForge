// src/map/assets.rs
//! Biblioteca de assets: globales (`assets/map_defaults/`, `%APPDATA%/FableForge/global_assets/`)
//! y personalizados de la historia (`05 - Medias/Assets/`).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::project::natural_cmp;
use crate::story::MEDIA_DIR;

pub const STORY_ASSETS_DIR: &str = "Assets";
const STORY_CATEGORY: &str = "Mis assets";

#[derive(Clone, Debug)]
pub struct AssetInfo {
    /// `g:<ruta relativa>` (global) o `s:<ruta relativa>` (de la historia).
    pub key: String,
    pub name: String,
    pub category: String,
    pub path: PathBuf,
    pub global: bool,
}

#[derive(Default)]
pub struct AssetLibrary {
    pub items: Vec<AssetInfo>,
    index: HashMap<String, usize>,
}

/// Candidatas (en orden de prioridad) para `assets/map_defaults/`: junto al directorio de trabajo,
/// junto al ejecutable (y hasta 3 carpetas por encima, útil en desarrollo: `target/release/`),
/// el árbol de código fuente y la carpeta de usuario (`%APPDATA%/FableForge/global_assets`).
fn candidate_dirs(
    cwd: Option<&Path>,
    exe_dir: Option<&Path>,
    manifest: &Path,
    config: Option<&Path>,
) -> Vec<PathBuf> {
    let rel = |base: &Path| base.join("assets").join("map_defaults");
    let mut cands: Vec<PathBuf> = Vec::new();
    if let Some(cwd) = cwd {
        cands.push(rel(cwd));
    }
    if let Some(dir) = exe_dir {
        cands.extend(dir.ancestors().take(4).map(rel));
    }
    cands.push(rel(manifest));
    if let Some(cfg) = config {
        cands.push(cfg.join("FableForge").join("global_assets"));
    }
    cands
}

/// Carpetas globales donde buscar assets (sin duplicados, solo las que existen).
/// Nunca falla: si no existe ninguna, la biblioteca global queda vacía.
pub fn global_dirs() -> Vec<PathBuf> {
    let cwd = std::env::current_dir().ok();
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf));
    let config = dirs::config_dir();
    // Carpeta de usuario para assets propios: se crea si falta (error ignorado).
    if let Some(cfg) = &config {
        let _ = fs::create_dir_all(cfg.join("FableForge").join("global_assets"));
    }
    let cands = candidate_dirs(
        cwd.as_deref(),
        exe_dir.as_deref(),
        Path::new(env!("CARGO_MANIFEST_DIR")),
        config.as_deref(),
    );
    let mut seen = HashSet::new();
    cands
        .into_iter()
        .filter(|p| p.is_dir())
        .filter(|p| seen.insert(fs::canonicalize(p).unwrap_or_else(|_| p.clone())))
        .collect()
}

pub fn story_assets_dir(story: &Path) -> PathBuf {
    story.join(MEDIA_DIR).join(STORY_ASSETS_DIR)
}

/// Categoría según el prefijo del nombre (`Oak_1.png` → Árboles).
pub fn category_from_prefix(stem: &str) -> String {
    let prefix: String = stem.chars().take_while(|c| c.is_alphabetic()).collect();
    match prefix.to_lowercase().as_str() {
        "oak" | "pine" | "birch" | "tree" | "palm" | "willow" | "fir" | "spruce" | "maple"
        | "elm" => "Árboles".into(),
        "bush" | "shrub" | "hedge" => "Arbustos".into(),
        "house" | "tower" | "castle" | "wall" | "hut" | "barn" | "building" | "ruin" | "tent"
        | "bridge" => "Estructuras".into(),
        "rock" | "stone" | "boulder" => "Rocas".into(),
        "" => "Varios".into(),
        _ => {
            let mut c = prefix.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        }
    }
}

fn is_png(p: &Path) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("png"))
}

fn category_rank(c: &str) -> usize {
    ["Árboles", "Arbustos", "Estructuras", "Rocas"]
        .iter()
        .position(|x| *x == c)
        .unwrap_or(if c == STORY_CATEGORY { 98 } else { 50 })
}

impl AssetLibrary {
    /// Escanea las carpetas globales y la de la historia.
    pub fn scan(story: &Path) -> Self {
        let mut items: Vec<AssetInfo> = Vec::new();
        for root in global_dirs() {
            scan_dir(&root, &root, true, 0, &mut items);
        }
        let own = story_assets_dir(story);
        scan_dir(&own, &own, false, 0, &mut items);

        let mut seen = HashSet::new();
        items.retain(|a| seen.insert(a.key.clone()));
        items.sort_by(|a, b| {
            category_rank(&a.category)
                .cmp(&category_rank(&b.category))
                .then_with(|| natural_cmp(&a.category, &b.category))
                .then_with(|| natural_cmp(&a.name, &b.name))
        });
        let index = items
            .iter()
            .enumerate()
            .map(|(i, a)| (a.key.clone(), i))
            .collect();
        Self { items, index }
    }

    pub fn get(&self, key: &str) -> Option<&AssetInfo> {
        self.index.get(key).map(|&i| &self.items[i])
    }

    /// Assets agrupados por categoría, en orden de visualización.
    pub fn categories(&self) -> Vec<(String, Vec<&AssetInfo>)> {
        let mut out: Vec<(String, Vec<&AssetInfo>)> = Vec::new();
        for a in &self.items {
            match out.last_mut() {
                Some((c, v)) if *c == a.category => v.push(a),
                _ => out.push((a.category.clone(), vec![a])),
            }
        }
        out
    }
}

fn scan_dir(root: &Path, dir: &Path, global: bool, depth: u32, out: &mut Vec<AssetInfo>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for path in rd.filter_map(|e| e.ok()).map(|e| e.path()) {
        if path.is_dir() {
            if depth < 2 {
                scan_dir(root, &path, global, depth + 1, out);
            }
            continue;
        }
        if !is_png(&path) {
            continue;
        }
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let rel_s = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let category = match rel.parent().and_then(|p| p.components().next_back()) {
            Some(c) => c.as_os_str().to_string_lossy().into_owned(),
            None if global => category_from_prefix(&stem),
            None => STORY_CATEGORY.to_owned(),
        };
        out.push(AssetInfo {
            key: format!("{}:{rel_s}", if global { "g" } else { "s" }),
            name: stem.replace('_', " "),
            category,
            path,
            global,
        });
    }
}

fn copy_unique(src: &Path, dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("No se pudo crear la carpeta de assets: {e}"))?;
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "asset".into());
    let mut target = dir.join(format!("{stem}.png"));
    let mut n = 2;
    while target.exists() {
        target = dir.join(format!("{stem} ({n}).png"));
        n += 1;
    }
    fs::copy(src, &target).map_err(|e| format!("No se pudo copiar '{}': {e}", src.display()))?;
    Ok(())
}

/// Copia PNGs sueltos a `05 - Medias/Assets/` de la historia. Devuelve cuántos se importaron.
pub fn import_files(story: &Path, files: &[PathBuf]) -> Result<usize, String> {
    let dir = story_assets_dir(story);
    let mut n = 0;
    for f in files.iter().filter(|f| is_png(f)) {
        copy_unique(f, &dir)?;
        n += 1;
    }
    Ok(n)
}

/// Copia los PNG de una carpeta externa (con subcarpetas) a `05 - Medias/Assets/<carpeta>/`.
pub fn import_folder(story: &Path, folder: &Path) -> Result<usize, String> {
    let name = folder
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Paquete".into());
    let dest = story_assets_dir(story).join(crate::story::sanitize_folder_name(&name));
    let mut n = 0;
    copy_tree(folder, &dest, 0, &mut n)?;
    Ok(n)
}

fn copy_tree(src: &Path, dest: &Path, depth: u32, n: &mut usize) -> Result<(), String> {
    let Ok(rd) = fs::read_dir(src) else {
        return Ok(());
    };
    for path in rd.filter_map(|e| e.ok()).map(|e| e.path()) {
        if path.is_dir() {
            if depth < 2 {
                let sub = path
                    .file_name()
                    .map(|s| s.to_os_string())
                    .unwrap_or_default();
                copy_tree(&path, &dest.join(sub), depth + 1, n)?;
            }
        } else if is_png(&path) {
            copy_unique(&path, dest)?;
            *n += 1;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_categories() {
        assert_eq!(category_from_prefix("Oak_1"), "Árboles");
        assert_eq!(category_from_prefix("Pine_1"), "Árboles");
        assert_eq!(category_from_prefix("Bush_2"), "Arbustos");
        assert_eq!(category_from_prefix("Castle_big"), "Estructuras");
        assert_eq!(category_from_prefix("Dragon_1"), "Dragon");
    }

    #[test]
    fn scan_and_import() {
        let base = std::env::temp_dir().join(format!("ff_assets_{}", std::process::id()));
        let story = base.join("story");
        let ext = base.join("pack");
        fs::create_dir_all(ext.join("Rocas")).unwrap();
        for f in ["a.png", "Rocas/r1.png"] {
            image::RgbaImage::new(2, 2).save(ext.join(f)).unwrap();
        }
        fs::write(ext.join("nota.txt"), "x").unwrap();
        assert_eq!(import_folder(&story, &ext).unwrap(), 2);
        assert_eq!(import_files(&story, &[ext.join("a.png")]).unwrap(), 1);
        assert_eq!(import_files(&story, &[ext.join("a.png")]).unwrap(), 1); // sin pisar: a (2).png
        let lib = AssetLibrary::scan(&story);
        let mine: Vec<_> = lib.items.iter().filter(|a| !a.global).collect();
        assert_eq!(mine.len(), 4);
        assert!(lib.get("s:a.png").is_some());
        assert!(lib.get("s:a (2).png").is_some());
        assert!(lib
            .items
            .iter()
            .any(|a| a.key == "s:pack/Rocas/r1.png" && a.category == "Rocas"));
        let _ = fs::remove_dir_all(base);
    }
}

#[cfg(test)]
mod portable_tests {
    use super::*;

    #[test]
    fn candidates_cover_cwd_exe_and_user_dirs() {
        let c = candidate_dirs(
            Some(Path::new("/work")),
            Some(Path::new("/opt/ff/target/release")),
            Path::new("/src"),
            Some(Path::new("/cfg")),
        );
        let md = |b: &str| Path::new(b).join("assets").join("map_defaults");
        assert_eq!(c[0], md("/work"));
        assert!(c.contains(&md("/opt/ff/target/release")));
        assert!(c.contains(&md("/opt/ff")));
        assert!(c.contains(&Path::new("/cfg").join("FableForge").join("global_assets")));
    }

    #[test]
    fn missing_dirs_never_fail() {
        let _ = global_dirs();
        let lib = AssetLibrary::scan(Path::new("/definitivamente/no/existe"));
        let _ = lib.items.len();
    }
}
