//! Markdown de FableForge: operaciones de edición (atajos y barra de formato) y vista previa.
//!
//! Sintaxis soportada: `#`/`##`/`###`, `**negrita**`, `*cursiva*`, `<u>subrayado</u>`, `~~tachado~~`,
//! listas con `- `, citas con `> `, `---`, alineación con `<div align="...">` y sangría con `&emsp;`.

use egui::{
    text::{LayoutJob, TextFormat},
    Align, Color32, FontFamily, FontId, Sense, Stroke, Ui, Vec2,
};

use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextAlign {
    Left,
    Center,
    Right,
    Justify,
}

impl TextAlign {
    fn tag(self) -> &'static str {
        match self {
            TextAlign::Left => "left",
            TextAlign::Center => "center",
            TextAlign::Right => "right",
            TextAlign::Justify => "justify",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "left" => Some(TextAlign::Left),
            "center" => Some(TextAlign::Center),
            "right" => Some(TextAlign::Right),
            "justify" => Some(TextAlign::Justify),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FontChoice {
    Serif,
    Sans,
}

impl FontChoice {
    pub const ALL: [FontChoice; 2] = [FontChoice::Serif, FontChoice::Sans];

    pub fn label(self) -> &'static str {
        match self {
            FontChoice::Serif => "Editorial Serif (Georgia)",
            FontChoice::Sans => "Moderna Sans",
        }
    }

    pub fn family(self, bold: bool) -> FontFamily {
        match (self, bold) {
            (FontChoice::Serif, false) => theme::serif(),
            (FontChoice::Serif, true) => theme::serif_bold(),
            (FontChoice::Sans, false) => FontFamily::Proportional,
            (FontChoice::Sans, true) => theme::sans_bold(),
        }
    }
}

/// Resultado de una edición: texto nuevo y selección (índices de carácter).
pub struct Edit {
    pub text: String,
    pub sel: (usize, usize),
}

fn ordered(sel: (usize, usize)) -> (usize, usize) {
    if sel.0 <= sel.1 {
        sel
    } else {
        (sel.1, sel.0)
    }
}

fn line_bounds(chars: &[char], pos: usize) -> (usize, usize) {
    let pos = pos.min(chars.len());
    let mut s = pos;
    while s > 0 && chars[s - 1] != '\n' {
        s -= 1;
    }
    let mut e = pos;
    while e < chars.len() && chars[e] != '\n' {
        e += 1;
    }
    (s, e)
}

/// Envuelve (o desenvuelve si ya está envuelto) la selección con marcadores.
pub fn wrap_selection(text: &str, sel: (usize, usize), left: &str, right: &str) -> Edit {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let (a, b) = ordered(sel);
    let (a, b) = (a.min(n), b.min(n));
    let l: Vec<char> = left.chars().collect();
    let r: Vec<char> = right.chars().collect();

    let surrounded = a >= l.len()
        && b + r.len() <= n
        && chars[a - l.len()..a] == l[..]
        && chars[b..b + r.len()] == r[..]
        && !(left == "*"
            && ((a >= 2 && chars[a - 2] == '*') || (b + 1 < n && chars[b + 1] == '*')));

    let mut out: Vec<char> = Vec::with_capacity(n + l.len() + r.len());
    if surrounded {
        out.extend(&chars[..a - l.len()]);
        out.extend(&chars[a..b]);
        out.extend(&chars[b + r.len()..]);
        Edit {
            text: out.into_iter().collect(),
            sel: (a - l.len(), b - l.len()),
        }
    } else {
        out.extend(&chars[..a]);
        out.extend(&l);
        out.extend(&chars[a..b]);
        out.extend(&r);
        out.extend(&chars[b..]);
        Edit {
            text: out.into_iter().collect(),
            sel: (a + l.len(), b + l.len()),
        }
    }
}

/// Nivel de título (0 = párrafo, 1..=3) de la línea del cursor.
pub fn heading_level(text: &str, cursor: usize) -> u8 {
    let chars: Vec<char> = text.chars().collect();
    let (s, e) = line_bounds(&chars, cursor);
    let line: String = chars[s..e].iter().collect();
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if (1..=3).contains(&hashes) && line.chars().nth(hashes) == Some(' ') {
        hashes as u8
    } else {
        0
    }
}

/// Convierte la línea actual en título (1), subtítulo (2) o párrafo (0).
pub fn set_heading(text: &str, cursor: usize, level: u8) -> Edit {
    let chars: Vec<char> = text.chars().collect();
    let (s, e) = line_bounds(&chars, cursor);
    let line: String = chars[s..e].iter().collect();
    let body = if line.starts_with('#') {
        let t = line.trim_start_matches('#');
        t.strip_prefix(' ').unwrap_or(t).to_owned()
    } else {
        line
    };
    let prefix = match level {
        1 => "# ",
        2 => "## ",
        _ => "",
    };
    let new_line = format!("{prefix}{body}");
    let new_cursor = s + new_line.chars().count();
    let mut out: String = chars[..s].iter().collect();
    out.push_str(&new_line);
    out.extend(chars[e..].iter());
    Edit {
        text: out,
        sel: (new_cursor, new_cursor),
    }
}

/// Sangría de primera línea con `&emsp;` sobre la línea actual.
pub fn indent(text: &str, cursor: usize, delta: i8) -> Option<Edit> {
    const EM: &str = "&emsp;";
    let chars: Vec<char> = text.chars().collect();
    let (s, e) = line_bounds(&chars, cursor);
    let line: String = chars[s..e].iter().collect();
    let new_line = if delta > 0 {
        if line.matches(EM).count() >= 8 && line.starts_with(EM) {
            return None;
        }
        format!("{EM}{line}")
    } else {
        line.strip_prefix(EM)?.to_owned()
    };
    let new_cursor = s + new_line.chars().count();
    let mut out: String = chars[..s].iter().collect();
    out.push_str(&new_line);
    out.extend(chars[e..].iter());
    Some(Edit {
        text: out,
        sel: (new_cursor, new_cursor),
    })
}

fn div_align(line: &str) -> Option<TextAlign> {
    let t = line.trim();
    let inner = t.strip_prefix("<div align=\"")?.strip_suffix("\">")?;
    TextAlign::parse(inner)
}

fn is_close(line: &str) -> bool {
    line.trim() == "</div>"
}

/// Alinea el párrafo actual envolviéndolo en `<div align="...">` (o desenvolviéndolo para izquierda).
pub fn align_paragraph(text: &str, cursor: usize, align: TextAlign) -> Option<Edit> {
    let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    let li = text
        .chars()
        .take(cursor)
        .filter(|c| *c == '\n')
        .count()
        .min(lines.len() - 1);

    // ¿Está dentro de un bloque <div align>?
    let mut start = None;
    for i in (0..=li).rev() {
        if div_align(&lines[i]).is_some() {
            start = Some(i);
            break;
        }
        if i < li && is_close(&lines[i]) {
            break;
        }
    }
    let mut enclosed = None;
    if let Some(st) = start {
        for j in st + 1..lines.len() {
            if is_close(&lines[j]) {
                if j >= li {
                    enclosed = Some((st, j));
                }
                break;
            }
            if div_align(&lines[j]).is_some() {
                break;
            }
        }
    }

    let last_content: usize;
    if let Some((st, en)) = enclosed {
        if align == TextAlign::Left {
            let mut inner: Vec<String> = lines[st + 1..en].to_vec();
            while inner.first().is_some_and(|l| l.trim().is_empty()) {
                inner.remove(0);
            }
            while inner.last().is_some_and(|l| l.trim().is_empty()) {
                inner.pop();
            }
            let count = inner.len();
            lines.splice(st..=en, inner);
            last_content = (st + count).saturating_sub(1);
        } else {
            lines[st] = format!("<div align=\"{}\">", align.tag());
            last_content = en.saturating_sub(1);
        }
    } else {
        if align == TextAlign::Left || lines[li].trim().is_empty() {
            return None;
        }
        let mut p0 = li;
        while p0 > 0 && !lines[p0 - 1].trim().is_empty() {
            p0 -= 1;
        }
        let mut p1 = li;
        while p1 + 1 < lines.len() && !lines[p1 + 1].trim().is_empty() {
            p1 += 1;
        }
        let mut block = vec![format!("<div align=\"{}\">", align.tag()), String::new()];
        block.extend(lines[p0..=p1].iter().cloned());
        block.push(String::new());
        block.push("</div>".to_owned());
        let inner_last = p0 + 2 + (p1 - p0);
        lines.splice(p0..=p1, block);
        last_content = inner_last;
    }

    let last_content = last_content.min(lines.len() - 1);
    let offset: usize = lines[..=last_content]
        .iter()
        .map(|l| l.chars().count() + 1)
        .sum::<usize>()
        - 1;
    Some(Edit {
        text: lines.join("\n"),
        sel: (offset, offset),
    })
}

// ---------------------------------------------------------------------------
// Vista previa
// ---------------------------------------------------------------------------

struct Style {
    font: FontChoice,
    size: f32,
    color: Color32,
}

/// Renderiza el Markdown con el estilo editorial del tema.
pub fn render_preview(ui: &mut Ui, text: &str, font: FontChoice, size: f32) {
    let text_color = ui.visuals().widgets.noninteractive.fg_stroke.color;
    let weak = ui.visuals().weak_text_color();
    let accent = ui.visuals().selection.bg_fill;
    let style = Style {
        font,
        size,
        color: text_color,
    };

    let mut align = TextAlign::Left;
    let mut para: Vec<&str> = Vec::new();
    let mut last_blank = true;

    fn flush(ui: &mut Ui, para: &mut Vec<&str>, st: &Style, align: TextAlign) {
        if para.is_empty() {
            return;
        }
        let joined = para.join("\n");
        para.clear();
        let job = inline_job(&joined, st, st.font, 1.0, false, st.color);
        draw_job(ui, job, align);
        ui.add_space(st.size * 0.55);
    }

    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() {
            flush(ui, &mut para, &style, align);
            if !last_blank {
                ui.add_space(style.size * 0.25);
            }
            last_blank = true;
            continue;
        }
        last_blank = false;

        if let Some(a) = div_align(t) {
            flush(ui, &mut para, &style, align);
            align = a;
            continue;
        }
        if is_close(t) {
            flush(ui, &mut para, &style, align);
            align = TextAlign::Left;
            continue;
        }
        let hashes = t.chars().take_while(|c| *c == '#').count();
        if (1..=3).contains(&hashes) && t.chars().nth(hashes) == Some(' ') {
            flush(ui, &mut para, &style, align);
            let scale = [2.0, 1.55, 1.3][hashes - 1];
            let body = t[hashes + 1..].trim();
            ui.add_space(style.size * 0.4);
            let job = inline_job(body, &style, FontChoice::Serif, scale, false, style.color);
            draw_job(ui, job, align);
            ui.add_space(style.size * 0.4);
            continue;
        }
        if t == "---" || t == "***" || t == "___" {
            flush(ui, &mut para, &style, align);
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);
            continue;
        }
        if let Some(rest) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            flush(ui, &mut para, &style, align);
            let job = inline_job(
                &format!("•  {rest}"),
                &style,
                style.font,
                1.0,
                false,
                style.color,
            );
            draw_job(ui, job, TextAlign::Left);
            ui.add_space(style.size * 0.2);
            continue;
        }
        if let Some(rest) = t.strip_prefix("> ") {
            flush(ui, &mut para, &style, align);
            let job = inline_job(&format!("▎ {rest}"), &style, style.font, 1.0, true, weak);
            let _ = accent;
            draw_job(ui, job, TextAlign::Left);
            ui.add_space(style.size * 0.4);
            continue;
        }
        para.push(line);
    }
    flush(ui, &mut para, &style, align);
}

