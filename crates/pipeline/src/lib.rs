//! Núcleo de transcripción y traducción, todo local y sin red.
//!
//! El recorrido de un archivo es siempre el mismo:
//!
//! ```text
//! audio → ffmpeg → 16 kHz mono → VAD ─┬→ ASR principal ──┐
//!                                     ├→ ASR de contraste ┤→ acuerdo + tiempos → traducción
//!                                     └→ diarización ─────┘
//! ```
//!
//! Los modelos nunca conviven en memoria: se carga uno, se usa para todos los tramos,
//! se libera, y recién ahí se carga el siguiente. En una máquina con 4 GB libres eso es
//! la diferencia entre trabajar y tirar de disco.

pub mod asr;
pub mod audio;
pub mod confidence;
pub mod denoise;
pub mod diarize;
pub mod models;
pub mod translate;
pub mod vad;

use std::path::Path;
use std::time::Instant;

use anyhow::Result;
use serde::{Deserialize, Serialize};

pub use models::{AsrModel, Models, MtModel};

use crate::asr::{RawWord, Recognizer, Utterance};
use crate::audio::Audio;
use crate::confidence::LOOP_LIMIT;
use crate::denoise::Denoiser;
use crate::diarize::{DiarOptions, SpeakerSpan};
use crate::translate::{Mt, MtOptions};
use crate::vad::{Span, VadOptions};

/// Una palabra con su tiempo absoluto dentro del archivo.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Word {
    pub text: String,
    pub start: f32,
    pub end: f32,
    /// El otro modelo no dijo esto mismo.
    #[serde(default)]
    pub low_conf: bool,
}

/// Un bloque de habla: la unidad con la que se trabaja. Se lee, se corrige y se copia entero.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Segment {
    pub id: usize,
    pub start: f32,
    pub end: f32,
    /// Índice de hablante, si se pidió diarización.
    pub speaker: Option<i32>,
    /// Texto en el idioma del audio.
    pub source: String,
    /// Traducción al inglés. `None` si el audio ya venía en inglés.
    pub target: Option<String>,
    pub words: Vec<Word>,
    /// `false` cuando los tiempos por palabra son estimados y no medidos.
    pub exact_timings: bool,
    /// Lo que oyó el segundo modelo, para poder compararlo a ojo.
    pub alt_source: Option<String>,
    /// Cuánto coincidieron los dos modelos, de 0 a 1.
    pub agreement: Option<f32>,
    pub mt_score: Option<f32>,
    /// El tramo trae repetición patológica.
    pub degenerate: bool,
    /// El modelo principal se enganchó repitiendo y este bloque viene del de contraste.
    #[serde(default)]
    pub repaired: bool,
}

/// Cuánto tardó cada etapa. Sirve para dar tiempos honestos en la interfaz y para
/// saber dónde se va el tiempo cuando algo va lento.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Timings {
    pub decode: f32,
    pub vad: f32,
    pub denoise: f32,
    pub diarize: f32,
    pub asr: f32,
    pub verify: f32,
    pub translate: f32,
    pub total: f32,
}

/// Resultado completo de procesar un archivo.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transcript {
    pub file: String,
    pub duration: f32,
    pub source_lang: String,
    pub asr_model: AsrModel,
    pub verified_with: Option<AsrModel>,
    pub mt_model: Option<MtModel>,
    pub denoised: bool,
    pub segments: Vec<Segment>,
    pub num_speakers: usize,
    /// Nombres puestos a mano, por índice de hablante. Vacío = "Hablante N".
    #[serde(default)]
    pub speaker_names: Vec<String>,
    pub timings: Timings,
    /// Se interrumpió a mitad; lo que hay es parcial pero utilizable.
    #[serde(default)]
    pub cancelled: bool,
}

impl Transcript {
    /// Factor de tiempo real: menos de 1 significa más rápido que escuchar el audio.
    pub fn rtf(&self) -> f32 {
        if self.duration > 0.0 {
            self.timings
                .total
                / self.duration
        } else {
            0.0
        }
    }
}

