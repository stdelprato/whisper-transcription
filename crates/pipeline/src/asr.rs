//! Reconocimiento de voz con sherpa-onnx.
//!
//! Dos motores, elegidos tras medir sobre FLEURS es_419 degradado a banda telefónica y
//! sobre una llamada real:
//!
//! * **Parakeet TDT 0.6B v3** — transducer. RTF 0.15, timestamps por token. Al ser
//!   transducer no puede caer en el bucle de repetición que sufre un decodificador
//!   autorregresivo: en todas las pruebas el 4-grama más repetido apareció como mucho 3 veces.
//! * **Canary 1B v2** — encoder-decoder. RTF 0.41, mejor texto (traducido al inglés da
//!   +1.6 BLEU sobre Parakeet), pero no da timestamps por token.

use anyhow::{Context, Result};
use sherpa_onnx::{
    OfflineCanaryModelConfig, OfflineRecognizer, OfflineRecognizerConfig,
    OfflineTransducerModelConfig,
};

use crate::models::{need, AsrModel, Models};

/// Una palabra reconstruida a partir de los tokens del modelo, con sus tiempos
/// relativos al inicio del tramo.
#[derive(Clone, Debug)]
pub struct RawWord {
    pub text: String,
    pub start: f32,
    pub end: f32,
}

/// Resultado de reconocer un tramo.
#[derive(Clone, Debug, Default)]
pub struct Utterance {
    pub text: String,
    pub words: Vec<RawWord>,
    /// `false` cuando los tiempos se estimaron repartiendo el tramo (Canary).
    pub exact_timings: bool,
    /// El modelo se fue por las ramas en este tramo. Se juzga sobre la salida cruda,
    /// antes de limpiarla, porque después de limpiar parece un tramo corto cualquiera.
    pub degenerate: bool,
}

pub struct Recognizer {
    inner: OfflineRecognizer,
    model: AsrModel,
}

impl Recognizer {
    pub fn new(models: &Models, model: AsrModel, threads: i32, src_lang: &str) -> Result<Self> {
        let dir = models.asr_dir(model);
        let mut cfg = OfflineRecognizerConfig::default();

        match model {
            AsrModel::Parakeet => {
                cfg.model_config
                    .transducer = OfflineTransducerModelConfig {
                    encoder: Some(need(&dir.join("encoder.int8.onnx"))?),
                    decoder: Some(need(&dir.join("decoder.int8.onnx"))?),
                    joiner: Some(need(&dir.join("joiner.int8.onnx"))?),
                };
                cfg.model_config
                    .model_type = Some("nemo_transducer".into());
            }
            AsrModel::Canary => {
                cfg.model_config
                    .canary = OfflineCanaryModelConfig {
                    encoder: Some(need(&dir.join("encoder.int8.onnx"))?),
                    decoder: Some(need(&dir.join("decoder.int8.onnx"))?),
                    src_lang: Some(src_lang.to_string()),
                    // Transcribimos en el idioma de origen: la traducción la hace después
                    // un modelo de texto dedicado, que mide mejor que la traducción directa.
                    tgt_lang: Some(src_lang.to_string()),
                    use_pnc: true,
                };
            }
        }

        cfg.model_config
            .tokens = Some(need(&dir.join("tokens.txt"))?);
        cfg.model_config
            .num_threads = threads;

        let inner = OfflineRecognizer::create(&cfg)
            .with_context(|| format!("no pude cargar el modelo de {}", dir.display()))?;
        Ok(Self { inner, model })
    }

    /// Reconoce un tramo de audio. Los tiempos devueltos son relativos al tramo.
    pub fn run(&self, samples: &[f32], sample_rate: u32) -> Result<Utterance> {
        if samples.is_empty() {
            return Ok(Utterance::default());
        }
        let stream = self
            .inner
            .create_stream();
        stream.accept_waveform(sample_rate as i32, samples);
        self.inner
            .decode(&stream);
        let Some(res) = stream.get_result() else {
            return Ok(Utterance::default());
        };

        let crudo = res
            .text
            .trim()
            .to_string();
        if crudo.is_empty() {
            return Ok(Utterance::default());
        }
        let dur = samples.len() as f32 / sample_rate as f32;
        // La degeneración se juzga sobre el texto crudo: si el modelo se fue por las ramas,
        // hay que saberlo antes de limpiarlo, porque después parece un tramo corto y normal.
        let degenerate = crate::confidence::is_degenerate(&crudo, dur);

        // Fuera los tokens de control. El export de Canary a veces los escupe como texto.
        let (tokens, ts, ds) = strip_control(&res);
        let text = if tokens.is_empty() {
            clean_text(&crudo)
        } else {
            text_from_tokens(&tokens)
        };
        if text
            .trim()
            .is_empty()
        {
            return Ok(Utterance {
                degenerate,
                ..Default::default()
            });
        }

        match (ts, ds) {
            (Some(ts), Some(ds)) if ts.len() == tokens.len() && !ts.is_empty() => Ok(Utterance {
                words: words_from_tokens(&tokens, &ts, &ds),
                text,
                exact_timings: true,
                degenerate,
            }),
            _ => Ok(Utterance {
                words: words_spread(&text, dur),
                text,
                exact_timings: false,
                degenerate,
            }),
        }
    }

