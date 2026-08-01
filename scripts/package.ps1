# Arma la carpeta que se le pasa a la usuaria: se copia y se abre el .exe. Sin instalador,
# sin permisos de administrador, sin tocar el registro.
#
#   .\scripts\package.ps1 [-Target C:\ruta\de\salida] [-SkipBuild]

param(
  [string]$Target = "$PSScriptRoot\..\dist\Transcriptor",
  [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$root = Resolve-Path "$PSScriptRoot\.."
$build = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root "target" }
$rel = Join-Path $build "release"

if (-not $SkipBuild) {
  Write-Host "compilando..." -ForegroundColor Cyan
  Push-Location $root
  try { cargo build --release } finally { Pop-Location }
  if ($LASTEXITCODE -ne 0) { throw "la compilacion fallo" }
}

New-Item -ItemType Directory -Force $Target | Out-Null
$Target = (Resolve-Path $Target).Path

# --- ejecutable y bibliotecas nativas
Copy-Item (Join-Path $rel "transcriptor.exe") (Join-Path $Target "Transcriptor.exe") -Force
foreach ($dll in Get-ChildItem $rel -Filter *.dll) {
  Copy-Item $dll.FullName $Target -Force
}

# --- ffmpeg: hace falta para abrir formatos que el reproductor del sistema no toca
$ff = $env:WHISPER_FFMPEG
if (-not $ff -and (Test-Path (Join-Path $root "tools\ffmpeg.exe"))) { $ff = Join-Path $root "tools\ffmpeg.exe" }
if (-not $ff) { $ff = (Get-Command ffmpeg -ErrorAction SilentlyContinue).Source }
if ($ff -and (Test-Path $ff)) {
  Copy-Item $ff (Join-Path $Target "ffmpeg.exe") -Force
} else {
  Write-Warning "no encontre ffmpeg.exe; copialo a mano dentro de $Target"
}

# --- modelos
$src = Join-Path $root "models"
$dst = Join-Path $Target "models"
if (Test-Path $src) {
  Write-Host "copiando modelos (unos minutos)..." -ForegroundColor Cyan
  robocopy $src $dst /E /NFL /NDL /NJH /NJS /NP | Out-Null
  if ($LASTEXITCODE -ge 8) { throw "fallo la copia de modelos" }
} else {
  Write-Warning "no hay carpeta models/ que copiar"
}

@"
Transcriptor
============

Abri "Transcriptor.exe". No hace falta instalar nada.

Todo el procesamiento ocurre en esta computadora. La aplicacion no envia audio
ni texto a ningun servidor: no usa internet en ningun momento.

Como se usa
-----------
1. Arrastra un audio a la ventana (o usa "Abrir audio").
2. Espera a que termine. Podes escuchar mientras procesa.
3. Clic en un bloque -> lo copia entero al portapapeles, listo para pegar en Word.
   Doble clic -> lo corregis ahi mismo.
   Clic en la hora -> salta a ese punto del audio.

Atajos
------
  espacio       reproducir / pausar
  flechas       3 segundos atras / adelante  (con Shift, 10 segundos)
  arriba/abajo  bloque anterior / siguiente

Que significan las marcas
-------------------------
  "N a revisar"  Los dos modelos de reconocimiento no coincidieron en esas
                 palabras. Estan resaltadas en la linea en espanol.
  "rescatado"    El modelo principal se engancho repitiendo y ese bloque se
                 tomo del segundo modelo.

Ajustes que conviene conocer
----------------------------
  Calidad        "Buena" es mas exacta; "Rapida" tarda bastante menos.
  Hablantes      Casi todas las llamadas son de dos personas. Si son tres o
                 cuatro, cambialo aca: vuelve a separar en medio minuto, sin
                 reprocesar el audio.
  Limpiar ruido  Apagado a proposito. Ayuda en algunas grabaciones y arruina
                 otras. Si un audio sale mal, probalo y compara.
"@ | Set-Content (Join-Path $Target "LEEME.txt") -Encoding utf8

$mb = (Get-ChildItem $Target -Recurse -File | Measure-Object Length -Sum).Sum / 1GB
Write-Host ("listo: {0}  ({1:N2} GB)" -f $Target, $mb) -ForegroundColor Green
