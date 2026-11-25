// src/icons.rs
//! Iconos SVG estilo Lucide (stroke 1.75), embebidos y tintados con el color del tema.
//!
//! Los trazos se dibujan en blanco; `Image::tint` los multiplica por el color pedido,
//! así que el resultado toma exactamente el color de texto del tema activo.

use egui::{load::Bytes, Color32, Image, ImageSource, Rect, Ui};
use std::borrow::Cow;

macro_rules! svg {
    ($body:expr) => {
        concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" "##,
            r##"fill="none" stroke="#ffffff" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">"##,
            $body,
            "</svg>"
        )
    };
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Home,
    BookOpen,
    Hourglass,
    Users,
    Map,
    PanelLeft,
    Plus,
    FolderOpen,
    Import,
    FolderPlus,
    FilePlus,
    ChevronRight,
    ChevronDown,
    Bold,
    Italic,
    Underline,
    AlignLeft,
    AlignCenter,
    AlignRight,
    AlignJustify,
    IndentIncrease,
    IndentDecrease,
    ArrowUpDown,
    MousePointer,
    Stamp,
    Hand,
    Eye,
    EyeOff,
    Lock,
    Unlock,
    ArrowLeft,
    Brush,
    Eraser,
    Bucket,
    Scatter,
    Route,
    Trash,
    Undo,
    Redo,
    ArrowUp,
    ArrowDown,
    Zone,
    Sun,
    Wall,
    Region,
}

