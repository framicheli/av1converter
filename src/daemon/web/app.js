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
  $("toast-message").textContent = message;
  el.classList.toggle("error", Boolean(isError));
  el.setAttribute("role", isError ? "alert" : "status");
  el.setAttribute("aria-live", isError ? "assertive" : "polite");
  el.classList.remove("hidden");
  // Popovers share the browser's top layer with dialogs. Reopening moves an
  // existing toast above a modal, so failures are never hidden by its backdrop.
  if (el.matches(":popover-open")) el.hidePopover();
  el.showPopover();
  clearTimeout(toast.timer);
  if (!isError) toast.timer = setTimeout(hideToast, 4000);
}

function hideToast() {
  clearTimeout(toast.timer);
  const el = $("toast");
  if (el.matches(":popover-open")) el.hidePopover();
  el.classList.add("hidden");
}

$("toast-close").addEventListener("click", hideToast);

// URL fragments never reach the HTTP server, proxy logs or Referer headers.
// Storage can be disabled by privacy settings, so it is strictly best-effort.
const token = (() => {
  const fromUrl = new URLSearchParams(location.hash.slice(1)).get("token");
  if (fromUrl) {
    try { sessionStorage.setItem("av1c_token", fromUrl); } catch { /* memory only */ }
    history.replaceState(null, "", location.pathname + location.search);
    return fromUrl;
  }
  try { return sessionStorage.getItem("av1c_token") || ""; } catch { return ""; }
})();

// ── Strings ─────────────────────────────────────────────────────────
//
// The language is whatever the config says, as it is for the TUI, so the map
// is fetched once at startup rather than negotiated or switched at runtime.
// Until it arrives — and if it never does, which means the API is unreachable
// or unauthorized — the English written into index.html stands.
let strings = {};

function tr(key, fallback) {
  return strings[key] ?? fallback ?? key;
}

// Interpolates `{name}` placeholders, which is all the formatting the
// translated strings need.
function trf(key, values, fallback) {
  return Object.entries(values).reduce(
    (text, [name, value]) => text.replaceAll(`{${name}}`, value),
    tr(key, fallback),
  );
}

// Applies the map to everything index.html marked up. Attribute keys are
// spelled out rather than derived so that only these three are ever settable
// from a translation.
function applyStrings(root = document) {
  for (const [attr, setter] of [
    ["data-i18n", (el, text) => { el.textContent = text; }],
    ["data-i18n-title", (el, text) => { el.title = text; }],
    ["data-i18n-aria-label", (el, text) => el.setAttribute("aria-label", text)],
  ]) {
    for (const el of root.querySelectorAll(`[${attr}]`)) {
      const text = strings[el.getAttribute(attr)];
      if (text != null) setter(el, text);
    }
  }
}

