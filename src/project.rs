// src/project.rs
//! Operaciones sobre `04 - Temporadas/`: CRUD de temporadas (carpetas) y capítulos (`.md`).

use crate::i18n::tr;
use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::story::{sanitize_folder_name, ENTITIES_DIR, LORE_DIR, MEDIA_DIR, SEASONS_DIR};

#[derive(Clone, Debug)]
pub struct Chapter {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct Season {
    pub name: String,
    pub path: PathBuf,
    pub chapters: Vec<Chapter>,
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn is_md(p: &Path) -> bool {
    p.extension()
        .map(|e| e.eq_ignore_ascii_case("md"))
        .unwrap_or(false)
}

/// Lista temporadas (orden natural) con sus capítulos.
pub fn list_seasons(story: &Path) -> Vec<Season> {
    let Ok(rd) = fs::read_dir(story.join(SEASONS_DIR)) else {
        return Vec::new();
    };
    let mut out: Vec<Season> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .map(|path| Season {
            name: file_name(&path),
            chapters: list_chapters(&path),
            path,
        })
        .collect();
    out.sort_by(|a, b| natural_cmp(&a.name, &b.name));
    out
}

fn list_chapters(dir: &Path) -> Vec<Chapter> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Chapter> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_md(p))
        .map(|path| Chapter {
            name: path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path,
        })
        .collect();
    out.sort_by(|a, b| natural_cmp(&a.name, &b.name));
    out
}

fn clean_name(input: &str, strip_md: bool) -> Result<String, String> {
    let mut s = input.trim();
    if strip_md && s.len() >= 3 && s[s.len() - 3..].eq_ignore_ascii_case(".md") {
        s = s[..s.len() - 3].trim_end();
    }
    if s.is_empty() {
        return Err(tr("El nombre no puede estar vacío.").to_owned());
    }
    Ok(sanitize_folder_name(s))
}

pub fn create_season(story: &Path, name: &str) -> Result<PathBuf, String> {
    let name = clean_name(name, false)?;
    let path = story.join(SEASONS_DIR).join(name);
    if path.exists() {
        return Err(tr("Ya existe una temporada con ese nombre.").to_owned());
    }
    fs::create_dir_all(&path).map_err(|e| format!("No se pudo crear la temporada: {e}"))?;
    Ok(path)
}

pub fn create_chapter(season_dir: &Path, name: &str) -> Result<PathBuf, String> {
    let name = clean_name(name, true)?;
    let path = season_dir.join(format!("{name}.md"));
    if path.exists() {
        return Err(tr("Ya existe un capítulo con ese nombre.").to_owned());
    }
    fs::write(&path, format!("# {name}\n\n"))
        .map_err(|e| format!("No se pudo crear el capítulo: {e}"))?;
    Ok(path)
}

/// Renombra una temporada (carpeta) o un capítulo (archivo `.md`). Devuelve la nueva ruta.
pub fn rename(path: &Path, new_name: &str) -> Result<PathBuf, String> {
    let is_dir = path.is_dir();
    let name = clean_name(new_name, !is_dir)?;
    let target = path.with_file_name(if is_dir { name } else { format!("{name}.md") });
    let same_name = file_name(path).to_lowercase() == file_name(&target).to_lowercase();
    if target != path && target.exists() && !same_name {
        return Err(tr("Ya existe un elemento con ese nombre.").to_owned());
    }
    fs::rename(path, &target).map_err(|e| format!("No se pudo renombrar: {e}"))?;
    Ok(target)
}

pub fn delete(path: &Path) -> Result<(), String> {
    let r = if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    r.map_err(|e| format!("No se pudo eliminar: {e}"))
}

pub fn duplicate_chapter(path: &Path) -> Result<PathBuf, String> {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut n = 1;
    let target = loop {
        let name = if n == 1 {
            format!("{stem} (copia).md")
        } else {
            format!("{stem} (copia {n}).md")
        };
        let t = path.with_file_name(name);
        if !t.exists() {
            break t;
        }
        n += 1;
    };
    fs::copy(path, &target).map_err(|e| format!("No se pudo duplicar: {e}"))?;
    Ok(target)
}