impl Icon {
    fn uri(self) -> &'static str {
        match self {
            Icon::Home => "bytes://icons/home.svg",
            Icon::BookOpen => "bytes://icons/book-open.svg",
            Icon::Hourglass => "bytes://icons/hourglass.svg",
            Icon::Users => "bytes://icons/users.svg",
            Icon::Map => "bytes://icons/map.svg",
            Icon::PanelLeft => "bytes://icons/panel-left.svg",
            Icon::Plus => "bytes://icons/plus.svg",
            Icon::FolderOpen => "bytes://icons/folder-open.svg",
            Icon::Import => "bytes://icons/import.svg",
            Icon::FolderPlus => "bytes://icons/folder-plus.svg",
            Icon::FilePlus => "bytes://icons/file-plus.svg",
            Icon::ChevronRight => "bytes://icons/chevron-right.svg",
            Icon::ChevronDown => "bytes://icons/chevron-down.svg",
            Icon::Bold => "bytes://icons/bold.svg",
            Icon::Italic => "bytes://icons/italic.svg",
            Icon::Underline => "bytes://icons/underline.svg",
            Icon::AlignLeft => "bytes://icons/align-left.svg",
            Icon::AlignCenter => "bytes://icons/align-center.svg",
            Icon::AlignRight => "bytes://icons/align-right.svg",
            Icon::AlignJustify => "bytes://icons/align-justify.svg",
            Icon::IndentIncrease => "bytes://icons/indent-increase.svg",
            Icon::IndentDecrease => "bytes://icons/indent-decrease.svg",
            Icon::ArrowUpDown => "bytes://icons/arrow-up-down.svg",
            Icon::MousePointer => "bytes://icons/mouse-pointer.svg",
            Icon::Stamp => "bytes://icons/stamp.svg",
            Icon::Hand => "bytes://icons/hand.svg",
            Icon::Eye => "bytes://icons/eye.svg",
            Icon::EyeOff => "bytes://icons/eye-off.svg",
            Icon::Lock => "bytes://icons/lock.svg",
            Icon::Unlock => "bytes://icons/unlock.svg",
            Icon::ArrowLeft => "bytes://icons/arrow-left.svg",
            Icon::Brush => "bytes://icons/brush.svg",
            Icon::Eraser => "bytes://icons/eraser.svg",
            Icon::Bucket => "bytes://icons/paint-bucket.svg",
            Icon::Scatter => "bytes://icons/scatter.svg",
            Icon::Route => "bytes://icons/route.svg",
            Icon::Trash => "bytes://icons/trash-2.svg",
            Icon::Undo => "bytes://icons/undo-2.svg",
            Icon::Redo => "bytes://icons/redo-2.svg",
            Icon::ArrowUp => "bytes://icons/arrow-up.svg",
            Icon::ArrowDown => "bytes://icons/arrow-down.svg",
            Icon::Zone => "bytes://icons/zone.svg",
            Icon::Sun => "bytes://icons/sun.svg",
            Icon::Wall => "bytes://icons/brick-wall.svg",
            Icon::Region => "bytes://icons/region.svg",
        }
    }

    fn svg(self) -> &'static str {
        match self {
            Icon::Home => svg!(
                r#"<path d="M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8"/><path d="M3 10a2 2 0 0 1 .709-1.528l7-5.999a2 2 0 0 1 2.582 0l7 5.999A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>"#
            ),
            Icon::BookOpen => svg!(
                r#"<path d="M12 7v14"/><path d="M3 18a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h5a4 4 0 0 1 4 4 4 4 0 0 1 4-4h5a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1h-6a3 3 0 0 0-3 3 3 3 0 0 0-3-3z"/>"#
            ),
            Icon::Hourglass => svg!(
                r#"<path d="M5 22h14"/><path d="M5 2h14"/><path d="M17 22v-4.172a2 2 0 0 0-.586-1.414L12 12l-4.414 4.414A2 2 0 0 0 7 17.828V22"/><path d="M7 2v4.172a2 2 0 0 0 .586 1.414L12 12l4.414-4.414A2 2 0 0 0 17 6.172V2"/>"#
            ),
            Icon::Users => svg!(
                r#"<path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M22 21v-2a4 4 0 0 0-3-3.87"/><path d="M16 3.13a4 4 0 0 1 0 7.75"/>"#
            ),
            Icon::Map => svg!(
                r#"<path d="M14.106 5.553a2 2 0 0 0 1.788 0l3.659-1.83A1 1 0 0 1 21 4.619v12.764a1 1 0 0 1-.553.894l-4.553 2.277a2 2 0 0 1-1.788 0l-4.212-2.106a2 2 0 0 0-1.788 0l-3.659 1.83A1 1 0 0 1 3 19.381V6.618a1 1 0 0 1 .553-.894l4.553-2.277a2 2 0 0 1 1.788 0z"/><path d="M15 5.764v15"/><path d="M9 3.236v15"/>"#
            ),
            Icon::PanelLeft => {
                svg!(r#"<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M9 3v18"/>"#)
            }
            Icon::Plus => svg!(r#"<path d="M5 12h14"/><path d="M12 5v14"/>"#),
            Icon::FolderOpen => svg!(
                r#"<path d="m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2"/>"#
            ),
            Icon::Import => svg!(
                r#"<path d="M12 15V3"/><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><path d="m7 10 5 5 5-5"/>"#
            ),
            Icon::FolderPlus => svg!(
                r#"<path d="M12 10v6"/><path d="M9 13h6"/><path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>"#
            ),
            Icon::FilePlus => svg!(
                r#"<path d="M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z"/><path d="M14 2v5a1 1 0 0 0 1 1h5"/><path d="M9 15h6"/><path d="M12 18v-6"/>"#
            ),
            Icon::ChevronRight => svg!(r#"<path d="m9 18 6-6-6-6"/>"#),
            Icon::ChevronDown => svg!(r#"<path d="m6 9 6 6 6-6"/>"#),
            Icon::Bold => svg!(
                r#"<path d="M6 12h9a4 4 0 0 1 0 8H7a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1h7a4 4 0 0 1 0 8"/>"#
            ),
            Icon::Italic => svg!(
                r#"<line x1="19" x2="10" y1="4" y2="4"/><line x1="14" x2="5" y1="20" y2="20"/><line x1="15" x2="9" y1="4" y2="20"/>"#
            ),
            Icon::Underline => {
                svg!(r#"<path d="M6 4v6a6 6 0 0 0 12 0V4"/><line x1="4" x2="20" y1="20" y2="20"/>"#)
            }
            Icon::AlignLeft => {
                svg!(r#"<path d="M21 5H3"/><path d="M15 12H3"/><path d="M17 19H3"/>"#)
            }
            Icon::AlignCenter => {
                svg!(r#"<path d="M21 5H3"/><path d="M17 12H7"/><path d="M19 19H5"/>"#)
            }
            Icon::AlignRight => {
                svg!(r#"<path d="M21 5H3"/><path d="M21 12H9"/><path d="M21 19H7"/>"#)
            }
            Icon::AlignJustify => {
                svg!(r#"<path d="M3 5h18"/><path d="M3 12h18"/><path d="M3 19h18"/>"#)
            }
            Icon::IndentIncrease => svg!(
                r#"<path d="M21 5H11"/><path d="M21 12H11"/><path d="M21 19H11"/><path d="m3 8 4 4-4 4"/>"#
            ),
            Icon::IndentDecrease => svg!(
                r#"<path d="M21 5H11"/><path d="M21 12H11"/><path d="M21 19H11"/><path d="m7 8-4 4 4 4"/>"#
            ),
            Icon::ArrowUpDown => svg!(
                r#"<path d="m21 16-4 4-4-4"/><path d="M17 20V4"/><path d="m3 8 4-4 4 4"/><path d="M7 4v16"/>"#
            ),
            Icon::MousePointer => svg!(
                r#"<path d="M4.037 4.688a.495.495 0 0 1 .651-.651l16 6.5a.495.495 0 0 1-.053.939l-6.532 1.86a1 1 0 0 0-.687.687l-1.86 6.532a.495.495 0 0 1-.939.053z"/>"#
            ),
            Icon::Stamp => svg!(
                r#"<path d="M5 22h14"/><path d="M19.27 13.73A2.5 2.5 0 0 0 17.5 13h-11A2.5 2.5 0 0 0 4 15.5V17a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-1.5c0-.66-.26-1.3-.73-1.77"/><path d="M14 13V8.5C14 7 15 7 15 5a3 3 0 0 0-3-3c-1.66 0-3 1-3 3s1 2 1 3.5V13"/>"#
            ),
            Icon::Hand => svg!(
                r#"<path d="M18 11V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2"/><path d="M14 10V4a2 2 0 0 0-2-2a2 2 0 0 0-2 2v2"/><path d="M10 10.5V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2v8"/><path d="M18 8a2 2 0 1 1 4 0v6a8 8 0 0 1-8 8h-2c-2.8 0-4.5-.86-5.99-2.34l-3.6-3.6a2 2 0 0 1 2.83-2.82L7 15"/>"#
            ),
            Icon::Eye => svg!(
                r#"<path d="M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0"/><circle cx="12" cy="12" r="3"/>"#
            ),
            Icon::EyeOff => svg!(
                r#"<path d="M10.733 5.076a10.744 10.744 0 0 1 11.205 6.575 1 1 0 0 1 0 .696 10.747 10.747 0 0 1-1.444 2.49"/><path d="M14.084 14.158a3 3 0 0 1-4.242-4.242"/><path d="M17.479 17.499a10.75 10.75 0 0 1-15.417-5.151 1 1 0 0 1 0-.696 10.75 10.75 0 0 1 4.446-5.143"/><path d="m2 2 20 20"/>"#
            ),
            Icon::Lock => svg!(
                r#"<rect width="18" height="11" x="3" y="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/>"#
            ),
            Icon::Unlock => svg!(
                r#"<rect width="18" height="11" x="3" y="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 9.9-1"/>"#
            ),
            Icon::ArrowLeft => svg!(r#"<path d="m12 19-7-7 7-7"/><path d="M19 12H5"/>"#),
            Icon::Brush => svg!(
                r#"<path d="m9.06 11.9 8.07-8.06a2.85 2.85 0 1 1 4.03 4.03l-8.06 8.08"/><path d="M7.07 14.94c-1.66 0-3 1.35-3 3.02 0 1.33-2.5 1.52-2 2.02 1.08 1.1 2.49 2.02 4 2.02 2.2 0 4-1.8 4-4.04a3.01 3.01 0 0 0-3-3.02z"/>"#
            ),
            Icon::Eraser => svg!(
                r#"<path d="m7 21-4.3-4.3c-1-1-1-2.5 0-3.4l9.6-9.6c1-1 2.5-1 3.4 0l5.6 5.6c1 1 1 2.5 0 3.4L13 21"/><path d="M22 21H7"/><path d="m5 11 9 9"/>"#
            ),
            Icon::Bucket => svg!(
                r#"<path d="m19 11-8-8-8.6 8.6a2 2 0 0 0 0 2.8l5.2 5.2c.8.8 2 .8 2.8 0L19 11Z"/><path d="m5 2 5 5"/><path d="M2 13h15"/><path d="M22 20a2 2 0 1 1-4 0c0-1.6 1.7-2.4 2-4 .3 1.6 2 2.4 2 4Z"/>"#
            ),
            Icon::Scatter => svg!(
                r#"<circle cx="7" cy="7" r="2.5"/><circle cx="17" cy="9" r="2"/><circle cx="10" cy="16" r="2"/><circle cx="18" cy="18" r="1.5"/>"#
            ),
            Icon::Route => svg!(
                r#"<circle cx="6" cy="19" r="3"/><path d="M9 19h8.5a3.5 3.5 0 0 0 0-7h-11a3.5 3.5 0 0 1 0-7H15"/><circle cx="18" cy="5" r="3"/>"#
            ),
            Icon::Trash => svg!(
                r#"<path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/><line x1="10" x2="10" y1="11" y2="17"/><line x1="14" x2="14" y1="11" y2="17"/>"#
            ),
            Icon::Undo => svg!(
                r#"<path d="M9 14 4 9l5-5"/><path d="M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5a5.5 5.5 0 0 1-5.5 5.5H11"/>"#
            ),
            Icon::Redo => svg!(
                r#"<path d="m15 14 5-5-5-5"/><path d="M20 9H9.5A5.5 5.5 0 0 0 4 14.5A5.5 5.5 0 0 0 9.5 20H13"/>"#
            ),
            Icon::ArrowUp => svg!(r#"<path d="m5 12 7-7 7 7"/><path d="M12 19V5"/>"#),
            Icon::ArrowDown => svg!(r#"<path d="M12 5v14"/><path d="m19 12-7 7-7-7"/>"#),
            Icon::Zone => svg!(r#"<polygon points="12 2.5 21.5 9.5 18 20.5 6 20.5 2.5 9.5"/>"#),
            Icon::Wall => svg!(
                r#"<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M12 9v6"/><path d="M16 15v6"/><path d="M16 3v6"/><path d="M3 15h18"/><path d="M3 9h18"/><path d="M8 15v6"/><path d="M8 3v6"/>"#
            ),
            Icon::Region => svg!(
                r#"<path d="M4 13c-1-5 3-9 8-9s9 3 8 8c-1 4-4 8-9 8s-6-3-7-7z"/><circle cx="4" cy="13" r="1.4"/><circle cx="12" cy="4" r="1.4"/><circle cx="20" cy="12" r="1.4"/><circle cx="11" cy="20" r="1.4"/>"#
            ),
            Icon::Sun => svg!(
                r#"<circle cx="12" cy="12" r="4"/><path d="M12 2v2"/><path d="M12 20v2"/><path d="m4.93 4.93 1.41 1.41"/><path d="m17.66 17.66 1.41 1.41"/><path d="M2 12h2"/><path d="M20 12h2"/><path d="m6.34 17.66-1.41 1.41"/><path d="m19.07 4.93-1.41 1.41"/>"#
            ),
        }
    }

    fn source(self) -> ImageSource<'static> {
        ImageSource::Bytes {
            uri: Cow::Borrowed(self.uri()),
            bytes: Bytes::Static(self.svg().as_bytes()),
        }
    }
}

/// Dibuja el icono dentro de `rect`, tintado con `color`.
pub fn paint(ui: &Ui, icon: Icon, rect: Rect, color: Color32) {
    Image::new(icon.source()).tint(color).paint_at(ui, rect);
}
