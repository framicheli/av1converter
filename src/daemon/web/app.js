"use strict";

// ── Helpers ─────────────────────────────────────────────────────────

const $ = (id) => document.getElementById(id);

function fmtBytes(n) {
  if (n == null) return "—";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let v = n, u = 0;
  while (v >= 1024 && u < units.length - 1) { v /= 1024; u++; }
  return `${v >= 10 || u === 0 ? Math.round(v) : v.toFixed(1)} ${units[u]}`;
}

function fmtDuration(secs) {
  if (secs == null) return "—";
  const h = Math.floor(secs / 3600), m = Math.floor((secs % 3600) / 60), s = Math.floor(secs % 60);
  return h > 0 ? `${h}h ${m}m` : m > 0 ? `${m}m ${s}s` : `${s}s`;
}

function toast(message, isError) {
  const el = $("toast");
  el.textContent = message;
  el.classList.toggle("error", Boolean(isError));
  el.classList.remove("hidden");
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => el.classList.add("hidden"), 3000);
}

// When the daemon requires a token, it can be handed over once as ?token=… in
// the URL. It is kept for the session and stripped from the address bar so it
// does not linger in history or get copied into a shared link.
const token = (() => {
  const fromUrl = new URLSearchParams(location.search).get("token");
  if (fromUrl) {
    sessionStorage.setItem("av1c_token", fromUrl);
    history.replaceState(null, "", location.pathname);
    return fromUrl;
  }
  return sessionStorage.getItem("av1c_token") || "";
})();

async function api(path, options) {
  const init = { ...options, headers: { ...(options && options.headers) } };
  if (token) init.headers.Authorization = `Bearer ${token}`;
  const response = await fetch(path, init);
  const body = await response.json().catch(() => ({}));
  if (response.status === 401) {
    sessionStorage.removeItem("av1c_token");
    throw new Error("Unauthorized — open the UI with ?token=… from your config");
  }
  if (!response.ok) throw new Error(body.error || `HTTP ${response.status}`);
  return body;
}

const post = (path, body) =>
  api(path, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: body === undefined ? "{}" : JSON.stringify(body),
  });

// ── Tabs ────────────────────────────────────────────────────────────

let activeTab = "dashboard";

for (const button of document.querySelectorAll(".tab")) {
  button.addEventListener("click", () => {
    activeTab = button.dataset.tab;
    for (const b of document.querySelectorAll(".tab")) b.classList.toggle("active", b === button);
    for (const name of ["dashboard", "queue", "settings"]) {
      $(`tab-${name}`).classList.toggle("hidden", name !== activeTab);
    }
    if (activeTab === "queue") refreshQueue();
    if (activeTab === "settings" && !settingsLoaded) loadSettings();
  });
}

// ── Polling ─────────────────────────────────────────────────────────

let paused = false;

async function poll() {
  try {
    const s = await api("/api/status");
    $("offline-banner").classList.add("hidden");
    paused = s.paused;

    const pill = $("status-pill");
    pill.textContent = s.encoding_active ? "Encoding" : s.paused ? "Paused" : "Idle";
    pill.className = `pill ${s.encoding_active ? "encoding" : s.paused ? "paused" : ""}`;

    if (s.current) {
      $("current-file").textContent = s.current.filename;
      const st = s.current.status;
      const pct = st.kind === "encoding" ? st.progress : 100;
      $("current-bar").style.width = `${pct}%`;
      $("current-pct").textContent = st.kind === "encoding" ? `${st.progress.toFixed(1)}%` : "";
      $("current-stage").textContent = st.kind === "verifying" ? "Verifying quality (VMAF)…" : "";
    } else {
      $("current-file").textContent = "Idle — nothing encoding";
      $("current-bar").style.width = "0%";
      $("current-pct").textContent = "";
      $("current-stage").textContent = "";
    }

    $("overall-bar").style.width = `${s.overall_progress}%`;
    $("overall-pct").textContent = `${s.overall_progress.toFixed(1)}%`;
    $("eta").textContent = s.eta_secs != null ? `ETA ${fmtDuration(s.eta_secs)}` : "";

    $("stat-total").textContent = s.counts.total;
    $("stat-converted").textContent = s.counts.converted;
    $("stat-skipped").textContent = s.counts.skipped;
    $("stat-errors").textContent = s.counts.errors;
    $("stat-saved").textContent = s.total_space_saved.human;
    $("stat-elapsed").textContent = s.elapsed_secs != null ? fmtDuration(s.elapsed_secs) : "—";

    $("meta-encoder").textContent = `Encoder: ${s.encoder}`;
    $("meta-version").textContent = `v${s.version}`;
    $("meta-uptime").textContent = `Up ${fmtDuration(s.uptime_secs)}`;

    $("btn-pause").textContent = s.paused ? "Resume" : "Pause";
    $("btn-cancel").disabled = !s.encoding_active;

    if (activeTab === "queue") await refreshQueue();
  } catch {
    $("offline-banner").classList.remove("hidden");
  }
}

