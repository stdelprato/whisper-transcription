"use strict";

const { invoke, convertFileSrc } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);
const audio = new Audio();

const state = {
  /** cola de trabajo: {path, name, status: pendiente|procesando|listo|falló, transcript} */
  queue: [],
  /** el archivo que se está viendo */
  shown: null,
  /** el que se está procesando */
  busy: null,
  segments: [],
  active: -1,
  startedAt: 0,
  duration: 0,
};

/* ------------------------------------------------------------------ ayudas */

function fmt(t) {
  if (!isFinite(t) || t < 0) t = 0;
  const s = Math.floor(t);
  const m = Math.floor(s / 60);
  return `${m}:${String(s % 60).padStart(2, "0")}`;
}

function esc(s) {
  return String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
}

const nombre = (p) => p.split(/[\\/]/).pop();

let toastTimer;
function toast(msg) {
  const el = $("toast");
  el.textContent = msg;
  el.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (el.hidden = true), 1800);
}

/* ------------------------------------------------------- ajustes y arranque */

function readOptions() {
  const o = {
    asr: $("o-asr").value,
    mt: $("o-mt").value,
    verify: $("o-verify").checked,
    denoise: $("o-denoise").checked,
    diarize: $("o-diarize").checked,
    speakers: parseInt($("o-speakers").value, 10) || 0,
    source_lang: $("o-lang").value,
    threads: parseInt($("o-threads").value, 10) || 4,
  };
  const p = PRESETS[$("o-preset").value];
  if (p) { o.asr = p.asr; o.mt = p.mt; o.verify = p.verify; }
  if (o.source_lang === "en") o.mt = "none";
  return o;
}

/**
 * Los tres niveles. `rtf` es cuánto tarda por segundo de audio, medido en la máquina
 * de prueba; se reemplaza por lo que tarde de verdad en esta en cuanto haya datos.
 */
const PRESETS = {
  fast: { asr: "parakeet", mt: "opus", verify: false, rtf: 0.64 },
  balanced: { asr: "canary", mt: "opus", verify: true, rtf: 1.49 },
  best: { asr: "canary", mt: "nllb", verify: true, rtf: 2.65 },
};

/** Lo que tardó de verdad, por preset, guardado entre sesiones. */
function rtfReal(nombre) {
  try {
    const v = JSON.parse(localStorage.getItem("rtf") || "{}")[nombre];
    return typeof v === "number" && v > 0 ? v : null;
  } catch { return null; }
}

function anotarRtf(nombre, rtf) {
  if (!(rtf > 0) || !isFinite(rtf)) return;
  try {
    const todos = JSON.parse(localStorage.getItem("rtf") || "{}");
    // Media móvil suave: una corrida rara no debería mover mucho la estimación.
    todos[nombre] = todos[nombre] ? todos[nombre] * 0.7 + rtf * 0.3 : rtf;
    localStorage.setItem("rtf", JSON.stringify(todos));
  } catch { /* sin almacenamiento, no pasa nada */ }
}

function mostrarEstimacion() {
  const nombre = $("o-preset").value;
  const p = PRESETS[nombre];
  const el = $("estimacion");
  if (!p) { el.textContent = ""; return; }
  const medido = rtfReal(nombre);
  const rtf = medido ?? p.rtf;
  const min = Math.round(rtf * 60);
  el.textContent = `1 hora de audio ≈ ${min < 60 ? `${min} min` : `${(rtf).toFixed(1)} h`}`
    + (medido ? " (medido acá)" : " (estimado)");
}

function syncPreset() {
  const nombre = $("o-preset").value;
  const custom = nombre === "custom";
  const p = PRESETS[nombre];
  if (p) {
    $("o-asr").value = p.asr;
    $("o-mt").value = p.mt;
    $("o-verify").checked = p.verify;
  }
  const soloTranscribir = $("o-lang").value === "en";
  $("o-asr").disabled = !custom;
  $("o-mt").disabled = !custom || soloTranscribir;
  $("o-verify").disabled = !custom;
  for (const el of document.querySelectorAll(".custom-only")) el.style.opacity = custom ? "1" : ".45";
  mostrarEstimacion();
}

async function boot() {
  const info = await invoke("app_info");
  $("o-threads").value = info.threads;
  const problemas = [];
  if (!info.models_root) problemas.push("No encuentro la carpeta de modelos.");
  else if (info.missing.length) problemas.push("Faltan modelos: " + info.missing.join(", "));
  if (!info.ffmpeg) problemas.push("No encuentro ffmpeg.exe.");
  if (problemas.length) {
    $("warn").textContent = problemas.join(" ");
    $("warn").hidden = false;
    $("options").hidden = false;
  }
  syncPreset();
}

