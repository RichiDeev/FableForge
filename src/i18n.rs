// src/i18n.rs
//! Internacionalización ligera (Español / Inglés).
//!
//! Las claves son los textos en español de la interfaz: `tr("Inicio")` devuelve «Home» cuando el
//! idioma activo es inglés y el mismo texto si no hay traducción. El idioma activo es global
//! (se fija al cargar los ajustes y al cambiarlo en Ajustes generales).

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    Es,
    En,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::Es, Language::En];

    /// Nombre del idioma en su propio idioma.
    pub fn native_name(self) -> &'static str {
        match self {
            Language::Es => "Español",
            Language::En => "English",
        }
    }

    /// Idioma del sistema (`en*` → inglés); español por defecto.
    pub fn detect() -> Language {
        match sys_locale::get_locale() {
            Some(l) if l.to_lowercase().starts_with("en") => Language::En,
            _ => Language::Es,
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn set_language(lang: Language) {
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

pub fn language() -> Language {
    if CURRENT.load(Ordering::Relaxed) == 1 {
        Language::En
    } else {
        Language::Es
    }
}

fn table() -> &'static HashMap<&'static str, &'static str> {
    static T: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    T.get_or_init(|| EN.iter().copied().collect())
}

/// Traduce `key` (texto en español) al idioma indicado.
pub fn t(lang: Language, key: &'static str) -> &'static str {
    match lang {
        Language::Es => key,
        Language::En => table().get(key).copied().unwrap_or(key),
    }
}

/// Traduce `key` al idioma activo.
pub fn tr(key: &'static str) -> &'static str {
    t(language(), key)
}

/// Como `tr`, para textos que no son literales (p. ej. etiquetas guardadas en disco).
pub fn tr_dyn(key: &str) -> String {
    match language() {
        Language::Es => key.to_owned(),
        Language::En => table()
            .get(key)
            .map_or_else(|| key.to_owned(), |v| (*v).to_owned()),
    }
}

/// Traduce una plantilla con `{}` y los sustituye por `args` en orden.
pub fn trf(key: &'static str, args: &[&dyn Display]) -> String {
    let mut out = String::new();
    let mut rest = tr(key);
    for a in args {
        match rest.find("{}") {
            Some(i) => {
                out.push_str(&rest[..i]);
                out.push_str(&a.to_string());
                rest = &rest[i + 2..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_and_falls_back() {
        assert_eq!(t(Language::Es, "Inicio"), "Inicio");
        assert_eq!(t(Language::En, "Inicio"), "Home");
        assert_eq!(t(Language::En, "texto sin traducir"), "texto sin traducir");
    }

    #[test]
    fn templates_fill_in_order() {
        set_language(Language::En);
        assert_eq!(
            trf("{} · Capa: {}", &[&"Mapa", &"Terreno"]),
            "Mapa · Layer: Terreno"
        );
        set_language(Language::Es);
        assert_eq!(trf("Año {}", &[&-340]), "Año -340");
    }
}

/// Tabla español → inglés.
static EN: &[(&str, &str)] = &[
    ("Se eliminará la temporada «{}» con {} capítulo(s). Esta acción no se puede deshacer.", "The season “{}” with {} chapter(s) will be deleted. This cannot be undone."),
    ("Se eliminará el capítulo «{}». Esta acción no se puede deshacer.", "The chapter “{}” will be deleted. This cannot be undone."),
    ("Temporada {}", "Season {}"),
    ("Capítulo {}", "Chapter {}"),
    ("Se eliminará «{}» junto con su texto e imagen. Esta acción no se puede deshacer.", "“{}” will be deleted along with its text and image. This cannot be undone."),
    ("Se quitará la categoría «{}» (debe estar vacía).", "The category “{}” will be removed (it must be empty)."),
    ("Se eliminará la ficha «{}» y su imagen. Esta acción no se puede deshacer.", "The entry “{}” and its image will be deleted. This cannot be undone."),
    ("Se eliminará «{}». Esta acción no se puede deshacer.", "“{}” will be deleted. This cannot be undone."),
    ("Se importaron {} asset(s).", "{} asset(s) imported."),
    ("Mapa exportado: {}", "Map exported: {}"),
    ("No se encontró el mapa vinculado «{}».", "Linked map “{}” was not found."),
    ("{} assets (mezcla aleatoria al dispersar)", "{} assets (random mix when scattering)"),
    ("Pincel: {} · clic estampa · Ctrl/Mayús+clic elige varios", "Brush: {} · click stamps · Ctrl/Shift+click picks several"),
    ("Paleta del estilo {}", "{} style palette"),
    ("{} (no encontrado)", "{} (not found)"),
    ("Cerrar historia y volver a Inicio", "Close story and go back to Home"),
    ("Inicio", "Home"),
    ("Ajustes generales", "General settings"),
    ("Cancelar", "Cancel"),
    ("Cerrar", "Close"),
    ("Crear historia", "Create story"),
    ("Título de la historia", "Story title"),
    ("Subtítulo / Género (opcional)", "Subtitle / Genre (optional)"),
    ("Cargar historia", "Load story"),
    ("No se encontraron historias en esta carpeta.", "No stories found in this folder."),
    ("Cambiar foto", "Change photo"),
    ("Cambiar carpeta", "Change folder"),
    ("Nombre", "Name"),
    ("Temas rápidos", "Quick themes"),
    ("Colores personalizados", "Custom colors"),
    ("· activos", "· active"),
    ("Carpeta donde se guardan tus historias", "Folder where your stories are saved"),
    ("Guardar y cerrar", "Save and close"),
    ("Perfil", "Profile"),
    ("Apariencia", "Appearance"),
    ("Almacenamiento", "Storage"),
    ("Restablecer al tema", "Reset to theme"),
    ("Renombrar", "Rename"),
    ("Eliminar", "Delete"),
    ("Duplicar", "Duplicate"),
    ("Vista previa", "Preview"),
    ("Editar", "Edit"),
    ("Temporadas", "Seasons"),
    ("Aún no hay temporadas. Crea una con el botón superior.", "No seasons yet. Create one with the button above."),
    ("Sin capítulos", "No chapters"),
    ("Selecciona o crea un capítulo", "Select or create a chapter"),
    ("Error al guardar", "Save error"),
    ("Guardando…", "Saving…"),
    ("Guardado", "Saved"),
    ("Tamaño de texto", "Text size"),
    ("Editar datos", "Edit details"),
    ("Elegir imagen…", "Choose image…"),
    ("Quitar", "Remove"),
    ("+ Nuevo evento de Lore", "+ New Lore event"),
    ("Cronología", "Timeline"),
    ("Aún no hay eventos. Crea uno con «+ Nuevo evento de Lore».", "No events yet. Create one with “+ New Lore event”."),
    ("Título", "Title"),
    ("Año (ej. 2021 o -340)", "Year (e.g. 2021 or -340)"),
    ("Etiqueta (opcional)", "Label (optional)"),
    ("Imagen (opcional)", "Image (optional)"),
    ("Escribe aquí el lore de este evento…", "Write this event's lore here…"),
    ("Eliminar ficha", "Delete entry"),
    ("+ Propiedad", "+ Property"),
    ("+ Rasgo", "+ Trait"),
    ("+ Nueva categoría", "+ New category"),
    ("+ Nueva ficha", "+ New entry"),
    ("No hay fichas en esta pestaña.", "There are no entries in this tab."),
    ("Crea una categoría propia para esta historia", "Create a custom category for this story"),
    ("Quita esta categoría (debe estar vacía)", "Remove this category (it must be empty)"),
    ("Quitar propiedad", "Remove property"),
    ("Quitar este rasgo", "Remove this trait"),
    ("Propiedad", "Property"),
    ("Valor", "Value"),
    ("Título del rasgo", "Trait title"),
    ("Ajustar", "Fit"),
    ("Exportar PNG", "Export PNG"),
    ("Entrar al mapa interior", "Enter inner map"),
    ("Crear sub-mapa interior", "Create inner sub-map"),
    ("Rotar +45°", "Rotate +45°"),
    ("Borrar", "Clear"),
    ("Girar 15° (R)", "Rotate 15° (R)"),
    ("Traer al frente (RePág)", "Bring to front (PgUp)"),
    ("Enviar atrás (AvPág)", "Send back (PgDn)"),
    ("Eliminar trazo (Supr)", "Delete stroke (Del)"),
    ("Eliminar (Supr)", "Delete (Del)"),
    ("Centrar", "Center"),
    ("Eliminar región (Supr)", "Delete region (Del)"),
    ("Eliminar zona (Supr)", "Delete zone (Del)"),
    ("+ Nueva capa", "+ New layer"),
    ("+ Importar PNG", "+ Import PNG"),
    ("+ Importar carpeta de assets", "+ Import assets folder"),
    ("+ Nueva zona (Z)", "+ New zone (Z)"),
    ("Pincel (B)", "Brush (B)"),
    ("Borrador (E)", "Eraser (E)"),
    ("Relleno (G)", "Fill (G)"),
    ("Mapas", "Maps"),
    ("+ Crear Nuevo Mapa", "+ Create New Map"),
    ("Todavía no hay mapas en esta historia.", "There are no maps in this story yet."),
    ("Crear nuevo mapa", "Create new map"),
    ("Nombre del mapa", "Map name"),
    ("Estilo visual", "Visual style"),
    ("Dimensiones", "Dimensions"),
    ("Modo de creación", "Creation mode"),
    ("Crear mapa", "Create map"),
    ("Eliminar mapa", "Delete map"),
    ("Mapa interior vinculado", "Linked inner map"),
    ("Estilo", "Style"),
    ("Cuadrícula", "Grid"),
    ("Dispersión", "Scatter"),
    ("Región vectorial", "Vector region"),
    ("Nueva muralla", "New wall"),
    ("Nuevo camino / río", "New path / river"),
    ("Modo", "Mode"),
    ("Objeto seleccionado", "Selected object"),
    ("Trazo seleccionado", "Selected stroke"),
    ("Arrastra uno para mover el grupo · Ctrl + clic suma o quita", "Drag one to move the group · Ctrl + click adds or removes"),
    ("Región seleccionada", "Selected region"),
    ("Zona seleccionada", "Selected zone"),
    ("Doble clic: renombrar", "Double-click: rename"),
    ("Todavía no hay zonas. Dibuja reinos, distritos o zonas de peligro.", "No zones yet. Draw kingdoms, districts or danger zones."),
    ("Clic en una zona para enfocarla en el mapa.", "Click a zone to focus it on the map."),
    ("Terreno", "Terrain"),
    ("Pincel", "Brush"),
    ("Textura PNG", "PNG texture"),
    ("Importa un PNG con «Camino» o «Muralla» en el nombre para usarlo aquí.", "Import a PNG with “Camino” or “Muralla” in its name to use it here."),
    ("Material", "Material"),
    ("Torres", "Towers"),
    ("Capa", "Layer"),
    ("Relleno", "Fill"),
    ("Color del borde", "Border color"),
    ("Color", "Color"),
    ("Color libre", "Free color"),
    ("Centrar y encuadrar el mapa completo", "Center and fit the whole map"),
    ("Volver a este mapa", "Back to this map"),
    ("Guarda el mapa completo como imagen", "Saves the whole map as an image"),
    ("Activa o desactiva la textura base del terreno y los PNG de caminos y murallas", "Toggles the base terrain texture and the path and wall PNGs"),
    ("Hora del día: sombras (06:00–19:00) y noche (19:30–05:30)", "Time of day: shadows (06:00–19:00) and night (19:30–05:30)"),
    ("El sub-mapa recibe, como referencia bloqueada, los ríos, caminos y assets del mapa general que están alrededor", "The sub-map receives, as a locked reference, the rivers, paths and assets of the parent map around it"),
    ("Altura (montañas, colinas): sombra más larga según la hora. Requiere Sombras activas.", "Height (mountains, hills): longer shadow depending on the hour. Requires Shadows on."),
    ("Supr", "Del"),
    ("Sobre el resto de assets de su capa", "Above the other assets in its layer"),
    ("Bajo el resto de assets de su capa", "Below the other assets in its layer"),
    ("Altura de la zona (colina, meseta): proyecta sombra según la hora. Requiere Sombras activas.", "Zone height (hill, plateau): casts a shadow depending on the hour. Requires Shadows on."),
    ("Crea una capa encima de la activa", "Creates a layer above the active one"),
    ("Clic: capa activa · doble clic: renombrar", "Click: active layer · double-click: rename"),
    ("Colorea un área cerrada (o el fondo) con el color elegido", "Fills a closed area (or the background) with the chosen color"),
    ("Tamaño", "Size"),
    ("Opacidad", "Opacity"),
    ("Radio", "Radius"),
    ("Separación", "Spacing"),
    ("Ancho", "Width"),
    ("Rotación", "Rotation"),
    ("Relieve", "Relief"),
    ("Escala", "Scale"),
    ("Grosor", "Thickness"),
    ("Dureza", "Hardness"),
    ("Cada", "Every"),
    ("Mapa", "Map"),
    ("Capas", "Layers"),
    ("Herramienta", "Tool"),
    ("Biblioteca", "Library"),
    ("Zonas", "Zones"),
    ("Sin enlace", "No link"),
    ("Vectorial (sin PNG)", "Vector (no PNG)"),
    ("Texturas", "Textures"),
    ("Sombras", "Shadows"),
    ("Mostrar cuadrícula", "Show grid"),
    ("Línea discontinua (sendero)", "Dashed line (trail)"),
    ("Línea discontinua", "Dashed line"),
    ("Borde nítido", "Sharp border"),
    ("Mostrar zonas", "Show zones"),
    ("Torres periódicas", "Periodic towers"),
    ("Tamaño fijo", "Fixed size"),
    ("Lienzo infinito", "Infinite canvas"),
    ("Lienzo en blanco", "Blank canvas"),
    ("Imagen base", "Base image"),
    ("Desde contorno", "From outline"),
    ("Cuadrada", "Square"),
    ("Hexagonal", "Hexagonal"),
    ("  · con mapa interior", "  · with inner map"),
    ("1 palabra", "1 word"),
    ("Abre una carpeta de proyecto existente desde cualquier ubicación.", "Opens an existing project folder from any location."),
    ("Acento", "Accent"),
    ("Acércate a otro trazo para anclarte a él y crear bifurcaciones (aro de conexión).", "Get close to another stroke to snap to it and create forks (connection ring)."),
    ("Agua", "Water"),
    ("Agua profunda", "Deep water"),
    ("Alinear a la derecha", "Align right"),
    ("Alinear a la izquierda", "Align left"),
    ("Arena", "Sand"),
    ("Assets", "Assets"),
    ("Aumentar sangría", "Increase indent"),
    ("Añadir foto", "Add photo"),
    ("Bajar capa", "Move layer down"),
    ("Barro", "Mud"),
    ("Bienvenido a\nFableForge", "Welcome to\nFableForge"),
    ("Bloquear capa", "Lock layer"),
    ("Borrador de terreno (E)", "Terrain eraser (E)"),
    ("Café Pastel Claro", "Pastel Coffee Light"),
    ("Café Pastel Oscuro", "Pastel Coffee Dark"),
    ("Caminos / Ríos (C)", "Paths / Rivers (C)"),
    ("Capítulos", "Chapters"),
    ("Cargar Historia", "Load Story"),
    ("Clic añade nodos · Enter o doble clic finaliza · Retroceso quita el último · Esc cancela.", "Click adds nodes · Enter or double-click finishes · Backspace removes the last · Esc cancels."),
    ("Clic añade vértices · clic derecho, Enter o doble clic cierra la zona · Retroceso quita el último · Esc cancela.", "Click adds vertices · right-click, Enter or double-click closes the zone · Backspace removes the last · Esc cancels."),
    ("Clic para añadir vértices; doble clic, clic derecho o Enter cierran la región. Color sólido y borde nítido a cualquier zoom; edita los vértices con Seleccionar.", "Click to add vertices; double-click, right-click or Enter closes the region. Solid color and sharp border at any zoom; edit vertices with Select."),
    ("Comienza un universo nuevo: FableForge prepara la carpeta con capítulos, lore, entidades y mapas.", "Start a new universe: FableForge prepares the folder with chapters, lore, entities and maps."),
    ("Contraer menú", "Collapse menu"),
    ("Copiar contexto cercano al crear", "Copy nearby context on create"),
    ("Costa / tinta", "Coast / ink"),
    ("Crea el primer evento de lore", "Create the first lore event"),
    ("Crea la primera ficha", "Create the first entry"),
    ("Crear", "Create"),
    ("Crear Historia", "Create Story"),
    ("Cursiva (Ctrl+I)", "Italic (Ctrl+I)"),
    ("Césped", "Grass"),
    ("Desbloquear capa", "Unlock layer"),
    ("Deshacer (Ctrl+Z)", "Undo (Ctrl+Z)"),
    ("Detalles", "Details"),
    ("Disminuir sangría", "Decrease indent"),
    ("Dispersar assets (D)", "Scatter assets (D)"),
    ("Edad", "Age"),
    ("Editar evento", "Edit event"),
    ("El borrador revela el fondo base (agua o papel) para esculpir costas e islas.", "The eraser reveals the base background (water or paper) to sculpt coasts and islands."),
    ("El nombre no puede estar vacío.", "The name cannot be empty."),
    ("Elegir carpeta de historias", "Choose stories folder"),
    ("Elegir foto de perfil", "Choose profile photo"),
    ("Elegir imagen", "Choose image"),
    ("Elegir imagen de la ficha", "Choose entry image"),
    ("Elegir imagen del evento", "Choose event image"),
    ("Elige una imagen para este modo de creación.", "Choose an image for this creation mode."),
    ("Elige uno o varios assets en la pestaña Assets y haz clic en el mapa para estamparlos.", "Pick one or more assets in the Assets tab and click the map to stamp them."),
    ("Eliminar capa (su contenido pasa a la capa vecina)", "Delete layer (its content moves to the neighboring layer)"),
    ("Eliminar evento", "Delete event"),
    ("Entidades", "Entities"),
    ("Estampar assets (P)", "Stamp assets (P)"),
    ("Expandir menú", "Expand menu"),
    ("Exportar mapa a PNG", "Export map to PNG"),
    ("Fondo", "Background"),
    ("Fondo base de agua (Atlas) o papel pergamino (Pergamino).", "Water base background (Atlas) or parchment paper (Parchment)."),
    ("Fondo principal", "Main background"),
    ("Guardar", "Save"),
    ("Imagen actual", "Current image"),
    ("Importar Historia", "Import Story"),
    ("Importar PNG a la biblioteca de la historia", "Import PNG to the story library"),
    ("Importar carpeta de assets", "Import assets folder"),
    ("Invertir: más antiguo arriba", "Reverse: oldest on top"),
    ("Invertir: más reciente arriba", "Reverse: newest on top"),
    ("Justificar", "Justify"),
    ("Lore y Cronología", "Lore & Timeline"),
    ("Mano alzada", "Freehand"),
    ("Mano: mover vista (H) · o clic central / derecho", "Hand: pan view (H) · or middle / right click"),
    ("Mantén presionado y arrastra para dibujar; al soltar se limpia el trazo.", "Hold and drag to draw; the stroke is cleaned up on release."),
    ("Mantén pulsado y arrastra. Rotación 0–360° y escala ×0.85–1.15 al azar. Con varios assets elegidos se mezclan.", "Hold and drag. Random rotation 0–360° and scale ×0.85–1.15. With several assets chosen they are mixed."),
    ("Mapa interior", "Inner map"),
    ("Matriz", "Matrix"),
    ("Mostrar capa", "Show layer"),
    ("Mostrar explorador", "Show explorer"),
    ("Murallas (W)", "Walls (W)"),
    ("Módulo Capítulos · Temporadas y editor Markdown (Etapa 2.2)", "Chapters module · Seasons and Markdown editor"),
    ("Negrita (Ctrl+B)", "Bold (Ctrl+B)"),
    ("Negro Total", "Pure Black"),
    ("Nieve", "Snow"),
    ("Ninguna seleccionada", "None selected"),
    ("No hay assets. Copia tus PNG en assets/map_defaults/ (junto al proyecto) o importa los tuyos con los botones de arriba.", "No assets. Copy your PNGs into assets/map_defaults/ (next to the project) or import yours with the buttons above."),
    ("No hay historia abierta.", "No story is open."),
    ("No se encontraron PNG para importar.", "No PNG files found to import."),
    ("No se puede eliminar: la capa de terreno y la última capa de objetos son obligatorias.", "Cannot delete: the terrain layer and the last object layer are required."),
    ("Nodos", "Nodes"),
    ("Nueva categoría", "New category"),
    ("Nueva temporada", "New season"),
    ("Nuevo capítulo", "New chapter"),
    ("Nuevo evento de Lore", "New Lore event"),
    ("Ocultar capa", "Hide layer"),
    ("Ocultar explorador", "Hide explorer"),
    ("Paneles y barras", "Panels and bars"),
    ("Pincel de terreno (B)", "Terrain brush (B)"),
    ("Párrafo", "Paragraph"),
    ("Píxeles oscuros → tierra, claros → agua; se genera una textura base nítida.", "Dark pixels → land, light → water; a sharp base texture is generated."),
    ("Quitar categoría", "Remove category"),
    ("Rasgos", "Traits"),
    ("Región vectorial: costas, lagos, plataformas nítidas (T)", "Vector region: coasts, lakes, sharp platforms (T)"),
    ("Rehacer (Ctrl+Y)", "Redo (Ctrl+Y)"),
    ("Relleno / cubeta (G)", "Fill / bucket (G)"),
    ("Retoma una de las historias guardadas en tu carpeta de historias.", "Resume one of the stories saved in your stories folder."),
    ("Roca", "Rock"),
    ("Se carga completa en la capa Terreno como fondo o guía.", "Loaded whole into the Terrain layer as a background or guide."),
    ("Se quitará la imagen", "The image will be removed"),
    ("Selecciona un asset, trazo, zona o región del mapa, o elige una herramienta de la barra izquierda. Ctrl + clic suma o quita assets; arrastra sobre un espacio vacío (o Mayús + arrastrar) para seleccionar por recuadro.", "Select an asset, stroke, zone or region on the map, or pick a tool from the left bar. Ctrl + click adds or removes assets; drag over empty space (or Shift + drag) to box-select."),
    ("Selecciona un evento en la línea de tiempo", "Select an event on the timeline"),
    ("Selecciona una ficha", "Select an entry"),
    ("Seleccionar (V)", "Select (V)"),
    ("Sin imagen", "No image"),
    ("Sin nombre", "Unnamed"),
    ("Sub-mapa creado; usa «Entrar al mapa interior».", "Sub-map created; use “Enter inner map”."),
    ("Subir capa", "Move layer up"),
    ("Subrayado", "Underline"),
    ("Subtítulo", "Subtitle"),
    ("Tarjetas e inputs", "Cards and inputs"),
    ("Tierra", "Dirt"),
    ("Volver a Mapas", "Back to Maps"),
    ("Ya existe un mapa con ese nombre.", "A map with that name already exists."),
    ("Zonas / Territorios (Z)", "Zones / Territories (Z)"),
    ("Inicio ", "Home "),
    ("Hexágonos", "Hexagons"),
    ("Cuadrícula cuadrada", "Square grid"),
    ("Autor", "Author"),
    ("Perfil de Autor (opcional)", "Author Profile (optional)"),
    ("Idioma / Language", "Language / Idioma"),
    ("Español", "Spanish"),
    ("Autor de la historia", "Story author"),
    ("por {}", "by {}"),
    ("{} palabras", "{} words"),
    ("Año {}", "Year {}"),
    ("{} assets seleccionados", "{} assets selected"),
    ("Eliminar categoría «{}»", "Delete category “{}”"),
    ("{} · Lienzo infinito", "{} · Infinite canvas"),
    ("Modificado {}", "Modified {}"),
    ("{} · Capa: {}", "{} · Layer: {}"),
    ("Dimensiones: {}", "Dimensions: {dims}"),
    ("{} nodos · arrastra un nodo para moverlo · doble clic en un nodo lo quita · doble clic en el trazo añade uno", "{} nodes · drag a node to move it · double-click a node to remove it · double-click the stroke to add one"),
    ("{} vértices · arrastra un vértice · doble clic en un vértice lo quita · doble clic en el borde añade uno", "{} vertices · drag a vertex · double-click a vertex to remove it · double-click the edge to add one"),
    ("Se creará en: {}", "Will be created in: {}"),
    ("Importar carpeta de historia", "Import story folder"),
    ("Imágenes", "Images"),
    ("Temporada 1", "Season 1"),
    ("Atlas", "Atlas"),
    ("Pergamino", "Parchment"),
    ("Personaje", "Character"),
    ("Objetos", "Objects"),
    ("Varios", "Misc"),
    ("Personajes", "Characters"),
    ("Lore", "Lore"),
    ("Camino", "Path"),
    ("Río", "River"),
    ("Muralla", "Wall"),
    ("Tierra ", "Dirt "),
    ("Piedra", "Stone"),
    ("Circular", "Round"),
    ("Alternada", "Alternating"),
    ("Claro", "Light"),
    ("Oscuro", "Dark"),
    ("El título no puede estar vacío.", "The title cannot be empty."),
    ("Ya existe una temporada con ese nombre.", "A season with that name already exists."),
    ("Ya existe un capítulo con ese nombre.", "A chapter with that name already exists."),
    ("Ya existe un elemento con ese nombre.", "An item with that name already exists."),
    ("El año debe ser un número entero (ej. 2021 o -340).", "The year must be a whole number (e.g. 2021 or -340)."),
    ("El evento ya no existe.", "The event no longer exists."),
    ("El nombre de la categoría no puede estar vacío.", "The category name cannot be empty."),
    ("Ya existe una categoría con ese nombre.", "A category with that name already exists."),
    ("Solo se pueden quitar categorías propias.", "Only custom categories can be removed."),
    ("La categoría tiene fichas: elimínalas o muévelas primero.", "The category has entries: delete or move them first."),
    ("Historia / Biografía", "History / Biography"),
    ("Descripción / Desarrollo", "Description / Development"),
    ("Fortalezas / Habilidades", "Strengths / Skills"),
    ("Debilidades / Miedos", "Weaknesses / Fears"),
    ("Objetivos / Motivaciones", "Goals / Motivations"),
    ("Conflictos / Vínculos", "Conflicts / Bonds"),
    ("Propiedades / Funcionamiento", "Properties / How it works"),
    ("Facción", "Faction"),
    ("Rango", "Rank"),
    ("Estado", "Status"),
    ("Categoría / Tipo", "Category / Type"),
    ("Portador actual", "Current bearer"),
    ("Categoría", "Category"),
    ("Nuevo personaje", "New character"),
    ("Nuevo objeto", "New object"),
    ("Nueva ficha", "New entry"),
    ("Seleccionar", "Select"),
    ("Mano", "Hand"),
    ("Relleno ", "Fill "),
    ("Zona", "Zone"),
    ("Región", "Region"),
    ("Sombras ", "Shadows "),
    ("Eliminar categoría", "Delete category"),
];