// No point polling a tab nobody is looking at; refresh as soon as it is again.
setInterval(() => {
  if (document.visibilityState === "visible") poll();
}, 1000);
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") poll();
});
poll();

// ── Queue table ─────────────────────────────────────────────────────

const BADGE_CLASS = {
  pending: "", analyzing: "analyzing", awaiting_config: "", ready: "",
  encoding: "encoding", verifying: "verifying",
  done: "done", done_vmaf: "done", done_vmaf_failed: "warn",
  skipped: "", error: "error", quality_warning: "warn",
};

function badgeText(st) {
  switch (st.kind) {
    case "encoding": return `Encoding ${st.progress.toFixed(1)}%`;
    case "done_vmaf": return `Done · VMAF ${st.vmaf.toFixed(1)}`;
    case "done_vmaf_failed": return "Done · VMAF failed";
    case "quality_warning": return `Low VMAF ${st.vmaf.toFixed(1)}`;
    case "awaiting_config": return "Awaiting config";
    case "skipped": return `Skipped · ${st.reason}`;
    case "error": return "Error";
    default: return st.kind.charAt(0).toUpperCase() + st.kind.slice(1);
  }
}

// Rows are kept and updated in place, keyed by job id. Rebuilding the table on
// every poll would throw away hover, focus and any text the user has selected,
// once a second, for the whole length of an encode.
const rows = new Map();

function createRow(job) {
  const tr = document.createElement("tr");

  const file = tr.insertCell();
  file.className = "filecell";
  const name = document.createElement("div");
  const sub = document.createElement("div");
  sub.className = "subline";
  file.append(name, sub);

  const source = tr.insertCell();
  const statusCell = tr.insertCell();
  const badge = document.createElement("span");
  const bar = document.createElement("div");
  bar.className = "row-bar hidden";
  const fill = document.createElement("div");
  fill.className = "bar-fill";
  const track = document.createElement("div");
  track.className = "bar";
  track.appendChild(fill);
  bar.appendChild(track);
  statusCell.append(badge, bar);

  const size = tr.insertCell();
  const saved = tr.insertCell();

  const tracks = document.createElement("button");
  tracks.className = "iconbtn";
  tracks.textContent = "Tracks";
  tracks.title = "Choose audio and subtitle tracks";
  tracks.addEventListener("click", () => openTracks(job.id));
  tr.insertCell().appendChild(tracks);

  const remove = document.createElement("button");
  remove.className = "iconbtn";
  remove.textContent = "✕";
  remove.title = "Remove from queue";
  remove.addEventListener("click", async () => {
    try {
      await post("/api/queue/remove", { id: job.id });
      refreshQueue();
    } catch (e) { toast(e.message, true); }
  });
  tr.insertCell().appendChild(remove);

  return { tr, name, sub, source, badge, fill, bar, size, saved, tracks, remove };
}

