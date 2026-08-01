//! Reducción de ruido con GTCRN (sherpa-onnx).
//!
//! Ojo con esto: en las pruebas **no siempre mejora**. Sobre ruido de fondo sintético
//! empeoró bastante el reconocimiento, y sobre la llamada telefónica real bajó la confianza
//! media. Por eso nunca se aplica a ciegas: o es un interruptor manual, o corre en la segunda
//! pasada solo para detectar en qué palabras las dos versiones no coinciden.

use anyhow::{Context, Result};
use sherpa_onnx::{
    OfflineSpeechDenoiser, OfflineSpeechDenoiserConfig, OfflineSpeechDenoiserGtcrnModelConfig,
    OfflineSpeechDenoiserModelConfig,
};

use crate::models::{need, Models};

pub struct Denoiser {
    inner: OfflineSpeechDenoiser,
}

impl Denoiser {
    pub fn new(models: &Models, threads: i32) -> Result<Self> {
        let model = need(&models.denoiser()).context("modelo de reducción de ruido")?;
        let cfg = OfflineSpeechDenoiserConfig {
            model: OfflineSpeechDenoiserModelConfig {
                gtcrn: OfflineSpeechDenoiserGtcrnModelConfig { model: Some(model) },
                num_threads: threads,
                ..Default::default()
            },
        };
        let inner =
            OfflineSpeechDenoiser::create(&cfg).context("no pude crear el reductor de ruido")?;
        Ok(Self { inner })
    }

    /// Limpia un tramo. Se trabaja tramo a tramo (no el archivo entero) para que el coste
    /// sea proporcional al habla real y no al silencio.
    pub fn run(&self, samples: &[f32], sample_rate: u32) -> Vec<f32> {
        if samples.is_empty() {
            return Vec::new();
        }
        self.inner
            .run(samples, sample_rate as i32)
            .samples
    }
}