async function api(path, options) {
  const init = { ...options, headers: { ...(options && options.headers) } };
  if (token) init.headers.Authorization = `Bearer ${token}`;
  const response = await fetch(path, init);
  const body = await response.json().catch(() => ({}));
  // The stored token is deliberately kept. Dropping it here turned a single
  // rejected request into a permanent logout, and it bought nothing: the only
  // way back in is #token=… in the URL, which overwrites it regardless.
  if (response.status === 401) {
    const error = new Error(tr("unauthorized", "Unauthorized — open the UI with #token=… from your config"));
    error.unauthorized = true;
    throw error;
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

let activeTab = "queue";

for (const button of document.querySelectorAll(".tab")) {
  button.addEventListener("click", () => {
    activeTab = button.dataset.tab;
    for (const b of document.querySelectorAll(".tab")) {
      b.classList.toggle("active", b === button);
      b.setAttribute("aria-selected", b === button ? "true" : "false");
      b.tabIndex = b === button ? 0 : -1;
    }
    for (const name of ["queue", "settings"]) {
      $(`tab-${name}`).classList.toggle("hidden", name !== activeTab);
    }
    if (activeTab === "queue") refreshQueue();
    if (activeTab === "settings" && !settingsLoaded) loadSettings();
  });
  button.addEventListener("keydown", (event) => {
    const tabs = [...document.querySelectorAll(".tab")];
    const offset = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
    const target = event.key === "Home" ? tabs[0]
      : event.key === "End" ? tabs.at(-1)
      : offset ? tabs[(tabs.indexOf(button) + offset + tabs.length) % tabs.length]
      : null;
    if (target) {
      event.preventDefault();
      target.click();
      target.focus();
    }
  });
}

// ── Polling ─────────────────────────────────────────────────────────

// Native progress keeps the visual fill and accessible value in sync.
function setProgress(name, pct) {
  const progress = $(`${name}-progress`);
  progress.value = pct;
  progress.textContent = `${Math.round(pct)}%`;
}

let pollInFlight = false;
async function poll() {
  if (pollInFlight) return;
  pollInFlight = true;
  try {
    const s = await api("/api/status");
    $("offline-banner").classList.add("hidden");

    const pill = $("status-pill");
    const statusKey = s.encoding_active ? "status_encoding"
      : s.counts.analyzing > 0 ? "badge_analyzing"
      : s.counts.awaiting_config > 0 ? "badge_awaiting_config"
      : s.counts.ready > 0 ? "badge_ready"
      : s.counts.pending > 0 ? "badge_pending"
      : "status_idle";
    pill.textContent = tr(statusKey);
    pill.className = `pill ${s.counts.active > 0 ? "encoding" : ""}`;

    if (s.current) {
      $("current-file").textContent = s.current.filename;
      const st = s.current.status;
      const pct = st.kind === "encoding" ? st.progress : 100;
      setProgress("current", pct);
      $("current-pct").textContent = st.kind === "encoding" ? `${st.progress.toFixed(1)}%` : "";
      $("current-stage").textContent = st.kind === "verifying" ? tr("verifying_vmaf") : "";
    } else {
      $("current-file").textContent = tr("idle_nothing");
      setProgress("current", 0);
      $("current-pct").textContent = "";
      $("current-stage").textContent = "";
    }

    setProgress("overall", s.overall_progress);
    $("overall-pct").textContent = `${s.overall_progress.toFixed(1)}%`;
    $("eta").textContent = s.eta_secs != null ? `${tr("eta")} ${fmtDuration(s.eta_secs)}` : "";

    $("stat-total").textContent = s.counts.total;
    $("stat-saved").textContent = s.total_space_saved.human;

    $("btn-cancel").disabled = !s.encoding_active;

    updateSummary(s);
    onDiscStatus(s.disc);

    if (activeTab === "queue") await refreshQueue();
  } catch (e) {
    // A refused token is not an unreachable daemon. Reporting both as
    // "unreachable" sent people hunting for a process that was answering fine.
    const banner = $("offline-banner");
    banner.textContent = e.unauthorized
      ? e.message
      : tr("offline", "Daemon unreachable — retrying…");
    banner.classList.remove("hidden");
  } finally {
    pollInFlight = false;
  }
}

// ── Batch summary ───────────────────────────────────────────────────
//
// The counts the daemon reports are cumulative for the session, so the strip
// describes the last completed run rather than the queue as it stands. It is
// dismissible, and a new batch starting re-arms it.

let summaryDismissed = false;
let wasActive = false;

$("summary-dismiss").addEventListener("click", () => {
  summaryDismissed = true;
  $("summary").classList.add("hidden");
});

// These are the daemon's running totals, not one batch's: it has no notion of
// a batch, and its counters run for the life of the process. Reporting a
// delta against the moment encoding started was tried and is worse — analysis
// failures are counted before any encode begins, so a batch with a corrupt
// file in it would subtract its own error away and report zero. The figures
// are labelled as totals and left as totals.
function updateSummary(s) {
  const { converted, skipped, errors } = s.counts;
  // A batch that started is a batch whose result has not been seen yet.
  if (s.counts.active > 0) summaryDismissed = false;

  const finished = s.counts.active === 0 && converted + skipped + errors > 0;
  const summary = $("summary");
  summary.classList.toggle("hidden", !finished || summaryDismissed);

  if (finished) {
    $("summary-converted").textContent = converted;
    $("summary-skipped").textContent = skipped;
    $("summary-errors").textContent = errors;
    // The server's own string, so this reads identically to the tile above it.
    $("summary-saved").textContent = s.total_space_saved.human;
    $("summary-time").textContent =
      s.elapsed_secs != null ? fmtDuration(s.elapsed_secs) : "—";
    $("summary-time-group").classList.toggle("hidden", s.elapsed_secs == null);

    // Colour follows the worst outcome in the run, so 3 errors among 40
    // conversions cannot read as a clean success at a glance.
    summary.classList.toggle("has-errors", errors > 0);
    summary.classList.toggle("has-skips", errors === 0 && skipped > 0);
    $("summary-errors-group").classList.toggle("bad", errors > 0);
    $("summary-skipped-group").classList.toggle("warn", skipped > 0);
  }

  // Announced only on the observed encoding → idle transition, so reloading
  // the page — or a queue reloaded from disk at startup — does not replay a
  // completion that already happened. Goes through the toast, which is
  // already the page's aria-live region.
  if (wasActive && finished) {
    toast(
      `${tr("summary_complete")} ${tr("summary_converted")}: ${converted}, ` +
      `${tr("badge_skipped")}: ${skipped}, ${tr("summary_errors")}: ${errors}`,
      errors > 0,
    );
  }
  wasActive = s.counts.active > 0;
}

// No point polling a tab nobody is looking at; refresh as soon as it is again.
setInterval(() => {
  if (document.visibilityState === "visible") poll();
}, 1000);
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") poll();
});

// Strings first, so nothing renders in English and then flips a moment later.
// If the fetch fails the page keeps the English in index.html and carries on:
// an unreachable or unauthorized daemon is already reported by the poll, and
// an untranslated UI beats a blank one.
(async () => {
  try {
    strings = await api("/api/strings");
    applyStrings();
    document.documentElement.lang = strings.html_lang ?? document.documentElement.lang;
  } catch {
    // Left in English on purpose.
  }
  poll();
})();

// ── Queue table ─────────────────────────────────────────────────────

const BADGE_CLASS = {
  pending: "", analyzing: "analyzing", awaiting_config: "", ready: "",
  ripping: "encoding", encoding: "encoding", verifying: "verifying",
  done: "done", done_vmaf: "done", done_vmaf_failed: "warn",
  skipped: "", error: "error", quality_warning: "warn",
};

// Every status the API can report has a key, so the fall-through never has to
// invent English from the wire value.
const BADGE_KEY = {
  pending: "badge_pending", analyzing: "badge_analyzing",
  awaiting_config: "badge_awaiting_config", ready: "badge_ready",
  verifying: "badge_verifying", done: "badge_done",
  skipped: "badge_skipped", error: "badge_error",
};