function updateRow(row, job) {
  row.name.textContent = job.filename;
  row.sub.textContent = [job.remux_only ? "remux" : "", job.source_deleted ? "source deleted" : ""]
    .filter(Boolean).join(" · ");
  row.source.textContent = `${job.resolution} ${job.hdr}`;

  row.badge.className = `badge ${BADGE_CLASS[job.status.kind] || ""}`;
  row.badge.textContent = badgeText(job.status);
  row.badge.title = job.status.kind === "error" ? job.status.message : "";

  const encoding = job.status.kind === "encoding";
  row.bar.classList.toggle("hidden", !encoding);
  if (encoding) row.fill.style.width = `${job.status.progress}%`;

  row.size.textContent = job.output_size != null
    ? `${fmtBytes(job.source_size)} → ${fmtBytes(job.output_size)}`
    : fmtBytes(job.source_size);

  // Negative means the output grew, so show the direction rather than "−0%".
  if (job.saved_percent == null) {
    row.saved.textContent = "";
    row.saved.className = "";
  } else {
    row.saved.textContent = `${job.saved_percent < 0 ? "+" : "−"}${Math.abs(job.saved_percent).toFixed(0)}%`;
    row.saved.className = job.saved_percent < 0 ? "grew" : "";
  }

  row.remove.disabled = ["encoding", "verifying"].includes(job.status.kind);
  // Tracks are only editable before the encode starts; afterwards the
  // selection is already baked into the running FFmpeg command.
  row.tracks.disabled = !["ready", "awaiting_config"].includes(job.status.kind);
}

async function refreshQueue() {
  let data;
  try {
    data = await api("/api/queue");
  } catch {
    return; // offline banner is handled by the status poll
  }
  const tbody = $("queue-body");
  $("queue-empty").classList.toggle("hidden", data.jobs.length > 0);

  const seen = new Set();
  for (const job of data.jobs) {
    seen.add(job.id);
    let row = rows.get(job.id);
    if (!row) {
      row = createRow(job);
      rows.set(job.id, row);
      tbody.appendChild(row.tr);
    }
    updateRow(row, job);
  }
  for (const [id, row] of rows) {
    if (!seen.has(id)) {
      row.tr.remove();
      rows.delete(id);
    }
  }
}

$("btn-pause").addEventListener("click", async () => {
  try { await post("/api/queue/pause", { paused: !paused }); poll(); }
  catch (e) { toast(e.message, true); }
});

$("btn-cancel").addEventListener("click", async () => {
  if (!confirm("Cancel the running encode?")) return;
  try { await post("/api/queue/cancel"); toast("Cancelling…"); }
  catch (e) { toast(e.message, true); }
});

$("btn-clear").addEventListener("click", async () => {
  try {
    const r = await post("/api/queue/clear_finished");
    toast(`Removed ${r.removed} finished job(s)`);
    refreshQueue();
  } catch (e) { toast(e.message, true); }
});

// ── Per-job track selection ─────────────────────────────────────────

// The modal owns its own copy of the selection while it is open. The queue
// poll keeps running underneath and rewrites rows in place; it must never
// reach in here and discard choices the user has not saved yet.
let trackEditor = null;

$("tracks-close").addEventListener("click", closeTracks);
$("tracks-modal").addEventListener("click", (e) => {
  if (e.target === $("tracks-modal")) closeTracks();
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && trackEditor) closeTracks();
});

function closeTracks() {
  trackEditor = null;
  $("tracks-modal").classList.add("hidden");
}

async function openTracks(id) {
  // The projected Opus bitrate comes from the audio settings, which are only
  // fetched when the settings tab is opened.
  if (!settingsLoaded) await loadSettings();
  let data;
  try {
    data = await api(`/api/job/tracks?id=${id}`);
  } catch (e) { return toast(e.message, true); }

  trackEditor = {
    id,
    audio: data.audio.map((t) => ({ ...t, mode: t.selected ? (t.opus ? "opus" : "copy") : "off" })),
    subtitles: data.subtitles.map((t) => ({ ...t })),
    editable: data.editable,
  };
  $("tracks-title").textContent = data.filename;
  $("tracks-save").disabled = !data.editable;
  $("tracks-note").textContent = data.editable
    ? ""
    : "This job is already encoding — tracks cannot be changed.";
  renderTracks();
  $("tracks-modal").classList.remove("hidden");
}

// Opus keeps the source channel layout, so the bitrate is simply the
// per-channel allowance times the channel count.
function projectedKbps(track) {
  const perChannel = config?.audio?.opus_bitrate_per_channel ?? 64;
  return (track.channels || 2) * perChannel;
}

function isAlreadyOpus(track) {
  return (config?.audio?.skip_already_opus ?? true)
    && (track.codec || "").toLowerCase() === "opus";
}

