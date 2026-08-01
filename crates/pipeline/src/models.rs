//! Resolución de rutas de modelos y presets.
//!
//! Todo vive bajo un único directorio raíz (`models/`) para que el instalador
//! pueda copiar/descargar una sola carpeta y la app la encuentre sin configurar nada.

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

/// Modelo de reconocimiento de voz.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AsrModel {
    /// NVIDIA Parakeet TDT 0.6B v3. Transducer: rápido y sin bucles de alucinación.
    /// Da timestamps por token, o sea karaoke exacto.
    Parakeet,
    /// NVIDIA Canary 1B v2. Encoder-decoder: mejor calidad de texto, ~2.7x más lento,
    /// y no emite timestamps por token.
    Canary,
}

/// Modelo de traducción es→en.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MtModel {
    /// Helsinki opus-mt-es-en. 153 MB, muy rápido.
    Opus,
    /// NLLB-200 distilled 600M. Mejor calidad (+2.1 BLEU sobre opus), más lento y pesado.
    Nllb,
}

impl AsrModel {
    pub fn dir_name(self) -> &'static str {
        match self {
            AsrModel::Parakeet => "asr/parakeet-tdt-0.6b-v3-int8",
            AsrModel::Canary => "asr/canary-1b-v2-int8",
        }
    }

    /// Si el modelo entrega timestamps por token (necesarios para karaoke exacto).
    pub fn has_word_timings(self) -> bool {
        matches!(self, AsrModel::Parakeet)
    }
}

impl MtModel {
    pub fn dir_name(self) -> &'static str {
        match self {
            MtModel::Opus => "mt/opus-es-en",
            MtModel::Nllb => "mt/nllb-200-distilled-600M",
        }
    }
}

/// Raíz del árbol de modelos.
#[derive(Clone, Debug)]
pub struct Models {
    pub root: PathBuf,
}

impl Models {
    pub fn new<P: AsRef<Path>>(root: P) -> Self {
        Self {
            root: root
                .as_ref()
                .to_path_buf(),
        }
    }

    /// Busca `models/` junto al ejecutable, y si no, en el directorio actual.
    /// `WHISPER_MODELS` la sobreescribe (útil para desarrollo).
    pub fn autodetect() -> Result<Self> {
        if let Ok(p) = std::env::var("WHISPER_MODELS") {
            let p = PathBuf::from(p);
            if p.is_dir() {
                return Ok(Self::new(p));
            }
            bail!("WHISPER_MODELS apunta a {} y no existe", p.display());
        }
        let mut candidates = Vec::new();
        if let Ok(exe) = std::env::current_exe() {
            if let Some(d) = exe.parent() {
                candidates.push(d.join("models"));
                // en `cargo run` el exe queda en target/<perfil>/
                candidates.push(d.join("../../../models"));
            }
        }
        candidates.push(PathBuf::from("models"));
        for c in candidates {
            if c.is_dir() {
                return Ok(Self::new(c));
            }
        }
        bail!("no encontré el directorio 'models'; definí WHISPER_MODELS")
    }

    pub fn asr_dir(&self, m: AsrModel) -> PathBuf {
        self.root
            .join(m.dir_name())
    }

    pub fn mt_dir(&self, m: MtModel) -> PathBuf {
        self.root
            .join(m.dir_name())
    }

    pub fn vad(&self) -> PathBuf {
        self.root
            .join("vad/silero_vad.onnx")
    }

    pub fn denoiser(&self) -> PathBuf {
        self.root
            .join("denoise/gtcrn_simple.onnx")
    }

    pub fn diar_segmentation(&self) -> PathBuf {
        self.root
            .join("diar/segmentation.onnx")
    }

    pub fn diar_embedding(&self) -> PathBuf {
        self.root
            .join("diar/embedding.onnx")
    }
}

/// Comprueba que el archivo exista y lo devuelve como `String` UTF-8 para la API de C.
pub fn need(p: &Path) -> Result<String> {
    if !p.is_file() {
        bail!("falta el modelo: {}", p.display());
    }
    Ok(p.to_string_lossy()
        .into_owned())
}