fn draw_job(ui: &mut Ui, mut job: LayoutJob, align: TextAlign) {
    let width = ui.available_width();
    job.wrap.max_width = width;
    job.halign = match align {
        TextAlign::Left | TextAlign::Justify => Align::LEFT,
        TextAlign::Center => Align::Center,
        TextAlign::Right => Align::RIGHT,
    };
    job.justify = align == TextAlign::Justify;
    let galley = ui.painter().layout_job(job);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, galley.size().y), Sense::hover());
    let x = match align {
        TextAlign::Left | TextAlign::Justify => rect.left(),
        TextAlign::Center => rect.center().x,
        TextAlign::Right => rect.right(),
    };
    ui.painter()
        .galley(egui::pos2(x, rect.top()), galley, Color32::PLACEHOLDER);
}

/// Parseo inline: `**`, `*`, `<u>`, `~~`, `&emsp;`, `&nbsp;` y escapes con `\`.
fn inline_job(
    text: &str,
    st: &Style,
    font: FontChoice,
    scale: f32,
    italic_all: bool,
    color: Color32,
) -> LayoutJob {
    let chars: Vec<char> = text.chars().collect();
    let size = st.size * scale;
    let mut job = LayoutJob::default();
    let (mut bold, mut italic, mut under, mut strike) = (false, italic_all, false, false);
    let mut buf = String::new();

    let flush = |job: &mut LayoutJob,
                 buf: &mut String,
                 bold: bool,
                 italic: bool,
                 under: bool,
                 strike: bool| {
        if buf.is_empty() {
            return;
        }
        let fmt = TextFormat {
            font_id: FontId::new(size, font.family(bold)),
            color,
            italics: italic,
            underline: if under {
                Stroke::new(1.0, color)
            } else {
                Stroke::NONE
            },
            strikethrough: if strike {
                Stroke::new(1.0, color)
            } else {
                Stroke::NONE
            },
            ..Default::default()
        };
        job.append(buf, 0.0, fmt);
        buf.clear();
    };

    let starts = |i: usize, pat: &str| -> bool {
        pat.chars()
            .enumerate()
            .all(|(k, c)| chars.get(i + k) == Some(&c))
    };

    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            buf.push(chars[i + 1]);
            i += 2;
        } else if starts(i, "**") {
            flush(&mut job, &mut buf, bold, italic, under, strike);
            bold = !bold;
            i += 2;
        } else if starts(i, "~~") {
            flush(&mut job, &mut buf, bold, italic, under, strike);
            strike = !strike;
            i += 2;
        } else if starts(i, "<u>") {
            flush(&mut job, &mut buf, bold, italic, under, strike);
            under = true;
            i += 3;
        } else if starts(i, "</u>") {
            flush(&mut job, &mut buf, bold, italic, under, strike);
            under = false;
            i += 4;
        } else if starts(i, "&emsp;") {
            buf.push_str("    ");
            i += 6;
        } else if starts(i, "&nbsp;") {
            buf.push(' ');
            i += 6;
        } else if chars[i] == '*' {
            flush(&mut job, &mut buf, bold, italic, under, strike);
            italic = !italic;
            i += 1;
        } else {
            buf.push(chars[i]);
            i += 1;
        }
    }
    flush(&mut job, &mut buf, bold, italic, under, strike);
    if job.sections.is_empty() {
        job.append(
            " ",
            0.0,
            TextFormat {
                font_id: FontId::new(size, font.family(false)),
                color,
                ..Default::default()
            },
        );
    }
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_and_unwrap() {
        let e = wrap_selection("hola mundo", (5, 10), "**", "**");
        assert_eq!(e.text, "hola **mundo**");
        assert_eq!(e.sel, (7, 12));
        let e2 = wrap_selection(&e.text, e.sel, "**", "**");
        assert_eq!(e2.text, "hola mundo");
    }

    #[test]
    fn wrap_empty_puts_cursor_between() {
        let e = wrap_selection("ab", (1, 1), "*", "*");
        assert_eq!(e.text, "a**b");
        assert_eq!(e.sel, (2, 2));
    }

    #[test]
    fn heading_cycle() {
        let e = set_heading("Hola\nChau", 2, 1);
        assert_eq!(e.text, "# Hola\nChau");
        assert_eq!(heading_level(&e.text, 1), 1);
        let e = set_heading(&e.text, 1, 2);
        assert_eq!(e.text, "## Hola\nChau");
        let e = set_heading(&e.text, 1, 0);
        assert_eq!(e.text, "Hola\nChau");
    }

    #[test]
    fn align_wrap_change_unwrap() {
        let e = align_paragraph("uno\n\ndos\ntres\n\ncuatro", 6, TextAlign::Center).unwrap();
        assert_eq!(e.text, "uno\n\n<div align=\"center\">\n\ndos\ntres\n\n</div>\n\ncuatro");
        let cur = e.text.find("tres").unwrap();
        let e2 = align_paragraph(&e.text, e.text[..cur].chars().count(), TextAlign::Right).unwrap();
        assert!(e2.text.contains("<div align=\"right\">"));
        let e3 = align_paragraph(&e2.text, cur, TextAlign::Left).unwrap();
        assert_eq!(e3.text, "uno\n\ndos\ntres\n\ncuatro");
    }

    #[test]
    fn indent_roundtrip() {
        let e = indent("hola", 2, 1).unwrap();
        assert_eq!(e.text, "&emsp;hola");
        assert_eq!(indent(&e.text, 3, -1).unwrap().text, "hola");
        assert!(indent("hola", 0, -1).is_none());
    }
}
