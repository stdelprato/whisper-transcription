"use strict";

const { invoke, convertFileSrc } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);
const audio = new Audio();

const state = {
  /** cola de trabajo: {path, name, status, transcript, live, progreso} */
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

/** lo que muestra el panel izquierdo */
const nav = { path: null, folders: [], audios: [] };

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
const tam = (b) => (b >= 1e6 ? `${(b / 1e6).toFixed(1)} MB` : `${Math.max(1, Math.round(b / 1e3))} KB`);

let toastTimer;
function toast(msg) {
  const el = $("toast");
  el.textContent = msg;
  el.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (el.hidden = true), 1800);
}

const enCola = (path) => state.queue.find((x) => x.path === path);

/* ------------------------------------------------------- ajustes y arranque */

const PRESETS = {
  fast: { asr: "parakeet", mt: "opus", verify: false, rtf: 0.64 },
  balanced: { asr: "canary", mt: "opus", verify: true, rtf: 1.49 },
  best: { asr: "canary", mt: "nllb", verify: true, rtf: 2.65 },
};

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

function rtfReal(n) {
  try {
    const v = JSON.parse(localStorage.getItem("rtf") || "{}")[n];
    return typeof v === "number" && v > 0 ? v : null;
  } catch { return null; }
}

function anotarRtf(n, rtf) {
  if (!(rtf > 0) || !isFinite(rtf)) return;
  try {
    const t = JSON.parse(localStorage.getItem("rtf") || "{}");
    // Media móvil suave: una corrida rara no debería mover mucho la estimación.
    t[n] = t[n] ? t[n] * 0.7 + rtf * 0.3 : rtf;
    localStorage.setItem("rtf", JSON.stringify(t));
  } catch { /* sin almacenamiento, no pasa nada */ }
}

function mostrarEstimacion() {
  const n = $("o-preset").value;
  const p = PRESETS[n];
  if (!p) { $("estimacion").textContent = ""; return; }
  const medido = rtfReal(n);
  const rtf = medido ?? p.rtf;
  const min = Math.round(rtf * 60);
  $("estimacion").textContent =
    `1 hora de audio ≈ ${min < 90 ? `${min} min` : `${rtf.toFixed(1)} h`}` +
    (medido ? " (medido acá)" : " (estimado)");
}

function syncPreset() {
  const n = $("o-preset").value;
  const custom = n === "custom";
  const p = PRESETS[n];
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
  abrirCarpeta(null);
}

/* --------------------------------------------------- panel de carpetas */

async function abrirCarpeta(path) {
  let l;
  try {
    l = await invoke("browse", { path: path ?? null });
  } catch (e) {
    toast(String(e));
    return;
  }
  nav.path = l.path;
  nav.folders = l.folders;
  nav.audios = l.audios;
  $("br-path").textContent = l.path;
  $("br-up").disabled = !l.parent;
  $("br-up").dataset.path = l.parent ?? "";

  $("br-folders").innerHTML = l.folders.length
    ? l.folders.map((f) =>
        `<div class="row ${f.workday ? "workday" : "otra"}" data-dir="${esc(f.path)}">
           <span class="ico">${f.workday ? "★" : "▸"}</span>
           <span class="nm">${esc(f.name)}</span>
         </div>`).join("")
    : `<div class="vacio">no hay subcarpetas</div>`;

  pintarArchivos();
}

/** Dibuja la lista de audios con el estado de cada uno. */
function pintarArchivos() {
  const l = nav.audios;
  $("br-files").innerHTML = l.length
    ? l.map((a) => filaArchivo(a)).join("")
    : `<div class="vacio">no hay audios en esta carpeta</div>`;

  const hechos = l.filter((a) => a.done || enCola(a.path)?.status === "listo").length;
  $("br-info").textContent = l.length ? `${hechos}/${l.length} listos` : "";
  $("br-all").disabled = !l.some((a) => !a.done && !enCola(a.path));
}