pub fn read_chapter(path: &Path) -> Result<String, String> {
    fs::read_to_string(path)
        .map(|s| s.replace("\r\n", "\n"))
        .map_err(|e| format!("No se pudo leer el capítulo: {e}"))
}

/// Escritura atómica: archivo temporal + rename.
pub fn write_chapter(path: &Path, text: &str) -> Result<(), String> {
    let tmp = path.with_extension("md.tmp");
    fs::write(&tmp, text).map_err(|e| format!("No se pudo guardar: {e}"))?;
    fs::rename(&tmp, path).map_err(|e| format!("No se pudo guardar: {e}"))
}

pub fn count_words(text: &str) -> usize {
    text.split_whitespace().count()
}

/// Orden natural: "Capítulo 2" < "Capítulo 10", sin distinguir mayúsculas.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let na = take_num(&mut ai);
                let nb = take_num(&mut bi);
                match na.cmp(&nb) {
                    Ordering::Equal => {}
                    o => return o,
                }
            }
            (Some(x), Some(y)) => {
                ai.next();
                bi.next();
                let lx = x.to_lowercase().next().unwrap_or(x);
                let ly = y.to_lowercase().next().unwrap_or(y);
                match lx.cmp(&ly) {
                    Ordering::Equal => {}
                    o => return o,
                }
            }
        }
    }
}

fn take_num(it: &mut std::iter::Peekable<std::str::Chars<'_>>) -> u64 {
    let mut n: u64 = 0;
    while let Some(c) = it.peek().copied() {
        let Some(d) = c.to_digit(10) else { break };
        n = n.saturating_mul(10).saturating_add(d as u64);
        it.next();
    }
    n
}

// ===========================================================================
// Medios (imágenes en `05 - Medias/`)
// ===========================================================================

pub const MEDIA_LORE: &str = "Lore";
pub const MEDIA_MAPS: &str = "Mapas";
pub const MEDIA_CHARACTERS: &str = "Personajes";

pub fn media_dir(story: &Path, sub: &str) -> PathBuf {
    story.join(MEDIA_DIR).join(sub)
}

/// Ruta completa de una imagen de `05 - Medias/<sub>/`.
pub fn media_file(story: &Path, sub: &str, file: &str) -> PathBuf {
    media_dir(story, sub).join(file)
}

