# Transcriptor

Transcribe y traduce audio en la propia computadora. Pensado para llamadas telefónicas
en español de mala calidad que hay que pasar a inglés.

Nada sale a internet: no hay API, no hay nube, no hay telemetría. El audio y el texto
no salen nunca de la máquina.

## Qué hay acá

```
crates/pipeline/    núcleo: audio → VAD → reconocimiento → contraste → traducción
src-tauri/          la aplicación de escritorio
ui/                 la interfaz (HTML/CSS/JS, sin npm)
scripts/package.ps1 arma la carpeta que se le entrega a la usuaria
legacy/             la herramienta anterior, en Python
models/             los modelos (no versionados: ~4 GB)
```

## Cómo se eligieron los modelos

Todo lo de abajo está medido en esta máquina (i5-1035G1, 4 hilos, int8), sobre
FLEURS es_419 degradado a banda telefónica, y contrastado después con una llamada real.

Reconocimiento, tasa de error de palabra en español:

| modelo | limpio | telefónico | con ruido | RTF |
|---|---|---|---|---|
| Whisper large-v3 | 3,80 % | 4,39 % | — | 1,96 |
| Canary 1B v2 | 4,59 % | 6,24 % | 7,71 % | 0,43 |
| Parakeet TDT 0,6B v3 | 5,07 % | 7,22 % | 8,78 % | 0,15 |

Traducción al inglés, BLEU sobre el mismo conjunto:

| camino | BLEU | chrF |
|---|---|---|
| techo teórico (español perfecto → opus-mt) | 29,09 | 59,86 |
| **Canary → NLLB-600M** | **27,50** | **57,00** |
| Whisper large-v3 → NLLB-600M | 26,44 | 56,94 |
| Parakeet → NLLB-600M | 25,94 | 55,65 |
| Canary → opus-mt | 25,43 | 56,59 |
| Parakeet → opus-mt | 23,87 | 55,11 |
| Whisper `translate` (la herramienta anterior) | 22,10 | 54,70 |
| Canary traduciendo directo del audio | 20,90 | 52,20 |

Dos conclusiones que decidieron el diseño:

1. **Traducir el texto gana a traducir el audio.** Pedirle al modelo de voz que traduzca
   directamente pierde entre 4,5 y 6,6 BLEU frente a reconocer y después traducir.
2. **Menos error de reconocimiento no implica mejor inglés.** Whisper reconoce mejor que
   Canary y aun así produce peor traducción. No hay que elegir entre calidad y velocidad:
   Canary → NLLB gana en las dos cosas frente a lo que había.

## El explorador no recorre nada en profundidad

El material se organiza en una carpeta base con una subcarpeta por jornada («Lunes 3»,
«Miércoles 5»), y al lado hay carpetas con miles de audios viejos. Por eso el explorador
**lista un solo directorio y para**: buscar audios recursivamente ahí tardaría muchísimo y
no serviría para nada. Las carpetas con nombre de jornada —con tilde o sin ella— se
detectan, se destacan y se ordenan de más reciente a más vieja; el resto queda visible
pero apagado.

## Los dos modelos trabajan juntos

Canary lee mejor pero es un encoder-decoder, y esos se enganchan repitiendo: en la llamada
de prueba escribió «no» setenta y cuatro veces seguidas en un tramo de cinco segundos.
Es exactamente la avería que hacía inservible la herramienta anterior.

Parakeet es un transducer y no puede caer en eso. Así que corre como segunda opinión y hace
tres cosas a la vez:

- **marca las dudas**: las palabras en las que los dos no coinciden se resaltan;
- **presta los tiempos**: Canary no emite tiempos por palabra y Parakeet sí, así que el
  resaltado que sigue al audio es exacto;
- **rescata los bloques rotos**: si el principal degenera y el otro no, se usa el otro.

## Cosas que se probaron y no se usan

- **Reducción de ruido a ciegas.** GTCRN mejora el audio telefónico sintético y arruina
  el ruidoso; sobre la llamada real *borra habla*: de diez tramos con desacuerdo total,
  en cinco la versión limpiada devolvió texto vacío, uno de ellos de trece segundos
  perfectamente audible. Queda como interruptor manual, apagado por defecto.
- **Agrupar hablantes sin decir cuántos son.** Con el umbral de ejemplo de sherpa encontró
  quince hablantes en una conversación de dos. Se subió el umbral y se absorben los grupos
  de uno o dos segundos, pero el número sigue siendo ajustable a mano.
- **Contrastar el audio crudo contra el audio limpiado** como señal de confianza. Suena
  bien y no funciona: la versión limpiada no es una segunda opinión, es una entrada rota.
  Marcaba como dudoso el 32 % de las palabras sin motivo.

## Compilar

Hace falta Rust 1.88 o superior, MSVC y CMake (`ct2rs` compila CTranslate2).

```bat
set RUSTFLAGS=-C target-feature=+crt-static
cargo build --release
```

`RUSTFLAGS` no es opcional: `ct2rs` enlaza la CRT estática y sin eso el enlazado falla
con `LNK2038`. `sherpa-onnx` va por enlazado dinámico (`features = ["shared"]`) porque sus
bibliotecas estáticas precompiladas piden un MSVC más nuevo del que hay acá.

Para probar el núcleo sin interfaz:

```bat
set WHISPER_MODELS=C:\ruta\models
pipe.exe llamada.mp3 --asr canary --mt nllb --speakers 2 --json salida.json
```

## Entregar

```powershell
.\scripts\package.ps1               # 3,3 GB, todo incluido — para pendrive o disco
.\scripts\package.ps1 -SinModelos   # 122 MB — para mandar por internet
```

Deja en `dist/Transcriptor/` una carpeta que se copia y se abre. Sin instalador ni permisos
de administrador. La variante liviana trae un `Descargar modelos.bat` que baja los ~3,2 GB
de modelos en la máquina de destino, una sola vez.

Ver [INSTALACION.md](INSTALACION.md) para el paso a paso de las dos máquinas.
