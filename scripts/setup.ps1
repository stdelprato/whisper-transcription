# Deja la máquina lista para compilar: comprueba lo que hace falta, baja ffmpeg si no está,
# baja los modelos y compila.
#
#   .\scripts\setup.ps1            todo
#   .\scripts\setup.ps1 -Check     solo mira qué falta, no toca nada

param([switch]$Check)

$ErrorActionPreference = "Stop"
$root = Resolve-Path "$PSScriptRoot\.."
$tools = Join-Path $root "tools"

$falta = @()
function Ok($q)   { Write-Host "  [ok]    $q" -ForegroundColor Green }
function Falta($q, $como) {
  Write-Host "  [FALTA] $q" -ForegroundColor Yellow
  Write-Host "          $como" -ForegroundColor DarkGray
  $script:falta += $q
}

Write-Host "`nRequisitos" -ForegroundColor Cyan

# --- Rust
$rustc = Get-Command rustc -ErrorAction SilentlyContinue
if ($rustc) {
  $v = [version](& rustc --version).Split(' ')[1]
  if ($v -ge [version]"1.88.0") { Ok "Rust $v" }
  else { Falta "Rust $v es viejo (hace falta 1.88+)" "rustup update stable" }
} else {
  Falta "Rust" "https://rustup.rs  — instalá rustup y elegí la opción por defecto"
}

# --- MSVC + CMake: los necesita CTranslate2, que se compila desde fuente
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vs = if (Test-Path $vswhere) {
  & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
}
if ($vs) { Ok "compilador de C++ ($([IO.Path]::GetFileName($vs)))" }
else {
  Falta "Herramientas de C++ de Visual Studio" @"
https://visualstudio.microsoft.com/visual-cpp-build-tools/
          En el instalador marcá 'Desarrollo para el escritorio con C++'.
"@
}

if (Get-Command cmake -ErrorAction SilentlyContinue) { Ok "CMake" }
else { Falta "CMake" "https://cmake.org/download/  (marcá 'Add CMake to the system PATH')" }

# --- ffmpeg: si no está en el PATH lo dejamos en tools\, sin instalar nada
$ff = (Get-Command ffmpeg -ErrorAction SilentlyContinue).Source
if (-not $ff -and (Test-Path "$tools\ffmpeg.exe")) { $ff = "$tools\ffmpeg.exe" }
if ($ff) { Ok "ffmpeg ($ff)" } else { Falta "ffmpeg" "lo baja este mismo script" }

# --- modelos
$modelos = Join-Path $root "models"
$clave = @(
  "vad\silero_vad.onnx",
  "asr\parakeet-tdt-0.6b-v3-int8\encoder.int8.onnx",
  "asr\canary-1b-v2-int8\encoder.int8.onnx",
  "mt\opus-es-en\model.bin",
  "mt\nllb-200-distilled-600M\model.bin"
)
$sinModelo = $clave | Where-Object { -not (Test-Path (Join-Path $modelos $_)) }
if ($sinModelo) { Falta "modelos ($($sinModelo.Count) de $($clave.Count) sin bajar)" "lo hace este mismo script" }
else { Ok "modelos" }

if ($Check) {
  Write-Host ""
  if ($falta) {
    Write-Host "faltan $($falta.Count) cosas" -ForegroundColor Yellow
    exit 1
  }
  Write-Host "está todo" -ForegroundColor Green
  exit 0
}

# --- lo que no se puede automatizar
$manual = $falta | Where-Object { $_ -notmatch "^(ffmpeg|modelos)" }
if ($manual) {
  Write-Host "`nEsto hay que instalarlo a mano antes de seguir:" -ForegroundColor Yellow
  $manual | ForEach-Object { Write-Host "  - $_" }
  exit 1
}

# --- ffmpeg
if (-not $ff) {
  Write-Host "`nBajando ffmpeg…" -ForegroundColor Cyan
  New-Item -ItemType Directory -Force $tools | Out-Null
  $zip = Join-Path $env:TEMP "ffmpeg.zip"
  curl.exe -L --fail --progress-bar -o $zip `
    "https://github.com/GyanD/codexffmpeg/releases/latest/download/ffmpeg-release-essentials.zip"
  if ($LASTEXITCODE -ne 0) {
    Write-Warning "no pude bajar ffmpeg; bajalo de https://www.gyan.dev/ffmpeg/builds/ y poné ffmpeg.exe en $tools"
  } else {
    $work = Join-Path $env:TEMP "ffmpeg-desempaque"
    Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
    Expand-Archive $zip $work -Force
    $exe = Get-ChildItem $work -Recurse -Filter ffmpeg.exe | Select-Object -First 1
    Copy-Item $exe.FullName "$tools\ffmpeg.exe" -Force
    Remove-Item $work -Recurse -Force; Remove-Item $zip -Force
    $ff = "$tools\ffmpeg.exe"
    Write-Host "  ffmpeg en $ff" -ForegroundColor Green
  }
}

# --- modelos
if ($sinModelo) { & "$PSScriptRoot\get-models.ps1" }

# --- compilar
Write-Host "`nCompilando (la primera vez tarda: CTranslate2 se compila desde fuente)…" -ForegroundColor Cyan
$env:RUSTFLAGS = "-C target-feature=+crt-static"
if ($ff) { $env:WHISPER_FFMPEG = $ff }
Push-Location $root
try { cargo build --release } finally { Pop-Location }
if ($LASTEXITCODE -ne 0) { throw "la compilación falló" }

Write-Host "`nListo." -ForegroundColor Green
Write-Host "  probar acá:     cargo run --release -p transcriptor"
Write-Host "  armar entrega:  .\scripts\package.ps1"