/// Copia una imagen elegida por el usuario a `05 - Medias/<sub>/` (PNG).
/// `square = true` recorta a cuadrado 512×512 (retratos); si no, limita a 1200 px.
/// Devuelve el nombre de archivo guardado.
pub fn import_image(story: &Path, sub: &str, src: &Path, square: bool) -> Result<String, String> {
    let img = image::open(src).map_err(|e| format!("No se pudo leer la imagen: {e}"))?;
    let img = if square {
        img.resize_to_fill(512, 512, image::imageops::FilterType::Lanczos3)
    } else if img.width() > 1200 || img.height() > 1200 {
        img.resize(1200, 1200, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let dir = media_dir(story, sub);
    fs::create_dir_all(&dir).map_err(|e| format!("No se pudo crear la carpeta de medios: {e}"))?;
    let stem: String = src
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .map(|s| sanitize_folder_name(&s))
        .unwrap_or_else(|| "imagen".to_owned())
        .chars()
        .take(40)
        .collect();
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let name = format!("{millis}_{stem}.png");
    img.save_with_format(dir.join(&name), image::ImageFormat::Png)
        .map_err(|e| format!("No se pudo guardar la imagen: {e}"))?;
    Ok(name)
}

pub fn remove_media(story: &Path, sub: &str, file: &str) {
    let _ = fs::remove_file(media_dir(story, sub).join(file));
}

// ===========================================================================
// Lore y cronología (`01 - Lore y Cronologia/`)
// ===========================================================================

pub const TIMELINE_FILE: &str = "timeline.json";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LoreEvent {
    /// Nombre del archivo `.md` con el cuerpo.
    pub file: String,
    pub title: String,
    pub year: i64,
    pub tag: String,
    /// Imagen en `05 - Medias/Lore/`.
    pub image: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct TimelineFile {
    events: Vec<LoreEvent>,
}

pub fn lore_dir(story: &Path) -> PathBuf {
    story.join(LORE_DIR)
}

fn save_timeline(story: &Path, events: &[LoreEvent]) -> Result<(), String> {
    let dir = lore_dir(story);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(&TimelineFile {
        events: events.to_vec(),
    })
    .map_err(|e| e.to_string())?;
    let tmp = dir.join("timeline.json.tmp");
    fs::write(&tmp, json).map_err(|e| format!("No se pudo guardar la cronología: {e}"))?;
    fs::rename(&tmp, dir.join(TIMELINE_FILE))
        .map_err(|e| format!("No se pudo guardar la cronología: {e}"))
}

/// Carga la cronología. Descarta entradas sin archivo y adopta `.md` sueltos (año 0).
pub fn load_timeline(story: &Path) -> Vec<LoreEvent> {
    let dir = lore_dir(story);
    let mut events: Vec<LoreEvent> = fs::read_to_string(dir.join(TIMELINE_FILE))
        .ok()
        .and_then(|s| serde_json::from_str::<TimelineFile>(&s).ok())
        .map(|t| t.events)
        .unwrap_or_default();

    let before = events.len();
    events.retain(|e| dir.join(&e.file).is_file());
    let mut changed = events.len() != before;

    if let Ok(rd) = fs::read_dir(&dir) {
        for p in rd.filter_map(|e| e.ok()).map(|e| e.path()) {
            if p.is_file() && is_md(&p) {
                let file = file_name(&p);
                if !events.iter().any(|e| e.file == file) {
                    let title = p
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    events.push(LoreEvent {
                        file,
                        title,
                        ..Default::default()
                    });
                    changed = true;
                }
            }
        }
    }
    if changed {
        let _ = save_timeline(story, &events);
    }
    events
}

pub fn parse_year(s: &str) -> Result<i64, String> {
    s.trim()
        .trim_start_matches('+')
        .parse::<i64>()
        .map_err(|_| "El año debe ser un número entero (ej. 2021 o -340).".to_owned())
}

pub fn create_event(
    story: &Path,
    title: &str,
    year: i64,
    tag: &str,
    image_src: Option<&Path>,
) -> Result<LoreEvent, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err(tr("El título no puede estar vacío.").to_owned());
    }
    let dir = lore_dir(story);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let base = sanitize_folder_name(title);
    let mut file = format!("{base}.md");
    let mut n = 2;
    while dir.join(&file).exists() {
        file = format!("{base} ({n}).md");
        n += 1;
    }
    let image = match image_src {
        Some(src) => Some(import_image(story, MEDIA_LORE, src, false)?),
        None => None,
    };
    fs::write(dir.join(&file), "").map_err(|e| format!("No se pudo crear el evento: {e}"))?;
    let ev = LoreEvent {
        file,
        title: title.to_owned(),
        year,
        tag: tag.trim().to_owned(),
        image,
    };
    let mut events = load_timeline(story);
    events.retain(|e| e.file != ev.file);
    events.push(ev.clone());
    save_timeline(story, &events)?;
    Ok(ev)
}

pub fn update_event(
    story: &Path,
    file: &str,
    title: &str,
    year: i64,
    tag: &str,
    new_image: Option<&Path>,
    remove_image: bool,
) -> Result<LoreEvent, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err(tr("El título no puede estar vacío.").to_owned());
    }
    let mut events = load_timeline(story);
    let idx = events
        .iter()
        .position(|e| e.file == file)
        .ok_or("El evento ya no existe.")?;
    let new_name = match new_image {
        Some(src) => Some(import_image(story, MEDIA_LORE, src, false)?),
        None => None,
    };
    let ev = &mut events[idx];
    if new_name.is_some() || remove_image {
        if let Some(old) = ev.image.take() {
            remove_media(story, MEDIA_LORE, &old);
        }
        ev.image = new_name;
    }
    ev.title = title.to_owned();
    ev.year = year;
    ev.tag = tag.trim().to_owned();
    let out = ev.clone();
    save_timeline(story, &events)?;
    Ok(out)
}

pub fn delete_event(story: &Path, file: &str) -> Result<(), String> {
    let mut events = load_timeline(story);
    if let Some(ev) = events.iter().find(|e| e.file == file) {
        if let Some(img) = &ev.image {
            remove_media(story, MEDIA_LORE, img);
        }
    }
    events.retain(|e| e.file != file);
    let _ = fs::remove_file(lore_dir(story).join(file));
    save_timeline(story, &events)
}

