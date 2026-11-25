// src/story.rs
//! Modelo de historia en disco: carpeta + `story.json` + subcarpetas base.

use crate::i18n::tr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

pub const STORY_FILE: &str = "story.json";
pub const LORE_DIR: &str = "01 - Lore y Cronologia";
pub const ENTITIES_DIR: &str = "02 - Personaje y Objeto";
pub const MAPS_DIR: &str = "03 - Mapas";
pub const SEASONS_DIR: &str = "04 - Temporadas";
pub const MEDIA_DIR: &str = "05 - Medias";
pub const MEDIA_SUBDIRS: [&str; 4] = ["Mapas", "Personajes", "Lore", "Assets"];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct StoryMeta {
    pub title: String,
    pub subtitle: String,
    /// Autor de la historia (por defecto, el nombre del perfil de autor).
    pub author: String,
    pub created_unix: u64,
    pub version: u32,
}

impl Default for StoryMeta {
    fn default() -> Self {
        Self {
            title: "Sin título".to_owned(),
            subtitle: String::new(),
            author: String::new(),
            created_unix: now_unix(),
            version: 1,
        }
    }
}

/// Historia abierta en la sesión.
#[derive(Clone, Debug)]
pub struct Story {
    pub meta: StoryMeta,
    pub path: PathBuf,
}

/// Entrada ligera para listar historias.
#[derive(Clone, Debug)]
pub struct StorySummary {
    pub title: String,
    pub subtitle: String,
    pub path: PathBuf,
    pub modified: Option<SystemTime>,
}

impl Story {
    /// Crea carpeta, `story.json` y subcarpetas dentro de `root`.
    pub fn create(root: &Path, title: &str, subtitle: &str, author: &str) -> Result<Story, String> {
        let title = title.trim();
        if title.is_empty() {
            return Err(tr("El título no puede estar vacío.").to_owned());
        }
        fs::create_dir_all(root).map_err(|e| format!("No se pudo crear la carpeta raíz: {e}"))?;

        let base = sanitize_folder_name(title);
        let mut path = root.join(&base);
        let mut n = 2;
        while path.exists() {
            path = root.join(format!("{base} ({n})"));
            n += 1;
        }
        fs::create_dir(&path)
            .map_err(|e| format!("No se pudo crear la carpeta de la historia: {e}"))?;

        let meta = StoryMeta {
            title: title.to_owned(),
            subtitle: subtitle.trim().to_owned(),
            author: author.trim().to_owned(),
            ..Default::default()
        };
        write_meta(&path, &meta)?;
        ensure_structure(&path)?;
        Ok(Story { meta, path })
    }

    /// Abre una carpeta: valida su `story.json` o lo inicializa si no existe.
    pub fn open(path: &Path) -> Result<Story, String> {
        if !path.is_dir() {
            return Err(tr("La ruta seleccionada no es una carpeta.").to_owned());
        }
        let file = path.join(STORY_FILE);
        let meta = if file.is_file() {
            let text = fs::read_to_string(&file)
                .map_err(|e| format!("No se pudo leer {STORY_FILE}: {e}"))?;
            serde_json::from_str::<StoryMeta>(&text)
                .map_err(|e| format!("{STORY_FILE} no es válido: {e}"))?
        } else {
            let title = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| "Sin título".to_owned());
            let meta = StoryMeta {
                title,
                ..Default::default()
            };
            write_meta(path, &meta)?;
            meta
        };
        ensure_structure(path)?;
        Ok(Story {
            meta,
            path: path.to_path_buf(),
        })
    }
}

fn write_meta(dir: &Path, meta: &StoryMeta) -> Result<(), String> {
    let json = serde_json::to_string_pretty(meta).map_err(|e| e.to_string())?;
    fs::write(dir.join(STORY_FILE), json)
        .map_err(|e| format!("No se pudo escribir {STORY_FILE}: {e}"))
}

/// Garantiza la estructura numerada de 5 carpetas, migrando limpiamente la de la Etapa 2.1.
fn ensure_structure(dir: &Path) -> Result<(), String> {
    let seasons = dir.join(SEASONS_DIR);
    migrate_legacy(dir);

    let fresh = !seasons.exists();
    let mk = |p: PathBuf| {
        fs::create_dir_all(&p).map_err(|e| format!("No se pudo crear '{}': {e}", p.display()))
    };
    mk(dir.join(LORE_DIR))?;
    mk(dir.join(ENTITIES_DIR))?;
    mk(dir.join(MAPS_DIR))?;
    mk(seasons.clone())?;
    for sub in MEDIA_SUBDIRS {
        mk(dir.join(MEDIA_DIR).join(sub))?;
    }

    // Temporada 1 con un primer capítulo, solo cuando la carpeta de temporadas es nueva.
    if fresh {
        let s1 = seasons.join("Temporada 1");
        mk(s1.clone())?;
        let ch = s1.join("Capítulo 1.md");
        if !ch.exists() {
            fs::write(&ch, "# Capítulo 1\n\n")
                .map_err(|e| format!("No se pudo crear el capítulo de ejemplo: {e}"))?;
        }
    }
    Ok(())
}