function filaArchivo(a) {
  const q = enCola(a.path);
  const st = q?.status;
  const listo = a.done || st === "listo";
  const clases = ["row"];
  let ico = "○";
  if (st === "procesando") { clases.push("doing"); ico = "◐"; }
  else if (listo) { clases.push("done"); ico = "✓"; }
  else if (st === "falló") { clases.push("failed"); ico = "!"; }
  if (a.path === state.shown) clases.push("sel");

  const barra = st === "procesando"
    ? `<div class="minitrack"><div class="minifill" style="width:${((q.progreso ?? 0) * 100).toFixed(0)}%"></div></div>`
    : "";

  return `<div class="${clases.join(" ")}" data-file="${esc(a.path)}">
      <span class="ico">${ico}</span>
      <span class="nm">${esc(a.name)}</span>
      <span class="meta">${st === "procesando" ? "procesando" : tam(a.size)}</span>
    </div>${barra}`;
}

$("br-folders").addEventListener("click", (e) => {
  const r = e.target.closest(".row");
  if (r) abrirCarpeta(r.dataset.dir);
});

$("br-files").addEventListener("click", (e) => {
  const r = e.target.closest(".row");
  if (!r) return;
  const path = r.dataset.file;
  const q = enCola(path);
  if (q?.status === "listo") showFile(path);
  else if (!q) enqueue([path]);
});

$("br-up").addEventListener("click", (e) => {
  if (e.currentTarget.dataset.path) abrirCarpeta(e.currentTarget.dataset.path);
});

$("br-home").addEventListener("click", () => abrirCarpeta(null));

$("br-pick").addEventListener("click", async () => {
  try {
    const p = await invoke("pick_root");
    if (p) { toast("carpeta base guardada"); abrirCarpeta(p); }
  } catch (e) { toast(String(e)); }
});

$("br-all").addEventListener("click", () => {
  const faltan = nav.audios.filter((a) => !a.done && !enCola(a.path)).map((a) => a.path);
  if (faltan.length) enqueue(faltan);
});

/* ------------------------------------------------------------- separador */

(function separador() {
  const sp = $("splitter");
  let arrastrando = false;
  const mover = (x) => {
    const ancho = Math.min(Math.max(x, 200), Math.min(560, window.innerWidth - 380));
    document.documentElement.style.setProperty("--side", `${Math.round(ancho)}px`);
  };
  sp.addEventListener("mousedown", (e) => {
    arrastrando = true;
    sp.classList.add("dragging");
    document.body.style.userSelect = "none";
    e.preventDefault();
  });
  window.addEventListener("mousemove", (e) => { if (arrastrando) mover(e.clientX); });
  window.addEventListener("mouseup", () => {
    if (!arrastrando) return;
    arrastrando = false;
    sp.classList.remove("dragging");
    document.body.style.userSelect = "";
    const v = getComputedStyle(document.documentElement).getPropertyValue("--side");
    try { localStorage.setItem("side", v.trim()); } catch { /* da igual */ }
  });
  try {
    const g = localStorage.getItem("side");
    if (g) document.documentElement.style.setProperty("--side", g);
  } catch { /* da igual */ }
})();

/* ------------------------------------------------------------------- cola */

async function enqueue(paths) {
  const nuevos = paths.filter((p) => !enCola(p));
  if (!nuevos.length) return;
  for (const path of nuevos) {
    // `live` acumula lo que llega mientras se procesa, para poder cambiar de archivo
    // y volver sin perder lo que ya se veía.
    state.queue.push({ path, name: nombre(path), status: "pendiente", transcript: null, live: [], progreso: 0 });
  }

  // Si un audio ya se procesó antes, se recupera en vez de repetirlo.
  let recuperados = 0;
  for (const path of nuevos) {
    const previo = await invoke("load_result", { path }).catch(() => null);
    if (!previo) continue;
    const f = enCola(path);
    f.status = "listo";
    f.transcript = previo;
    f.live = previo.segments;
    recuperados++;
  }
  if (recuperados) toast(`${recuperados} ${recuperados === 1 ? "audio ya estaba" : "audios ya estaban"} procesados`);

  pintarArchivos();
  if (!state.shown) showFile(nuevos[0]);
  processNext();
}

async function processNext() {
  if (state.busy) return;
  const f = state.queue.find((x) => x.status === "pendiente");
  if (!f) { $("job").hidden = true; return; }
  state.busy = f.path;
  f.status = "procesando";
  f.progreso = 0;
  if (!state.shown || enCola(state.shown)?.status !== "listo") showFile(f.path);
  pintarArchivos();

  state.startedAt = Date.now();
  $("job").hidden = false;
  $("job-name").textContent = f.name;
  $("job-stage").textContent = "abriendo…";
  $("job-eta").textContent = "";
  $("fill").style.width = "0%";

  try {
    await invoke("start", { path: f.path, opts: readOptions() });
  } catch (e) {
    f.status = "falló";
    state.busy = null;
    pintarArchivos();
    toast(String(e));
    processNext();
  }
}