// ===========================================================================
// Entidades (`02 - Personaje y Objeto/`)
// ===========================================================================

/// Categoría de fichas: una carpeta dentro de `02 - Personaje y Objeto/`. Las tres de fábrica
/// (`Personaje`, `Objetos`, `Varios`) siempre existen; el resto las crea el usuario por historia y
/// se guardan en `categorias.json`.
#[derive(Clone, PartialEq, Eq, Debug, Hash, Serialize, Deserialize)]
pub struct Category {
    /// Nombre de la carpeta (único, en minúsculas).
    pub key: String,
    pub label: String,
    #[serde(skip)]
    pub builtin: bool,
}

pub const KEY_PERSONAJES: &str = "personajes";
pub const KEY_OBJETOS: &str = "objetos";
pub const KEY_VARIOS: &str = "varios";
const CATEGORIES_FILE: &str = "categorias.json";

impl Category {
    pub fn builtins() -> Vec<Category> {
        [
            (KEY_PERSONAJES, "Personaje"),
            (KEY_OBJETOS, "Objetos"),
            (KEY_VARIOS, "Varios"),
        ]
        .into_iter()
        .map(|(k, l)| Category {
            key: k.to_owned(),
            label: l.to_owned(),
            builtin: true,
        })
        .collect()
    }

    pub fn is_character(&self) -> bool {
        self.key == KEY_PERSONAJES
    }

    pub fn new_title(&self) -> &'static str {
        match self.key.as_str() {
            KEY_PERSONAJES => "Nuevo personaje",
            KEY_OBJETOS => "Nuevo objeto",
            _ => "Nueva ficha",
        }
    }

    /// Título de la sección de texto largo.
    pub fn body_title(&self) -> &'static str {
        if self.is_character() {
            "Historia / Biografía"
        } else {
            "Descripción / Desarrollo"
        }
    }
}

fn categories_file(story: &Path) -> PathBuf {
    story.join(ENTITIES_DIR).join(CATEGORIES_FILE)
}

/// Categorías de la historia: las de fábrica y luego las propias.
pub fn load_categories(story: &Path) -> Vec<Category> {
    let mut out = Category::builtins();
    let custom: Vec<Category> = fs::read_to_string(categories_file(story))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    for c in custom {
        if !c.key.is_empty() && !out.iter().any(|o| o.key == c.key) {
            out.push(c);
        }
    }
    out
}

fn save_custom_categories(story: &Path, list: &[Category]) -> Result<(), String> {
    let custom: Vec<&Category> = list.iter().filter(|c| !c.builtin).collect();
    fs::create_dir_all(story.join(ENTITIES_DIR)).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(&custom).map_err(|e| e.to_string())?;
    fs::write(categories_file(story), json)
        .map_err(|e| format!("No se pudieron guardar las categorías: {e}"))
}

/// Crea una categoría propia. Falla si el nombre está vacío o ya existe (sin distinguir mayúsculas).
pub fn add_category(story: &Path, label: &str) -> Result<Category, String> {
    let label = label.trim();
    if label.is_empty() {
        return Err(tr("El nombre de la categoría no puede estar vacío.").to_owned());
    }
    let key = sanitize_folder_name(label).to_lowercase();
    let mut all = load_categories(story);
    if all
        .iter()
        .any(|c| c.key == key || c.label.to_lowercase() == label.to_lowercase())
    {
        return Err(tr("Ya existe una categoría con ese nombre.").to_owned());
    }
    let cat = Category {
        key,
        label: label.to_owned(),
        builtin: false,
    };
    all.push(cat.clone());
    save_custom_categories(story, &all)?;
    fs::create_dir_all(entities_dir(story, &cat.key)).map_err(|e| e.to_string())?;
    Ok(cat)
}