function badgeText(st) {
  switch (st.kind) {
    case "encoding": return `${tr("status_encoding")} ${st.progress.toFixed(1)}%`;
    case "ripping": return `${tr("status_ripping")} ${st.progress.toFixed(1)}%`;
    case "done_vmaf": return `${tr("badge_done")} · VMAF ${st.vmaf.toFixed(1)}`;
    case "done_vmaf_failed": return `${tr("badge_done")} · ${tr("badge_vmaf_failed")}`;
    case "quality_warning": return `${tr("badge_low_vmaf")} ${st.vmaf.toFixed(1)}`;
    case "skipped": return `${tr("badge_skipped")} · ${st.reason}`;
    default: return tr(BADGE_KEY[st.kind] ?? "", st.kind);
  }
}

// Rows are kept and updated in place, keyed by job id. Rebuilding the table on
// every poll would throw away hover, focus and any text the user has selected,
// once a second, for the whole length of an encode.
const rows = new Map();
const promptedTrackJobs = new Set();
let openingTracks = false;

function createRow(job) {
  const row = document.createElement("tr");

  const file = row.insertCell();
  file.className = "filecell";
  const name = document.createElement("div");
  const sub = document.createElement("div");
  sub.className = "subline";
  file.append(name, sub);

  const source = row.insertCell();
  const statusCell = row.insertCell();
  const badge = document.createElement("span");
  const bar = document.createElement("progress");
  bar.className = "bar row-bar hidden";
  bar.setAttribute("aria-label", tr("status_encoding"));
  bar.max = 100;
  bar.value = 0;
  const detail = document.createElement("div");
  detail.id = `status-detail-${job.id}`;
  detail.className = "status-detail hidden";
  statusCell.append(badge, bar, detail);

  const size = row.insertCell();
  const saved = row.insertCell();

  const tracks = document.createElement("button");
  tracks.className = "iconbtn";
  tracks.textContent = tr("tracks_title");
  tracks.title = tr("tracks_hint");
  tracks.setAttribute("aria-label", tr("tracks_hint"));
  tracks.addEventListener("click", () => openTracks(job.id));
  row.insertCell().appendChild(tracks);

  const remove = document.createElement("button");
  remove.className = "iconbtn";
  remove.textContent = "✕";
  remove.title = tr("remove_from_queue");
  remove.setAttribute("aria-label", tr("remove_from_queue"));
  remove.addEventListener("click", async () => {
    if (remove.getAttribute("aria-busy") === "true") return;
    remove.disabled = true;
    remove.setAttribute("aria-busy", "true");
    try {
      await post("/api/queue/remove", { id: job.id });
      refreshQueue();
    } catch (e) { toast(e.message, true); }
    finally {
      remove.removeAttribute("aria-busy");
      if (remove.isConnected) refreshQueue();
    }
  });
  row.insertCell().appendChild(remove);

  return { tr: row, name, sub, source, badge, detail, bar, size, saved, tracks, remove };
}

function updateRow(row, job) {
  row.name.textContent = job.filename;
  row.sub.textContent = [
    job.remux_only ? tr("tag_remux") : "",
    job.source_deleted ? tr("tag_source_deleted") : "",
  ].filter(Boolean).join(" · ");
  row.source.textContent = `${job.resolution} ${job.hdr}`;

  row.badge.className = `badge ${BADGE_CLASS[job.status.kind] || ""}`;
  row.badge.textContent = badgeText(job.status);
  const detail = job.status.kind === "error" ? job.status.message
    : job.status.kind === "done_vmaf_failed" ? job.status.reason
    : "";
  row.detail.textContent = detail;
  row.detail.classList.toggle("hidden", !detail);
  if (detail) row.badge.setAttribute("aria-describedby", row.detail.id);
  else row.badge.removeAttribute("aria-describedby");

  // A rip fills the same bar as an encode: same shape of work, same row.
  const live = ["encoding", "ripping"].includes(job.status.kind);
  row.bar.classList.toggle("hidden", !live);
  if (live) row.bar.value = job.status.progress;

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

  row.remove.disabled = row.remove.getAttribute("aria-busy") === "true"
    || ["encoding", "verifying", "ripping"].includes(job.status.kind);
  row.remove.title = tr("remove_from_queue");
  row.remove.setAttribute("aria-label", `${tr("remove_from_queue")}: ${job.filename}`);
  row.tracks.textContent = tr("tracks_title");
  row.tracks.title = tr("tracks_hint");
  row.tracks.setAttribute("aria-label", `${tr("tracks_hint")}: ${job.filename}`);
  row.bar.setAttribute("aria-label", `${tr("status_encoding")}: ${job.filename}`);
  // Tracks are only editable before the encode starts; afterwards the
  // selection is already baked into the running FFmpeg command.
  row.tracks.disabled = !["ready", "awaiting_config"].includes(job.status.kind);
}

let queueRefresh = null;
let clearingFinished = false;
let hasFinishedJobs = false;
$("btn-clear").disabled = true;

function updateClearFinished() {
  $("btn-clear").disabled = clearingFinished || !hasFinishedJobs;
}

function refreshQueue() {
  if (queueRefresh) return queueRefresh;
  queueRefresh = refreshQueueNow().finally(() => { queueRefresh = null; });
  return queueRefresh;
}

async function refreshQueueNow() {
  let data;
  try {
    data = await api("/api/queue");
  } catch {
    return; // offline banner is handled by the status poll
  }
  const tbody = $("queue-body");
  $("queue-empty").classList.toggle("hidden", data.jobs.length > 0);
  hasFinishedJobs = data.jobs.some((job) => [
    "done", "done_vmaf", "done_vmaf_failed", "skipped", "error", "quality_warning",
  ].includes(job.status.kind));
  updateClearFinished();

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
      promptedTrackJobs.delete(id);
    }
  }

  const next = !data.jobs.some((job) => job.status.kind === "analyzing")
    && data.jobs.find((job) =>
      job.status.kind === "awaiting_config" && !promptedTrackJobs.has(job.id));
  if (next && !openingTracks && !$("tracks-modal").open) {
    promptedTrackJobs.add(next.id);
    if (!await openTracks(next.id)) promptedTrackJobs.delete(next.id);
  }
}