fn is_empty_dir(p: &Path) -> bool {
    fs::read_dir(p)
        .map(|mut d| d.next().is_none())
        .unwrap_or(false)
}

/// Mueve las carpetas antiguas (`lore`, `entidades`, `mapas`, `capitulos`) a la estructura numerada.
fn migrate_legacy(dir: &Path) {
    for (old, new) in [
        ("lore", LORE_DIR),
        ("entidades", ENTITIES_DIR),
        ("mapas", MAPS_DIR),
    ] {
        let o = dir.join(old);
        if !o.is_dir() {
            continue;
        }
        let n = dir.join(new);
        if !n.exists() {
            let _ = fs::rename(&o, &n);
        } else if is_empty_dir(&o) {
            let _ = fs::remove_dir(&o);
        }
    }
    let old = dir.join("capitulos");
    if old.is_dir() {
        if is_empty_dir(&old) {
            let _ = fs::remove_dir(&old);
        } else {
            let seasons = dir.join(SEASONS_DIR);
            let target = seasons.join("Temporada 1");
            if !target.exists() && fs::create_dir_all(&seasons).is_ok() {
                let _ = fs::rename(&old, &target);
            }
        }
    }
}

/// Lista las historias (subcarpetas con `story.json` válido) de `root`, más recientes primero.
pub fn scan(root: &Path) -> Vec<StorySummary> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<StorySummary> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            let file = path.join(STORY_FILE);
            if !path.is_dir() || !file.is_file() {
                return None;
            }
            let meta: StoryMeta = serde_json::from_str(&fs::read_to_string(&file).ok()?).ok()?;
            let modified = fs::metadata(&file).and_then(|m| m.modified()).ok();
            Some(StorySummary {
                title: meta.title,
                subtitle: meta.subtitle,
                path,
                modified,
            })
        })
        .collect();
    out.sort_by(|a, b| b.modified.cmp(&a.modified));
    out
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Convierte un título en un nombre de carpeta válido en Windows.
pub fn sanitize_folder_name(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let mut name: String = cleaned
        .trim()
        .trim_end_matches('.')
        .chars()
        .take(80)
        .collect();
    name = name.trim().to_owned();
    if name.is_empty() {
        name = "Historia".to_owned();
    }
    const RESERVED: [&str; 8] = ["CON", "PRN", "AUX", "NUL", "COM1", "COM2", "LPT1", "LPT2"];
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(&name)) {
        name.push('_');
    }
    name
}

/// Fecha `dd/mm/aaaa` (UTC) sin dependencias externas.
pub fn format_date(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{d:02}/{m:02}/{y}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ff_test_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn create_has_numbered_structure() {
        let root = tmp("create");
        let s = Story::create(&root, "Mi Historia", "", "Ana").unwrap();
        assert_eq!(s.meta.author, "Ana");
        for d in [
            LORE_DIR,
            ENTITIES_DIR,
            MAPS_DIR,
            SEASONS_DIR,
            "05 - Medias/Assets",
            "05 - Medias/Personajes",
        ] {
            assert!(s.path.join(d).is_dir(), "{d}");
        }
        assert!(s
            .path
            .join("04 - Temporadas/Temporada 1/Capítulo 1.md")
            .is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn migrates_legacy_21_layout() {
        let root = tmp("legacy");
        let p = root.join("vieja");
        for d in ["capitulos", "lore", "entidades", "mapas"] {
            fs::create_dir_all(p.join(d)).unwrap();
        }
        fs::write(p.join("lore/notas.md"), "x").unwrap();
        fs::write(p.join("capitulos/uno.md"), "y").unwrap();
        fs::write(p.join(STORY_FILE), r#"{"title":"Vieja"}"#).unwrap();
        let s = Story::open(&p).unwrap();
        assert!(s.path.join(LORE_DIR).join("notas.md").is_file());
        assert!(s.path.join("04 - Temporadas/Temporada 1/uno.md").is_file());
        assert!(!p.join("lore").exists() && !p.join("capitulos").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn migrates_empty_legacy_and_adds_default_season() {
        let root = tmp("legacy_empty");
        let p = root.join("v");
        for d in ["capitulos", "lore"] {
            fs::create_dir_all(p.join(d)).unwrap();
        }
        let s = Story::open(&p).unwrap();
        assert!(!p.join("capitulos").exists());
        assert!(s
            .path
            .join("04 - Temporadas/Temporada 1/Capítulo 1.md")
            .is_file());
        let _ = fs::remove_dir_all(root);
    }
}
