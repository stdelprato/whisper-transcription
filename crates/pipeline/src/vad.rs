//! Detección de actividad de voz (Silero VAD vía sherpa-onnx).
//!
//! El VAD corre **siempre sobre el audio crudo**, aunque después se denoisee: así los
//! límites de los segmentos son idénticos en las dos pasadas y se pueden comparar palabra
//! por palabra.

use anyhow::{Context, Result};
use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};

use crate::audio::Audio;
use crate::models::{need, Models};

/// Tramo de habla, en índices de muestra sobre el audio completo.
#[derive(Clone, Copy, Debug)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn len(&self) -> usize {
        self.end
            .saturating_sub(self.start)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Parámetros del VAD. Los valores por defecto están afinados para audio telefónico
/// de mala calidad: umbral algo bajo para no perder habla floja, y silencios de medio
/// segundo para cortar en pausas naturales.
#[derive(Clone, Copy, Debug)]
pub struct VadOptions {
    pub threshold: f32,
    pub min_silence: f32,
    pub min_speech: f32,
    /// Corte duro: ningún segmento supera esto (el ASR se degrada en tramos muy largos).
    pub max_speech: f32,
    /// Margen que se añade a cada lado para no comerse consonantes iniciales/finales.
    pub pad: f32,
}

impl Default for VadOptions {
    fn default() -> Self {
        Self {
            threshold: 0.45,
            min_silence: 0.5,
            min_speech: 0.2,
            max_speech: 20.0,
            pad: 0.15,
        }
    }
}

const WINDOW: usize = 512;

/// Divide el audio en tramos de habla.
pub fn detect(models: &Models, audio: &Audio, opts: &VadOptions, threads: i32) -> Result<Vec<Span>> {
    let model = need(&models.vad()).context("modelo de VAD")?;

    let cfg = VadModelConfig {
        silero_vad: SileroVadModelConfig {
            model: Some(model),
            threshold: opts.threshold,
            min_silence_duration: opts.min_silence,
            min_speech_duration: opts.min_speech,
            window_size: WINDOW as i32,
            max_speech_duration: opts.max_speech,
        },
        sample_rate: audio.sample_rate as i32,
        num_threads: threads,
        ..Default::default()
    };

    let vad = VoiceActivityDetector::create(&cfg, opts.max_speech + 5.0)
        .context("no pude crear el detector de voz")?;

    let mut spans = Vec::new();
    let drain = |vad: &VoiceActivityDetector, spans: &mut Vec<Span>| {
        while let Some(seg) = vad.front() {
            let start = seg.start() as usize;
            let n = seg.n() as usize;
            drop(seg);
            vad.pop();
            if n > 0 {
                spans.push(Span {
                    start,
                    end: start + n,
                });
            }
        }
    };

    for chunk in audio
        .samples
        .chunks(WINDOW)
    {
        vad.accept_waveform(chunk);
        drain(&vad, &mut spans);
    }
    vad.flush();
    drain(&vad, &mut spans);

    let pad = (opts.pad * audio.sample_rate as f32) as usize;
    let total = audio
        .samples
        .len();
    for s in &mut spans {
        s.start = s
            .start
            .saturating_sub(pad);
        s.end = (s.end + pad).min(total);
    }
    Ok(spans)
}

/// Si el VAD no encuentra nada (audio muy saturado o muy bajo), troceamos a ciegas
/// para no devolver una transcripción vacía.
pub fn fallback_chunks(audio: &Audio, chunk_secs: f32) -> Vec<Span> {
    let n = (chunk_secs * audio.sample_rate as f32) as usize;
    let total = audio
        .samples
        .len();
    (0..total)
        .step_by(n.max(1))
        .map(|start| Span {
            start,
            end: (start + n).min(total),
        })
        .filter(|s| !s.is_empty())
        .collect()
}