$("btn-cancel").addEventListener("click", async () => {
  if (!confirm(tr("cancel_encoding_prompt"))) return;
  try { await post("/api/queue/cancel"); toast(tr("cancelling")); }
  catch (e) { toast(e.message, true); }
});

$("btn-clear").addEventListener("click", async () => {
  if (clearingFinished) return;
  const button = $("btn-clear");
  clearingFinished = true;
  button.setAttribute("aria-busy", "true");
  updateClearFinished();
  try {
    const r = await post("/api/queue/clear_finished");
    toast(trf("removed_finished", { n: r.removed }));
    refreshQueue();
  } catch (e) { toast(e.message, true); }
  finally {
    clearingFinished = false;
    button.removeAttribute("aria-busy");
    updateClearFinished();
  }
});

// ── Per-job track selection ─────────────────────────────────────────

// The modal owns its own copy of the selection while it is open. The queue
// poll keeps running underneath and rewrites rows in place; it must never
// reach in here and discard choices the user has not saved yet.
let trackEditor = null;

$("tracks-close").addEventListener("click", closeTracks);
// <dialog> gives focus trapping, Esc and an inert background for free. Esc
// closes without going through closeTracks(), so the editor is discarded on
// the close event instead — the one place every path passes through.
$("tracks-modal").addEventListener("close", () => { trackEditor = null; });

function closeTracks() {
  $("tracks-modal").close();
}

async function openTracks(id) {
  if (openingTracks || $("tracks-modal").open) return false;
  openingTracks = true;
  // The projected Opus bitrate comes from the audio settings, which are only
  // fetched when the settings tab is opened.
  try {
    if (!settingsLoaded) await loadSettings();
    const data = await api(`/api/job/tracks?id=${id}`);
    trackEditor = {
      id,
      audio: data.audio.map((t) => ({ ...t, mode: t.selected ? (t.opus ? "opus" : "copy") : "off" })),
      subtitles: data.subtitles.map((t) => ({ ...t })),
      editable: data.editable,
      remuxOnly: data.remux_only,
      // null for anything that is not a Dolby Vision source — there is no RPU
      // to keep, so the choice is not offered at all.
      dv: data.dv,
      dvMode: data.dv ? data.dv.mode : null,
    };
    $("tracks-title").textContent = data.filename;
    $("tracks-save").disabled = !data.editable;
    $("tracks-note").textContent = data.editable
      ? ""
      : tr("tracks_locked");
    $("tracks-apply-remaining").checked = false;
    $("tracks-apply-remaining").disabled = !data.editable;
    $("tracks-apply-wrap").classList.toggle("hidden", data.remaining === 0);
    renderTracks();
    $("tracks-modal").showModal();
    $("tracks-body")
      .querySelector("input:not(:disabled), select:not(:disabled), button:not(:disabled)")
      ?.focus();
    return true;
  } catch (e) {
    toast(e.message, true);
    return false;
  } finally {
    openingTracks = false;
  }
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

  body.appendChild(groupHeading(tr("options")));
  body.appendChild(remuxRow());
  if (trackEditor.dv) body.appendChild(dvRow());

  // Toggles, not select-all buttons: when everything is already selected the
  // second press clears it, matching the TUI's 'a' and 's' keys.
  body.appendChild(groupHeading(tr("heading_audio"), audio.length > 0 && toggleAllButton(
    "toggle-all-audio",
    audio.every((t) => t.mode !== "off"),
    (selectAll) => {
      // Clearing drops the Opus marks with the selection, as the TUI does —
      // the server refuses Opus indices for tracks it is not writing.
      for (const track of audio) {
        if (!selectAll) track.mode = "off";
        else if (track.mode === "off") track.mode = "copy";
      }
    },
  )));
  if (audio.length === 0) body.appendChild(emptyNote(tr("no_audio_tracks")));
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
    select.setAttribute("aria-label", track.name);
    const modes = [["off", tr("track_exclude")], ["copy", tr("track_copy")], ["opus", tr("track_opus")]];
    for (const [value, text] of modes) {
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

  body.appendChild(groupHeading(tr("heading_subtitles"), subtitles.length > 0 && toggleAllButton(
    "toggle-all-subtitles",
    subtitles.every((t) => t.selected),
    (selectAll) => {
      for (const track of subtitles) track.selected = selectAll;
    },
  )));
  if (subtitles.length === 0) body.appendChild(emptyNote(tr("no_subtitle_tracks")));
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
    box.setAttribute("aria-label", track.name);
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
    node.textContent = tr("already_opus_copied");
  } else {
    node.textContent = `→ Opus ${projectedKbps(track)}k`;
  }
}

function groupHeading(text, toggle) {
  const node = document.createElement("div");
  node.className = "track-group";
  const label = document.createElement("span");
  label.textContent = text;
  node.appendChild(label);
  if (toggle) node.appendChild(toggle);
  return node;
}

// `allSelected` decides the label and what the press does, so one press always
// undoes the last one.
function toggleAllButton(id, allSelected, apply) {
  const button = document.createElement("button");
  button.id = id;
  button.type = "button";
  button.className = "iconbtn";
  button.textContent = allSelected ? tr("clear_all") : tr("select_all");
  button.disabled = !trackEditor.editable;
  button.addEventListener("click", () => {
    apply(!allSelected);
    renderTracks();
    $(id).focus();
  });
  return button;
}

