// src/ui/sidebar.rs
//! Barra lateral izquierda.
//!
//! - Sin historia abierta: fija y compacta (64 px) con solo `Inicio` y el perfil.
//! - Con historia abierta: módulos de la historia, volver a Inicio y colapsar/expandir (64 / 200 px).

use std::sync::Arc;

use egui::{
    load::SizedTexture, text::LayoutJob, Align, Align2, Color32, CornerRadius, FontId, Frame, Id,
    Image, Layout, Margin, Rect, Response, Sense, Stroke, TextureHandle, Ui, Vec2,
};

use crate::app::Section;
use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::theme;

const W_COMPACT: f32 = 64.0;
const W_EXPANDED: f32 = 200.0;
const PAD: i8 = 10;
const BTN_H: f32 = 40.0;
const AVATAR: f32 = 44.0;

pub struct Output {
    pub open_settings: bool,
    pub close_story: bool,
}

pub fn show(
    ui: &mut Ui,
    collapsed: &mut bool,
    active: &mut Section,
    story_open: bool,
    user_name: &str,
    avatar: Option<&TextureHandle>,
) -> Output {
    let mut out = Output {
        open_settings: false,
        close_story: false,
    };

    let target = if story_open && !*collapsed {
        W_EXPANDED
    } else {
        W_COMPACT
    };
    let width = ui
        .ctx()
        .animate_value_with_time(Id::unique("sidebar_width"), target, 0.12);
    let show_labels = width > 130.0;

    let fill = ui.visuals().faint_bg_color;
    let border = ui.visuals().window_stroke.color;

    egui::Panel::left("sidebar")
        .exact_size(width)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            Frame::new()
                .fill(fill)
                .stroke(Stroke::new(1.0, border))
                .inner_margin(Margin::same(PAD)),
        )
        .show(ui, |ui| {
            if story_open {
                let tip = if *collapsed {
                    tr("Expandir menú")
                } else {
                    tr("Contraer menú")
                };
                if toggle_button(ui, show_labels).on_hover_text(tip).clicked() {
                    *collapsed = !*collapsed;
                }
                ui.add_space(8.0);
            }

            // Inicio: activo en el dashboard; con historia abierta cierra la historia.
            let home = nav_button(ui, Icon::Home, tr("Inicio"), !story_open, show_labels);
            if story_open {
                if home
                    .clone()
                    .on_hover_text(tr("Cerrar historia y volver a Inicio"))
                    .clicked()
                {
                    out.close_story = true;
                }
            } else if !show_labels {
                home.on_hover_text(tr("Inicio"));
            }

            if story_open {
                ui.add_space(10.0);
                for section in Section::MODULES {
                    let resp = nav_button(
                        ui,
                        section.icon(),
                        section.label(),
                        *active == section,
                        show_labels,
                    );
                    if !show_labels {
                        resp.clone().on_hover_text(section.label());
                    }
                    if resp.clicked() {
                        *active = section;
                    }
                    ui.add_space(4.0);
                }
            }

            ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
                profile_block(ui, user_name, avatar, show_labels, &mut out.open_settings);
            });
        });

    out
}

/// Botón discreto (solo icono) para colapsar/expandir.
fn toggle_button(ui: &mut Ui, expanded: bool) -> Response {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), Sense::click());
    let center_x = if expanded {
        rect.right() - 16.0
    } else {
        rect.center().x
    };
    let hit = Rect::from_center_size(egui::pos2(center_x, rect.center().y), Vec2::splat(32.0));
    let v = ui.visuals();
    if resp.hovered() {
        ui.painter()
            .rect_filled(hit, CornerRadius::same(6), v.widgets.hovered.weak_bg_fill);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let color = v.weak_text_color();
    icons::paint(
        ui,
        Icon::PanelLeft,
        Rect::from_center_size(hit.center(), Vec2::splat(18.0)),
        color,
    );
    resp
}

fn nav_button(ui: &mut Ui, icon: Icon, label: &str, selected: bool, show_label: bool) -> Response {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), BTN_H), Sense::click());
    let v = ui.visuals();
    let text = v.widgets.noninteractive.fg_stroke.color;
    let (bg, fg) = if selected {
        (v.selection.bg_fill, v.selection.stroke.color)
    } else if resp.hovered() {
        (v.widgets.hovered.weak_bg_fill, text)
    } else {
        (Color32::TRANSPARENT, text)
    };

    ui.painter().rect_filled(rect, CornerRadius::same(6), bg);
    let icon_rect =
        Rect::from_center_size(rect.left_center() + Vec2::new(22.0, 0.0), Vec2::splat(20.0));
    icons::paint(ui, icon, icon_rect, fg);
    if show_label {
        ui.painter().with_clip_rect(rect).text(
            rect.left_center() + Vec2::new(44.0, 0.0),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(14.5),
            fg,
        );
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn profile_block(
    ui: &mut Ui,
    name: &str,
    avatar: Option<&TextureHandle>,
    show_name: bool,
    open_settings: &mut bool,
) {
    let h = AVATAR + if show_name { 28.0 } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::click());

    let avatar_rect = Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + AVATAR / 2.0),
        Vec2::splat(AVATAR),
    );
    let accent = ui.visuals().selection.bg_fill;
    let on_accent = ui.visuals().selection.stroke.color;
    let text_color = ui.visuals().widgets.noninteractive.fg_stroke.color;
    let border = ui.visuals().window_stroke.color;

    match avatar {
        Some(tex) => {
            Image::new(SizedTexture::new(tex.id(), Vec2::splat(AVATAR)))
                .corner_radius(CornerRadius::same((AVATAR / 2.0) as u8))
                .paint_at(ui, avatar_rect);
        }
        None => {
            ui.painter()
                .circle_filled(avatar_rect.center(), AVATAR / 2.0, accent);
            ui.painter().text(
                avatar_rect.center(),
                Align2::CENTER_CENTER,
                initials(name),
                FontId::new(17.0, theme::serif()),
                on_accent,
            );
        }
    }
    let ring = if resp.hovered() { accent } else { border };
    ui.painter()
        .circle_stroke(avatar_rect.center(), AVATAR / 2.0, Stroke::new(1.5, ring));

    if show_name {
        let mut job = LayoutJob::simple(
            name.to_owned(),
            FontId::new(14.0, theme::serif()),
            text_color,
            rect.width(),
        );
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        job.wrap.overflow_character = Some('…');
        let galley: Arc<_> = ui.painter().layout_job(job);
        let pos = egui::pos2(
            rect.center().x - galley.size().x / 2.0,
            avatar_rect.bottom() + 6.0,
        );
        ui.painter().galley(pos, galley, text_color);
    }

    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.on_hover_text(tr("Ajustes generales")).clicked() {
        *open_settings = true;
    }
}

fn initials(name: &str) -> String {
    let s: String = name
        .split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .flat_map(|c| c.to_uppercase())
        .collect();
    if s.is_empty() {
        "?".to_owned()
    } else {
        s
    }
}
