//! Marcas de baja confianza contrastando dos modelos distintos.
//!
//! sherpa-onnx no expone probabilidades por token: el JSON del reconocedor trae
//! `text`, `tokens`, `timestamps` y `durations`, nada más. Lo comprobé leyendo la salida
//! cruda de la C API. Así que la confianza hay que construirla de otra forma.
//!
//! El primer intento fue reconocer dos veces, con el audio crudo y con el audio limpiado.
//! No sirve: sobre la llamada real el reductor de ruido **borra habla**. De diez tramos
//! con desacuerdo total, en cinco la versión limpiada devolvió texto vacío, incluido uno
//! de trece segundos perfectamente audible. Eso no es una segunda opinión, es una entrada
//! rota, y marcaba como dudoso el 32 % de las palabras sin motivo.
//!
//! Lo que sí es una segunda opinión: **el otro modelo**. Parakeet es un transducer y
//! Canary un encoder-decoder; son arquitecturas independientes, entrenadas por separado.
//! Cuando las dos leen lo mismo, casi seguro está bien; cuando difieren, ahí conviene mirar.
//! De paso Parakeet aporta los tiempos por palabra que Canary no da.

use std::collections::HashMap;

use crate::asr::RawWord;

/// Normaliza para comparar: minúsculas, sin puntuación, sin tildes.
fn norm(s: &str) -> String {
    s.chars()
        .filter_map(|c| {
            let c = c.to_lowercase().next()?;
            Some(match c {
                'á' => 'a',
                'é' => 'e',
                'í' => 'i',
                'ó' => 'o',
                'ú' | 'ü' => 'u',
                c if c.is_alphanumeric() => c,
                _ => return None,
            })
        })
        .collect()
}