/// Avisos de progreso para la interfaz.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Progress {
    Decoded {
        duration: f32,
    },
    Segmented {
        count: usize,
    },
    Diarized {
        speakers: usize,
    },
    /// Bloque reconocido por el modelo principal. Todavía sin contrastar ni traducir.
    Recognized {
        done: usize,
        total: usize,
        segment: Box<Segment>,
    },
    Verifying {
        total: usize,
    },
    /// El segundo modelo ya contrastó este bloque.
    Refined {
        done: usize,
        total: usize,
        segment: Box<Segment>,
    },
    Translating {
        total: usize,
    },
    Translated {
        id: usize,
        text: String,
        score: Option<f32>,
    },
    Done,
}

/// Lo que la interfaz responde a cada aviso de progreso.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    Continue,
    /// Cortar acá y devolver lo que haya. Procesar una hora de audio lleva su rato;
    /// tiene que poder pararse sin perder lo ya hecho.
    Abort,
}

/// Envoltorio del callback que recuerda si ya se pidió cancelar.
struct Emit<'a> {
    inner: &'a mut dyn FnMut(Progress) -> Flow,
    aborted: bool,
}

impl Emit<'_> {
    /// Emite un aviso. Devuelve `false` si hay que parar.
    fn send(&mut self, p: Progress) -> bool {
        if self.aborted {
            return false;
        }
        if (self.inner)(p) == Flow::Abort {
            self.aborted = true;
        }
        !self.aborted
    }
}

#[derive(Clone, Debug)]
pub struct Options {
    /// Modelo que produce el texto que se muestra.
    pub asr: AsrModel,
    /// Contrastar con el otro modelo para marcar dudas y afinar los tiempos.
    pub verify: bool,
    /// `None` cuando el audio ya está en inglés y solo hay que transcribir.
    pub mt: Option<MtModel>,
    /// Limpiar el audio antes de reconocer. Interruptor manual a propósito:
    /// en las pruebas ayudó en unas grabaciones y arruinó otras.
    pub denoise: bool,
    pub diarize: bool,
    /// Número de hablantes si se conoce. `None` lo deduce el propio diarizador.
    pub speakers: Option<i32>,
    pub threads: i32,
    /// Idioma del audio: `es`, `en`, ...
    pub source_lang: String,
    pub vad: VadOptions,
    pub diar: DiarOptions,
    pub mt_opts: MtOptions,
    /// Bloques traducidos por lote.
    pub mt_batch: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            asr: AsrModel::Canary,
            verify: true,
            // opus-mt y no NLLB a propósito. NLLB traduce mejor (27,50 BLEU contra 25,43),
            // pero tiene ocho veces más parámetros y en esta máquina la etapa de traducción
            // pasa de 36 a 282 segundos: el archivo entero tarda cuatro veces más. Para un
            // borrador que después se corrige a mano, no compensa. Queda a un clic de
            // distancia en el preset de máxima calidad.
            mt: Some(MtModel::Opus),
            denoise: false,
            diarize: true,
            speakers: None,
            threads: default_threads(),
            source_lang: "es".into(),
            vad: VadOptions::default(),
            diar: DiarOptions::default(),
            mt_opts: MtOptions::default(),
            mt_batch: 8,
        }
    }
}

impl Options {
    /// Un solo modelo y el traductor chico, para tener algo legible cuanto antes.
    /// Medido en esta máquina: RTF 0,61.
    pub fn fast() -> Self {
        Self {
            asr: AsrModel::Parakeet,
            verify: false,
            mt: Some(MtModel::Opus),
            ..Default::default()
        }
    }

    /// El mejor inglés posible, a costa de tardar unas cuatro veces más. RTF 2,65.
    pub fn best() -> Self {
        Self {
            asr: AsrModel::Canary,
            verify: true,
            mt: Some(MtModel::Nllb),
            ..Default::default()
        }
    }

    /// Solo transcribir, sin traducir (el audio ya está en inglés).
    pub fn transcribe_only(lang: &str) -> Self {
        Self {
            mt: None,
            source_lang: lang.to_string(),
            ..Default::default()
        }
    }