/// Quita una categoría propia; debe estar vacía.
pub fn remove_category(story: &Path, key: &str) -> Result<(), String> {
    let mut all = load_categories(story);
    let Some(pos) = all.iter().position(|c| c.key == key && !c.builtin) else {
        return Err(tr("Solo se pueden quitar categorías propias.").to_owned());
    };
    if !list_entities(story, key).is_empty() {
        return Err(tr("La categoría tiene fichas: elimínalas o muévelas primero.").to_owned());
    }
    all.remove(pos);
    save_custom_categories(story, &all)?;
    let _ = fs::remove_dir(entities_dir(story, key)); // solo si quedó vacía
    Ok(())
}

/// Propiedad clave-valor de una ficha (`Propiedad: Valor`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Prop {
    pub key: String,
    pub value: String,
}

/// Bloque de rasgo libre: título editable y texto.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Trait {
    pub title: String,
    pub text: String,
}

/// Ficha de entidad. `age` es el único campo base fijo (personajes); el resto de datos son
/// propiedades y rasgos dinámicos. Los campos «legacy» se leen de archivos anteriores y se
/// migran a esas listas al cargar.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Entity {
    pub name: String,
    pub image: Option<String>,
    pub age: String,
    /// Propiedades clave-valor (agregar, renombrar, quitar).
    pub props: Vec<Prop>,
    /// Rasgos: bloques de texto con título propio.
    pub traits: Vec<Trait>,
    /// Texto largo principal (biografía / descripción).
    pub bio: String,
    // --- legacy (formato anterior) ---
    pub faction: String,
    pub rank: String,
    pub status: String,
    pub strengths: String,
    pub weaknesses: String,
    pub goals: String,
    pub conflicts: String,
    pub bearer: String,
    pub properties: String,
    pub origin: String,
    pub category: String,
    pub description: String,
}

impl Entity {
    /// Ficha nueva con las propiedades y rasgos habituales de la categoría.
    pub fn seeded(name: &str, cat_key: &str) -> Self {
        let prop = |k: &str| Prop {
            key: k.to_owned(),
            value: String::new(),
        };
        let tr = |t: &str| Trait {
            title: t.to_owned(),
            text: String::new(),
        };
        let (props, traits) = match cat_key {
            KEY_PERSONAJES => (
                vec![prop("Facción"), prop("Rango"), prop("Estado")],
                vec![
                    tr("Fortalezas / Habilidades"),
                    tr("Debilidades / Miedos"),
                    tr("Objetivos / Motivaciones"),
                    tr("Conflictos / Vínculos"),
                ],
            ),
            KEY_OBJETOS => (
                vec![prop("Categoría / Tipo"), prop("Portador actual")],
                vec![tr("Propiedades / Funcionamiento")],
            ),
            _ => (vec![prop("Categoría")], Vec::new()),
        };
        Self {
            name: name.to_owned(),
            props,
            traits,
            ..Default::default()
        }
    }

    /// Migra los campos del formato anterior a propiedades y rasgos (idempotente).
    pub fn migrate_legacy(&mut self) {
        fn take(s: &mut String) -> Option<String> {
            let v = std::mem::take(s);
            (!v.trim().is_empty()).then_some(v)
        }
        let mut props: Vec<(&str, Option<String>)> = vec![
            ("Facción", take(&mut self.faction)),
            ("Rango", take(&mut self.rank)),
            ("Estado", take(&mut self.status)),
            ("Categoría / Tipo", take(&mut self.category)),
            ("Portador actual", take(&mut self.bearer)),
        ];
        for (k, v) in props.drain(..) {
            if let Some(v) = v {
                self.props.push(Prop {
                    key: k.to_owned(),
                    value: v,
                });
            }
        }
        let traits = [
            ("Fortalezas / Habilidades", take(&mut self.strengths)),
            ("Debilidades / Miedos", take(&mut self.weaknesses)),
            ("Objetivos / Motivaciones", take(&mut self.goals)),
            ("Conflictos / Vínculos", take(&mut self.conflicts)),
            ("Propiedades / Funcionamiento", take(&mut self.properties)),
        ];
        for (t, v) in traits {
            if let Some(v) = v {
                self.traits.push(Trait {
                    title: t.to_owned(),
                    text: v,
                });
            }
        }
        // Texto largo: la biografía ya vive en `bio`; origen y descripción se unen a ella.
        for extra in [take(&mut self.origin), take(&mut self.description)]
            .into_iter()
            .flatten()
        {
            if !self.bio.trim().is_empty() {
                self.bio.push_str("\n\n");
            }
            self.bio.push_str(&extra);
        }
    }
}