/// Subsecuencia común más larga: devuelve los pares `(i, j)` de palabras que coinciden.
fn lcs_pairs(a: &[String], b: &[String]) -> Vec<(usize, usize)> {
    let (n, m) = (a.len(), b.len());
    if n == 0 || m == 0 {
        return Vec::new();
    }
    // Tabla completa: los tramos son cortos (segundos de habla), así que cabe de sobra.
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if a[i] == b[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut pairs = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if a[i] == b[j] {
            pairs.push((i, j));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    pairs
}

/// Resultado de contrastar dos transcripciones del mismo tramo.
pub struct Comparison {
    /// Una marca por palabra de `primary`: `true` = el otro modelo dijo otra cosa.
    pub low_conf: Vec<bool>,
    /// Cuánto coinciden, de 0 a 1.
    pub agreement: f32,
    /// Pares `(índice en primary, índice en reference)` de palabras coincidentes.
    pub pairs: Vec<(usize, usize)>,
}

/// Contrasta la transcripción de referencia contra la principal.
pub fn compare(primary: &[RawWord], reference: &[RawWord]) -> Comparison {
    let a: Vec<String> = primary
        .iter()
        .map(|w| norm(&w.text))
        .collect();
    let b: Vec<String> = reference
        .iter()
        .map(|w| norm(&w.text))
        .collect();
    let pairs = lcs_pairs(&a, &b);
    let denom = a.len() + b.len();
    let agreement = if denom == 0 {
        1.0
    } else {
        2.0 * pairs.len() as f32 / denom as f32
    };
    let mut low_conf = vec![true; a.len()];
    for (i, _) in &pairs {
        low_conf[*i] = false;
    }
    Comparison {
        low_conf,
        agreement,
        pairs,
    }
}

/// Copia los tiempos del modelo que sí los mide a las palabras del que no.
///
/// Las palabras que los dos modelos comparten toman el tiempo medido tal cual. Las que
/// solo dice el modelo principal se reparten proporcionalmente entre las dos coincidencias
/// que las rodean, así que quedan dentro del hueco correcto aunque no sean exactas.
pub fn transfer_timings(
    primary: &mut [RawWord],
    reference: &[RawWord],
    pairs: &[(usize, usize)],
    span_start: f32,
    span_end: f32,
) {
    if pairs.is_empty() || primary.is_empty() {
        return;
    }
    for &(i, j) in pairs {
        primary[i].start = reference[j].start;
        primary[i].end = reference[j].end;
    }

    // Anclas: inicio del tramo, cada coincidencia, y final del tramo.
    let mut anchor_idx: Vec<usize> = pairs
        .iter()
        .map(|&(i, _)| i)
        .collect();
    anchor_idx.dedup();

    let first = anchor_idx[0];
    let last = *anchor_idx
        .last()
        .expect("hay al menos un ancla");

    // Antes de la primera coincidencia: desde el inicio del tramo, no desde cero.
    let head_end = primary[first].start;
    interpolate(&mut primary[..first], span_start.min(head_end), head_end);
    // Después de la última.
    let tail_start = primary[last].end;
    interpolate(&mut primary[last + 1..], tail_start, span_end.max(tail_start));
    // Huecos intermedios.
    for w in anchor_idx.windows(2) {
        let (a, b) = (w[0], w[1]);
        if b > a + 1 {
            let (from, to) = (primary[a].end, primary[b].start);
            interpolate(&mut primary[a + 1..b], from, to.max(from));
        }
    }
}

/// Reparte `[from, to]` entre las palabras dadas, en proporción a su longitud.
fn interpolate(words: &mut [RawWord], from: f32, to: f32) {
    if words.is_empty() {
        return;
    }
    let total: f32 = words
        .iter()
        .map(|w| {
            w.text
                .chars()
                .count() as f32
                + 1.0
        })
        .sum();
    let span = (to - from).max(0.0);
    let mut t = from;
    for w in words.iter_mut() {
        let share = span
            * (w.text
                .chars()
                .count() as f32
                + 1.0)
            / total;
        w.start = t;
        w.end = t + share;
        t += share;
    }
}

/// Cuántas veces se repite el n-grama más frecuente. Un valor alto delata el bucle
/// de alucinación clásico de los decodificadores autorregresivos (la herramienta vieja
/// llegó a repetir una frase 3903 veces).
pub fn loop_score(text: &str, n: usize) -> usize {
    let words: Vec<String> = text
        .split_whitespace()
        .map(norm)
        .filter(|w| !w.is_empty())
        .collect();
    if words.len() < n * 2 {
        return 0;
    }
    let mut counts: HashMap<&[String], usize> = HashMap::new();
    for w in words.windows(n) {
        *counts
            .entry(w)
            .or_insert(0) += 1;
    }
    counts
        .into_values()
        .max()
        .unwrap_or(0)
}

/// Umbral a partir del cual damos un tramo por degenerado.
pub const LOOP_LIMIT: usize = 6;

#[cfg(test)]
mod tests {
    use super::*;

    fn words(s: &str) -> Vec<RawWord> {
        s.split_whitespace()
            .enumerate()
            .map(|(i, w)| RawWord {
                text: w.to_string(),
                start: i as f32,
                end: i as f32 + 1.0,
            })
            .collect()
    }

    #[test]
    fn identico_no_marca_nada() {
        let a = words("hola que tal estas");
        let c = compare(&a, &a);
        assert!(c
            .low_conf
            .iter()
            .all(|x| !x));
        assert!((c.agreement - 1.0).abs() < 1e-6);
    }

    #[test]
    fn marca_solo_lo_que_difiere() {
        let a = words("tienes audiencia el martes");
        let b = words("tienes ausencia el martes");
        let c = compare(&a, &b);
        assert_eq!(c.low_conf, vec![false, true, false, false]);
        assert!(c.agreement > 0.7 && c.agreement < 1.0);
    }

    #[test]
    fn tildes_y_puntuacion_no_cuentan() {
        let a = words("¿Qué onda?");
        let b = words("que onda");
        let c = compare(&a, &b);
        assert!(c
            .low_conf
            .iter()
            .all(|x| !x));
    }

    #[test]
    fn los_tiempos_medidos_pasan_al_otro_modelo() {
        // El modelo principal no tiene tiempos reales; el de referencia sí.
        let mut principal = words("hoy tenemos audiencia temprano");
        for w in &mut principal {
            w.start = 0.0;
            w.end = 0.0;
        }
        let referencia = vec![
            RawWord {
                text: "hoy".into(),
                start: 1.0,
                end: 1.4,
            },
            RawWord {
                text: "tenemos".into(),
                start: 1.4,
                end: 2.0,
            },
            RawWord {
                text: "audiencia".into(),
                start: 2.0,
                end: 2.9,
            },
        ];
        let c = compare(&principal, &referencia);
        transfer_timings(&mut principal, &referencia, &c.pairs, 0.8, 4.0);

        assert_eq!(principal[0].start, 1.0);
        assert_eq!(principal[2].end, 2.9);
        // "temprano" no está en la referencia: cae en el hueco hasta el final del tramo.
        assert!(principal[3].start >= 2.9 && principal[3].end <= 4.0);
    }

    #[test]
    fn lo_que_va_antes_del_primer_acuerdo_arranca_en_el_tramo() {
        // El modelo principal oyó una palabra de más al principio.
        let mut principal = words("eh tenemos audiencia");
        let referencia = vec![
            RawWord {
                text: "tenemos".into(),
                start: 5.4,
                end: 6.0,
            },
            RawWord {
                text: "audiencia".into(),
                start: 6.0,
                end: 6.8,
            },
        ];
        let c = compare(&principal, &referencia);
        transfer_timings(&mut principal, &referencia, &c.pairs, 5.0, 7.0);

        // "eh" tiene que caer entre el inicio del tramo y la primera palabra medida,
        // no al principio del archivo.
        assert!(principal[0].start >= 5.0, "arrancó en {}", principal[0].start);
        assert!(principal[0].end <= 5.4);
        assert_eq!(principal[1].start, 5.4);
    }

    #[test]
    fn detecta_bucle() {
        let sano = "hoy vamos a revisar el expediente antes de la audiencia del martes";
        assert!(loop_score(sano, 4) < LOOP_LIMIT);
        let roto = "gracias por ver el video ".repeat(20);
        assert!(loop_score(&roto, 4) >= LOOP_LIMIT);
    }
}
