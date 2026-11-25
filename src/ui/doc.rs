//! Utilidad de autoguardado con retardo (800 ms de inactividad).

use std::time::Duration;

use egui::Context;

const AUTOSAVE_SECS: f64 = 0.8;

#[derive(Default)]
pub struct Autosave {
    pub dirty: bool,
    pub last_edit: f64,
    pub error: Option<String>,
}

impl Autosave {
    /// Marca cambios pendientes.
    pub fn mark(&mut self, ctx: &Context) {
        self.dirty = true;
        self.last_edit = ctx.input(|i| i.time);
    }

    /// `true` cuando ya pasó el retardo y hay que guardar ahora; si no, agenda un repintado.
    pub fn due(&self, ctx: &Context) -> bool {
        if !self.dirty {
            return false;
        }
        let elapsed = ctx.input(|i| i.time) - self.last_edit;
        if elapsed >= AUTOSAVE_SECS {
            true
        } else {
            ctx.request_repaint_after(Duration::from_secs_f64((AUTOSAVE_SECS - elapsed).max(0.05)));
            false
        }
    }

    /// Registra el resultado de un guardado (ante error reintenta tras el retardo).
    pub fn finish(&mut self, ctx: &Context, result: Result<(), String>) {
        match result {
            Ok(()) => {
                self.dirty = false;
                self.error = None;
            }
            Err(e) => {
                self.error = Some(e);
                self.last_edit = ctx.input(|i| i.time);
            }
        }
    }

    /// Texto de estado y si es un error.
    pub fn status(&self) -> (&'static str, bool) {
        if self.error.is_some() {
            ("Error al guardar", true)
        } else if self.dirty {
            ("Guardando…", false)
        } else {
            ("Guardado", false)
        }
    }
}