#[derive(Clone, Debug)]
pub struct EntitySummary {
    pub id: String,
    pub name: String,
    pub role: String,
    pub image: Option<String>,
}

impl EntitySummary {
    pub fn from_entity(id: &str, e: &Entity) -> Self {
        // Subtítulo de la tarjeta: Rango, Facción o Categoría, lo primero que tenga valor.
        let pick = |names: &[&str]| {
            names.iter().find_map(|n| {
                e.props
                    .iter()
                    .find(|p| {
                        p.key.trim().to_lowercase().starts_with(n) && !p.value.trim().is_empty()
                    })
                    .map(|p| p.value.clone())
            })
        };
        let role = pick(&["rango", "facci", "categor"])
            .or_else(|| {
                e.props
                    .iter()
                    .find(|p| !p.value.trim().is_empty())
                    .map(|p| p.value.clone())
            })
            .unwrap_or_default();
        Self {
            id: id.to_owned(),
            name: e.name.clone(),
            role,
            image: e.image.clone(),
        }
    }
}

pub fn entities_dir(story: &Path, key: &str) -> PathBuf {
    story.join(ENTITIES_DIR).join(key)
}

fn entity_path(story: &Path, key: &str, id: &str) -> PathBuf {
    entities_dir(story, key).join(format!("{id}.json"))
}

fn read_entity(p: &Path) -> Option<Entity> {
    let mut e: Entity = serde_json::from_str(&fs::read_to_string(p).ok()?).ok()?;
    e.migrate_legacy();
    Some(e)
}

pub fn list_entities(story: &Path, key: &str) -> Vec<EntitySummary> {
    let Ok(rd) = fs::read_dir(entities_dir(story, key)) else {
        return Vec::new();
    };
    let mut out: Vec<EntitySummary> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("json"))
        })
        .filter_map(|p| {
            let id = p.file_stem()?.to_string_lossy().into_owned();
            Some(EntitySummary::from_entity(&id, &read_entity(&p)?))
        })
        .collect();
    out.sort_by(|a, b| natural_cmp(&a.name, &b.name));
    out
}

pub fn load_entity(story: &Path, key: &str, id: &str) -> Result<Entity, String> {
    let text = fs::read_to_string(entity_path(story, key, id))
        .map_err(|e| format!("No se pudo leer la ficha: {e}"))?;
    let mut e: Entity =
        serde_json::from_str(&text).map_err(|e| format!("La ficha no es válida: {e}"))?;
    e.migrate_legacy();
    Ok(e)
}

pub fn save_entity(story: &Path, key: &str, id: &str, ent: &Entity) -> Result<(), String> {
    let path = entity_path(story, key, id);
    fs::create_dir_all(entities_dir(story, key)).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(ent).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json).map_err(|e| format!("No se pudo guardar la ficha: {e}"))?;
    fs::rename(&tmp, &path).map_err(|e| format!("No se pudo guardar la ficha: {e}"))
}

/// Crea una ficha con ese nombre. Devuelve su id (nombre de archivo sin extensión).
pub fn create_entity(story: &Path, key: &str, name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(tr("El nombre no puede estar vacío.").to_owned());
    }
    let base = sanitize_folder_name(name);
    let mut id = base.clone();
    let mut n = 2;
    while entity_path(story, key, &id).exists() {
        id = format!("{base} ({n})");
        n += 1;
    }
    save_entity(story, key, &id, &Entity::seeded(name, key))?;
    Ok(id)
}