    /// El modelo que hace de segunda opinión: siempre el que no es el principal.
    pub fn verifier(&self) -> Option<AsrModel> {
        if !self.verify {
            return None;
        }
        Some(match self.asr {
            AsrModel::Canary => AsrModel::Parakeet,
            AsrModel::Parakeet => AsrModel::Canary,
        })
    }
}

pub fn default_threads() -> i32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as i32)
        .unwrap_or(4)
        .clamp(1, 8)
}

/// Procesa un archivo de principio a fin.
///
/// `on` recibe cada aviso de progreso y decide si seguir. Si devuelve [`Flow::Abort`],
/// el proceso se detiene en el siguiente punto seguro y se devuelve lo hecho hasta ahí
/// marcado como parcial.
pub fn run<P: AsRef<Path>>(
    models: &Models,
    path: P,
    opts: &Options,
    on: &mut dyn FnMut(Progress) -> Flow,
) -> Result<Transcript> {
    let path = path.as_ref();
    let t_all = Instant::now();
    let mut timings = Timings::default();
    let mut em = Emit {
        inner: on,
        aborted: false,
    };

    let t = Instant::now();
    let audio = audio::decode(path)?;
    timings.decode = t
        .elapsed()
        .as_secs_f32();
    em.send(Progress::Decoded {
        duration: audio.duration(),
    });

    let t = Instant::now();
    let mut spans = vad::detect(models, &audio, &opts.vad, opts.threads)?;
    if spans.is_empty() {
        spans = vad::fallback_chunks(&audio, opts.vad.max_speech);
    }
    timings.vad = t
        .elapsed()
        .as_secs_f32();
    em.send(Progress::Segmented { count: spans.len() });

    // Si se pidió limpiar el audio, se hace una sola vez y solo sobre los tramos con habla:
    // las dos pasadas de reconocimiento comparten el mismo material.
    let t = Instant::now();
    let cleaned: Option<Vec<Vec<f32>>> = if opts.denoise {
        let d = Denoiser::new(models, opts.threads)?;
        Some(
            spans
                .iter()
                .map(|s| d.run(&audio.samples[s.start..s.end], audio.sample_rate))
                .collect(),
        )
    } else {
        None
    };
    timings.denoise = t
        .elapsed()
        .as_secs_f32();

    let t = Instant::now();
    let speaker_spans = if opts.diarize && !em.aborted {
        let s = diarize::diarize(models, &audio, &diar_options(opts), opts.threads)?;
        em.send(Progress::Diarized {
            speakers: distinct_speakers(&s),
        });
        s
    } else {
        Vec::new()
    };
    timings.diarize = t
        .elapsed()
        .as_secs_f32();

    // --- pasada principal
    let t = Instant::now();
    let mut segments = transcribe_pass(
        models,
        opts.asr,
        opts,
        &audio,
        &spans,
        cleaned.as_deref(),
        &speaker_spans,
        &mut em,
    )?;
    timings.asr = t
        .elapsed()
        .as_secs_f32();

    // --- segunda opinión
    if let Some(verifier) = opts.verifier() {
        if !em.aborted {
            let t = Instant::now();
            em.send(Progress::Verifying {
                total: segments.len(),
            });
            let rec = Recognizer::new(models, verifier, opts.threads, &opts.source_lang)?;
            let total = segments.len();
            for (done, seg) in segments
                .iter_mut()
                .enumerate()
            {
                let span = spans[seg.id];
                let input = match &cleaned {
                    Some(c) => &c[seg.id][..],
                    None => &audio.samples[span.start..span.end],
                };
                let other = rec.run(input, audio.sample_rate)?;
                merge_verification(seg, other, verifier.has_word_timings(), audio.secs(span.start));
                if !em.send(Progress::Refined {
                    done: done + 1,
                    total,
                    segment: Box::new(seg.clone()),
                }) {
                    break;
                }
            }
            drop(rec);
            timings.verify = t
                .elapsed()
                .as_secs_f32();
        }
    }

    // --- traducción
    if let Some(mt_model) = opts.mt {
        if !em.aborted {
            let t = Instant::now();
            translate_all(models, mt_model, &mut segments, opts, &mut em)?;
            timings.translate = t
                .elapsed()
                .as_secs_f32();
        }
    }

    timings.total = t_all
        .elapsed()
        .as_secs_f32();
    let cancelled = em.aborted;
    em.aborted = false;
    em.send(Progress::Done);

    let num_speakers = segments
        .iter()
        .filter_map(|s| s.speaker)
        .collect::<std::collections::BTreeSet<_>>()
        .len();

    Ok(Transcript {
        file: path
            .display()
            .to_string(),
        duration: audio.duration(),
        source_lang: opts
            .source_lang
            .clone(),
        asr_model: opts.asr,
        verified_with: opts.verifier(),
        mt_model: opts.mt,
        denoised: opts.denoise,
        segments,
        num_speakers,
        speaker_names: Vec::new(),
        timings,
        cancelled,
    })
}

