//! Traducción es→en con CTranslate2.
//!
//! La traducción se hace en una etapa aparte, sobre el texto ya reconocido, en vez de
//! pedirle al modelo de voz que traduzca directamente. No es una preferencia de diseño:
//! sobre el mismo conjunto de evaluación, traducir el texto gana por un margen amplio.
//!
//! | camino                                   | BLEU  | chrF  |
//! |------------------------------------------|-------|-------|
//! | Canary → NLLB-600M                       | 27.50 | 57.00 |
//! | Whisper large-v3 → NLLB-600M             | 26.44 | 56.94 |
//! | Parakeet → NLLB-600M                     | 25.94 | 55.65 |
//! | Canary → opus-mt                         | 25.43 | 56.59 |
//! | Parakeet → opus-mt                       | 23.87 | 55.11 |
//! | Whisper `translate` (la herramienta vieja)| 22.10 | 54.70 |
//! | Canary traduciendo directo (AST)         | 20.90 | 52.20 |

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use ct2rs::tokenizers::{hf, sentencepiece as sp};
use ct2rs::{ComputeType, Config, Device, TranslationOptions, Tokenizer as Ct2Tokenizer, Translator};

use crate::models::{MtModel, Models};

/// Etiqueta de idioma que usa NLLB-200.
const NLLB_SRC: &str = "spa_Latn";
const NLLB_TGT: &str = "eng_Latn";

/// Envuelve el tokenizador de NLLB para poner la etiqueta de idioma a mano.
///
/// El `tokenizer.json` que viene con el modelo trae fijada la etiqueta por defecto del
/// original (inglés), así que si se deja el post-procesador automático se le está diciendo
/// al modelo que el texto de entrada es inglés cuando es español.
struct NllbTokenizer {
    inner: hf::Tokenizer,
}

impl NllbTokenizer {
    fn new(dir: &Path) -> Result<Self> {
        let mut inner = hf::Tokenizer::new(dir)
            .with_context(|| format!("no pude leer tokenizer.json en {}", dir.display()))?;
        inner.disable_spacial_token();
        Ok(Self { inner })
    }
}

fn is_special(tok: &str) -> bool {
    if tok.starts_with('<') && tok.ends_with('>') {
        return true;
    }
    // etiquetas de idioma tipo `eng_Latn`
    let b = tok.as_bytes();
    b.len() == 8
        && b[3] == b'_'
        && b[..3]
            .iter()
            .all(|c| c.is_ascii_lowercase())
        && b[4].is_ascii_uppercase()
}

impl Ct2Tokenizer for NllbTokenizer {
    fn encode(&self, input: &str) -> Result<Vec<String>> {
        let mut v = self
            .inner
            .encode(input)?;
        v.insert(0, NLLB_SRC.to_string());
        v.push("</s>".to_string());
        Ok(v)
    }

    fn decode(&self, tokens: Vec<String>) -> Result<String> {
        self.inner
            .decode(
                tokens
                    .into_iter()
                    .filter(|t| !is_special(t))
                    .collect(),
            )
    }
}

enum Backend {
    Opus(Translator<sp::Tokenizer>),
    Nllb(Translator<NllbTokenizer>),
}

/// Opciones de decodificación. `no_repeat_ngram` es un seguro barato: con entradas
/// basura un traductor también puede engancharse repitiendo.
#[derive(Clone, Copy, Debug)]
pub struct MtOptions {
    pub beam_size: usize,
    pub max_output_tokens: usize,
    pub no_repeat_ngram: usize,
}

impl Default for MtOptions {
    fn default() -> Self {
        Self {
            beam_size: 4,
            max_output_tokens: 512,
            no_repeat_ngram: 6,
        }
    }
}

pub struct Translation {
    pub text: String,
    /// Puntuación del modelo; sirve para señalar frases donde la traducción fue dudosa.
    pub score: Option<f32>,
}

pub struct Mt {
    backend: Backend,
    opts: MtOptions,
}