pub fn delete_entity(story: &Path, key: &str, id: &str) -> Result<(), String> {
    if let Ok(ent) = load_entity(story, key, id) {
        if let Some(img) = ent.image {
            remove_media(story, MEDIA_CHARACTERS, &img);
        }
    }
    fs::remove_file(entity_path(story, key, id))
        .map_err(|e| format!("No se pudo eliminar la ficha: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ff_proj_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn timeline_crud_and_adoption() {
        let root = tmp("timeline");
        let a = create_event(&root, "La caída", -340, "guerra", None).unwrap();
        let b = create_event(&root, "La caída", 2021, "", None).unwrap();
        assert_ne!(a.file, b.file);
        fs::write(lore_dir(&root).join("suelto.md"), "x").unwrap();
        let evs = load_timeline(&root);
        assert_eq!(evs.len(), 3);
        assert!(evs.iter().any(|e| e.file == "suelto.md" && e.year == 0));
        let u = update_event(&root, &a.file, "Otra", 5, "t", None, false).unwrap();
        assert_eq!((u.title.as_str(), u.year), ("Otra", 5));
        delete_event(&root, &a.file).unwrap();
        assert_eq!(load_timeline(&root).len(), 2);
        assert!(parse_year("-340").unwrap() == -340 && parse_year("abc").is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn entity_crud() {
        let root = tmp("ent");
        let id = create_entity(&root, KEY_PERSONAJES, "Antonio").unwrap();
        let id2 = create_entity(&root, KEY_PERSONAJES, "Antonio").unwrap();
        assert_ne!(id, id2);
        let mut e = load_entity(&root, KEY_PERSONAJES, &id).unwrap();
        // Una ficha nueva de personaje trae propiedades y rasgos editables.
        assert_eq!(e.props.len(), 3);
        assert_eq!(e.traits.len(), 4);
        e.props[1].value = "Capitán".into();
        e.props.push(Prop {
            key: "Altura".into(),
            value: "1,80 m".into(),
        });
        e.traits.remove(0);
        save_entity(&root, KEY_PERSONAJES, &id, &e).unwrap();
        let again = load_entity(&root, KEY_PERSONAJES, &id).unwrap();
        assert_eq!((again.props.len(), again.traits.len()), (4, 3));
        let list = list_entities(&root, KEY_PERSONAJES);
        assert_eq!(list.len(), 2);
        assert!(list.iter().any(|s| s.role == "Capitán"));
        assert!(list_entities(&root, KEY_OBJETOS).is_empty());
        delete_entity(&root, KEY_PERSONAJES, &id).unwrap();
        assert_eq!(list_entities(&root, KEY_PERSONAJES).len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn custom_categories_persist_per_story() {
        let root = tmp("cats");
        assert_eq!(load_categories(&root).len(), 3);
        let c = add_category(&root, "Criaturas").unwrap();
        assert!(!c.builtin && c.key == "criaturas");
        assert!(add_category(&root, "criaturas").is_err());
        assert!(add_category(&root, "Objetos").is_err());
        assert!(add_category(&root, "  ").is_err());
        let all = load_categories(&root);
        assert_eq!(all.len(), 4);
        assert_eq!(all[3].label, "Criaturas");
        // Fichas dentro de la categoría propia.
        let id = create_entity(&root, &c.key, "Grifo").unwrap();
        assert_eq!(list_entities(&root, &c.key).len(), 1);
        assert!(remove_category(&root, &c.key).is_err()); // no está vacía
        delete_entity(&root, &c.key, &id).unwrap();
        remove_category(&root, &c.key).unwrap();
        assert_eq!(load_categories(&root).len(), 3);
        assert!(remove_category(&root, KEY_VARIOS).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_sheets_migrate_to_dynamic_fields() {
        let root = tmp("legacy");
        let dir = entities_dir(&root, KEY_PERSONAJES);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("Vieja.json"),
            r#"{"name":"Vieja","age":"40","faction":"Gremio","rank":"Maestra","strengths":"Fuerte","bio":"Nació"}"#,
        )
        .unwrap();
        let e = load_entity(&root, KEY_PERSONAJES, "Vieja").unwrap();
        assert_eq!(e.age, "40");
        assert!(e
            .props
            .iter()
            .any(|p| p.key == "Rango" && p.value == "Maestra"));
        assert!(e
            .traits
            .iter()
            .any(|t| t.title.starts_with("Fortalezas") && t.text == "Fuerte"));
        assert_eq!(e.bio, "Nació");
        assert!(e.faction.is_empty() && e.strengths.is_empty());
        assert_eq!(list_entities(&root, KEY_PERSONAJES)[0].role, "Maestra");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn natural_order() {
        let mut v = vec!["Capítulo 10", "capítulo 2", "Capítulo 1"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["Capítulo 1", "capítulo 2", "Capítulo 10"]);
    }
}