/// Cómo llamar a un hablante: el nombre que le pusieron, o "Hablante N".
pub fn speaker_label(names: &[String], speaker: i32) -> String {
    names
        .get(speaker.max(0) as usize)
        .filter(|n| !n.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| format!("Hablante {}", speaker + 1))
}

fn diar_options(opts: &Options) -> DiarOptions {
    let mut d = opts.diar;
    d.num_speakers = opts.speakers;
    d
}

/// Vuelve a separar hablantes sobre un resultado ya hecho, sin repetir el reconocimiento.
///
/// La separación es una etapa aparte del reconocimiento, así que cambiar cuántas personas
/// hay en la llamada cuesta medio minuto en vez de reprocesar el audio entero. Casi todas
/// las llamadas son de dos personas, pero cuando son tres o cuatro conviene poder decirlo
/// y ver si separa bien sin volver a empezar.
pub fn rediarize(
    models: &Models,
    transcript: &mut Transcript,
    speakers: Option<i32>,
    diar: DiarOptions,
    threads: i32,
) -> Result<usize> {
    let audio = audio::decode(&transcript.file)?;
    let mut opts = diar;
    opts.num_speakers = speakers;
    let spans = diarize::diarize(models, &audio, &opts, threads)?;
    for seg in &mut transcript.segments {
        seg.speaker = diarize::speaker_at(&spans, seg.start, seg.end);
    }
    transcript.num_speakers = transcript
        .segments
        .iter()
        .filter_map(|s| s.speaker)
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    Ok(transcript.num_speakers)
}

fn distinct_speakers(spans: &[SpeakerSpan]) -> usize {
    spans
        .iter()
        .map(|s| s.speaker)
        .collect::<std::collections::BTreeSet<_>>()
        .len()
}

/// Reconoce todos los tramos con un modelo y libera el modelo al terminar.
#[allow(clippy::too_many_arguments)]
fn transcribe_pass(
    models: &Models,
    model: AsrModel,
    opts: &Options,
    audio: &Audio,
    spans: &[Span],
    cleaned: Option<&[Vec<f32>]>,
    speaker_spans: &[SpeakerSpan],
    em: &mut Emit<'_>,
) -> Result<Vec<Segment>> {
    if em.aborted {
        return Ok(Vec::new());
    }
    let rec = Recognizer::new(models, model, opts.threads, &opts.source_lang)?;
    let mut segments = Vec::with_capacity(spans.len());

    for (i, span) in spans
        .iter()
        .enumerate()
    {
        let input = match cleaned {
            Some(c) => &c[i][..],
            None => &audio.samples[span.start..span.end],
        };
        let mut utt = rec.run(input, audio.sample_rate)?;
        if utt
            .text
            .is_empty()
        {
            continue;
        }
        let start = audio.secs(span.start);
        let end = audio.secs(span.end);

        let words = std::mem::take(&mut utt.words)
            .into_iter()
            .map(|w| Word {
                text: w.text,
                start: start + w.start,
                end: start + w.end,
                low_conf: false,
            })
            .collect();

        let segment = Segment {
            id: i,
            start,
            end,
            speaker: diarize::speaker_at(speaker_spans, start, end),
            degenerate: confidence::loop_score(&utt.text, 4) >= LOOP_LIMIT,
            source: utt.text,
            target: None,
            words,
            exact_timings: utt.exact_timings,
            alt_source: None,
            agreement: None,
            mt_score: None,
            repaired: false,
        };
        let seguir = em.send(Progress::Recognized {
            done: i + 1,
            total: spans.len(),
            segment: Box::new(segment.clone()),
        });
        segments.push(segment);
        if !seguir {
            break;
        }
    }

    drop(rec);
    Ok(segments)
}