/* ------------------------------------------------------------------- cola */

async function enqueue(paths) {
  const nuevos = paths.filter((p) => !state.queue.some((q) => q.path === p));
  if (!nuevos.length) return;
  for (const path of nuevos) {
    // `live` acumula lo que va llegando mientras se procesa, para poder cambiar de
    // archivo y volver sin perder lo que ya se veía.
    state.queue.push({ path, name: nombre(path), status: "pendiente", transcript: null, live: [] });
  }
  drawQueue();

  // Si un audio ya se procesó antes, se recupera en vez de repetirlo.
  let recuperados = 0;
  for (const path of nuevos) {
    const previo = await invoke("load_result", { path }).catch(() => null);
    if (!previo) continue;
    const f = state.queue.find((x) => x.path === path);
    f.status = "listo";
    f.transcript = previo;
    f.live = previo.segments;
    recuperados++;
  }
  if (recuperados) toast(`${recuperados} ${recuperados === 1 ? "audio ya estaba" : "audios ya estaban"} procesados`);

  drawQueue();
  if (!state.shown) showFile(nuevos[0]);
  processNext();
}

function drawQueue() {
  const q = $("queue");
  q.hidden = state.queue.length < 1;
  q.innerHTML = state.queue.map((f) => {
    const clases = ["chip"];
    if (f.status === "listo") clases.push("done");
    else if (f.status === "falló") clases.push("failed");
    if (f.path === state.busy || f.path === state.shown) clases.push("current");
    return `<span class="${clases.join(" ")}" data-path="${esc(f.path)}">
      <span class="dot"></span>${esc(f.name)}</span>`;
  }).join("");
}

$("queue").addEventListener("click", (e) => {
  const chip = e.target.closest(".chip");
  if (!chip) return;
  const f = state.queue.find((x) => x.path === chip.dataset.path);
  if (f && f.status === "listo") showFile(f.path);
});

async function processNext() {
  if (state.busy) return;
  const f = state.queue.find((x) => x.status === "pendiente");
  if (!f) return;
  state.busy = f.path;
  f.status = "procesando";
  drawQueue();
  if (!state.shown) showFile(f.path);

  state.startedAt = Date.now();
  $("progress").hidden = false;
  $("fill").style.width = "0%";
  $("stage").textContent = `abriendo ${f.name}…`;
  $("eta").textContent = "";
  try {
    await invoke("start", { path: f.path, opts: readOptions() });
  } catch (e) {
    f.status = "falló";
    state.busy = null;
    $("progress").hidden = true;
    drawQueue();
    toast(String(e));
    processNext();
  }
}

/* --------------------------------------------------- mostrar un archivo */

async function showFile(path) {
  const f = state.queue.find((x) => x.path === path);
  if (!f) return;
  state.shown = path;
  state.active = -1;
  state.segments = f.transcript ? f.transcript.segments : f.live;
  $("file").textContent = f.name;
  $("empty").hidden = true;
  drawAll();
  drawQueue();
  $("toggle-export").disabled = !f.transcript;
  if (f.transcript) await invoke("set_transcript", { transcript: f.transcript });

  try {
    const playable = await invoke("prepare_playback", { path });
    audio.pause();
    audio.src = convertFileSrc(playable);
    $("player").hidden = false;
  } catch (e) {
    toast("No puedo reproducir este archivo: " + e);
  }
}

/* --------------------------------------------------------------- progreso */

const PESO = { asr: 0.55, verify: 0.2, mt: 0.25 };

function setProgress(frac, texto) {
  $("fill").style.width = (Math.min(1, Math.max(0, frac)) * 100).toFixed(1) + "%";
  $("stage").textContent = texto;
  const t = (Date.now() - state.startedAt) / 1000;
  $("eta").textContent = frac > 0.05 && frac < 1 ? "faltan ~" + fmt(t / frac - t) : "";
}

