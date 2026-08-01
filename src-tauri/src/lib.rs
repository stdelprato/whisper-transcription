//! Capa de aplicación: expone el núcleo a la interfaz y gestiona el trabajo en curso.

pub mod browse;
mod export;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use pipeline::models::{AsrModel, Models, MtModel};
use pipeline::{Flow, Options, Progress, Transcript};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;

/// Extensiones que el visor web reproduce sin ayuda.
const REPRODUCIBLES: &[&str] = &[
    "mp3", "m4a", "mp4", "aac", "wav", "ogg", "oga", "opus", "webm", "flac",
];

use browse::AUDIOS;

#[derive(Default)]
pub struct AppState {
    cancel: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    transcript: Mutex<Option<Transcript>>,
}

/// Ajustes tal como los manda la interfaz.
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct UiOptions {
    pub asr: String,
    pub mt: String,
    pub verify: bool,
    pub denoise: bool,
    pub diarize: bool,
    /// `0` = automático.
    pub speakers: i32,
    pub source_lang: String,
    pub threads: i32,
}

impl Default for UiOptions {
    fn default() -> Self {
        Self {
            asr: "canary".into(),
            mt: "nllb".into(),
            verify: true,
            denoise: false,
            diarize: true,
            speakers: 0,
            source_lang: "es".into(),
            threads: pipeline::default_threads(),
        }
    }
}

impl UiOptions {
    fn to_options(&self) -> Options {
        let mut o = Options {
            asr: if self.asr == "parakeet" {
                AsrModel::Parakeet
            } else {
                AsrModel::Canary
            },
            mt: match self.mt.as_str() {
                "opus" => Some(MtModel::Opus),
                "none" => None,
                _ => Some(MtModel::Nllb),
            },
            verify: self.verify,
            denoise: self.denoise,
            diarize: self.diarize,
            speakers: if self.speakers > 0 {
                Some(self.speakers)
            } else {
                None
            },
            source_lang: self
                .source_lang
                .clone(),
            threads: self
                .threads
                .clamp(1, 16),
            ..Default::default()
        };
        // Si el audio ya está en inglés no hay nada que traducir.
        if o.source_lang == "en" {
            o.mt = None;
        }
        o
    }
}

/// Lo que la interfaz necesita saber al arrancar.
#[derive(Serialize)]
pub struct Info {
    models_root: Option<String>,
    missing: Vec<String>,
    ffmpeg: bool,
    threads: i32,
}

fn models() -> Result<Models, String> {
    Models::autodetect().map_err(|e| e.to_string())
}

#[tauri::command]
fn app_info() -> Info {
    let m = models().ok();
    let mut missing = Vec::new();
    if let Some(m) = &m {
        let mut check = |p: PathBuf| {
            if !p.exists() {
                missing.push(
                    p.strip_prefix(&m.root)
                        .unwrap_or(&p)
                        .display()
                        .to_string(),
                );
            }
        };
        check(m.vad());
        check(m.denoiser());
        check(m.diar_segmentation());
        check(m.diar_embedding());
        check(
            m.asr_dir(AsrModel::Parakeet)
                .join("encoder.int8.onnx"),
        );
        check(
            m.asr_dir(AsrModel::Canary)
                .join("encoder.int8.onnx"),
        );
        check(
            m.mt_dir(MtModel::Opus)
                .join("model.bin"),
        );
        check(
            m.mt_dir(MtModel::Nllb)
                .join("model.bin"),
        );
    }
    Info {
        models_root: m.map(|m| {
            m.root
                .display()
                .to_string()
        }),
        missing,
        ffmpeg: std::process::Command::new(pipeline::audio::ffmpeg_bin())
            .arg("-version")
            .output()
            .is_ok(),
        threads: pipeline::default_threads(),
    }
}

#[tauri::command]
fn pick_audio(app: AppHandle, from: Option<String>) -> Vec<String> {
    let dir = from
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(browse::default_root);
    app.dialog()
        .file()
        .set_directory(dir)
        .add_filter("Audio y vídeo", AUDIOS)
        .blocking_pick_files()
        .unwrap_or_default()
        .into_iter()
        .map(|f| f.to_string())
        .collect()
}