// A labelled row in the Options group, styled like the track rows above it.
function optionRow(id, labelText, hintText, control) {
  const row = document.createElement("div");
  row.className = "track-row";

  const info = document.createElement("div");
  info.className = "track-info";
  const name = document.createElement("label");
  name.className = "name";
  name.textContent = labelText;
  info.appendChild(name);
  if (hintText) {
    const hint = document.createElement("div");
    hint.className = "sub";
    hint.textContent = hintText;
    info.appendChild(hint);
  }

  control.id = `opt-${id}`;
  name.htmlFor = control.id;
  row.append(info, control);
  return row;
}

function remuxRow() {
  const box = document.createElement("input");
  box.type = "checkbox";
  box.checked = trackEditor.remuxOnly;
  box.disabled = !trackEditor.editable;
  box.addEventListener("change", () => {
    trackEditor.remuxOnly = box.checked;
    // The DV choice only applies to a real encode, so its row changes state.
    renderTracks();
    $("opt-remux").focus();
  });
  return optionRow("remux", tr("remux_only"), tr("remux_hint"), box);
}

function dvRow() {
  const { dv, remuxOnly, editable } = trackEditor;
  const select = document.createElement("select");
  for (const [value, text] of [["keep", tr("dv_keep")], ["hdr10", tr("dv_hdr10")]]) {
    const option = document.createElement("option");
    option.value = value;
    option.textContent = text;
    // Only SVT-AV1 can write the RPU; the server refuses the rest, so the
    // option is not offered rather than offered and rejected.
    option.disabled = value === "keep" && !dv.can_keep;
    select.appendChild(option);
  }
  select.value = trackEditor.dvMode;
  select.disabled = !editable || remuxOnly;
  select.addEventListener("change", () => { trackEditor.dvMode = select.value; });

  const profile = dv.profile == null ? "" : trf("dv_profile", { n: dv.profile });
  const source = trf("dv_source_hint", { profile });
  const hint = remuxOnly
    ? tr("dv_remux_hint")
    : dv.can_keep ? source : `${source} ${tr("dv_requires_svt")}`;
  return optionRow("dolby-vision", tr("dolby_vision"), hint, select);
}

function emptyNote(text) {
  const node = document.createElement("div");
  node.className = "muted";
  node.textContent = text;
  return node;
}

$("tracks-save").addEventListener("click", async () => {
  if (!trackEditor) return;
  const save = $("tracks-save");
  save.disabled = true;
  const { id, audio, subtitles, remuxOnly, dv, dvMode } = trackEditor;
  try {
    const r = await post("/api/job/tracks", {
      id,
      audio_indices: audio.filter((t) => t.mode !== "off").map((t) => t.index),
      audio_to_opus: audio.filter((t) => t.mode === "opus").map((t) => t.index),
      subtitle_indices: subtitles.filter((t) => t.selected).map((t) => t.index),
      remux_only: remuxOnly,
      apply_to_remaining: $("tracks-apply-remaining").checked,
      // Omitted entirely for a non-DV source, which the server rejects.
      ...(dv ? { dv_mode: dvMode } : {}),
    });
    closeTracks();
    toast(r.applied > 1
      ? trf("tracks_applied", { n: r.applied })
      : tr("tracks_updated"));
    refreshQueue();
  } catch (e) { toast(e.message, true); }
  finally {
    if (trackEditor) save.disabled = !trackEditor.editable;
  }
});

// ── File browser ────────────────────────────────────────────────────

const browser = { mode: "file", path: "" };

// Join a directory and an entry name without doubling the separator at root.
const joinPath = (dir, name) => (dir.endsWith("/") ? `${dir}${name}` : `${dir}/${name}`);

$("btn-add-file").addEventListener("click", () => openBrowser("file"));
$("btn-add-folder").addEventListener("click", () => openBrowser("folder"));
$("btn-add-recursive").addEventListener("click", () => openBrowser("folder_recursive"));
$("browser-close").addEventListener("click", () => $("browser").close());
$("browser-hidden").addEventListener("change", () => loadDir(browser.path));
$("browser-choose").addEventListener("click", () => addToQueue(browser.path, browser.mode));

for (const dialog of document.querySelectorAll("dialog")) {
  dialog.addEventListener("click", (event) => {
    if (event.target === dialog) dialog.close();
  });
}

function setBrowserMode(mode) {
  browser.mode = mode;
  $("browser-title").textContent =
    mode === "file" ? tr("select_video_file") :
    mode === "folder" ? tr("select_folder") : tr("select_folder_recursive");
  $("browser-choose").classList.toggle("hidden", mode === "file");
}

function openBrowser(mode) {
  setBrowserMode(mode);
  loadDir(browser.path);
  $("browser").showModal();
}