impl Mt {
    pub fn new(models: &Models, model: MtModel, threads: usize, opts: MtOptions) -> Result<Self> {
        let dir = models.mt_dir(model);
        if !dir.is_dir() {
            return Err(anyhow!(
                "falta el modelo de traducción: {}",
                dir.display()
            ));
        }
        let cfg = Config {
            device: Device::CPU,
            compute_type: ComputeType::INT8,
            num_threads_per_replica: threads,
            ..Default::default()
        };
        let backend = match model {
            MtModel::Opus => Backend::Opus(
                Translator::with_tokenizer(&dir, sp::Tokenizer::new(&dir)?, &cfg)
                    .context("no pude cargar opus-mt")?,
            ),
            MtModel::Nllb => Backend::Nllb(
                Translator::with_tokenizer(&dir, NllbTokenizer::new(&dir)?, &cfg)
                    .context("no pude cargar NLLB")?,
            ),
        };
        Ok(Self { backend, opts })
    }

    fn options(&self) -> TranslationOptions<String, String> {
        TranslationOptions {
            beam_size: self
                .opts
                .beam_size,
            max_decoding_length: self
                .opts
                .max_output_tokens,
            no_repeat_ngram_size: self
                .opts
                .no_repeat_ngram,
            return_scores: true,
            ..Default::default()
        }
    }

    fn translate_flat(&self, texts: &[String]) -> Result<Vec<(String, Option<f32>)>> {
        let opts = self.options();
        match &self.backend {
            Backend::Opus(t) => t.translate_batch(texts, &opts, None),
            Backend::Nllb(t) => {
                let prefixes: Vec<Vec<String>> = vec![vec![NLLB_TGT.to_string()]; texts.len()];
                t.translate_batch_with_target_prefix(texts, &prefixes, &opts, None)
            }
        }
    }

    /// Traduce un lote de bloques al inglés, en el mismo orden.
    ///
    /// Cada bloque se parte en frases antes de traducir. No es un detalle cosmético:
    /// opus-mt está entrenado frase a frase y, con dos oraciones en la misma entrada,
    /// se come la segunda. En la llamada de prueba perdió tres frases enteras — para
    /// alguien que usa esto como borrador de trabajo, perder texto es lo peor que puede pasar.
    pub fn translate(&self, texts: &[String]) -> Result<Vec<Translation>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let mut flat: Vec<String> = Vec::new();
        let mut ranges: Vec<(usize, usize)> = Vec::with_capacity(texts.len());
        for t in texts {
            let start = flat.len();
            flat.extend(split_sentences(t, MAX_SENTENCE_CHARS));
            ranges.push((start, flat.len()));
        }

        let out = self.translate_flat(&flat)?;

        Ok(ranges
            .into_iter()
            .map(|(a, b)| {
                let parts = &out[a..b];
                let text = parts
                    .iter()
                    .map(|(t, _)| t.trim())
                    .filter(|t| !t.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                let scores: Vec<f32> = parts
                    .iter()
                    .filter_map(|(_, s)| *s)
                    .collect();
                let score = if scores.is_empty() {
                    None
                } else {
                    Some(
                        scores
                            .iter()
                            .sum::<f32>()
                            / scores.len() as f32,
                    )
                };
                Translation { text, score }
            })
            .collect())
    }
}

/// Por encima de esto la frase se parte igual, aunque no haya punto: los traductores
/// tienen un límite de longitud y prefieren cortar ellos antes que truncar.
const MAX_SENTENCE_CHARS: usize = 220;

/// Abreviaturas tras las que un punto no cierra frase.
const ABBREV: &[&str] = &[
    "sr", "sra", "srta", "dr", "dra", "lic", "ing", "ud", "uds", "etc", "av", "no", "num", "pag",
    "ej", "ee", "uu", "vs", "p", "art", "fig", "aprox", "min", "seg", "hrs",
];

/// Parte un bloque en frases.
pub fn split_sentences(text: &str, max_chars: usize) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    let chars: Vec<char> = text
        .chars()
        .collect();
    let mut out: Vec<String> = Vec::new();
    let mut start = 0usize;

    for i in 0..chars.len() {
        if !matches!(chars[i], '.' | '!' | '?' | '\u{2026}') {
            continue;
        }
        // Se salta la puntuación seguida (`?!`, `...`) para cortar una sola vez.
        if chars
            .get(i + 1)
            .is_some_and(|c| matches!(c, '.' | '!' | '?' | '\u{2026}' | '"' | '\''))
        {
            continue;
        }
        let Some(next) = chars[i + 1..]
            .iter()
            .find(|c| !c.is_whitespace())
        else {
            break; // puntuación final: el resto ya es la última frase
        };
        // Tiene que haber espacio después y empezar algo nuevo.
        if !chars
            .get(i + 1)
            .is_some_and(|c| c.is_whitespace())
        {
            continue;
        }
        if !(next.is_uppercase() || *next == '¿' || *next == '¡') {
            continue;
        }
        if is_abbrev(&chars[start..i]) {
            continue;
        }
        push_piece(&mut out, &chars[start..=i], max_chars);
        start = i + 1;
    }
    push_piece(&mut out, &chars[start..], max_chars);
    out
}

