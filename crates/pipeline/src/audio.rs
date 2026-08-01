//! Decodificación de audio a 16 kHz mono f32.
//!
//! Se apoya en ffmpeg porque los audios que llegan pueden venir en cualquier formato
//! (mp3, m4a, opus de WhatsApp, wma, amr de grabadoras telefónicas, ogg, vídeo...).
//! Se distribuye como binario acompañante; no hay ninguna llamada de red.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

pub const SAMPLE_RATE: u32 = 16_000;

/// Forma de onda mono en punto flotante.
#[derive(Clone, Debug, Default)]
pub struct Audio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl Audio {
    pub fn duration(&self) -> f32 {
        self.samples
            .len() as f32
            / self.sample_rate as f32
    }

    /// Convierte un índice de muestra a segundos.
    pub fn secs(&self, sample: usize) -> f32 {
        sample as f32 / self.sample_rate as f32
    }
}

/// Ubica el binario de ffmpeg: variable de entorno, luego junto al ejecutable, luego el PATH.
pub fn ffmpeg_bin() -> PathBuf {
    if let Ok(p) = std::env::var("WHISPER_FFMPEG") {
        return PathBuf::from(p);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            for name in ["ffmpeg.exe", "ffmpeg"] {
                let c = d.join(name);
                if c.is_file() {
                    return c;
                }
            }
        }
    }
    PathBuf::from("ffmpeg")
}

/// Decodifica cualquier archivo soportado por ffmpeg a 16 kHz mono f32.
pub fn decode<P: AsRef<Path>>(path: P) -> Result<Audio> {
    let path = path.as_ref();
    if !path.is_file() {
        bail!("no existe el archivo de audio: {}", path.display());
    }
    let ff = ffmpeg_bin();
    let out = Command::new(&ff)
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
        ])
        .arg(path)
        .args([
            "-vn",
            "-map",
            "a:0",
            "-ac",
            "1",
            "-ar",
            &SAMPLE_RATE.to_string(),
            "-f",
            "f32le",
            "-",
        ])
        .output()
        .with_context(|| {
            format!(
                "no pude ejecutar ffmpeg ({}). Definí WHISPER_FFMPEG o poné ffmpeg en el PATH.",
                ff.display()
            )
        })?;

    if !out
        .status
        .success()
    {
        bail!(
            "ffmpeg falló al decodificar {}: {}",
            path.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }

    let samples = bytes_to_f32(&out.stdout);
    if samples.is_empty() {
        bail!("{} no tiene pista de audio utilizable", path.display());
    }
    Ok(Audio {
        samples,
        sample_rate: SAMPLE_RATE,
    })
}

fn bytes_to_f32(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Escribe un WAV de 16 bits. Solo se usa para depuración y para exportar el audio limpio.
pub fn write_wav<P: AsRef<Path>>(path: P, samples: &[f32], sample_rate: u32) -> Result<()> {
    let p = path
        .as_ref()
        .to_string_lossy()
        .into_owned();
    if !sherpa_onnx::write(&p, samples, sample_rate as i32) {
        bail!("no pude escribir {p}");
    }
    Ok(())
}
