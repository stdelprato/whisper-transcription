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

        let text = res
            .text
            .trim()
            .to_string();
        if text.is_empty() {
            return Ok(Utterance::default());
        }

        let dur = samples.len() as f32 / sample_rate as f32;
        match (&res.timestamps, &res.durations) {
            (Some(ts), Some(ds)) if ts.len() == res.tokens.len() && !ts.is_empty() => Ok(Utterance {
                words: words_from_tokens(&res.tokens, ts, ds),
                text,
                exact_timings: true,
            }),
            _ => Ok(Utterance {
                words: words_spread(&text, dur),
                text,
                exact_timings: false,
            }),
        }
    }

    pub fn model(&self) -> AsrModel {
        self.model
    }
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