function renderTracks() {
  const body = $("tracks-body");
  body.textContent = "";
  const { audio, subtitles, editable } = trackEditor;

  body.appendChild(groupHeading("Audio"));
  if (audio.length === 0) body.appendChild(emptyNote("No audio tracks"));
  for (const track of audio) {
    const row = document.createElement("div");
    row.className = "track-row";

    const info = document.createElement("div");
    info.className = "track-info";
    const name = document.createElement("div");
    name.className = "name";
    name.textContent = track.name;
    const sub = document.createElement("div");
    sub.className = "sub";
    sub.textContent = `${track.bitrate} · ${track.sample_rate}`;
    info.append(name, sub);

    const target = document.createElement("span");
    target.className = "track-target";

    const select = document.createElement("select");
    for (const [value, text] of [["off", "Exclude"], ["copy", "Copy"], ["opus", "Opus"]]) {
      const option = document.createElement("option");
      option.value = value;
      option.textContent = text;
      select.appendChild(option);
    }
    select.value = track.mode;
    select.disabled = !editable;
    select.addEventListener("change", () => {
      track.mode = select.value;
      setTarget(target, track);
    });
    setTarget(target, track);

    row.append(info, target, select);
    body.appendChild(row);
  }

  body.appendChild(groupHeading("Subtitles"));
  if (subtitles.length === 0) body.appendChild(emptyNote("No subtitle tracks"));
  for (const track of subtitles) {
    const row = document.createElement("div");
    row.className = "track-row";

    const info = document.createElement("div");
    info.className = "track-info";
    const name = document.createElement("div");
    name.className = "name";
    name.textContent = track.name;
    info.appendChild(name);

    const box = document.createElement("input");
    box.type = "checkbox";
    box.checked = track.selected;
    box.disabled = !editable;
    box.addEventListener("change", () => { track.selected = box.checked; });

    row.append(info, box);
    body.appendChild(row);
  }
}

function setTarget(node, track) {
  if (track.mode !== "opus") {
    node.textContent = "";
  } else if (isAlreadyOpus(track)) {
    node.textContent = "already Opus — copied";
  } else {
    node.textContent = `→ Opus ${projectedKbps(track)}k`;
  }
}

function groupHeading(text) {
  const node = document.createElement("div");
  node.className = "track-group";
  node.textContent = text;
  return node;
}

function emptyNote(text) {
  const node = document.createElement("div");
  node.className = "muted";
  node.textContent = text;
  return node;
}

$("tracks-save").addEventListener("click", async () => {
  if (!trackEditor) return;
  const { id, audio, subtitles } = trackEditor;
  try {
    await post("/api/job/tracks", {
      id,
      audio_indices: audio.filter((t) => t.mode !== "off").map((t) => t.index),
      audio_to_opus: audio.filter((t) => t.mode === "opus").map((t) => t.index),
      subtitle_indices: subtitles.filter((t) => t.selected).map((t) => t.index),
    });
    closeTracks();
    toast("Tracks updated");
    refreshQueue();
  } catch (e) { toast(e.message, true); }
});

// ── File browser ────────────────────────────────────────────────────

const browser = { mode: "file", path: "" };

// Join a directory and an entry name without doubling the separator at root.
const joinPath = (dir, name) => (dir.endsWith("/") ? `${dir}${name}` : `${dir}/${name}`);

$("btn-add-file").addEventListener("click", () => openBrowser("file"));
$("btn-add-folder").addEventListener("click", () => openBrowser("folder"));
$("btn-add-recursive").addEventListener("click", () => openBrowser("folder_recursive"));
$("browser-close").addEventListener("click", () => setBrowserMode("file"));
$("browser-hidden").addEventListener("change", () => loadDir(browser.path));
$("browser-choose").addEventListener("click", () => addToQueue(browser.path, browser.mode));

function setBrowserMode(mode) {
  browser.mode = mode;
  $("browser-title").textContent =
    mode === "file" ? "Select a video file" :
    mode === "folder" ? "Select a folder" : "Select a folder (recursive)";
  $("browser-choose").classList.toggle("hidden", mode === "file");
  $("browser-close").classList.toggle("hidden", mode === "file");
}

function openBrowser(mode) {
  setBrowserMode(mode);
  loadDir(browser.path);
  $("browser").scrollIntoView({ behavior: "smooth", block: "nearest" });
}

loadDir(browser.path);

