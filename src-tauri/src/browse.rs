//! Explorador de carpetas de trabajo.
//!
//! El material vive en una carpeta base (`GoGlobal`) con una subcarpeta por jornada:
//! «Lunes 3», «Miércoles 12». Al lado hay muchísimos audios viejos que no interesan,
//! así que **acá no se recorre nada en profundidad**: se lista un directorio y punto.
//! Buscar audios recursivamente en esa base tardaría una eternidad y no serviría de nada.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Extensiones que se muestran como audio abrible.
pub const AUDIOS: &[&str] = &[
    "mp3", "wav", "m4a", "mp4", "aac", "ogg", "oga", "opus", "flac", "wma", "amr", "3gp", "aiff",
    "webm", "mkv", "avi", "mov",
];

/// Tope de archivos que se listan de una carpeta. Si alguien abre por error la carpeta
/// con los miles de audios viejos, la interfaz no se cuelga: muestra los primeros y avisa.
const TOPE: usize = 400;

const DIAS: [&str; 7] = [
    "lunes",
    "martes",
    "miercoles",
    "jueves",
    "viernes",
    "sabado",
    "domingo",
];

#[derive(Serialize)]
pub struct FolderEntry {
    pub name: String,
    pub path: String,
    /// El nombre parece una jornada de trabajo («Lunes 3»).
    pub workday: bool,
    /// Segundos desde epoch de la última modificación; ordena de más nueva a más vieja.
    pub modified: u64,
}

#[derive(Serialize)]
pub struct AudioEntry {
    pub name: String,
    pub path: String,
    pub size: u64,
    /// Ya tiene una transcripción guardada al lado.
    pub done: bool,
}

#[derive(Serialize)]
pub struct Listing {
    pub path: String,
    pub parent: Option<String>,
    pub folders: Vec<FolderEntry>,
    pub audios: Vec<AudioEntry>,
    /// Cuántos audios hay de verdad, por si se recortó la lista.
    pub total_audios: usize,
    /// Alguna carpeta hija tiene nombre de jornada.
    pub has_workdays: bool,
}

/// Minúsculas y sin tildes, para que «Miércoles» y «miercoles» sean lo mismo.
fn normaliza(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            c.to_lowercase()
                .collect::<Vec<_>>()
        })
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            c => c,
        })
        .collect()
}

/// ¿El nombre es una jornada? Día de la semana seguido de un número, en cualquier forma:
/// «Lunes 3», «lunes3», «MARTES-4», «Miércoles 12».
pub fn es_jornada(nombre: &str) -> bool {
    let n = normaliza(nombre);
    DIAS.iter().any(|d| {
        n.starts_with(d)
            && n[d.len()..]
                .chars()
                .any(|c| c.is_ascii_digit())
    })
}

fn modificado(m: &std::fs::Metadata) -> u64 {
    m.modified()
        .ok()
        .and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .ok()
        })
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn es_audio(p: &Path) -> bool {
    p.extension()
        .map(|e| {
            AUDIOS.contains(
                &e.to_string_lossy()
                    .to_lowercase()
                    .as_str(),
            )
        })
        .unwrap_or(false)
}

/// Lista **un solo** directorio: sus carpetas hijas y sus audios. Nunca baja más de un nivel.
pub fn list(dir: &Path) -> std::io::Result<Listing> {
    let mut folders = Vec::new();
    let mut audios = Vec::new();
    let mut total_audios = 0usize;

    for entry in std::fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        let Ok(meta) = entry.metadata() else { continue };
        let path = entry.path();
        let name = entry
            .file_name()
            .to_string_lossy()
            .into_owned();

        if meta.is_dir() {
            if name.starts_with('.') {
                continue;
            }
            folders.push(FolderEntry {
                workday: es_jornada(&name),
                name,
                path: path
                    .display()
                    .to_string(),
                modified: modificado(&meta),
            });
        } else if es_audio(&path) {
            total_audios += 1;
            if audios.len() < TOPE {
                let done = path
                    .file_stem()
                    .map(|s| {
                        path.with_file_name(format!(
                            "{}.transcripcion.json",
                            s.to_string_lossy()
                        ))
                    })
                    .map(|p| p.is_file())
                    .unwrap_or(false);
                audios.push(AudioEntry {
                    name,
                    path: path
                        .display()
                        .to_string(),
                    size: meta.len(),
                    done,
                });
            }
        }
    }

    // Las jornadas primero y las más recientes arriba; el resto, por nombre.
    folders.sort_by(|a, b| {
        b.workday
            .cmp(&a.workday)
            .then(
                b.modified
                    .cmp(&a.modified),
            )
            .then_with(|| {
                normaliza(&a.name).cmp(&normaliza(&b.name))
            })
    });
    audios.sort_by(|a, b| normaliza(&a.name).cmp(&normaliza(&b.name)));

    Ok(Listing {
        path: dir
            .display()
            .to_string(),
        parent: dir
            .parent()
            .map(|p| {
                p.display()
                    .to_string()
            }),
        has_workdays: folders
            .iter()
            .any(|f| f.workday),
        folders,
        audios,
        total_audios,
    })
}

/// Ajustes que sobreviven entre sesiones. Van al lado del ejecutable para que viajen
/// con la carpeta cuando se copia a otra máquina.
#[derive(Serialize, Deserialize, Default)]
pub struct Settings {
    /// Carpeta base del explorador.
    pub root: Option<String>,
}

fn settings_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|e| {
            e.parent()
                .map(|d| d.join("ajustes.json"))
        })
        .unwrap_or_else(|| PathBuf::from("ajustes.json"))
}

pub fn load_settings() -> Settings {
    std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_settings(s: &Settings) -> Result<(), String> {
    let json = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    std::fs::write(settings_path(), json).map_err(|e| {
        format!(
            "no pude guardar los ajustes en {}: {e}",
            settings_path().display()
        )
    })
}

/// La carpeta con la que arranca el explorador.
///
/// Sin configurar, `GoGlobal` en el escritorio del usuario. Eso resuelve solo en cada
/// máquina, sin escribir la ruta de nadie en el código.
pub fn default_root() -> PathBuf {
    if let Some(r) = load_settings()
        .root
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
    {
        return r;
    }
    let home = std::env::var("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    for c in [
        home.join("Desktop/GoGlobal"),
        home.join("Escritorio/GoGlobal"),
        home.join("OneDrive/Desktop/GoGlobal"),
        home.join("Desktop"),
        home.join("Escritorio"),
    ] {
        if c.is_dir() {
            return c;
        }
    }
    home
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconoce_las_jornadas() {
        for n in [
            "Lunes 3",
            "lunes3",
            "MARTES-4",
            "Miércoles 12",
            "miercoles 12",
            "Sábado 1",
            "domingo 30",
            "Jueves  7",
        ] {
            assert!(es_jornada(n), "deberia ser jornada: {n}");
        }
    }

    #[test]
    fn no_confunde_otras_carpetas() {
        for n in [
            "audios viejos",
            "Lunes",          // sin número
            "GoGlobal",
            "2024",
            "Entregado",
            "clientes 2023",  // número pero no empieza con día
        ] {
            assert!(!es_jornada(n), "no deberia ser jornada: {n}");
        }
    }

    #[test]
    fn normaliza_tildes() {
        assert_eq!(normaliza("Miércoles"), "miercoles");
        assert_eq!(normaliza("SÁBADO"), "sabado");
    }
}