/// Lista una carpeta: sus subcarpetas y sus audios. Un solo nivel, nunca recursivo.
#[tauri::command]
fn browse(path: Option<String>) -> Result<browse::Listing, String> {
    let dir = path
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(browse::default_root);
    browse::list(&dir).map_err(|e| format!("no pude leer {}: {e}", dir.display()))
}

/// La carpeta base con la que arranca el explorador.
#[tauri::command]
fn get_root() -> String {
    browse::default_root()
        .display()
        .to_string()
}

/// Elige otra carpeta base y la recuerda.
#[tauri::command]
fn pick_root(app: AppHandle) -> Result<Option<String>, String> {
    let Some(dir) = app
        .dialog()
        .file()
        .set_directory(browse::default_root())
        .blocking_pick_folder()
    else {
        return Ok(None);
    };
    let path = dir.to_string();
    let mut s = browse::load_settings();
    s.root = Some(path.clone());
    browse::save_settings(&s)?;
    Ok(Some(path))
}

#[tauri::command]
fn save_as(app: AppHandle, name: String, ext: String) -> Option<String> {
    app.dialog()
        .file()
        .set_file_name(&name)
        .add_filter(&ext, &[ext.as_str()])
        .blocking_save_file()
        .map(|f| {
            f.to_string()
        })
}

/// Devuelve una ruta que el reproductor del visor pueda abrir.
///
/// Los formatos raros de grabadora telefónica (amr, wma) el visor no los toca, así que
/// se convierten una vez a un archivo temporal y se reproduce ese.
#[tauri::command]
fn prepare_playback(path: String) -> Result<String, String> {
    let p = Path::new(&path);
    let ext = p
        .extension()
        .map(|e| {
            e.to_string_lossy()
                .to_lowercase()
        })
        .unwrap_or_default();
    if REPRODUCIBLES.contains(&ext.as_str()) {
        return Ok(path);
    }
    let dir = std::env::temp_dir().join("transcriptor");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stem = p
        .file_stem()
        .map(|s| {
            s.to_string_lossy()
                .into_owned()
        })
        .unwrap_or_else(|| "audio".into());
    let out = dir.join(format!("{stem}.m4a"));
    if out.exists() {
        return Ok(out
            .display()
            .to_string());
    }
    let status = std::process::Command::new(pipeline::audio::ffmpeg_bin())
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(p)
        .args(["-vn", "-ac", "1", "-c:a", "aac", "-b:a", "96k"])
        .arg(&out)
        .status()
        .map_err(|e| format!("no pude ejecutar ffmpeg: {e}"))?;
    if !status.success() {
        return Err("ffmpeg no pudo preparar el audio para reproducir".into());
    }
    Ok(out
        .display()
        .to_string())
}

#[tauri::command]
fn start(app: AppHandle, state: State<'_, AppState>, path: String, opts: UiOptions) -> Result<(), String> {
    if state
        .running
        .swap(true, Ordering::SeqCst)
    {
        return Err("ya hay un audio en proceso".into());
    }
    state
        .cancel
        .store(false, Ordering::SeqCst);
    let cancel = state
        .cancel
        .clone();
    let running = state
        .running
        .clone();

    std::thread::spawn(move || {
        let result = (|| -> Result<Transcript, String> {
            let m = models()?;
            let o = opts.to_options();
            let app2 = app.clone();
            let mut on = move |p: Progress| {
                let _ = app2.emit("progress", &p);
                if cancel.load(Ordering::SeqCst) {
                    Flow::Abort
                } else {
                    Flow::Continue
                }
            };
            pipeline::run(&m, &path, &o, &mut on).map_err(|e| format!("{e:#}"))
        })();

        running.store(false, Ordering::SeqCst);
        match result {
            Ok(t) => {
                save_result(&t);
                if let Some(state) = app.try_state::<AppState>() {
                    *state
                        .transcript
                        .lock()
                        .unwrap() = Some(t.clone());
                }
                let _ = app.emit("finished", &t);
            }
            Err(e) => {
                let _ = app.emit("failed", e);
            }
        }
    });
    Ok(())
}

#[tauri::command]
fn cancel(state: State<'_, AppState>) {
    state
        .cancel
        .store(true, Ordering::SeqCst);
}