/// Cruza el bloque con lo que oyó el segundo modelo.
///
/// Hace tres cosas: marca las palabras en las que los dos no coinciden, le presta al
/// principal los tiempos medidos si él no los tiene, y —lo más importante— rescata el
/// bloque cuando el principal se engancha repitiendo.
///
/// Ese último caso no es teórico. Canary, que es encoder-decoder, repitió "no" setenta y
/// cuatro veces seguidas en un tramo de cinco segundos de la llamada de prueba: la misma
/// avería que hacía inservible la herramienta anterior. Parakeet es un transducer y no
/// puede caer en eso, así que cuando el principal degenera y el de contraste no, se usa
/// el del contraste y se deja constancia.
fn merge_verification(
    seg: &mut Segment,
    other: Utterance,
    other_has_timings: bool,
    span_start: f32,
) {
    if other
        .text
        .trim()
        .is_empty()
    {
        // Sin texto no hay opinión que contrastar; mejor no marcar nada que marcarlo todo.
        seg.alt_source = Some(String::new());
        return;
    }

    let reference: Vec<RawWord> = other
        .words
        .iter()
        .map(|w| RawWord {
            text: w
                .text
                .clone(),
            start: span_start + w.start,
            end: span_start + w.end,
        })
        .collect();

    let mut primary: Vec<RawWord> = seg
        .words
        .iter()
        .map(|w| RawWord {
            text: w
                .text
                .clone(),
            start: w.start,
            end: w.end,
        })
        .collect();

    let cmp = confidence::compare(&primary, &reference);
    seg.agreement = Some(cmp.agreement);

    let other_degenerate = confidence::loop_score(&other.text, 4) >= LOOP_LIMIT;
    if seg.degenerate && !other_degenerate {
        let roto = std::mem::replace(&mut seg.source, other.text);
        seg.alt_source = Some(roto);
        seg.words = reference
            .into_iter()
            .map(|w| Word {
                text: w.text,
                start: w.start,
                end: w.end,
                low_conf: false,
            })
            .collect();
        seg.exact_timings = other.exact_timings;
        seg.degenerate = false;
        seg.repaired = true;
        return;
    }

    if other_has_timings && other.exact_timings && !seg.exact_timings {
        confidence::transfer_timings(&mut primary, &reference, &cmp.pairs, seg.start, seg.end);
        seg.exact_timings = true;
    }

    for ((w, src), low) in seg
        .words
        .iter_mut()
        .zip(primary)
        .zip(
            cmp.low_conf
                .into_iter()
                .chain(std::iter::repeat(false)),
        )
    {
        w.start = src.start;
        w.end = src.end;
        w.low_conf = low;
    }

    seg.alt_source = Some(other.text);
}

fn translate_all(
    models: &Models,
    model: MtModel,
    segments: &mut [Segment],
    opts: &Options,
    em: &mut Emit<'_>,
) -> Result<()> {
    if segments.is_empty() {
        return Ok(());
    }
    em.send(Progress::Translating {
        total: segments.len(),
    });
    let mt = Mt::new(models, model, opts.threads as usize, opts.mt_opts)?;

    for chunk in segments.chunks_mut(
        opts.mt_batch
            .max(1),
    ) {
        let texts: Vec<String> = chunk
            .iter()
            .map(|s| {
                s.source
                    .clone()
            })
            .collect();
        let out = mt.translate(&texts)?;
        let mut seguir = true;
        for (seg, tr) in chunk
            .iter_mut()
            .zip(out)
        {
            seguir = em.send(Progress::Translated {
                id: seg.id,
                text: tr
                    .text
                    .clone(),
                score: tr.score,
            });
            seg.target = Some(tr.text);
            seg.mt_score = tr.score;
        }
        if !seguir {
            break;
        }
    }
    Ok(())
}