    pub fn model(&self) -> AsrModel {
        self.model
    }
}

/// ¿Es un token de control y no habla? `<|startofcontext|>`, `<|pnc|>`, `<unk>`...
fn es_control(tok: &str) -> bool {
    let t = tok.trim();
    (t.starts_with("<|") && t.ends_with("|>"))
        || matches!(t, "<unk>" | "<pad>" | "<s>" | "</s>" | "<blank>")
}

/// Saca los tokens de control y sus tiempos correspondientes.
///
/// El export de Canary a sherpa filtra sus marcas internas al texto, a veces cientos
/// seguidas y sin espacios entre ellas. Sin esto acaban en la transcripción tal cual.
fn strip_control(
    res: &sherpa_onnx::OfflineRecognizerResult,
) -> (Vec<String>, Option<Vec<f32>>, Option<Vec<f32>>) {
    let alineado = |v: &Option<Vec<f32>>| {
        v.as_ref()
            .filter(|x| {
                x.len()
                    == res
                        .tokens
                        .len()
            })
            .cloned()
    };
    let ts = alineado(&res.timestamps);
    let ds = alineado(&res.durations);

    let mut tokens = Vec::with_capacity(
        res.tokens
            .len(),
    );
    let (mut nts, mut nds) = (Vec::new(), Vec::new());
    for (i, tok) in res
        .tokens
        .iter()
        .enumerate()
    {
        if es_control(tok) {
            continue;
        }
        tokens.push(tok.clone());
        if let Some(v) = &ts {
            nts.push(v[i]);
        }
        if let Some(v) = &ds {
            nds.push(v[i]);
        }
    }
    (
        tokens,
        ts.map(|_| nts),
        ds.map(|_| nds),
    )
}

/// Quita las marcas `<|...|>` de un texto suelto, cuando no hay tokens con los que rehacerlo.
fn clean_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut resto = text;
    while let Some(a) = resto.find("<|") {
        out.push_str(&resto[..a]);
        match resto[a..].find("|>") {
            Some(b) => resto = &resto[a + b + 2..],
            None => {
                resto = "";
                break;
            }
        }
    }
    out.push_str(resto);
    out.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Rehace el texto a partir de los tokens ya limpios.
fn text_from_tokens(tokens: &[String]) -> String {
    let mut out = String::new();
    for tok in tokens {
        let abre = tok.starts_with(' ') || tok.starts_with('\u{2581}');
        let pieza = tok.replace('\u{2581}', " ");
        let pieza = pieza.trim_start();
        if pieza.is_empty() {
            continue;
        }
        if abre && !out.is_empty() {
            out.push(' ');
        }
        out.push_str(pieza);
    }
    out.trim()
        .to_string()
}

/// Une los tokens sub-palabra en palabras. Un token que empieza con espacio (Parakeet)
/// o con `▁` (sentencepiece) abre palabra nueva.
fn words_from_tokens(tokens: &[String], ts: &[f32], ds: &[f32]) -> Vec<RawWord> {
    let mut out: Vec<RawWord> = Vec::new();
    for (i, tok) in tokens
        .iter()
        .enumerate()
    {
        let starts_word = tok.starts_with(' ') || tok.starts_with('\u{2581}');
        let piece = tok.replace('\u{2581}', " ");
        let piece = piece.trim_start();
        let start = ts
            .get(i)
            .copied()
            .unwrap_or(0.0);
        let end = start
            + ds.get(i)
                .copied()
                .unwrap_or(0.0);

        match out.last_mut() {
            Some(w) if !starts_word => {
                w.text
                    .push_str(piece);
                w.end = end.max(w.end);
            }
            _ => out.push(RawWord {
                text: piece.to_string(),
                start,
                end,
            }),
        }
    }
    out.retain(|w| {
        !w.text
            .trim()
            .is_empty()
    });
    out
}

/// Reparte el tramo entre las palabras en proporción a su longitud. Es una aproximación;
/// la interfaz lo señala para no dar por exacto un tiempo que no lo es.
fn words_spread(text: &str, dur: f32) -> Vec<RawWord> {
    let parts: Vec<&str> = text
        .split_whitespace()
        .collect();
    if parts.is_empty() {
        return Vec::new();
    }
    let total: usize = parts
        .iter()
        .map(|p| p.chars().count() + 1)
        .sum();
    let mut t = 0.0;
    parts
        .iter()
        .map(|p| {
            let share = dur * (p.chars().count() + 1) as f32 / total as f32;
            let w = RawWord {
                text: (*p).to_string(),
                start: t,
                end: t + share,
            };
            t += share;
            w
        })
        .collect()
}
