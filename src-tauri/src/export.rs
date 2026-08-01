//! Exportación del resultado a formatos que sirvan fuera de la app.
//!
//! Ella trabaja sobre una plantilla de Word, así que lo que más se usa es copiar bloques
//! al portapapeles. El archivo exportado es para archivar o para pasarlo entero.

use std::fmt::Write as _;

use pipeline::{Segment, Transcript};

/// Formatos disponibles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// Texto plano con marca de tiempo por bloque.
    Txt,
    /// Subtítulos, por si quiere seguir el audio en un reproductor.
    Srt,
    /// Rich Text: Word lo abre como un documento normal, con las dudas resaltadas.
    Rtf,
}

/// Qué texto se vuelca.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Content {
    /// Solo la traducción.
    Target,
    /// Solo el idioma original.
    Source,
    /// Los dos, uno debajo del otro.
    Both,
}

pub fn render(t: &Transcript, format: Format, content: Content, with_speakers: bool) -> String {
    match format {
        Format::Txt => txt(t, content, with_speakers),
        Format::Srt => srt(t, content),
        Format::Rtf => rtf(t, content, with_speakers),
    }
}

fn stamp(secs: f32) -> String {
    let s = secs.max(0.0) as u32;
    format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

fn srt_stamp(secs: f32) -> String {
    let ms = (secs.max(0.0) * 1000.0) as u32;
    format!(
        "{:02}:{:02}:{:02},{:03}",
        ms / 3_600_000,
        (ms / 60_000) % 60,
        (ms / 1000) % 60,
        ms % 1000
    )
}

fn speaker_label(seg: &Segment) -> String {
    seg.speaker
        .map(|k| format!("Hablante {}", k + 1))
        .unwrap_or_default()
}

/// Líneas a volcar para un bloque, en orden.
fn lines(seg: &Segment, content: Content) -> Vec<&str> {
    let target = seg
        .target
        .as_deref()
        .unwrap_or("");
    match content {
        Content::Target if !target.is_empty() => vec![target],
        Content::Target => vec![seg.source.as_str()],
        Content::Source => vec![seg.source.as_str()],
        Content::Both if target.is_empty() => vec![seg.source.as_str()],
        Content::Both => vec![target, seg.source.as_str()],
    }
}

fn txt(t: &Transcript, content: Content, with_speakers: bool) -> String {
    let mut out = String::new();
    for seg in &t.segments {
        let spk = speaker_label(seg);
        let head = if with_speakers && !spk.is_empty() {
            format!("[{}] {}", stamp(seg.start), spk)
        } else {
            format!("[{}]", stamp(seg.start))
        };
        let _ = writeln!(out, "{head}");
        for l in lines(seg, content) {
            let _ = writeln!(out, "{l}");
        }
        out.push('\n');
    }
    out
}

fn srt(t: &Transcript, content: Content) -> String {
    let mut out = String::new();
    for (i, seg) in t
        .segments
        .iter()
        .enumerate()
    {
        let _ = writeln!(out, "{}", i + 1);
        let _ = writeln!(
            out,
            "{} --> {}",
            srt_stamp(seg.start),
            srt_stamp(seg.end.max(seg.start + 0.4))
        );
        for l in lines(seg, content) {
            let _ = writeln!(out, "{l}");
        }
        out.push('\n');
    }
    out
}

/// Escapa a RTF: llaves, barra invertida y todo lo que no sea ASCII.
fn rtf_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            '\\' | '{' | '}' => {
                out.push('\\');
                out.push(c);
            }
            '\n' => out.push_str("\\line "),
            c if c.is_ascii() => out.push(c),
            c => {
                // RTF quiere enteros de 16 bits con signo; lo de fuera del plano básico va en pares.
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    let _ = write!(out, "\\u{}?", *u as i16);
                }
            }
        }
    }
    out
}

fn rtf(t: &Transcript, content: Content, with_speakers: bool) -> String {
    let mut out = String::from(
        "{\\rtf1\\ansi\\deff0{\\fonttbl{\\f0 Calibri;}}\
         {\\colortbl;\\red0\\green0\\blue0;\\red150\\green150\\blue150;\\red176\\green80\\blue0;}\
         \\fs22\n",
    );
    for seg in &t.segments {
        let spk = speaker_label(seg);
        let head = if with_speakers && !spk.is_empty() {
            format!("{}  {}", stamp(seg.start), spk)
        } else {
            stamp(seg.start)
        };
        let _ = write!(
            out,
            "{{\\cf2\\fs18 {}}}\\par\n",
            rtf_escape(&head)
        );

        let target = seg
            .target
            .as_deref()
            .unwrap_or("");
        let mostrar_ambos = matches!(content, Content::Both) && !target.is_empty();
        let principal = match content {
            Content::Source => seg.source.as_str(),
            _ if target.is_empty() => seg.source.as_str(),
            _ => target,
        };
        let _ = write!(out, "{}\\par\n", rtf_escape(principal));

        if mostrar_ambos {
            // El original va en gris debajo, que es como lo usa para verificar.
            let _ = write!(
                out,
                "{{\\cf2\\i {}}}\\par\n",
                rtf_escape(&seg.source)
            );
        }
        let dudosas: Vec<&str> = seg
            .words
            .iter()
            .filter(|w| w.low_conf)
            .map(|w| {
                w.text
                    .as_str()
            })
            .collect();
        if !dudosas.is_empty() {
            let _ = write!(
                out,
                "{{\\cf3\\fs16 revisar: {}}}\\par\n",
                rtf_escape(&dudosas.join(" "))
            );
        }
        out.push_str("\\par\n");
    }
    out.push('}');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapa_lo_que_rompe_rtf() {
        assert_eq!(rtf_escape("a{b}c\\d"), "a\\{b\\}c\\\\d");
        assert_eq!(rtf_escape("audiencia"), "audiencia");
        // Las tildes salen como escapes numéricos, no como bytes crudos.
        assert!(rtf_escape("¿Qué?").starts_with("\\u"));
        assert!(!rtf_escape("mañana").contains('ñ'));
    }

    #[test]
    fn las_marcas_de_tiempo_cuadran() {
        assert_eq!(stamp(0.0), "00:00:00");
        assert_eq!(stamp(3725.0), "01:02:05");
        assert_eq!(srt_stamp(3725.5), "01:02:05,500");
    }
}