/// Vuelve a separar hablantes sin repetir el reconocimiento.
#[tauri::command]
fn rediarize(state: State<'_, AppState>, speakers: i32, threads: i32) -> Result<Transcript, String> {
    let m = models()?;
    let mut guard = state
        .transcript
        .lock()
        .unwrap();
    let t = guard
        .as_mut()
        .ok_or("todavía no hay nada procesado")?;
    pipeline::rediarize(
        &m,
        t,
        if speakers > 0 { Some(speakers) } else { None },
        Default::default(),
        threads.clamp(1, 16),
    )
    .map_err(|e| format!("{e:#}"))?;
    Ok(t.clone())
}

/// Dónde se guarda el resultado de un audio: al lado del propio audio.
///
/// Procesar diez llamadas lleva horas. Si se cierra la ventana, o se corta la luz, o
/// simplemente quiere retomarlo mañana, el trabajo tiene que seguir ahí.
fn result_path(audio: &str) -> PathBuf {
    let p = Path::new(audio);
    let stem = p
        .file_stem()
        .map(|s| {
            s.to_string_lossy()
                .into_owned()
        })
        .unwrap_or_else(|| "audio".into());
    p.with_file_name(format!("{stem}.transcripcion.json"))
}

fn save_result(t: &Transcript) {
    if let Ok(json) = serde_json::to_string(t) {
        let _ = std::fs::write(result_path(&t.file), json);
    }
}

/// Devuelve el resultado guardado de un audio, si lo hay.
#[tauri::command]
fn load_result(path: String) -> Option<Transcript> {
    let raw = std::fs::read_to_string(result_path(&path)).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Vuelve a poner en curso un resultado ya calculado, para exportarlo o retocarlo
/// después de cambiar de archivo en la cola.
#[tauri::command]
fn set_transcript(state: State<'_, AppState>, transcript: Transcript) {
    *state
        .transcript
        .lock()
        .unwrap() = Some(transcript);
}

/// Guarda las correcciones hechas a mano para que la exportación las use.
#[tauri::command]
fn update_segment(
    state: State<'_, AppState>,
    id: usize,
    target: Option<String>,
    source: Option<String>,
) -> Result<(), String> {
    let mut guard = state
        .transcript
        .lock()
        .unwrap();
    let t = guard
        .as_mut()
        .ok_or("todavía no hay nada procesado")?;
    let seg = t
        .segments
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or("ese bloque no existe")?;
    if let Some(v) = target {
        seg.target = Some(v);
    }
    if let Some(v) = source {
        seg.source = v;
    }
    save_result(t);
    Ok(())
}

/// Le pone nombre a un hablante ("Cliente", "Abogada"...) en vez de "Hablante 2".
#[tauri::command]
fn set_speaker_name(state: State<'_, AppState>, index: i32, name: String) -> Result<(), String> {
    let mut guard = state
        .transcript
        .lock()
        .unwrap();
    let t = guard
        .as_mut()
        .ok_or("todavía no hay nada procesado")?;
    let i = index.max(0) as usize;
    if t.speaker_names
        .len()
        <= i
    {
        t.speaker_names
            .resize(i + 1, String::new());
    }
    t.speaker_names[i] = name
        .trim()
        .to_string();
    save_result(t);
    Ok(())
}

#[tauri::command]
fn copy_text(app: AppHandle, text: String) -> Result<(), String> {
    app.clipboard()
        .write_text(text)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn export(
    state: State<'_, AppState>,
    path: String,
    format: export::Format,
    content: export::Content,
    speakers: bool,
) -> Result<(), String> {
    let guard = state
        .transcript
        .lock()
        .unwrap();
    let t = guard
        .as_ref()
        .ok_or("todavía no hay nada que exportar")?;
    let body = export::render(t, format, content, speakers);
    std::fs::write(&path, body).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            app_info,
            pick_audio,
            browse,
            get_root,
            pick_root,
            save_as,
            prepare_playback,
            start,
            cancel,
            rediarize,
            set_transcript,
            load_result,
            set_speaker_name,
            update_segment,
            copy_text,
            export,
        ])
        .run(tauri::generate_context!())
        .expect("no pude arrancar la ventana");
}