listen("progress", (ev) => {
  const p = ev.payload;
  const f = state.queue.find((x) => x.path === state.busy);
  if (!f) return;
  // Los bloques se acumulan siempre; se dibujan solo si es el archivo que se está mirando.
  const mirando = state.shown === state.busy;

  switch (p.kind) {
    case "decoded":
      state.duration = p.duration;
      setProgress(0.01, `audio de ${fmt(p.duration)}`);
      break;
    case "segmented":
      setProgress(0.02, `${p.count} tramos de habla`);
      break;
    case "diarized":
      setProgress(0.05, `${p.speakers} ${p.speakers === 1 ? "hablante" : "hablantes"}`);
      break;
    case "recognized":
      upsert(f, p.segment, mirando);
      setProgress(0.05 + PESO.asr * (p.done / p.total), `reconociendo ${p.done} de ${p.total}`);
      break;
    case "verifying":
      setProgress(0.6, "contrastando con el segundo modelo…");
      break;
    case "refined":
      upsert(f, p.segment, mirando);
      setProgress(0.6 + PESO.verify * (p.done / p.total), `contrastando ${p.done} de ${p.total}`);
      break;
    case "translating":
      setProgress(0.8, "traduciendo…");
      break;
    case "translated": {
      const seg = f.live.find((s) => s.id === p.id);
      if (seg) {
        seg.target = p.text;
        if (mirando) render(seg);
      }
      const hechos = f.live.filter((s) => s.target != null).length;
      setProgress(0.8 + PESO.mt * (hechos / Math.max(1, f.live.length)),
        `traduciendo ${hechos} de ${f.live.length}`);
      break;
    }
  }
});

listen("finished", async (ev) => {
  const t = ev.payload;
  const f = state.queue.find((x) => x.path === state.busy);
  if (f) { f.status = "listo"; f.transcript = t; f.live = t.segments; }
  state.busy = null;
  if (!t.cancelled && t.duration > 0) {
    anotarRtf($("o-preset").value, t.timings.total / t.duration);
    mostrarEstimacion();
  }
  $("progress").hidden = true;
  drawQueue();

  if (!f || state.shown === f.path) {
    state.segments = t.segments;
    drawAll();
    $("toggle-export").disabled = false;
    await invoke("set_transcript", { transcript: t });
  }
  const dudosas = t.segments.reduce((n, s) => n + s.words.filter((w) => w.low_conf).length, 0);
  toast(t.cancelled
    ? `detenido — ${t.segments.length} bloques listos`
    : `${f ? f.name : ""} listo en ${fmt(t.timings.total)} · ${t.segments.length} bloques · ${dudosas} para revisar`);
  processNext();
});

listen("failed", (ev) => {
  const f = state.queue.find((x) => x.path === state.busy);
  if (f) f.status = "falló";
  state.busy = null;
  $("progress").hidden = true;
  drawQueue();
  toast("Falló: " + ev.payload);
  processNext();
});

/* --------------------------------------------------------------- bloques */

function nodeFor(id) {
  return $("segments").querySelector(`[data-id="${id}"]`);
}

/** El nombre que le pusieron al hablante, o "Hablante N". */
function nombreHablante(k) {
  const f = state.queue.find((x) => x.path === state.shown);
  const puesto = f?.transcript?.speaker_names?.[k];
  return puesto && puesto.trim() ? puesto : `Hablante ${k + 1}`;
}

function segmentHtml(seg) {
  const spk = seg.speaker == null ? ""
    : `<span class="badge spk spk-${(seg.speaker % 3) + 1}" data-spk="${seg.speaker}"
         title="Doble clic para ponerle nombre">${esc(nombreHablante(seg.speaker))}</span>`;
  const rescatado = seg.repaired
    ? `<span class="badge repaired" title="El modelo principal se enganchó repitiendo; este texto viene del segundo modelo">rescatado</span>`
    : "";
  const n = seg.words.filter((w) => w.low_conf).length;
  const marca = n
    ? `<span class="badge low" title="Los dos modelos no coincidieron en estas palabras">${n} a revisar</span>`
    : "";

  const palabras = seg.words.length
    ? seg.words.map((w) => `<span class="w" data-a="${w.start.toFixed(2)}" data-b="${w.end.toFixed(2)}">${
        w.low_conf ? `<mark>${esc(w.text)}</mark>` : esc(w.text)}</span>`).join(" ")
    : esc(seg.source);

  const ingles = seg.target ? esc(seg.target) : "";
  const cuerpo = ingles
    ? `<p class="en" data-role="target">${ingles}</p><p class="es">${palabras}</p>`
    : `<p class="en" data-role="source">${palabras}</p>`;

  return `<div class="meta">
      <button class="ts" title="Saltar acá">${fmt(seg.start)}</button>
      ${spk}${rescatado}${marca}
    </div>${cuerpo}`;
}

function articleFor(seg) {
  const el = document.createElement("article");
  el.className = "seg";
  el.dataset.id = seg.id;
  el.innerHTML = segmentHtml(seg);
  return el;
}

