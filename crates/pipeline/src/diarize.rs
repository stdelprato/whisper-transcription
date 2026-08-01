//! Diarización: quién habla en cada momento.
//!
//! pyannote-segmentation-3.0 para los límites y CAM++ para los embeddings de voz,
//! agrupando con clustering rápido. Cuando no se sabe cuánta gente hay (lo normal en
//! una llamada), se agrupa por umbral en vez de fijar el número de hablantes.

use anyhow::{Context, Result};
use sherpa_onnx::{
    FastClusteringConfig, OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig,
    OfflineSpeakerSegmentationModelConfig, OfflineSpeakerSegmentationPyannoteModelConfig,
    SpeakerEmbeddingExtractorConfig,
};

use crate::audio::Audio;
use crate::models::{need, Models};

/// Tramo atribuido a un hablante, en segundos.
#[derive(Clone, Copy, Debug)]
pub struct SpeakerSpan {
    pub start: f32,
    pub end: f32,
    pub speaker: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct DiarOptions {
    /// Número de hablantes si se conoce; `None` para deducirlo.
    pub num_speakers: Option<i32>,
    /// Umbral de agrupación cuando el número es desconocido. Más alto = menos hablantes.
    ///
    /// Sobre la llamada real, dejando el número libre, el agrupador parte de más: con el
    /// valor de ejemplo de sherpa (0.5) encontró **quince** hablantes en una conversación
    /// de dos. A 0.8 los dos principales se quedan con 136 de los 143 segundos de habla y
    /// lo que sobra son fragmentos de uno o dos segundos, que [`merge_tiny`] absorbe.
    pub threshold: f32,
    pub min_duration_on: f32,
    pub min_duration_off: f32,
    /// Fracción mínima del habla para considerar que un grupo es una persona y no ruido.
    pub min_share: f32,
}

impl Default for DiarOptions {
    fn default() -> Self {
        Self {
            num_speakers: None,
            threshold: 0.8,
            min_duration_on: 0.3,
            min_duration_off: 0.5,
            min_share: 0.03,
        }
    }
}

/// Corre la diarización sobre el audio completo.
pub fn diarize(
    models: &Models,
    audio: &Audio,
    opts: &DiarOptions,
    threads: i32,
) -> Result<Vec<SpeakerSpan>> {
    let cfg = OfflineSpeakerDiarizationConfig {
        segmentation: OfflineSpeakerSegmentationModelConfig {
            pyannote: OfflineSpeakerSegmentationPyannoteModelConfig {
                model: Some(need(&models.diar_segmentation())?),
            },
            num_threads: threads,
            ..Default::default()
        },
        embedding: SpeakerEmbeddingExtractorConfig {
            model: Some(need(&models.diar_embedding())?),
            num_threads: threads,
            ..Default::default()
        },
        clustering: FastClusteringConfig {
            num_clusters: opts
                .num_speakers
                .unwrap_or(-1),
            threshold: opts.threshold,
        },
        min_duration_on: opts.min_duration_on,
        min_duration_off: opts.min_duration_off,
    };

    let sd = OfflineSpeakerDiarization::create(&cfg).context("no pude crear el diarizador")?;
    let Some(res) = sd.process(&audio.samples) else {
        return Ok(Vec::new());
    };
    let mut spans: Vec<SpeakerSpan> = res
        .sort_by_start_time()
        .into_iter()
        .map(|s| SpeakerSpan {
            start: s.start,
            end: s.end,
            speaker: s.speaker,
        })
        .collect();

    if opts
        .num_speakers
        .is_none()
    {
        merge_tiny(&mut spans, opts.min_share);
    }
    renumber(&mut spans);
    Ok(spans)
}

/// Reasigna los grupos con muy poco tiempo al hablante real más cercano.
///
/// El agrupador tiende a dejar sueltos trocitos de uno o dos segundos —solapamientos,
/// ruidos de línea, un "ajá" de fondo— y cada uno aparece como una persona nueva. Nadie
/// quiere ver "Hablante 16" en una llamada entre dos.
fn merge_tiny(spans: &mut [SpeakerSpan], min_share: f32) {
    use std::collections::BTreeMap;

    let mut time: BTreeMap<i32, f32> = BTreeMap::new();
    for s in spans.iter() {
        *time
            .entry(s.speaker)
            .or_insert(0.0) += s.end - s.start;
    }
    let total: f32 = time
        .values()
        .sum();
    if total <= 0.0 {
        return;
    }
    let real: Vec<i32> = time
        .iter()
        .filter(|(_, &t)| t / total >= min_share)
        .map(|(&k, _)| k)
        .collect();
    if real.is_empty() || real.len() == time.len() {
        return;
    }

    // Para cada trocito, se busca el tramo real más próximo en el tiempo y se hereda su etiqueta.
    let anchors: Vec<(f32, f32, i32)> = spans
        .iter()
        .filter(|s| real.contains(&s.speaker))
        .map(|s| (s.start, s.end, s.speaker))
        .collect();
    if anchors.is_empty() {
        return;
    }
    for s in spans.iter_mut() {
        if real.contains(&s.speaker) {
            continue;
        }
        let mid = (s.start + s.end) / 2.0;
        let best = anchors
            .iter()
            .min_by(|a, b| {
                distance(mid, a.0, a.1)
                    .partial_cmp(&distance(mid, b.0, b.1))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .expect("hay al menos un tramo real");
        s.speaker = best.2;
    }
}

fn distance(t: f32, start: f32, end: f32) -> f32 {
    if t < start {
        start - t
    } else if t > end {
        t - end
    } else {
        0.0
    }
}

/// Renumera a 0, 1, 2... por orden de aparición, para que las etiquetas de la interfaz
/// sean "Hablante 1" y "Hablante 2" y no los índices internos del agrupador.
fn renumber(spans: &mut [SpeakerSpan]) {
    use std::collections::HashMap;

    let mut map: HashMap<i32, i32> = HashMap::new();
    for s in spans.iter_mut() {
        let next = map.len() as i32;
        s.speaker = *map
            .entry(s.speaker)
            .or_insert(next);
    }
}

/// Elige el hablante que más solapa con `[start, end)`.
pub fn speaker_at(spans: &[SpeakerSpan], start: f32, end: f32) -> Option<i32> {
    let mut best: Option<(f32, i32)> = None;
    for s in spans {
        let overlap = end.min(s.end) - start.max(s.start);
        if overlap <= 0.0 {
            continue;
        }
        match best {
            Some((o, _)) if o >= overlap => {}
            _ => best = Some((overlap, s.speaker)),
        }
    }
    best.map(|(_, spk)| spk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_grupos_diminutos_se_absorben() {
        // Dos personas hablando y un "grupo" de medio segundo que no es nadie.
        let mut spans = vec![
            SpeakerSpan {
                start: 0.0,
                end: 60.0,
                speaker: 0,
            },
            SpeakerSpan {
                start: 60.0,
                end: 60.5,
                speaker: 9,
            },
            SpeakerSpan {
                start: 61.0,
                end: 100.0,
                speaker: 3,
            },
        ];
        merge_tiny(&mut spans, 0.03);
        renumber(&mut spans);
        let distintos: std::collections::BTreeSet<i32> = spans
            .iter()
            .map(|s| s.speaker)
            .collect();
        assert_eq!(distintos.len(), 2);
        assert_eq!(spans[0].speaker, 0);
        assert_eq!(spans[2].speaker, 1);
    }

    #[test]
    fn renumera_por_orden_de_aparicion() {
        let mut spans = vec![
            SpeakerSpan {
                start: 0.0,
                end: 1.0,
                speaker: 7,
            },
            SpeakerSpan {
                start: 1.0,
                end: 2.0,
                speaker: 2,
            },
            SpeakerSpan {
                start: 2.0,
                end: 3.0,
                speaker: 7,
            },
        ];
        renumber(&mut spans);
        assert_eq!(
            spans
                .iter()
                .map(|s| s.speaker)
                .collect::<Vec<_>>(),
            vec![0, 1, 0]
        );
    }

    #[test]
    fn elige_el_hablante_con_mas_solape() {
        let spans = vec![
            SpeakerSpan {
                start: 0.0,
                end: 5.0,
                speaker: 0,
            },
            SpeakerSpan {
                start: 5.0,
                end: 12.0,
                speaker: 1,
            },
        ];
        assert_eq!(speaker_at(&spans, 1.0, 4.0), Some(0));
        assert_eq!(speaker_at(&spans, 4.0, 11.0), Some(1));
        assert_eq!(speaker_at(&spans, 20.0, 25.0), None);
    }
}