fn is_abbrev(before: &[char]) -> bool {
    let word: String = before
        .iter()
        .rev()
        .take_while(|c| c.is_alphanumeric())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let w = word.to_lowercase();
    if w.is_empty() {
        // Puntuación pegada a puntuación (`?!`): ahí sí termina la frase.
        return false;
    }
    // Una sola letra suele ser una inicial ("J. Pérez").
    let inicial = w
        .chars()
        .count()
        == 1
        && w.chars()
            .all(|c| c.is_alphabetic());
    inicial || ABBREV.contains(&w.as_str())
}

fn push_piece(out: &mut Vec<String>, piece: &[char], max_chars: usize) {
    let s: String = piece
        .iter()
        .collect();
    let s = s
        .trim()
        .to_string();
    if s.is_empty() {
        return;
    }
    if s.chars()
        .count()
        <= max_chars
    {
        out.push(s);
        return;
    }
    // Demasiado larga: se corta en la pausa más cercana disponible.
    for sep in [';', ',', ' '] {
        let mut acc = String::new();
        let mut chunks: Vec<String> = Vec::new();
        for part in s.split_inclusive(sep) {
            if !acc.is_empty()
                && acc
                    .chars()
                    .count()
                    + part
                        .chars()
                        .count()
                    > max_chars
            {
                chunks.push(
                    acc.trim()
                        .to_string(),
                );
                acc = String::new();
            }
            acc.push_str(part);
        }
        if !acc
            .trim()
            .is_empty()
        {
            chunks.push(
                acc.trim()
                    .to_string(),
            );
        }
        if chunks.len() > 1
            && chunks
                .iter()
                .all(|c| {
                    c.chars()
                        .count()
                        <= max_chars
                })
        {
            out.extend(chunks);
            return;
        }
    }
    out.push(s);
}

#[cfg(test)]
mod tests {
    use super::{is_special, split_sentences, MAX_SENTENCE_CHARS};

    fn split(t: &str) -> Vec<String> {
        split_sentences(t, MAX_SENTENCE_CHARS)
    }

    #[test]
    fn parte_donde_termina_la_frase() {
        // Este es el caso real que perdía texto con opus-mt.
        let v = split("si te llegan a decir que lo descuartizaron, no pasa nada. Tú sigue con tu vida");
        assert_eq!(
            v,
            vec![
                "si te llegan a decir que lo descuartizaron, no pasa nada.",
                "Tú sigue con tu vida"
            ]
        );
    }

    #[test]
    fn respeta_abreviaturas_y_decimales() {
        assert_eq!(split("Habló el Sr. Gómez ayer").len(), 1);
        assert_eq!(split("Costó 3.5 millones de pesos").len(), 1);
        assert_eq!(split("Vino J. Pérez a la audiencia").len(), 1);
    }

    #[test]
    fn no_parte_de_mas() {
        assert_eq!(split("¿Qué onda? ¿Cómo estás hijo?").len(), 2);
        assert_eq!(split("¿En serio?! No puede ser.").len(), 2);
        assert_eq!(split("Eso... eso no me lo esperaba").len(), 1);
    }

    #[test]
    fn corta_las_frases_larguisimas() {
        let larga = "hablamos de esto, y de aquello, y de lo otro, ".repeat(12);
        let v = split(&larga);
        assert!(v.len() > 1);
        assert!(v
            .iter()
            .all(|s| s.chars().count() <= MAX_SENTENCE_CHARS));
    }

    #[test]
    fn nada_se_pierde_al_partir() {
        let t = "Primero esto. Después lo otro. Y al final lo de más allá.";
        let v = split(t);
        let junto: String = v
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let orig: String = t
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(junto, orig);
    }

    #[test]
    fn reconoce_tokens_especiales() {
        assert!(is_special("</s>"));
        assert!(is_special("<unk>"));
        assert!(is_special("eng_Latn"));
        assert!(is_special("spa_Latn"));
        assert!(!is_special("hearing"));
        assert!(!is_special("▁audiencia"));
    }
}