/** Añade o actualiza un bloque en el archivo `f`, sin rehacer la lista entera. */
function upsert(f, seg, dibujar) {
  const i = f.live.findIndex((s) => s.id === seg.id);
  if (i >= 0) {
    f.live[i] = seg;
    if (dibujar) render(seg);
    return;
  }
  f.live.push(seg);
  if (!dibujar) return;
  $("empty").hidden = true;
  $("segments").appendChild(articleFor(seg));
}

function render(seg) {
  const el = nodeFor(seg.id);
  if (!el) { drawAll(); return; }
  if (el.classList.contains("editing")) return; // no pisar lo que está corrigiendo
  el.innerHTML = segmentHtml(seg);
}

function drawAll() {
  const cont = $("segments");
  cont.replaceChildren(...state.segments.map(articleFor));
  $("empty").hidden = state.segments.length > 0;
}

/* --------------------------------------------------- copiar, corregir, ir */

const textoDe = (seg) => (seg.target && seg.target.trim() ? seg.target : seg.source);

async function copiar(seg, el) {
  await invoke("copy_text", { text: textoDe(seg) });
  el.classList.add("copied");
  setTimeout(() => el.classList.remove("copied"), 700);
  toast("bloque copiado");
}

function editar(el, seg) {
  const p = el.querySelector(".en");
  if (!p) return;
  const campo = p.dataset.role;
  el.classList.add("editing");
  p.contentEditable = "true";
  p.focus();
  const r = document.createRange();
  r.selectNodeContents(p);
  r.collapse(false);
  const sel = window.getSelection();
  sel.removeAllRanges();
  sel.addRange(r);

  const terminar = async () => {
    p.contentEditable = "false";
    el.classList.remove("editing");
    const texto = p.textContent.trim();
    if (campo === "target") seg.target = texto; else seg.source = texto;
    render(seg);
    try {
      await invoke("update_segment", {
        id: seg.id,
        target: campo === "target" ? texto : null,
        source: campo === "source" ? texto : null,
      });
    } catch (e) { toast(String(e)); }
  };
  p.addEventListener("blur", terminar, { once: true });
  p.addEventListener("keydown", (e) => {
    if (e.key === "Escape" || (e.key === "Enter" && !e.shiftKey)) { e.preventDefault(); p.blur(); }
  });
}

// Un clic copia, dos corrigen. Se espera un instante para no copiar al ir a corregir.
let clicPendiente;

$("segments").addEventListener("click", (e) => {
  const el = e.target.closest(".seg");
  if (!el || el.classList.contains("editing")) return;
  const seg = state.segments.find((s) => s.id === +el.dataset.id);
  if (!seg) return;
  if (e.target.closest(".ts")) {
    audio.currentTime = seg.start;
    if (audio.paused) audio.play();
    return;
  }
  clearTimeout(clicPendiente);
  clicPendiente = setTimeout(() => copiar(seg, el), 220);
});

$("segments").addEventListener("dblclick", (e) => {
  clearTimeout(clicPendiente);
  const chip = e.target.closest(".badge.spk");
  if (chip) { renombrarHablante(chip); return; }
  const el = e.target.closest(".seg");
  if (!el || el.classList.contains("editing")) return;
  const seg = state.segments.find((s) => s.id === +el.dataset.id);
  if (seg) editar(el, seg);
});

/** Cambia "Hablante 2" por el nombre que quiera, en todos los bloques de esa persona. */
function renombrarHablante(chip) {
  const k = +chip.dataset.spk;
  const f = state.queue.find((x) => x.path === state.shown);
  if (!f?.transcript) return;
  chip.contentEditable = "true";
  chip.focus();
  document.execCommand?.("selectAll", false, null);

  const terminar = async () => {
    chip.contentEditable = "false";
    const nombre = chip.textContent.trim();
    const names = f.transcript.speaker_names ?? (f.transcript.speaker_names = []);
    while (names.length <= k) names.push("");
    names[k] = nombre === `Hablante ${k + 1}` ? "" : nombre;
    try {
      await invoke("set_transcript", { transcript: f.transcript });
      await invoke("set_speaker_name", { index: k, name: names[k] });
    } catch (e) { toast(String(e)); }
    drawAll();
  };
  chip.addEventListener("blur", terminar, { once: true });
  chip.addEventListener("keydown", (ev) => {
    if (ev.key === "Enter" || ev.key === "Escape") { ev.preventDefault(); chip.blur(); }
  });
}

/* ------------------------------------------------------------ reproductor */

function saltar(d) {
  audio.currentTime = Math.max(0, Math.min(audio.duration || 1e9, audio.currentTime + d));
}

