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

async function api(path, options) {
  const response = await fetch(path, options);
  const body = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(body.error || `HTTP ${response.status}`);
  return body;
}

const post = (path, body) =>
  api(path, { method: "POST", body: body === undefined ? "{}" : JSON.stringify(body) });

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

setInterval(poll, 1000);
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

async function refreshQueue() {
  let data;
  try {
    data = await api("/api/queue");
  } catch {
    return; // offline banner is handled by the status poll
  }
  const tbody = $("queue-body");
  tbody.textContent = "";

  if (data.jobs.length === 0) {
    const tr = tbody.insertRow();
    const td = tr.insertCell();
    td.colSpan = 6;
    td.className = "muted center";
    td.textContent = "Queue is empty — add files to start encoding";
    return;
  }

  for (const job of data.jobs) {
    const tr = tbody.insertRow();

    const file = tr.insertCell();
    file.className = "filecell";
    file.textContent = job.filename;
    if (job.remux_only || job.source_deleted) {
      const sub = document.createElement("div");
      sub.className = "subline";
      sub.textContent = [job.remux_only ? "remux" : "", job.source_deleted ? "source deleted" : ""]
        .filter(Boolean).join(" · ");
      file.appendChild(sub);
    }

    tr.insertCell().textContent = `${job.resolution} ${job.hdr}`;

    const statusCell = tr.insertCell();
    const badge = document.createElement("span");
    badge.className = `badge ${BADGE_CLASS[job.status.kind] || ""}`;
    badge.textContent = badgeText(job.status);
    if (job.status.kind === "error") badge.title = job.status.message;
    statusCell.appendChild(badge);
    if (job.status.kind === "encoding") {
      const wrap = document.createElement("div");
      wrap.className = "row-bar";
      wrap.innerHTML = '<div class="bar"><div class="bar-fill"></div></div>';
      wrap.querySelector(".bar-fill").style.width = `${job.status.progress}%`;
      statusCell.appendChild(wrap);
    }

    const size = tr.insertCell();
    size.textContent = job.output_size != null
      ? `${fmtBytes(job.source_size)} → ${fmtBytes(job.output_size)}`
      : fmtBytes(job.source_size);

    tr.insertCell().textContent = job.saved_percent != null ? `−${job.saved_percent.toFixed(0)}%` : "";

    const actions = tr.insertCell();
    const remove = document.createElement("button");
    remove.className = "iconbtn";
    remove.textContent = "✕";
    remove.title = "Remove from queue";
    remove.disabled = ["encoding", "verifying"].includes(job.status.kind);
    remove.addEventListener("click", async () => {
      try {
        await post("/api/queue/remove", { id: job.id });
        refreshQueue();
      } catch (e) { toast(e.message, true); }
    });
    actions.appendChild(remove);
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

// ── File browser ────────────────────────────────────────────────────

const browser = { mode: "file", path: "" };

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
    addEntry(`📁 ${d.name}`, "", () => loadDir(`${data.path}/${d.name}`.replace("//", "/")));
  }
  for (const f of data.files) {
    const filePath = `${data.path}/${f.name}`.replace("//", "/");
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
    toast(`Added ${r.added} file(s) to the queue`);
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
    { group: "Daemon (restart required)" },
    { path: "daemon.enabled", label: "Web daemon enabled", type: "checkbox" },
    { path: "daemon.bind_address", label: "Bind address", type: "text" },
    { path: "daemon.port", label: "Port", type: "number", min: 1, max: 65535 },
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