async function loadDir(path) {
  const request = loadDir.request = (loadDir.request || 0) + 1;
  $("browser").setAttribute("aria-busy", "true");
  let data;
  try {
    data = await api(`/api/fs?path=${encodeURIComponent(path)}${$("browser-hidden").checked ? "&hidden=1" : ""}`);
  } catch (e) {
    if (request === loadDir.request) toast(e.message, true);
    return;
  } finally {
    if (request === loadDir.request) $("browser").removeAttribute("aria-busy");
  }
  if (request !== loadDir.request) return;

  browser.path = data.path;
  $("browser-path").textContent = data.path;
  const list = $("browser-list");
  list.textContent = "";

  // Each row is a <button> so it is reachable by Tab and activated by
  // Enter/Space without a hand-rolled keydown handler. Rows that lead
  // nowhere are disabled rather than dead click targets.
  const addEntry = (mark, label, kind, onclick, sizeText) => {
    const li = document.createElement("li");
    const button = document.createElement("button");
    button.type = "button";
    // The mark is decorative, so the kind it conveys visually has to reach a
    // screen reader through the accessible name instead.
    button.setAttribute("aria-label", `${label}, ${kind}`);

    const marker = document.createElement("span");
    marker.className = "mark";
    marker.setAttribute("aria-hidden", "true");
    marker.textContent = mark;

    const name = document.createElement("span");
    name.textContent = label;
    button.append(marker, name);

    if (sizeText) {
      const size = document.createElement("span");
      size.className = "size";
      size.textContent = sizeText;
      button.appendChild(size);
    }
    if (onclick) button.addEventListener("click", onclick);
    else button.disabled = true;

    li.appendChild(button);
    list.appendChild(li);
  };

  if (data.parent) {
    addEntry("..", tr("parent_directory"), tr("kind_folder"), () => loadDir(data.parent));
  }
  for (const d of data.dirs) {
    const kind = d.symlink
      ? `${tr("kind_folder")}, ${tr("kind_symlink")}`
      : tr("kind_folder");
    addEntry("/", `${d.name}${d.symlink ? " →" : ""}`, kind, () => loadDir(joinPath(data.path, d.name)));
  }
  for (const f of data.files) {
    const filePath = joinPath(data.path, f.name);
    const [mark, kind] = f.is_video
      ? [">", tr("kind_video")]
      : ["·", tr("kind_file")];
    if (f.is_video && browser.mode === "file") {
      addEntry(mark, f.name, kind, () => addToQueue(filePath, "file"), fmtBytes(f.size));
    } else {
      addEntry(mark, f.name, `${kind}, ${tr("kind_not_selectable")}`, null, fmtBytes(f.size));
    }
  }
}

async function addToQueue(path, mode) {
  if (addToQueue.running) return;
  addToQueue.running = true;
  const choose = $("browser-choose");
  choose.disabled = true;
  $("browser").setAttribute("aria-busy", "true");
  const previous = choose.textContent;
  if (mode === "folder_recursive") choose.textContent = tr("scanning");
  try {
    const r = await post("/api/queue/add", { path, mode });
    $("browser").close();
    const skipped = r.already_queued
      ? `, ${trf("already_queued", { n: r.already_queued })}`
      : "";
    toast(r.added > 0
      ? `${trf("added_files", { n: r.added })}${skipped}`
      : trf("nothing_added", { n: r.already_queued }));
    refreshQueue();
  } catch (e) { toast(e.message, true); }
  finally {
    choose.disabled = false;
    choose.textContent = previous;
    $("browser").removeAttribute("aria-busy");
    addToQueue.running = false;
  }
}

// ── Disc import ─────────────────────────────────────────────────────
//
// The dialog holds the drive list it was opened with and the titles chosen so
// far; everything else about the disc — scanning, the titles, the failure that
// stopped it — comes from the status poll, which is already running.

let disc = null;

$("btn-add-disc").addEventListener("click", openDisc);
$("disc-close").addEventListener("click", () => $("disc-modal").close());
// Esc closes without going through the button, so the state is dropped on the
// close event: the one place every path passes through. A scan still running
// is called off, since it holds the drive.
$("disc-modal").addEventListener("close", () => {
  // A rip closes this dialog on its way to the queue, where it is cancelled
  // like any other job; only an abandoned scan is called off here.
  if (disc && !disc.ripping && discState.scanning) {
    post("/api/discs/cancel").catch(() => {});
  }
  disc = null;
});

async function openDisc() {
  if ($("disc-modal").open) return;
  disc = { drives: [], drive: null, selected: new Set(), error: null, loading: true };
  renderDisc();
  $("disc-modal").showModal();
  try {
    const { drives } = await api("/api/discs");
    disc.drives = drives;
    disc.loading = false;
    // A list of one is not a choice.
    if (drives.length === 1) await scanDisc(drives[0].id);
  } catch (e) {
    disc.loading = false;
    disc.error = e.message;
  }
  renderDisc();
}

async function scanDisc(id) {
  disc.drive = id;
  disc.selected.clear();
  disc.error = null;
  try {
    await post("/api/discs/scan", { drive: id });
  } catch (e) {
    disc.error = e.message;
  }
  renderDisc();
}

// The last `disc` block from /api/status, so the dialog can render between
// polls without asking for it again.
let discState = { active: false, scanning: false, titles: [], error: null };

function onDiscStatus(state) {
  discState = state ?? discState;
  if ($("disc-modal").open) renderDisc();
}