$("play").addEventListener("click", () => (audio.paused ? audio.play() : audio.pause()));
$("back").addEventListener("click", () => saltar(-3));
$("fwd").addEventListener("click", () => saltar(3));
$("seek").addEventListener("input", (e) => {
  if (audio.duration) audio.currentTime = (e.target.value / 1000) * audio.duration;
});
$("rate").addEventListener("input", (e) => {
  audio.playbackRate = e.target.value / 100;
  $("rate-label").textContent = audio.playbackRate.toFixed(2) + "×";
});

audio.addEventListener("play", () => ($("play").textContent = "❚❚"));
audio.addEventListener("pause", () => ($("play").textContent = "▶"));

audio.addEventListener("timeupdate", () => {
  const t = audio.currentTime;
  const d = audio.duration || state.duration || 0;
  $("time").textContent = `${fmt(t)} / ${fmt(d)}`;
  if (d) $("seek").value = Math.round((t / d) * 1000);

  const i = state.segments.findIndex((s) => t >= s.start && t < s.end);
  if (i !== state.active) {
    const antes = state.active >= 0 && state.segments[state.active];
    if (antes) nodeFor(antes.id)?.classList.remove("active");
    state.active = i;
    if (i >= 0) {
      const el = nodeFor(state.segments[i].id);
      el?.classList.add("active");
      if ($("follow").checked) el?.scrollIntoView({ block: "center", behavior: "smooth" });
    }
  }
  if (state.active >= 0) {
    const seg = state.segments[state.active];
    if ($("loopseg").checked && t >= seg.end - 0.05) { audio.currentTime = seg.start; return; }
    const el = nodeFor(seg.id);
    if (el) for (const w of el.querySelectorAll(".w")) {
      w.classList.toggle("on", t >= +w.dataset.a && t < +w.dataset.b);
    }
  }
});

document.addEventListener("keydown", (e) => {
  if (document.activeElement?.isContentEditable) return;
  if (["INPUT", "SELECT", "TEXTAREA"].includes(document.activeElement?.tagName)) return;
  const paso = e.shiftKey ? 10 : 3;
  if (e.code === "Space") { e.preventDefault(); audio.paused ? audio.play() : audio.pause(); }
  else if (e.code === "ArrowLeft") { e.preventDefault(); saltar(-paso); }
  else if (e.code === "ArrowRight") { e.preventDefault(); saltar(paso); }
  else if (e.code === "ArrowUp" || e.code === "ArrowDown") {
    e.preventDefault();
    const base = state.active < 0 ? 0 : state.active;
    const i = Math.max(0, Math.min(state.segments.length - 1, base + (e.code === "ArrowDown" ? 1 : -1)));
    if (state.segments[i]) audio.currentTime = state.segments[i].start;
  }
});

/* ------------------------------------------------------------- botonera */

$("open").addEventListener("click", async () => {
  const paths = await invoke("pick_audio");
  if (paths?.length) enqueue(paths);
});

$("cancel").addEventListener("click", () => {
  invoke("cancel");
  $("stage").textContent = "deteniendo…";
});

$("toggle-options").addEventListener("click", () => ($("options").hidden = !$("options").hidden));
$("toggle-export").addEventListener("click", () => ($("export").hidden = !$("export").hidden));
$("o-preset").addEventListener("change", syncPreset);
$("o-lang").addEventListener("change", syncPreset);

$("o-speakers").addEventListener("change", async () => {
  const f = state.queue.find((x) => x.path === state.shown);
  if (!f?.transcript || state.busy) return;
  toast("separando hablantes de nuevo…");
  try {
    await invoke("set_transcript", { transcript: f.transcript });
    const t = await invoke("rediarize", {
      speakers: parseInt($("o-speakers").value, 10) || 0,
      threads: parseInt($("o-threads").value, 10) || 4,
    });
    f.transcript = t;
    state.segments = t.segments;
    drawAll();
    toast(`${t.num_speakers} ${t.num_speakers === 1 ? "hablante" : "hablantes"}`);
  } catch (e) {
    toast(String(e));
  }
});

$("do-export").addEventListener("click", async () => {
  const format = $("e-format").value;
  const base = nombre(state.shown || "transcripcion").replace(/\.[^.]+$/, "");
  const path = await invoke("save_as", { name: `${base}.${format}`, ext: format });
  if (!path) return;
  try {
    await invoke("export", {
      path, format,
      content: $("e-content").value,
      speakers: $("e-speakers").checked,
    });
    toast("guardado");
  } catch (e) {
    toast(String(e));
  }
});

listen("tauri://drag-drop", (ev) => {
  const paths = ev.payload?.paths ?? [];
  if (paths.length) enqueue(paths);
});

boot();