/* --------------------------------------------------- mostrar un archivo */

async function showFile(path) {
  const f = enCola(path);
  state.shown = path;
  state.active = -1;
  state.segments = f ? (f.transcript ? f.transcript.segments : f.live) : [];
  $("file").textContent = nombre(path);
  drawAll();
  pintarArchivos();
  $("toggle-export").disabled = !f?.transcript;
  if (f?.transcript) await invoke("set_transcript", { transcript: f.transcript });

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
  const f = enCola(state.busy);
  if (f) {
    f.progreso = frac;
    const fila = $("br-files").querySelector(`[data-file="${CSS.escape(f.path)}"]`);
    const barra = fila?.nextElementSibling?.querySelector(".minifill");
    if (barra) barra.style.width = `${(frac * 100).toFixed(0)}%`;
  }
  $("fill").style.width = (Math.min(1, Math.max(0, frac)) * 100).toFixed(1) + "%";
  $("job-stage").textContent = texto;
  const t = (Date.now() - state.startedAt) / 1000;
  $("job-eta").textContent = frac > 0.05 && frac < 1 ? "faltan ~" + fmt(t / frac - t) : "";
}

listen("progress", (ev) => {
  const p = ev.payload;
  const f = enCola(state.busy);
  if (!f) return;
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
      if (seg) { seg.target = p.text; if (mirando) render(seg); }
      const hechos = f.live.filter((s) => s.target != null).length;
      setProgress(0.8 + PESO.mt * (hechos / Math.max(1, f.live.length)),
        `traduciendo ${hechos} de ${f.live.length}`);
      break;
    }
  }
});

listen("finished", async (ev) => {
  const t = ev.payload;
  const f = enCola(state.busy);
  if (f) { f.status = "listo"; f.transcript = t; f.live = t.segments; f.progreso = 1; }
  state.busy = null;
  if (!t.cancelled && t.duration > 0) {
    anotarRtf($("o-preset").value, t.timings.total / t.duration);
    mostrarEstimacion();
  }
  // Refresca la lista para que el audio recién hecho salga con su tilde.
  const a = nav.audios.find((x) => x.path === f?.path);
  if (a) a.done = true;
  pintarArchivos();

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
  const f = enCola(state.busy);
  if (f) f.status = "falló";
  state.busy = null;
  pintarArchivos();
  toast("Falló: " + ev.payload);
  processNext();
});

/* --------------------------------------------------------------- bloques */

const nodeFor = (id) => $("segments").querySelector(`[data-id="${id}"]`);

/** El nombre que le pusieron al hablante, o "Hablante N". */
function nombreHablante(k) {
  const puesto = enCola(state.shown)?.transcript?.speaker_names?.[k];
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
  $("segments").replaceChildren(...state.segments.map(articleFor));
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

/** Cambia "Hablante 2" por el nombre que quiera, en todos los bloques de esa persona. */
function renombrarHablante(chip) {
  const k = +chip.dataset.spk;
  const f = enCola(state.shown);
  if (!f?.transcript) return;
  chip.contentEditable = "true";
  chip.focus();
  document.execCommand?.("selectAll", false, null);

  const terminar = async () => {
    chip.contentEditable = "false";
    const nom = chip.textContent.trim();
    const names = f.transcript.speaker_names ?? (f.transcript.speaker_names = []);
    while (names.length <= k) names.push("");
    names[k] = nom === `Hablante ${k + 1}` ? "" : nom;
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
  const paths = await invoke("pick_audio", { from: nav.path });
  if (paths?.length) enqueue(paths);
});

$("cancel").addEventListener("click", () => {
  invoke("cancel");
  $("job-stage").textContent = "deteniendo…";
});

$("toggle-options").addEventListener("click", () => ($("options").hidden = !$("options").hidden));
$("toggle-export").addEventListener("click", () => ($("export").hidden = !$("export").hidden));
$("o-preset").addEventListener("change", syncPreset);
$("o-lang").addEventListener("change", syncPreset);

$("o-speakers").addEventListener("change", async () => {
  const f = enCola(state.shown);
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
