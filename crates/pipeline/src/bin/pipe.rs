//! Herramienta de línea de comandos para probar el núcleo sin interfaz.
//!
//! ```text
//! pipe <audio> [--asr parakeet|canary] [--mt opus|nllb|none] [--no-verify]
//!              [--denoise] [--no-diar] [--speakers N] [--lang es]
//!              [--threads N] [--json salida.json] [--preset fast|balanced|best]
//! ```

use std::io::Write;

use anyhow::{bail, Result};
use pipeline::models::{AsrModel, Models, MtModel};
use pipeline::{run, Flow, Options, Progress};

const USO: &str = "uso: pipe <audio> [--asr parakeet|canary] [--mt opus|nllb|none] [--no-verify] \
                   [--denoise] [--no-diar] [--speakers N] [--lang es] [--threads N] \
                   [--json salida.json] [--preset fast|balanced|best]";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args()
        .skip(1)
        .collect();
    if args.is_empty() || args[0] == "-h" || args[0] == "--help" {
        eprintln!("{USO}");
        return Ok(());
    }

    let file = args[0].clone();
    let mut opts = Options::default();
    let mut json_out: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        let flag = args[i].clone();
        let value = |i: &mut usize| -> Result<String> {
            *i += 1;
            args.get(*i)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("falta el valor de {flag}"))
        };
        match args[i].as_str() {
            "--preset" => {
                opts = match value(&mut i)?.as_str() {
                    "fast" => Options::fast(),
                    "best" => Options::best(),
                    "balanced" => Options::default(),
                    v => bail!("preset desconocido: {v}"),
                }
            }
            "--asr" => {
                opts.asr = match value(&mut i)?.as_str() {
                    "parakeet" => AsrModel::Parakeet,
                    "canary" => AsrModel::Canary,
                    v => bail!("motor de voz desconocido: {v}"),
                }
            }
            "--mt" => {
                opts.mt = match value(&mut i)?.as_str() {
                    "opus" => Some(MtModel::Opus),
                    "nllb" => Some(MtModel::Nllb),
                    "none" => None,
                    v => bail!("traductor desconocido: {v}"),
                }
            }
            "--no-verify" => opts.verify = false,
            "--denoise" => opts.denoise = true,
            "--no-diar" => opts.diarize = false,
            "--speakers" => opts.speakers = Some(value(&mut i)?.parse()?),
            "--beam" => {
                opts.mt_opts
                    .beam_size = value(&mut i)?.parse()?
            }
            "--no-repeat" => {
                opts.mt_opts
                    .no_repeat_ngram = value(&mut i)?.parse()?
            }
            "--lang" => opts.source_lang = value(&mut i)?,
            "--threads" => opts.threads = value(&mut i)?.parse()?,
            "--json" => json_out = Some(value(&mut i)?),
            v => bail!("opción desconocida: {v}\n{USO}"),
        }
        i += 1;
    }

    let models = Models::autodetect()?;
    eprintln!("modelos: {}", models.root.display());
    eprintln!(
        "motor: {:?}{} | traductor: {:?} | ruido: {} | hablantes: {}",
        opts.asr,
        opts.verifier()
            .map(|v| format!(" (contrastado con {v:?})"))
            .unwrap_or_default(),
        opts.mt,
        if opts.denoise { "limpiado" } else { "original" },
        opts.speakers
            .map(|n| n.to_string())
            .unwrap_or_else(|| "auto".into()),
    );

    let mut last = std::time::Instant::now();
    let mut tick = |etapa: &str, done: usize, total: usize| {
        if last
            .elapsed()
            .as_millis()
            > 400
            || done == total
        {
            last = std::time::Instant::now();
            eprint!("\r  {etapa} {done}/{total}      ");
            let _ = std::io::stderr().flush();
        }
    };
    let mut on = |p: Progress| {
        match p {
            Progress::Decoded { duration } => eprintln!("audio: {duration:.1}s"),
            Progress::Segmented { count } => eprintln!("tramos de habla: {count}"),
            Progress::Diarized { speakers } => eprintln!("hablantes detectados: {speakers}"),
            Progress::Recognized { done, total, .. } => tick("reconociendo", done, total),
            Progress::Verifying { total } => eprintln!("\ncontrastando {total} tramos"),
            Progress::Refined { done, total, .. } => tick("contrastando", done, total),
            Progress::Translating { total } => eprintln!("\ntraduciendo {total} bloques"),
            Progress::Translated { .. } => {}
            Progress::Done => eprintln!("\nlisto"),
        }
        Flow::Continue
    };

    let tr = run(&models, &file, &opts, &mut on)?;

    println!();
    for s in &tr.segments {
        let spk = s
            .speaker
            .map(|k| format!("H{} ", k + 1))
            .unwrap_or_default();
        let flag = match (s.degenerate, s.repaired) {
            (true, _) => " [REPETICIÓN]",
            (_, true) => " [RESCATADO]",
            _ => "",
        };
        let ag = s
            .agreement
            .map(|a| format!("{:3.0}% ", a * 100.0))
            .unwrap_or_default();
        println!("[{:7.2} → {:7.2}] {ag}{spk}{flag}", s.start, s.end);
        println!("   ES  {}", s.source);
        if let Some(t) = &s.target {
            println!("   EN  {t}");
        }
        let dudosas: Vec<&str> = s
            .words
            .iter()
            .filter(|w| w.low_conf)
            .map(|w| w.text.as_str())
            .collect();
        if !dudosas.is_empty() {
            println!("   ??  {}", dudosas.join(" "));
        }
    }

    let marcadas: usize = tr
        .segments
        .iter()
        .flat_map(|s| &s.words)
        .filter(|w| w.low_conf)
        .count();
    let palabras: usize = tr
        .segments
        .iter()
        .map(|s| s.words.len())
        .sum();
    let t = &tr.timings;
    println!(
        "\n{:.0}s de audio en {:.0}s (RTF {:.3})\n\
         bloques {} | palabras {} | dudosas {} ({:.0}%) | hablantes {}\n\
         etapas: decodificar {:.0}s · VAD {:.0}s · ruido {:.0}s · hablantes {:.0}s · \
         ASR {:.0}s · contraste {:.0}s · traducción {:.0}s",
        tr.duration,
        t.total,
        tr.rtf(),
        tr.segments
            .len(),
        palabras,
        marcadas,
        100.0 * marcadas as f32 / palabras.max(1) as f32,
        tr.num_speakers,
        t.decode,
        t.vad,
        t.denoise,
        t.diarize,
        t.asr,
        t.verify,
        t.translate,
    );

    if let Some(p) = json_out {
        std::fs::write(&p, serde_json::to_string_pretty(&tr)?)?;
        println!("escrito {p}");
    }
    Ok(())
}