function renderDisc() {
  const body = $("disc-body");
  const note = $("disc-note");
  const rip = $("disc-rip");
  body.textContent = "";
  note.textContent = "";
  rip.disabled = true;
  if (!disc) return;

  // A failure from either side of the exchange reads the same way here.
  const failure = disc.error ?? discState.error;
  if (failure) body.appendChild(discNote(failure, true));

  if (disc.loading) {
    body.appendChild(discNote(tr("disc_scanning")));
    return;
  }
  if (disc.drives.length === 0) {
    if (!failure) body.appendChild(discNote(tr("disc_no_drive"), true));
    return;
  }

  // More than one drive, and none picked yet: choose one first.
  if (disc.drive == null) {
    body.appendChild(groupHeading(tr("disc_select_drive")));
    for (const drive of disc.drives) {
      const row = document.createElement("button");
      row.className = "disc-drive";
      row.textContent = `${drive.name} — ${drive.disc_label ?? tr("disc_drive_empty")}`;
      row.addEventListener("click", () => scanDisc(drive.id));
      body.appendChild(row);
    }
    return;
  }

  const drive = disc.drives.find((d) => d.id === disc.drive);
  const heading = [
    drive?.name,
    drive?.disc_label ?? tr("disc_drive_empty"),
    discState.disc_type,
  ].filter(Boolean).join(" · ");
  body.appendChild(groupHeading(heading));

  if (discState.scanning) {
    body.appendChild(discNote(tr("disc_scanning")));
    return;
  }
  if (discState.titles.length === 0) {
    if (!failure) body.appendChild(discNote(tr("disc_no_titles")));
    return;
  }

  for (const title of discState.titles) {
    const row = document.createElement("label");
    row.className = "track-row";

    const box = document.createElement("input");
    box.type = "checkbox";
    box.checked = disc.selected.has(title.id);
    box.addEventListener("change", () => {
      if (box.checked) disc.selected.add(title.id);
      else disc.selected.delete(title.id);
      renderDisc();
    });
    row.appendChild(box);

    const name = document.createElement("span");
    name.className = "track-name";
    name.textContent = title.name;
    row.appendChild(name);

    const meta = document.createElement("span");
    meta.className = "muted";
    meta.textContent = [
      title.duration,
      title.size,
      `${title.chapters} ${tr("disc_chapters")}`,
      ...title.tracks,
    ].filter(Boolean).join(" · ");
    row.appendChild(meta);

    body.appendChild(row);
  }

  note.textContent = `${disc.selected.size} ${tr("selected")}`;
  rip.disabled = disc.selected.size === 0 || discState.active;
}

function discNote(text, bad = false) {
  const note = document.createElement("p");
  note.className = bad ? "disc-error" : "muted";
  note.textContent = text;
  return note;
}

$("disc-rip").addEventListener("click", async () => {
  if (!disc || disc.selected.size === 0) return;
  const button = $("disc-rip");
  button.disabled = true;
  try {
    await post("/api/discs/rip", {
      drive: disc.drive,
      titles: [...disc.selected],
    });
    disc.ripping = true;
    $("disc-modal").close();
    refreshQueue();
  } catch (e) {
    toast(e.message, true);
    button.disabled = false;
  }
});

// ── Settings ────────────────────────────────────────────────────────

let settingsLoaded = false;
let config = null;
let savedConfig = null;
let settingsSaving = false;

const cloneConfig = (value) => JSON.parse(JSON.stringify(value));

function updateSettingsActions() {
  const dirty = savedConfig != null && JSON.stringify(config) !== JSON.stringify(savedConfig);
  $("btn-save-settings").disabled = settingsSaving || !dirty;
  $("btn-reset-settings").disabled = settingsSaving || !dirty;
}

// Native language names and product names are not translated — they read the
// same in every locale.
const LANGS = [["en", "English"], ["it", "Italiano"], ["es", "Español"], ["fr", "Français"], ["de", "Deutsch"], ["zh", "中文"]];
const ENCODERS = [["SvtAv1", "SVT-AV1 (Software)"], ["Nvenc", "NVENC (NVIDIA)"], ["Qsv", "Quick Sync (Intel)"], ["Amf", "AMF (AMD)"]];
const NVENC_PRESETS = ["p1", "p2", "p3", "p4", "p5", "p6", "p7"].map((p) => [p, p]);

const RF_KEY = { SvtAv1: "crf", Nvenc: "nvenc_cq", Qsv: "qsv_quality", Amf: "amf_quality" };

// Built on each render rather than held in a const: the string map arrives
// after this file is evaluated, so a const would capture the untranslated text.
const presets = () => [
  ["low", tr("qp_low")], ["medium", tr("qp_medium")],
  ["high", tr("qp_high")], ["custom", tr("qp_custom")],
];
const audioModes = () => [
  ["copy", tr("cfg_audio_mode_copy")], ["opus", tr("cfg_audio_mode_opus")],
];
const rfTiers = () => [
  ["sd", tr("rf_sd")], ["hd", tr("rf_hd")], ["full_hd", tr("rf_full_hd")],
  ["full_hd_hdr", tr("rf_full_hd_hdr")], ["full_hd_dv", tr("rf_full_hd_dv")],
  ["uhd", tr("rf_uhd")], ["uhd_hdr", tr("rf_uhd_hdr")], ["uhd_dv", tr("rf_uhd_dv")],
];