async function loadDir(path) {
  let data;
  try {
    data = await api(`/api/fs?path=${encodeURIComponent(path)}${$("browser-hidden").checked ? "&hidden=1" : ""}`);
  } catch (e) { toast(e.message, true); return; }

  browser.path = data.path;
  $("browser-path").textContent = data.path;
  const list = $("browser-list");
  list.textContent = "";

  const addEntry = (label, cls, onclick, sizeText) => {
    const li = document.createElement("li");
    if (cls) li.className = cls;
    const name = document.createElement("span");
    name.textContent = label;
    li.appendChild(name);
    if (sizeText) {
      const size = document.createElement("span");
      size.className = "size";
      size.textContent = sizeText;
      li.appendChild(size);
    }
    if (onclick) li.addEventListener("click", onclick);
    list.appendChild(li);
  };

  if (data.parent) addEntry("📁 ..", "", () => loadDir(data.parent));
  for (const d of data.dirs) {
    addEntry(`📁 ${d.name}${d.symlink ? " ↗" : ""}`, "", () => loadDir(joinPath(data.path, d.name)));
  }
  for (const f of data.files) {
    const filePath = joinPath(data.path, f.name);
    if (f.is_video && browser.mode === "file") {
      addEntry(`🎬 ${f.name}`, "", () => addToQueue(filePath, "file"), fmtBytes(f.size));
    } else {
      addEntry(`${f.is_video ? "🎬" : "·"} ${f.name}`, "disabled", null, fmtBytes(f.size));
    }
  }
}

async function addToQueue(path, mode) {
  try {
    const r = await post("/api/queue/add", { path, mode });
    setBrowserMode("file");
    const skipped = r.already_queued ? `, ${r.already_queued} already queued` : "";
    toast(r.added > 0
      ? `Added ${r.added} file(s) to the queue${skipped}`
      : `Nothing added — ${r.already_queued} file(s) already queued`);
    refreshQueue();
  } catch (e) { toast(e.message, true); }
}

// ── Settings ────────────────────────────────────────────────────────

let settingsLoaded = false;
let config = null;

const LANGS = [["en", "English"], ["it", "Italiano"], ["es", "Español"], ["fr", "Français"], ["de", "Deutsch"], ["zh", "中文"]];
const ENCODERS = [["SvtAv1", "SVT-AV1 (Software)"], ["Nvenc", "NVENC (NVIDIA)"], ["Qsv", "Quick Sync (Intel)"], ["Amf", "AMF (AMD)"]];
const PRESETS = [["low", "Low"], ["medium", "Medium"], ["high", "High"], ["custom", "Custom"]];
const NVENC_PRESETS = ["p1", "p2", "p3", "p4", "p5", "p6", "p7"].map((p) => [p, p]);
const RF_TIERS = [["sd", "RF SD"], ["hd", "RF HD (720p)"], ["full_hd", "RF 1080p SDR"], ["full_hd_hdr", "RF 1080p HDR"],
  ["full_hd_dv", "RF 1080p DV"], ["uhd", "RF 4K SDR"], ["uhd_hdr", "RF 4K HDR"], ["uhd_dv", "RF 4K DV"]];

const RF_KEY = { SvtAv1: "crf", Nvenc: "nvenc_cq", Qsv: "qsv_quality", Amf: "amf_quality" };

const AUDIO_MODES = [["copy", "Copy the source tracks"], ["opus", "Convert to Opus"]];

