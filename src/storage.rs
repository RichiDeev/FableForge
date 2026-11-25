// src/storage.rs
//! Persistencia en disco: `user_settings.json` y copia del avatar.
//! Ubicación: `%APPDATA%/FableForge` en Windows (equivalente nativo en otros SO).

use crate::i18n::{tr, Language};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const APP_DIR: &str = "FableForge";
const SETTINGS_FILE: &str = "user_settings.json";
const AVATAR_FILE: &str = "avatar.png";
const AVATAR_SIZE: u32 = 256;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    Light,
    Dark,
    /// Negro profundo con bordes suaves.
    Black,
    /// Oscuro con acento verde suave.
    Matrix,
}

impl ThemeMode {
    pub const ALL: [ThemeMode; 4] = [
        ThemeMode::Light,
        ThemeMode::Dark,
        ThemeMode::Black,
        ThemeMode::Matrix,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ThemeMode::Light => tr("Café Pastel Claro"),
            ThemeMode::Dark => tr("Café Pastel Oscuro"),
            ThemeMode::Black => tr("Negro Total"),
            ThemeMode::Matrix => tr("Matriz"),
        }
    }
}

/// Colores elegidos a mano (RGBA sin premultiplicar). Si existen, reemplazan a los del preset.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct CustomColors {
    /// Fondo principal.
    pub background: [u8; 4],
    /// Paneles y barras.
    pub panels: [u8; 4],
    /// Tarjetas e inputs.
    pub cards: [u8; 4],
    pub accent: [u8; 4],
    pub text: [u8; 4],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub user_name: String,
    pub theme: ThemeMode,
    pub root_folder: PathBuf,
    pub sidebar_collapsed: bool,
    /// Colores personalizados del tema activo (`None` = usar el preset tal cual).
    pub custom_colors: Option<CustomColors>,
    /// Idioma de la interfaz (por defecto el del sistema; español si no se detecta).
    pub language: Language,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            user_name: "Autor".to_owned(),
            theme: ThemeMode::Light,
            root_folder: default_root_folder(),
            sidebar_collapsed: false,
            custom_colors: None,
            language: Language::detect(),
        }
    }
}

impl Settings {
    /// Carga la configuración; si no existe o está corrupta, usa valores por defecto.
    pub fn load() -> Self {
        fs::read_to_string(settings_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        fs::create_dir_all(config_dir())
            .map_err(|e| format!("No se pudo crear la carpeta de configuración: {e}"))?;
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(settings_path(), json)
            .map_err(|e| format!("No se pudo guardar la configuración: {e}"))?;
        // La carpeta raíz de historias se crea si no existe.
        fs::create_dir_all(&self.root_folder)
            .map_err(|e| format!("No se pudo crear la carpeta de historias: {e}"))?;
        Ok(())
    }
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR)
}

pub fn settings_path() -> PathBuf {
    config_dir().join(SETTINGS_FILE)
}

pub fn avatar_path() -> PathBuf {
    config_dir().join(AVATAR_FILE)
}

fn default_root_folder() -> PathBuf {
    dirs::document_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR)
}

/// Copia la imagen elegida como `avatar.png` (recortada a cuadrado 256×256).
pub fn import_avatar(src: &Path) -> Result<(), String> {
    let img = image::open(src).map_err(|e| format!("No se pudo leer la imagen: {e}"))?;
    let square = img.resize_to_fill(
        AVATAR_SIZE,
        AVATAR_SIZE,
        image::imageops::FilterType::Lanczos3,
    );
    fs::create_dir_all(config_dir()).map_err(|e| e.to_string())?;
    square
        .save_with_format(avatar_path(), image::ImageFormat::Png)
        .map_err(|e| format!("No se pudo guardar el avatar: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_defaults_and_roundtrips() {
        let old: Settings = serde_json::from_str(r#"{"user_name":"Ana"}"#).unwrap();
        assert!(Language::ALL.contains(&old.language));
        let s = Settings {
            language: Language::En,
            ..Settings::default()
        };
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.language, Language::En);
    }

    #[test]
    fn theme_and_custom_colors_roundtrip() {
        let mut s = Settings {
            theme: ThemeMode::Matrix,
            ..Settings::default()
        };
        s.custom_colors = Some(CustomColors {
            background: [1, 2, 3, 255],
            panels: [4, 5, 6, 255],
            cards: [7, 8, 9, 255],
            accent: [10, 11, 12, 255],
            text: [13, 14, 15, 255],
        });
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"matrix\""));
        let l: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(l.theme, ThemeMode::Matrix);
        assert_eq!(l.custom_colors, s.custom_colors);
        // Un archivo anterior (sin colores propios, tema «dark») sigue cargando.
        let old: Settings = serde_json::from_str(r#"{"user_name":"Ana","theme":"dark"}"#).unwrap();
        assert_eq!(old.theme, ThemeMode::Dark);
        assert!(old.custom_colors.is_none());
    }
}