// The shape of this list mirrors the Rust config schema, but every label it
// shows comes from the same key map the rest of the UI uses — not a second
// English list that would have to be translated all over again.
function settingsFields(cfg) {
  const fields = [
    { group: tr("group_general") },
    { path: "language", label: tr("cfg_language"), type: "select", options: LANGS },
    { path: "encoder", label: tr("cfg_encoder"), type: "select", options: ENCODERS, rebuild: true },
    { path: "quality_preset", label: tr("cfg_quality_preset"), type: "select", options: presets(), rebuild: true },
    { group: tr("group_quality") },
    { path: "quality.vmaf_enabled", label: tr("cfg_vmaf_enabled"), type: "checkbox", rebuild: true },
    { path: "quality.vmaf_threshold", label: tr("cfg_vmaf_threshold"), type: "number", min: 0, max: 100, step: 0.1, disabled: !cfg.quality.vmaf_enabled },
    { path: "quality.delete_source_on_success", label: tr("cfg_delete_source"), type: "checkbox", disabled: !cfg.quality.vmaf_enabled, warning: tr("delete_source_warning") },
  ];
  if (["SvtAv1", "Nvenc"].includes(cfg.encoder)) {
    fields.push({ group: tr("group_performance") });
    if (cfg.encoder === "SvtAv1") {
      fields.push({ path: "performance.svt_preset", label: tr("cfg_svt_preset"), type: "number", min: 0, max: 13 });
    } else {
      fields.push({ path: "performance.nvenc_preset", label: tr("cfg_nvenc_preset"), type: "select", options: NVENC_PRESETS });
    }
  }
  if (cfg.quality_preset === "custom") {
    fields.push({ group: `${tr("group_rate_factors")} (${RF_KEY[cfg.encoder]})` });
    const maxQuality = cfg.encoder === "SvtAv1" ? 63 : 51;
    for (const [tier, label] of rfTiers()) {
      fields.push({ path: `presets.${tier}.${RF_KEY[cfg.encoder]}`, label, type: "number", min: 0, max: maxQuality });
    }
  }
  fields.push(
    { group: tr("group_output") },
    { path: "output.suffix", label: tr("cfg_output_suffix"), type: "text" },
    { path: "output.container", label: tr("cfg_output_container"), type: "text" },
    { path: "output.same_directory", label: tr("cfg_same_directory"), type: "checkbox", rebuild: true },
    { path: "output.output_directory", label: tr("cfg_output_directory"), type: "text", nullable: true, disabled: cfg.output.same_directory, required: !cfg.output.same_directory },
    { group: tr("group_tracks") },
    { path: "tracks.preferred_audio_languages", label: tr("cfg_audio_languages"), type: "list" },
    { path: "tracks.preferred_subtitle_languages", label: tr("cfg_subtitle_languages"), type: "list" },
    { path: "tracks.select_all_fallback", label: tr("cfg_select_all_fallback"), type: "checkbox" },
    { group: tr("group_audio") },
    { path: "audio.default_mode", label: tr("cfg_audio_default"), type: "select", options: audioModes() },
    { path: "audio.opus_bitrate_per_channel", label: tr("cfg_opus_bitrate"), type: "number", min: 16, max: 256 },
    { path: "audio.skip_already_opus", label: tr("cfg_skip_already_opus"), type: "checkbox" },
    // The [daemon] block is deliberately absent: browse_root confines this file
    // browser and auth_token guards this API, so the server refuses to let a
    // client widen its own access. Edit those in config.toml or the TUI.
    { group: tr("group_daemon") },
    { note: tr("daemon_note") },
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
    // The path is already unique per field and stable across rebuilds, so it
    // makes a better id than a counter that shifts when the form is rebuilt.
    const inputId = `set-${field.path.replace(/\./g, "-")}`;
    label.htmlFor = inputId;
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
      if (field.step != null) input.step = field.step;
      input.value = field.type === "list" ? (value || []).join(", ") : value ?? "";
    }
    input.id = inputId;
    input.disabled = Boolean(field.disabled);
    input.required = Boolean(field.required || field.type === "number");
    let touched = false;
    const updateValidity = () => input.setAttribute("aria-invalid", input.validity.valid ? "false" : "true");
    input.addEventListener("blur", () => {
      touched = true;
      updateValidity();
    });
    input.addEventListener("input", () => { if (touched) updateValidity(); });

    input.addEventListener("change", () => {
      if (!input.validity.valid) {
        input.reportValidity();
        return;
      }
      let parsed;
      if (field.type === "checkbox") parsed = input.checked;
      else if (field.type === "number") parsed = Number(input.value);
      else if (field.type === "list") parsed = input.value.split(",").map((s) => s.trim()).filter(Boolean);
      else if (field.nullable && input.value.trim() === "") parsed = null;
      else parsed = input.value;
      setPath(config, field.path, parsed);
      if (field.rebuild) buildSettingsForm();
      updateSettingsActions();
    });

    row.appendChild(input);
    if (field.warning) {
      const warning = document.createElement("div");
      warning.id = `${inputId}-warning`;
      warning.className = "field-warning";
      warning.textContent = field.warning;
      input.setAttribute("aria-describedby", warning.id);
      row.appendChild(warning);
    }
    form.appendChild(row);
  }
  updateSettingsActions();
}

async function loadSettings() {
  try {
    config = await api("/api/settings");
    savedConfig = cloneConfig(config);
    settingsLoaded = true;
    buildSettingsForm();
  } catch (e) { toast(e.message, true); }
}

$("btn-save-settings").addEventListener("click", async () => {
  const form = $("settings-form");
  if (settingsSaving || !form.reportValidity()) return;
  settingsSaving = true;
  form.inert = true;
  form.setAttribute("aria-busy", "true");
  updateSettingsActions();
  const languageChanged = savedConfig?.language !== config.language;
  try {
    config = await post("/api/settings", config);
    savedConfig = cloneConfig(config);
    if (languageChanged) {
      strings = await api("/api/strings");
      applyStrings();
      document.documentElement.lang = strings.html_lang ?? document.documentElement.lang;
    }
    buildSettingsForm();
    updateSettingsActions();
    toast(tr("saved_exclaim"));
  } catch (e) { toast(e.message, true); }
  finally {
    settingsSaving = false;
    form.inert = false;
    form.removeAttribute("aria-busy");
    updateSettingsActions();
  }
});

$("btn-reset-settings").addEventListener("click", () => {
  config = cloneConfig(savedConfig);
  buildSettingsForm();
  updateSettingsActions();
});
