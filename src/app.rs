// src/app.rs
//! Estado principal de FableForge, navegación y área central.

use egui::{Frame, Margin, RichText, ScrollArea};

use crate::i18n::tr;
use crate::icons::Icon;
use crate::storage::{self, Settings};
use crate::story::Story;
use crate::textures::TextureCache;
use crate::theme;
use crate::ui::{
    chapters::ChaptersView, dashboard::Dashboard, entities::EntitiesView, lore::LoreView,
    maps::MapsView, settings::SettingsDialog, sidebar,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Inicio,
    Capitulos,
    Lore,
    Entidades,
    Mapas,
}

impl Section {
    /// Módulos que se desbloquean al abrir una historia.
    pub const MODULES: [Section; 4] = [
        Section::Capitulos,
        Section::Lore,
        Section::Entidades,
        Section::Mapas,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Section::Inicio => tr("Inicio"),
            Section::Capitulos => tr("Capítulos"),
            Section::Lore => tr("Lore y Cronología"),
            Section::Entidades => tr("Entidades"),
            Section::Mapas => tr("Mapas"),
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Section::Inicio => Icon::Home,
            Section::Capitulos => Icon::BookOpen,
            Section::Lore => Icon::Hourglass,
            Section::Entidades => Icon::Users,
            Section::Mapas => Icon::Map,
        }
    }

    fn placeholder(self) -> &'static str {
        match self {
            Section::Inicio => "",
            Section::Capitulos => tr("Módulo Capítulos · Temporadas y editor Markdown (Etapa 2.2)"),
            Section::Lore | Section::Entidades | Section::Mapas => "",
        }
    }
}

pub struct FableForgeApp {
    settings: Settings,
    section: Section,
    story: Option<Story>,
    textures: TextureCache,
    settings_dialog: SettingsDialog,
    dashboard: Dashboard,
    chapters: ChaptersView,
    lore: LoreView,
    entities: EntitiesView,
    maps: MapsView,
}

impl FableForgeApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let settings = Settings::load();
        crate::i18n::set_language(settings.language);
        theme::install_fonts(&cc.egui_ctx);
        theme::apply(
            &cc.egui_ctx,
            settings.theme,
            settings.custom_colors.as_ref(),
        );
        Self {
            settings,
            section: Section::Inicio,
            story: None,
            textures: TextureCache::default(),
            settings_dialog: SettingsDialog::default(),
            dashboard: Dashboard::default(),
            chapters: ChaptersView::default(),
            lore: LoreView::default(),
            entities: EntitiesView::default(),
            maps: MapsView::default(),
        }
    }
}

impl FableForgeApp {
    fn flush_all(&mut self) {
        self.chapters.flush();
        self.lore.flush();
        self.entities.flush();
        self.maps.flush();
    }
}

impl eframe::App for FableForgeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Avatar: textura cacheada desde `avatar.png` (si existe).
        let avatar_path = storage::avatar_path();
        let avatar = if avatar_path.is_file() {
            self.textures.get(&ctx, &avatar_path).ok().cloned()
        } else {
            None
        };

        let out = sidebar::show(
            ui,
            &mut self.settings.sidebar_collapsed,
            &mut self.section,
            self.story.is_some(),
            &self.settings.user_name,
            avatar.as_ref(),
        );
        if out.open_settings {
            self.settings_dialog.open(&self.settings);
        }
        if out.close_story {
            self.flush_all();
            self.chapters = ChaptersView::default();
            self.lore = LoreView::default();
            self.entities = EntitiesView::default();
            self.maps = MapsView::default();
            self.story = None;
            self.section = Section::Inicio;
        }

        // Guarda al salir de cada módulo.
        if self.section != Section::Capitulos {
            self.chapters.flush();
        }
        if self.section != Section::Lore {
            self.lore.flush();
        }
        if self.section != Section::Entidades {
            self.entities.flush();
        }
        if self.section != Section::Mapas {
            self.maps.flush();
        }

        let mut opened: Option<Story> = None;
        let fill = ui.visuals().panel_fill;
        if let (Some(story), Section::Capitulos) = (&self.story, self.section) {
            self.chapters.show(ui, story);
        } else if let (Some(story), Section::Lore) = (&self.story, self.section) {
            self.lore.show(ui, story, &mut self.textures);
        } else if let (Some(story), Section::Entidades) = (&self.story, self.section) {
            self.entities.show(ui, story, &mut self.textures);
        } else if let (Some(story), Section::Mapas) = (&self.story, self.section) {
            self.maps.show(ui, story, &mut self.textures);
        } else {
            egui::CentralPanel::default()
                .frame(
                    Frame::new()
                        .fill(fill)
                        .inner_margin(Margin::symmetric(56, 40)),
                )
                .show(ui, |ui| {
                    ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| match &self.story {
                            None => {
                                ui.label(
                                    RichText::new(tr("Bienvenido a\nFableForge"))
                                        .family(theme::serif())
                                        .size(52.0),
                                );
                                ui.add_space(36.0);
                                self.dashboard.author = self.settings.user_name.clone();
                                opened = self.dashboard.show(ui, &self.settings.root_folder);
                            }
                            Some(story) => {
                                let weak = ui.visuals().weak_text_color();
                                let head = if story.meta.author.trim().is_empty() {
                                    story.meta.title.clone()
                                } else {
                                    format!(
                                        "{} · {}",
                                        story.meta.title,
                                        crate::i18n::trf("por {}", &[&story.meta.author.trim()])
                                    )
                                };
                                ui.label(
                                    RichText::new(head)
                                        .family(theme::serif())
                                        .size(15.0)
                                        .color(weak),
                                );
                                ui.label(
                                    RichText::new(self.section.label())
                                        .family(theme::serif())
                                        .size(46.0),
                                );
                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new(self.section.placeholder())
                                        .size(14.0)
                                        .color(weak),
                                );
                            }
                        });
                });
        }

        if let Some(story) = opened {
            self.story = Some(story);
            self.section = Section::Capitulos;
        }

        self.settings_dialog
            .show(&ctx, &mut self.settings, &mut self.textures);
    }

    fn on_exit(&mut self) {
        self.flush_all();
    }
}