function settingsFields(cfg) {
  const fields = [
    { group: "General" },
    { path: "language", label: "Language", type: "select", options: LANGS },
    { path: "encoder", label: "Encoder", type: "select", options: ENCODERS, rebuild: true },
    { path: "quality_preset", label: "Quality preset", type: "select", options: PRESETS, rebuild: true },
    { group: "Quality" },
    { path: "quality.vmaf_threshold", label: "VMAF threshold", type: "number", min: 0, max: 100 },
    { path: "quality.vmaf_enabled", label: "VMAF verification", type: "checkbox" },
    { path: "quality.delete_source_on_success", label: "Delete source if VMAF passes", type: "checkbox" },
    { group: "Performance" },
    { path: "performance.svt_preset", label: "SVT-AV1 preset (0–13)", type: "number", min: 0, max: 13 },
    { path: "performance.nvenc_preset", label: "NVENC preset", type: "select", options: NVENC_PRESETS },
  ];
  if (cfg.quality_preset === "custom") {
    fields.push({ group: `Rate factors (${RF_KEY[cfg.encoder]})` });
    for (const [tier, label] of RF_TIERS) {
      fields.push({ path: `presets.${tier}.${RF_KEY[cfg.encoder]}`, label, type: "number", min: 0, max: 63 });
    }
  }
  fields.push(
    { group: "Output" },
    { path: "output.suffix", label: "Output suffix", type: "text" },
    { path: "output.container", label: "Output container", type: "text" },
    { path: "output.same_directory", label: "Same directory output", type: "checkbox" },
    { path: "output.output_directory", label: "Output directory (if not same)", type: "text", nullable: true },
    { group: "Tracks" },
    { path: "tracks.preferred_audio_languages", label: "Preferred audio languages", type: "list" },
    { path: "tracks.preferred_subtitle_languages", label: "Preferred subtitle languages", type: "list" },
    { path: "tracks.select_all_fallback", label: "Select all tracks as fallback", type: "checkbox" },
    { group: "Audio" },
    { path: "audio.default_mode", label: "New files default to", type: "select", options: AUDIO_MODES },
    { path: "audio.opus_bitrate_per_channel", label: "Opus kbps per channel", type: "number", min: 16, max: 256 },
    { path: "audio.skip_already_opus", label: "Skip tracks already in Opus", type: "checkbox" },
    // The [daemon] block is deliberately absent: browse_root confines this file
    // browser and auth_token guards this API, so the server refuses to let a
    // client widen its own access. Edit those in config.toml or the TUI.
    { group: "Daemon" },
    { note: "Bind address, port, browse root and access token are only editable in config.toml or the TUI, and need a restart." },
  );
  return fields;
}

const getPath = (obj, path) => path.split(".").reduce((o, k) => (o == null ? o : o[k]), obj);

function setPath(obj, path, value) {
  const keys = path.split(".");
  const last = keys.pop();
  const target = keys.reduce((o, k) => o[k], obj);
  target[last] = value;
}

function buildSettingsForm() {
  const form = $("settings-form");
  form.textContent = "";
  for (const field of settingsFields(config)) {
    if (field.group) {
      const heading = document.createElement("div");
      heading.className = "field-group";
      heading.textContent = field.group;
      form.appendChild(heading);
      continue;
    }
    if (field.note) {
      const note = document.createElement("div");
      note.className = "muted";
      note.textContent = field.note;
      form.appendChild(note);
      continue;
    }
    const row = document.createElement("div");
    row.className = "field";
    const label = document.createElement("label");
    label.textContent = field.label;
    row.appendChild(label);

    const value = getPath(config, field.path);
    let input;
    if (field.type === "select") {
      input = document.createElement("select");
      for (const [val, text] of field.options) {
        const option = document.createElement("option");
        option.value = val;
        option.textContent = text;
        input.appendChild(option);
      }
      input.value = value;
    } else if (field.type === "checkbox") {
      input = document.createElement("input");
      input.type = "checkbox";
      input.checked = Boolean(value);
    } else {
      input = document.createElement("input");
      input.type = field.type === "number" ? "number" : "text";
      if (field.min != null) input.min = field.min;
      if (field.max != null) input.max = field.max;
      input.value = field.type === "list" ? (value || []).join(", ") : value ?? "";
    }

    input.addEventListener("change", () => {
      let parsed;
      if (field.type === "checkbox") parsed = input.checked;
      else if (field.type === "number") parsed = Number(input.value);
      else if (field.type === "list") parsed = input.value.split(",").map((s) => s.trim()).filter(Boolean);
      else if (field.nullable && input.value.trim() === "") parsed = null;
      else parsed = input.value;
      setPath(config, field.path, parsed);
      if (field.rebuild) buildSettingsForm();
    });

    row.appendChild(input);
    form.appendChild(row);
  }
}

async function loadSettings() {
  try {
    config = await api("/api/settings");
    settingsLoaded = true;
    buildSettingsForm();
  } catch (e) { toast(e.message, true); }
}

$("btn-save-settings").addEventListener("click", async () => {
  try {
    config = await post("/api/settings", config);
    buildSettingsForm();
    toast("Settings saved");
  } catch (e) { toast(e.message, true); }
});
