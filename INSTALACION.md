# Instalación

Hay dos máquinas con papeles distintos:

- **La tuya** compila y arma la entrega. Necesita herramientas de desarrollo.
- **La de ella** solo abre la app. **No necesita instalar absolutamente nada.**

---

## En tu máquina

### 1. Instalar tres cosas a mano

Solo estas tres; el resto lo hace el script.

| qué | de dónde | nota |
|---|---|---|
| **Rust 1.88+** | <https://rustup.rs> | aceptá la opción por defecto. Si ya lo tenés: `rustup update stable` |
| **Herramientas de C++ de Visual Studio** | <https://visualstudio.microsoft.com/visual-cpp-build-tools/> | en el instalador marcá **«Desarrollo para el escritorio con C++»** |
| **CMake** | <https://cmake.org/download/> | marcá **«Add CMake to the system PATH»** |

Las dos últimas hacen falta porque CTranslate2 (el motor de traducción) se compila
desde fuente. No hay una versión precompilada para Rust en Windows.

Para ver qué te falta sin tocar nada:

```powershell
.\scripts\setup.ps1 -Check
```

### 2. El resto es un comando

```powershell
git clone https://github.com/stdelprato/whisper-transcription.git
cd whisper-transcription
.\scripts\setup.ps1
```

Eso baja ffmpeg, baja los modelos (~3,2 GB) y compila. La primera compilación tarda
un rato largo porque incluye CTranslate2; las siguientes son de un par de minutos.

Para probarlo:

```powershell
$env:WHISPER_MODELS = "$PWD\models"
$env:WHISPER_FFMPEG = "$PWD\tools\ffmpeg.exe"
cargo run --release -p transcriptor
```

O sin interfaz, para ver los números:

```powershell
.\target\release\pipe.exe llamada.mp3 --preset balanced --speakers 2
```

### 3. Armar lo que le pasás a ella

Hay dos formas, según cómo se lo hagas llegar.

**Si es por pendrive o disco** — todo adentro, ella no toca nada:

```powershell
.\scripts\package.ps1
```

Deja `dist\Transcriptor\`, unos **3,3 GB**: el ejecutable, las bibliotecas, ffmpeg,
los modelos y un `LEEME.txt`.

**Si es por internet** — 3,3 GB por Drive es una tortura de subida y de bajada, y 3,1 de
esos GB son modelos públicos que se bajan igual de rápido (o más) desde la fuente:

```powershell
.\scripts\package.ps1 -SinModelos
```

Deja **122 MB**. Eso es lo que subís. En su máquina, doble clic en
**`Descargar modelos.bat`** una sola vez y listo.

---

## En la máquina de ella

1. Copiale la carpeta `Transcriptor` entera. Que quede en un lugar fijo,
   por ejemplo `C:\Transcriptor`.
2. Si la armaste con `-SinModelos`: doble clic en **`Descargar modelos.bat`**
   y dejalo terminar (~3,2 GB, una sola vez).
3. Doble clic en **`Transcriptor.exe`**.

No hay instalador, no pide permisos de administrador y no toca el registro. Pasado el
paso 2, la aplicación no vuelve a abrir ninguna conexión de red.

### Si viene de una descarga, Windows la va a marcar

Cualquier `.exe` bajado de internet arrastra una marca que dispara SmartScreen:
*«Windows protegió tu PC»*. Es porque el ejecutable no está firmado, no porque haya algo
raro. Se resuelve con **Más información → Ejecutar de todas formas**.

Para evitarle el susto, quitale la marca a toda la carpeta antes de que la abra:

```powershell
Get-ChildItem C:\Transcriptor -Recurse | Unblock-File
```

Conviene mandarle un acceso directo al escritorio: clic derecho sobre `Transcriptor.exe`
→ *Enviar a* → *Escritorio (crear acceso directo)*.

### Lo único que podría faltarle

**WebView2.** Es el componente de Microsoft que dibuja la interfaz. Viene de fábrica en
Windows 11 y en Windows 10 actualizado, así que lo más probable es que ya esté. Si al abrir
la app no aparece ninguna ventana, instalá el *Evergreen Bootstrapper* desde
<https://developer.microsoft.com/microsoft-edge/webview2/> — son 2 MB y se instala solo.

Para comprobarlo de antemano, en su máquina:

```powershell
Get-ItemProperty "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" -ErrorAction SilentlyContinue | Select-Object pv
```

Si devuelve un número de versión, está.

### Cuánto va a tardar

Medido en la máquina de desarrollo (i5-1035G1). La suya (i3-1115G4, 2 núcleos) es algo
más lenta, y la app se recalibra sola con las primeras corridas y muestra el tiempo real.

| calidad | 1 hora de audio |
|---|---|
| Rápida | ~40 min |
| Equilibrada (por defecto) | ~1 h 30 |
| Máxima | ~2 h 40 |

Usa **1,4 GB de memoria** como máximo, así que entra sin problema en sus 8 GB.

---

## Si algo falla

**«No encuentro la carpeta de modelos»** — la carpeta `models` tiene que estar al lado del
`.exe`. Si la moviste, definí la variable `WHISPER_MODELS` con la ruta.

**«No encuentro ffmpeg.exe»** — igual: tiene que estar al lado del `.exe`, o en la variable
`WHISPER_FFMPEG`.

**Un audio no se reproduce** — la app lo convierte sola a un temporal cuando el formato no
es reproducible (wma, amr). Si falla, es que ffmpeg no está donde debería.

**Se cerró a mitad de un audio largo** — no se perdió: el resultado se guarda solo, al lado
del audio, como `<nombre>.transcripcion.json`. Al volver a abrir ese audio lo recupera en
lugar de reprocesarlo.
