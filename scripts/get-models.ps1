# Descarga los modelos. Son ~3,2 GB; tarda según la conexión.
# No hace falta Python ni nada más: todo viene ya cuantizado a int8.
#
#   .\scripts\get-models.ps1              desde el repo
#   .\descargar-modelos.ps1               desde la carpeta de la app
#   .\get-models.ps1 -Dest D:\otro\lado
#
# Se puede cortar y volver a lanzar: lo ya bajado no se vuelve a bajar.

param([string]$Dest)

$ErrorActionPreference = "Stop"

if (-not $Dest) {
  # En el repo los modelos van en ..\models; en la carpeta entregada, al lado del script.
  $Dest = if (Test-Path (Join-Path $PSScriptRoot "..\Cargo.toml")) {
    Join-Path (Resolve-Path (Join-Path $PSScriptRoot "..")) "models"
  } else {
    Join-Path $PSScriptRoot "models"
  }
}
$M = $Dest

$SHERPA = "https://github.com/k2-fsa/sherpa-onnx/releases/download"
$HF = "https://huggingface.co"

function Fetch($url, $dst) {
  if (Test-Path $dst) { Write-Host "  ya está: $(Split-Path $dst -Leaf)" -ForegroundColor DarkGray; return }
  New-Item -ItemType Directory -Force (Split-Path $dst) | Out-Null
  Write-Host "  bajando $(Split-Path $dst -Leaf)…"
  $tmp = "$dst.parcial"
  curl.exe -L --fail --progress-bar -o $tmp $url
  if ($LASTEXITCODE -ne 0) { Remove-Item $tmp -ErrorAction SilentlyContinue; throw "falló la descarga: $url" }
  Move-Item $tmp $dst -Force
}

# Baja un .tar.bz2 de sherpa y saca los archivos que interesan.
function FetchArchive($url, $dstDir, $map) {
  if (Test-Path (Join-Path $dstDir ($map.Values | Select-Object -First 1))) {
    Write-Host "  ya está: $(Split-Path $dstDir -Leaf)" -ForegroundColor DarkGray; return
  }
  $tmp = Join-Path $env:TEMP ("modelo-" + [IO.Path]::GetFileName($url))
  Fetch $url $tmp
  $work = Join-Path $env:TEMP ("desempaque-" + [Guid]::NewGuid().ToString("N").Substring(0, 8))
  New-Item -ItemType Directory -Force $work | Out-Null
  try {
    tar -xf $tmp -C $work
    if ($LASTEXITCODE -ne 0) { throw "no pude desempaquetar $tmp" }
    New-Item -ItemType Directory -Force $dstDir | Out-Null
    foreach ($origen in $map.Keys) {
      $encontrado = Get-ChildItem $work -Recurse -File -Filter (Split-Path $origen -Leaf) |
        Where-Object { $_.FullName -like "*$($origen -replace '/', '\')" } | Select-Object -First 1
      if (-not $encontrado) { throw "el paquete no trae $origen" }
      Copy-Item $encontrado.FullName (Join-Path $dstDir $map[$origen]) -Force
    }
  } finally {
    Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item $tmp -Force -ErrorAction SilentlyContinue
  }
}

function FetchHf($repo, $files, $dstDir) {
  foreach ($f in $files) { Fetch "$HF/$repo/resolve/main/$f" (Join-Path $dstDir $f) }
}

Write-Host "`n[1/6] detección de voz" -ForegroundColor Cyan
Fetch "$SHERPA/asr-models/silero_vad.onnx" "$M\vad\silero_vad.onnx"

Write-Host "`n[2/6] reducción de ruido" -ForegroundColor Cyan
Fetch "$SHERPA/speech-enhancement-models/gtcrn_simple.onnx" "$M\denoise\gtcrn_simple.onnx"

Write-Host "`n[3/6] separación de hablantes" -ForegroundColor Cyan
FetchArchive "$SHERPA/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2" `
  "$M\diar" @{ "model.onnx" = "segmentation.onnx" }
Fetch "$SHERPA/speaker-recongition-models/3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx" `
  "$M\diar\embedding.onnx"

Write-Host "`n[4/6] Parakeet TDT 0.6B v3  (~670 MB)" -ForegroundColor Cyan
FetchArchive "$SHERPA/asr-models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2" `
  "$M\asr\parakeet-tdt-0.6b-v3-int8" @{
    "encoder.int8.onnx" = "encoder.int8.onnx"
    "decoder.int8.onnx" = "decoder.int8.onnx"
    "joiner.int8.onnx"  = "joiner.int8.onnx"
    "tokens.txt"        = "tokens.txt"
  }

Write-Host "`n[5/6] Canary 1B v2  (~1,8 GB)" -ForegroundColor Cyan
FetchHf "YaroslavGor/sherpa-onnx-nemo-canary-1b-v2-int" `
  @("encoder.int8.onnx", "encoder.int8.onnx.data", "decoder.int8.onnx", "tokens.txt") `
  "$M\asr\canary-1b-v2-int8"

Write-Host "`n[6/6] traductores  (~670 MB)" -ForegroundColor Cyan
FetchHf "mijuanlo/opus-mt-es-en-ct2-int8" `
  @("model.bin", "config.json", "shared_vocabulary.json", "source.spm", "target.spm") `
  "$M\mt\opus-es-en"
FetchHf "JustFrederik/nllb-200-distilled-600M-ct2-int8" `
  @("model.bin", "config.json", "shared_vocabulary.txt", "tokenizer.json", "tokenizer_config.json",
    "special_tokens_map.json", "sentencepiece.bpe.model") `
  "$M\mt\nllb-200-distilled-600M"

$gb = (Get-ChildItem $M -Recurse -File | Measure-Object Length -Sum).Sum / 1GB
Write-Host ("`nlisto: {0}  ({1:N2} GB)" -f $M, $gb) -ForegroundColor Green
if (Test-Path (Join-Path $PSScriptRoot "Transcriptor.exe")) {
  Write-Host "ya podés abrir Transcriptor.exe" -ForegroundColor Green
  Write-Host "`n(esta ventana se cierra sola en 20 segundos)" -ForegroundColor DarkGray
  Start-Sleep -Seconds 20
}
exit 0
