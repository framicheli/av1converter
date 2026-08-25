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

// Writes into one of the two live regions in index.html. Their role and
// aria-live are fixed in the markup and never reassigned.
function announce(message, isError) {
  const region = $(isError ? "live-assertive" : "live-polite");
  // A repeat of the current text gets a trailing space; the region content
  // then differs and the announcement fires again.
  region.textContent = region.textContent === message ? `${message} ` : message;
}

function toast(message, isError) {
  const el = $("toast");
  $("toast-message").textContent = message;
  el.classList.toggle("error", Boolean(isError));
  announce(message, isError);
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
let token = (() => {
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

// A progress element without a value attribute is indeterminate.
function setIndeterminate(name) {
  const progress = $(`${name}-progress`);
  progress.removeAttribute("value");
  progress.textContent = "";
}

// Label shown beside the current-file bar, one per working status.
const PHASE_KEY = {
  ripping: "status_ripping", analyzing: "badge_analyzing",
  encoding: "status_encoding", verifying: "verifying_vmaf",
};

// Poll updates leave unchanged text nodes intact.
function setText(el, text) {
  if (el.textContent !== text) el.textContent = text;
}

// Offline mode keeps cached values visible and disables server mutations.
let offline = false;
function setOffline(next) {
  if (offline === next) return;
  offline = next;
  $("offline-banner").classList.toggle("hidden", !next);
  if (next) $("queue-table").setAttribute("aria-describedby", "offline-banner");
  else $("queue-table").removeAttribute("aria-describedby");
  for (const button of document.querySelectorAll("#tab-queue .toolbar button")) {
    button.disabled = next;
  }
  updateClearFinished();
  updateSettingsActions();
  for (const row of rows.values()) row.applyDisabled();
  updateModalActions();
  if (next) announce($("offline-banner").textContent, true);
}

function updateModalActions() {
  $("browser-list").inert = offline;
  $("browser-hidden").disabled = offline;
  $("tracks-body").inert = offline;
  $("tracks-apply-remaining").disabled = offline || !trackEditor?.editable;
  $("tracks-save").disabled = offline || !trackEditor?.editable || Boolean(trackEditor?.saving);
  $("disc-body").inert = offline;
  updateBrowserChoose();
  updateDiscFooter();
}

let pollInFlight = false;
async function poll() {
  if (pollInFlight) return;
  pollInFlight = true;
  try {
    const s = await api("/api/status");
    setOffline(false);

    const pill = $("status-pill");
    const ripping = s.current?.status.kind === "ripping";
    const statusKey = ripping ? "status_ripping"
      : s.encoding_active ? "status_encoding"
      : s.counts.analyzing > 0 ? "badge_analyzing"
      // AwaitingConfig requires track confirmation before queue processing continues.
      : s.counts.awaiting_config > 0 ? "confirm_tracks"
      : s.counts.ready > 0 ? "badge_ready"
      : s.counts.pending > 0 ? "badge_pending"
      : "status_idle";
    setText(pill, tr(statusKey));
    pill.className = `pill ${s.counts.active > 0 ? "encoding" : ""}`;

    if (s.current) {
      const st = s.current.status;
      setText($("current-file"), s.current.filename);
      // Encoding and ripping carry a percentage; the other phases do not.
      if (st.kind === "encoding" || st.kind === "ripping") {
        setProgress("current", st.progress);
        setText($("current-pct"), `${st.progress.toFixed(1)}%`);
      } else {
        setIndeterminate("current");
        setText($("current-pct"), "");
      }
      setText($("current-stage"), tr(PHASE_KEY[st.kind] ?? ""));
    } else {
      setText($("current-file"), tr("idle_nothing"));
      setProgress("current", 0);
      setText($("current-pct"), "");
      setText($("current-stage"), "");
    }

    setProgress("overall", s.overall_progress);
    setText($("overall-pct"), `${s.overall_progress.toFixed(1)}%`);
    setText($("eta"), s.eta_secs != null ? `${tr("eta")} ${fmtDuration(s.eta_secs)}` : "");

    setText($("stat-total"), String(s.counts.total));
    setText($("stat-saved"), s.total_space_saved.human);

    $("btn-cancel").disabled = !s.encoding_active;

    updateSummary(s);
    onDiscStatus(s.disc);

    if (activeTab === "queue") await refreshQueue();
  } catch (e) {
    // A refused token is not an unreachable daemon. Reporting both as
    // "unreachable" sent people hunting for a process that was answering fine.
    // Runs once per second for the length of an outage. The text is set before
    // setOffline() reads it for the announcement.
    setText(
      $("offline-banner"),
      e.unauthorized ? e.message : tr("offline", "Daemon unreachable — retrying…"),
    );
    setOffline(true);
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
// Raised on the first encoding → idle transition this page observes. The
// daemon's counters outlive any one page load.
let sawCompletion = false;

$("summary-dismiss").addEventListener("click", () => {
  summaryDismissed = true;
  $("summary").classList.add("hidden");
});

// The daemon resets these totals when fresh work follows a settled queue,
// before analysis starts, so failures during analysis belong to the new batch.
function updateSummary(s) {
  const { converted, skipped, errors, cancelled = 0 } = s.counts;
  // A batch that started is a batch whose result has not been seen yet.
  if (s.counts.active > 0) summaryDismissed = false;

  const finished = s.counts.active === 0 && converted + skipped + errors > 0;
  if (wasActive && finished) sawCompletion = true;

  const summary = $("summary");
  const show = finished && sawCompletion && !summaryDismissed;
  summary.classList.toggle("hidden", !show);

  if (show) {
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
  // completion that already happened.
  if (wasActive && finished) {
    // Headline names the outcome: completed, errored, or stopped.
    const headline = errors > 0 ? "summary_failed"
      : cancelled > 0 ? "summary_stopped"
      : "summary_complete";
    toast(
      `${tr(headline)} ${tr("session_totals")} — ` +
      `${tr("summary_converted")}: ${converted}, ` +
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
  pending: "", analyzing: "analyzing", awaiting_config: "warn", ready: "",
  ripping: "encoding", encoding: "encoding", verifying: "verifying",
  done: "done", done_vmaf: "done", done_vmaf_failed: "warn",
  skipped: "", error: "error", quality_warning: "warn",
};

// Every status the API can report has a key, so the fall-through never has to
// invent English from the wire value.
const BADGE_KEY = {
  pending: "badge_pending", analyzing: "badge_analyzing",
  awaiting_config: "confirm_tracks", ready: "badge_ready",
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

  const file = document.createElement("th");
  file.scope = "row";
  file.className = "filecell";
  row.appendChild(file);
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
  const confirm = document.createElement("button");
  confirm.className = "badge warn hidden";
  confirm.textContent = tr("confirm_tracks");
  confirm.addEventListener("click", () => openTracks(job.id));
  statusCell.append(badge, confirm, bar, detail);

  const size = row.insertCell();
  const saved = row.insertCell();

  const moveUp = document.createElement("button");
  moveUp.className = "iconbtn";
  moveUp.textContent = "↑";
  moveUp.addEventListener("click", async () => {
    if (moveUp.getAttribute("aria-busy") === "true") return;
    moveUp.disabled = true;
    moveUp.setAttribute("aria-busy", "true");
    try {
      await post("/api/queue/move_up", { id: job.id });
      refreshQueue();
    } catch (e) { toast(e.message, true); }
    finally {
      moveUp.removeAttribute("aria-busy");
      if (moveUp.isConnected) refreshQueue();
    }
  });
  row.insertCell().appendChild(moveUp);

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

  const entry = { tr: row, name, sub, source, badge, confirm, detail, bar, size, saved, moveUp, tracks, remove, kind: job.status.kind, tracksEditable: job.tracks_editable, canMoveUp: job.can_move_up };
  entry.applyDisabled = () => {
    entry.moveUp.disabled = offline
      || entry.moveUp.getAttribute("aria-busy") === "true"
      || !entry.canMoveUp;
    entry.remove.disabled = offline
      || entry.remove.getAttribute("aria-busy") === "true"
      || ["encoding", "verifying", "ripping"].includes(entry.kind);
    entry.tracks.disabled = offline || !entry.tracksEditable;
    entry.confirm.disabled = offline;
  };
  return entry;
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
  const awaitingTracks = job.status.kind === "awaiting_config";
  row.badge.classList.toggle("hidden", awaitingTracks);
  row.confirm.classList.toggle("hidden", !awaitingTracks);
  row.confirm.textContent = tr("confirm_tracks");
  row.confirm.setAttribute("aria-label", `${tr("confirm_tracks")}: ${job.filename}`);
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

  if (job.saved_percent == null) {
    row.saved.textContent = "";
    row.saved.className = "";
  } else {
    const percent = Math.round(Math.abs(job.saved_percent));
    row.saved.textContent = percent === 0
      ? "0%"
      : `${job.saved_percent < 0 ? "+" : "−"}${percent}%`;
    row.saved.className = job.saved_percent < 0 ? "grew" : "";
  }

  row.kind = job.status.kind;
  row.tracksEditable = job.tracks_editable;
  row.canMoveUp = job.can_move_up;
  row.applyDisabled();
  row.moveUp.title = tr("move_up");
  row.moveUp.setAttribute("aria-label", `${tr("move_up")}: ${job.filename}`);
  row.remove.title = tr("remove_from_queue");
  row.remove.setAttribute("aria-label", `${tr("remove_from_queue")}: ${job.filename}`);
  row.tracks.textContent = tr("tracks_title");
  row.tracks.title = tr("tracks_hint");
  row.tracks.setAttribute("aria-label", `${tr("tracks_hint")}: ${job.filename}`);
  row.bar.setAttribute("aria-label", `${tr("status_encoding")}: ${job.filename}`);
  // The Tracks button is the accented action on a row that awaits config.
  row.tracks.classList.toggle("primary", job.status.kind === "awaiting_config");
}

let queueRefresh = null;
let clearingFinished = false;
let hasFinishedJobs = false;
$("btn-clear").disabled = true;

function updateClearFinished() {
  $("btn-clear").disabled = offline || clearingFinished || !hasFinishedJobs;
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
    return; // the status poll owns the offline banner
  }
  const tbody = $("queue-body");
  $("queue-empty").classList.toggle("hidden", data.jobs.length > 0);
  hasFinishedJobs = data.jobs.some((job) => [
    "done", "done_vmaf", "done_vmaf_failed", "skipped", "error", "quality_warning",
  ].includes(job.status.kind));
  updateClearFinished();

  const seen = new Set();
  for (const [position, job] of data.jobs.entries()) {
    seen.add(job.id);
    let row = rows.get(job.id);
    if (!row) {
      row = createRow(job);
      rows.set(job.id, row);
    }
    if (tbody.children[position] !== row.tr) {
      tbody.insertBefore(row.tr, tbody.children[position] || null);
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
  if (next && !openingTracks && !document.querySelector("dialog[open]")) {
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

// The modal keeps an editable copy of one job's track selection.
let trackEditor = null;

$("tracks-close").addEventListener("click", closeTracks);
$("tracks-back").addEventListener("click", closeTracks);
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
    const editor = trackEditor = {
      id,
      audio: data.audio.map((t) => ({ ...t, mode: t.selected ? (t.opus ? "opus" : "copy") : "off" })),
      subtitles: data.subtitles.map((t) => ({ ...t })),
      editable: data.editable,
      remuxOnly: data.remux_only,
      dv: data.dv,
      dvMode: data.dv ? data.dv.mode : null,
    };
    $("tracks-filename").textContent = ` — ${data.filename}`;
    $("tracks-save").disabled = offline || !data.editable;
    $("tracks-save").removeAttribute("aria-busy");
    $("tracks-note").textContent = data.editable
      ? ""
      : tr("tracks_locked");
    $("tracks-apply-remaining").checked = false;
    $("tracks-apply-remaining").disabled = offline || !data.editable;
    $("tracks-apply-wrap").classList.toggle("hidden", data.remaining === 0);
    // The server maps the selection onto other files by track order.
    $("tracks-apply-hint").classList.toggle("hidden", data.remaining === 0);
    renderTracks();
    if (trackEditor !== editor) return false;
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

// Track projections use the last settings accepted by the daemon.
function projectedKbps(track) {
  const perChannel = savedConfig?.audio?.opus_bitrate_per_channel ?? 64;
  return (track.channels || 2) * perChannel;
}

function isAlreadyOpus(track) {
  return (savedConfig?.audio?.skip_already_opus ?? true)
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
  const node = document.createElement("h3");
  node.className = "track-group";
  const label = document.createElement("span");
  label.textContent = text;
  node.appendChild(label);
  if (toggle) node.appendChild(toggle);
  return node;
}

function toggleAllButton(id, allSelected, apply, options = {}) {
  const { enabled = trackEditor?.editable ?? true, rerender = renderTracks } = options;
  const button = document.createElement("button");
  button.id = id;
  button.type = "button";
  button.className = "iconbtn";
  button.textContent = allSelected ? tr("clear_all") : tr("select_all");
  button.disabled = !enabled;
  button.addEventListener("click", () => {
    apply(!allSelected);
    rerender();
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
  const editor = trackEditor;
  if (editor.saving) return;
  editor.saving = true;
  const save = $("tracks-save");
  save.disabled = true;
  save.setAttribute("aria-busy", "true");
  const { id, audio, subtitles, remuxOnly, dv, dvMode } = editor;
  try {
    const r = await post("/api/job/tracks", {
      id,
      audio_indices: audio.filter((t) => t.mode !== "off").map((t) => t.index),
      audio_to_opus: audio.filter((t) => t.mode === "opus").map((t) => t.index),
      subtitle_indices: subtitles.filter((t) => t.selected).map((t) => t.index),
      remux_only: remuxOnly,
      apply_to_remaining: $("tracks-apply-remaining").checked,
      ...(dv ? { dv_mode: dvMode } : {}),
    });
    if (trackEditor === editor) closeTracks();
    toast(r.applied > 1
      ? trf("tracks_applied", { n: r.applied })
      : tr("tracks_updated"));
    refreshQueue();
  } catch (e) { toast(e.message, true); }
  finally {
    editor.saving = false;
    if (trackEditor === editor) {
      save.removeAttribute("aria-busy");
      save.disabled = offline || !editor.editable;
    }
  }
});

// ── File browser ────────────────────────────────────────────────────

const browser = { mode: "file", path: "", session: 0 };

// Join a directory and an entry name without doubling the separator at root.
const joinPath = (dir, name) => (dir.endsWith("/") ? `${dir}${name}` : `${dir}/${name}`);

$("btn-add-file").addEventListener("click", () => openBrowser("file"));
$("btn-add-folder").addEventListener("click", () => openBrowser("folder"));
$("btn-add-recursive").addEventListener("click", () => openBrowser("folder_recursive"));
$("browser-close").addEventListener("click", () => $("browser").close());
$("browser-hidden").addEventListener("change", () => loadDir(browser.path));
$("browser-choose").addEventListener("click", () => {
  // Picker mode returns the path to its caller; queue modes act on it here.
  if (browser.onPick) {
    const pick = browser.onPick;
    $("browser").close();
    pick(browser.path);
  } else if (browser.mode === "disc") scanDiscFolder(browser.path);
  else addToQueue(browser.path, browser.mode);
});
$("browser").addEventListener("close", () => {
  browser.session += 1;
  browser.onPick = null;
});

for (const dialog of document.querySelectorAll("dialog")) {
  if (dialog.id === "tracks-modal") continue;
  dialog.addEventListener("click", (event) => {
    if (event.target === dialog) dialog.close();
  });
}

function setBrowserMode(mode, onPick) {
  browser.mode = mode;
  browser.onPick = onPick ?? null;
  $("browser-title").textContent =
    mode === "file" ? tr("select_video_file") :
    mode === "folder" ? tr("select_folder") :
    mode === "disc" ? tr("disc_select_folder") : tr("select_folder_recursive");
  $("browser-choose").textContent =
    mode === "disc" ? tr("disc_scan_this_folder") : tr("select_this_folder");
  $("browser-choose").classList.toggle("hidden", mode === "file");
  updateBrowserChoose();
}

function openBrowser(mode, onPick, startPath = browser.path) {
  browser.session += 1;
  setBrowserMode(mode, onPick);
  $("browser").showModal();
  loadDir(startPath || browser.path, true);
}

function updateBrowserChoose() {
  $("browser-choose").disabled = offline
    || Boolean(addToQueue.running)
    || Boolean(scanDiscFolder.running);
}

async function loadDir(path, takeFocus = false) {
  const request = loadDir.request = (loadDir.request || 0) + 1;
  const session = browser.session;
  $("browser").setAttribute("aria-busy", "true");
  $("browser-busy").classList.remove("hidden");
  let data;
  try {
    data = await api(`/api/fs?path=${encodeURIComponent(path)}${$("browser-hidden").checked ? "&hidden=1" : ""}`);
  } catch (e) {
    if (request === loadDir.request && session === browser.session) toast(e.message, true);
    return;
  } finally {
    if (request === loadDir.request && session === browser.session) {
      $("browser").removeAttribute("aria-busy");
      $("browser-busy").classList.add("hidden");
    }
  }
  if (request !== loadDir.request || session !== browser.session) return;

  browser.path = data.path;
  renderCrumbs(data.path);
  // Focus target for a directory with no selectable entries.
  $("browser-title").tabIndex = -1;
  const list = $("browser-list");
  list.textContent = "";

  // Each row is a <button> so it is reachable by Tab and activated by
  // Enter/Space without a hand-rolled keydown handler. Rows that lead
  // nowhere are disabled rather than dead click targets.
  const addEntry = (mark, label, kind, onclick, sizeText) => {
    const li = document.createElement("li");
    const button = document.createElement("button");
    button.type = "button";
    button.setAttribute("aria-label", [label, kind, sizeText].filter(Boolean).join(", "));

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

  // Focus moves to the first row on the initial load, and whenever the element
  // that held it was removed by the rebuild. The Hidden files checkbox reloads
  // the list and keeps focus.
  const active = document.activeElement;
  if (takeFocus || !active || active === document.body || !active.isConnected) {
    (list.querySelector("button:not(:disabled)") ?? $("browser-title")).focus();
  }
}

// One button per segment of the current path, rooted at "/". The segment for
// the directory on screen is inert and marked aria-current.
function renderCrumbs(path) {
  const crumbs = $("browser-path");
  crumbs.textContent = "";
  const segments = path.split("/").filter(Boolean);
  const add = (label, target, isCurrent) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "crumb";
    button.textContent = label;
    button.setAttribute("aria-label", `${tr("kind_folder")}: ${label}`);
    if (isCurrent) {
      button.setAttribute("aria-current", "true");
      button.disabled = true;
    } else {
      button.addEventListener("click", () => loadDir(target));
    }
    crumbs.appendChild(button);
  };
  add("/", "/", segments.length === 0);
  let walked = "";
  segments.forEach((segment, index) => {
    walked += `/${segment}`;
    add(segment, walked, index === segments.length - 1);
  });
}

async function addToQueue(path, mode) {
  if (addToQueue.running) return;
  addToQueue.running = true;
  const session = browser.session;
  const choose = $("browser-choose");
  updateBrowserChoose();
  $("browser").setAttribute("aria-busy", "true");
  const previous = choose.textContent;
  if (mode === "folder_recursive") choose.textContent = tr("scanning");
  try {
    const r = await post("/api/queue/add", { path, mode });
    if (browser.session === session && $("browser").open) $("browser").close();
    const skipped = r.already_queued
      ? `, ${trf("already_queued", { n: r.already_queued })}`
      : "";
    toast(r.added > 0
      ? `${trf("added_files", { n: r.added })}${skipped}`
      : trf("nothing_added", { n: r.already_queued }));
    refreshQueue();
  } catch (e) { toast(e.message, true); }
  finally {
    addToQueue.running = false;
    if (browser.session === session) {
      choose.textContent = previous;
      $("browser").removeAttribute("aria-busy");
    }
    updateBrowserChoose();
  }
}

// ── Disc import ─────────────────────────────────────────────────────
//
// The dialog holds the drive list it was opened with and the titles chosen so
// far; everything else about the disc — scanning, the titles, the failure that
// stopped it — comes from the status poll, which is already running.

let disc = null;
// Raised while the file browser is up in place of this dialog, so the close
// below leaves the state alone and does not call off a scan.
let discBrowsing = false;

$("btn-add-disc").addEventListener("click", openDisc);
$("disc-close").addEventListener("click", () => $("disc-modal").close());
// Esc closes without going through the button, so the state is dropped on the
// close event: the one place every path passes through. A scan still running
// is called off, since it holds the drive.
$("disc-modal").addEventListener("close", () => {
  if (discBrowsing) return;
  // A rip closes this dialog on its way to the queue, where it is cancelled
  // like any other job; only an abandoned scan is called off here.
  if (disc && !disc.ripping && (disc.scanPending || discState.scanning)) {
    post("/api/discs/cancel").catch(() => {});
  }
  disc = null;
  discShapeRendered = null;
});

async function openDisc() {
  if ($("disc-modal").open) return;
  const session = disc = {
    drives: [], drive: null, folder: null, selected: new Set(),
    error: null, loading: true, scanPending: false,
  };
  renderDisc();
  $("disc-modal").showModal();
  try {
    const { drives } = await api("/api/discs");
    if (disc !== session) return;
    session.drives = drives;
    session.loading = false;
    // A list of one is not a choice.
    if (drives.length === 1) await scanDisc(drives[0].id);
  } catch (e) {
    if (disc !== session) return;
    session.loading = false;
    session.error = e.message;
  }
  if (disc === session) renderDisc();
}

// Two dialogs stack in the top layer in the order they were opened, so the
// disc modal steps aside for the browser and comes back when it closes.
$("disc-folder").addEventListener("click", () => {
  if (!disc || offline) return;
  discBrowsing = true;
  $("disc-modal").close();
  openBrowser("disc");
});

$("browser").addEventListener("close", () => {
  if (!discBrowsing) return;
  discBrowsing = false;
  $("disc-modal").showModal();
  renderDisc();
});

async function scanDiscFolder(path) {
  if (!disc || disc.loading || scanDiscFolder.running) return;
  const session = disc;
  const browserSession = browser.session;
  scanDiscFolder.running = true;
  session.drive = null;
  session.folder = path;
  session.selected.clear();
  session.error = null;
  session.loading = true;
  session.scanPending = true;
  updateBrowserChoose();
  try {
    await post("/api/discs/scan", { folder: path });
    if (disc !== session) post("/api/discs/cancel").catch(() => {});
  } catch (e) {
    if (disc === session) {
      session.error = e.message;
      session.loading = false;
      session.scanPending = false;
    }
  } finally {
    scanDiscFolder.running = false;
    updateBrowserChoose();
  }
  if (disc === session && browser.session === browserSession && $("browser").open) {
    $("browser").close();
  }
}

async function scanDisc(id) {
  if (!disc || disc.loading) return;
  const session = disc;
  session.folder = null;
  session.drive = id;
  session.selected.clear();
  session.error = null;
  session.loading = true;
  session.scanPending = true;
  renderDisc();
  try {
    await post("/api/discs/scan", { drive: id });
    if (disc !== session) post("/api/discs/cancel").catch(() => {});
  } catch (e) {
    if (disc === session) {
      session.error = e.message;
      session.loading = false;
      session.scanPending = false;
    }
  }
  if (disc === session) renderDisc();
}

// The last `disc` block from /api/status, so the dialog can render between
// polls without asking for it again.
let discState = { active: false, scanning: false, titles: [], error: null };

function onDiscStatus(state) {
  discState = state ?? discState;
  // Held until the poll reports the scan, its titles or its failure. The window
  // between the request returning and the next poll reports neither.
  if (disc?.loading
      && (discState.scanning || discState.titles.length > 0 || discState.error)) {
    disc.loading = false;
  }
  if (disc?.scanPending && !discState.scanning
      && (discState.titles.length > 0 || discState.error)) {
    disc.scanPending = false;
  }
  if ($("disc-modal").open) renderDisc();
}

// Signature of everything the body shows apart from which titles are ticked.
// renderDisc() rebuilds the body only when this value changes; it is called on
// every status poll.
function discShape() {
  return JSON.stringify([
    disc && [disc.loading, disc.drive, disc.folder, disc.error, disc.drives.map((d) => d.id)],
    discState.scanning,
    discState.active,
    discState.error,
    discState.disc_type,
    discState.titles.map((t) => t.id),
  ]);
}

let discShapeRendered = null;

function renderDisc() {
  const shape = discShape();
  if (shape !== discShapeRendered) {
    discShapeRendered = shape;
    renderDiscBody();
  }
  updateDiscFooter();
}

// Repaints the selected count and the Rip button. Leaves the title list.
function updateDiscFooter() {
  const listed = Boolean(disc) && !disc.loading && !discState.scanning
    && discState.titles.length > 0;
  $("disc-note").textContent = listed ? `${disc.selected.size} ${tr("selected")}` : "";
  $("disc-rip").disabled = offline || !listed || disc.selected.size === 0 || discState.active;
  $("disc-folder").disabled = offline || (Boolean(disc) && (discState.active || disc.loading));
}

function renderDiscBody() {
  const body = $("disc-body");
  body.textContent = "";
  if (!disc) return;

  // A failure from either side of the exchange reads the same way here.
  const failure = disc.error ?? discState.error;
  if (failure) body.appendChild(discNote(failure, true));

  if (disc.loading) {
    body.appendChild(discNote(tr("disc_scanning")));
    return;
  }
  // Nothing picked yet: choose a drive, or the folder button in the footer.
  // No drive at all is a note rather than a failure, since that button remains.
  if (disc.drive == null && disc.folder == null) {
    if (disc.drives.length === 0) {
      if (!failure) body.appendChild(discNote(tr("disc_no_drive")));
      return;
    }
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
  const source = (disc.folder != null
    ? [disc.folder, discState.disc_type]
    : [drive?.name, drive?.disc_label ?? tr("disc_drive_empty"), discState.disc_type]
  ).filter(Boolean).join(" · ");

  if (discState.scanning) {
    body.appendChild(groupHeading(source));
    body.appendChild(discNote(tr("disc_scanning")));
    return;
  }
  if (discState.titles.length === 0) {
    body.appendChild(groupHeading(source));
    if (!failure) body.appendChild(discNote(tr("disc_no_titles")));
    return;
  }

  // Select-all toggle shared with the track dialog, wording shared with the TUI.
  body.appendChild(groupHeading(
    `${tr("disc_select_titles")} — ${source}`,
    toggleAllButton(
      "toggle-all-titles",
      discState.titles.every((t) => disc.selected.has(t.id)),
      (selectAll) => {
        disc.selected.clear();
        if (selectAll) for (const t of discState.titles) disc.selected.add(t.id);
      },
      { enabled: !discState.active, rerender: () => { discShapeRendered = null; renderDisc(); } },
    ),
  ));

  for (const title of discState.titles) {
    const row = document.createElement("label");
    row.className = "track-row";

    const box = document.createElement("input");
    box.type = "checkbox";
    box.checked = disc.selected.has(title.id);
    box.disabled = discState.active;
    box.addEventListener("change", () => {
      if (box.checked) disc.selected.add(title.id);
      else disc.selected.delete(title.id);
      updateDiscFooter();
    });
    row.appendChild(box);

    // Name above its details, the layout the audio rows use. A flex sibling
    // without min-width:0 takes its full intrinsic width.
    const info = document.createElement("div");
    info.className = "track-info";
    const name = document.createElement("div");
    name.className = "name";
    name.textContent = title.name;
    const meta = document.createElement("div");
    meta.className = "sub";
    meta.textContent = [
      title.duration,
      title.size,
      `${title.chapters} ${tr("disc_chapters")}`,
      ...title.tracks,
    ].filter(Boolean).join(" · ");
    info.append(name, meta);
    row.appendChild(info);

    body.appendChild(row);
  }
}

function discNote(text, bad = false) {
  const note = document.createElement("p");
  note.className = bad ? "disc-error" : "muted";
  note.textContent = text;
  return note;
}

$("disc-rip").addEventListener("click", async () => {
  if (!disc || disc.selected.size === 0) return;
  const session = disc;
  const button = $("disc-rip");
  button.disabled = true;
  try {
    await post("/api/discs/rip", {
      ...(session.folder != null ? { folder: session.folder } : { drive: session.drive }),
      titles: [...session.selected],
    });
    if (disc === session) {
      session.ripping = true;
      $("disc-modal").close();
    }
    refreshQueue();
  } catch (e) {
    toast(e.message, true);
    if (disc === session) updateDiscFooter();
  }
});

// ── Settings ────────────────────────────────────────────────────────

let settingsLoaded = false;
let config = null;
let savedConfig = null;
let settingsSaving = false;
let settingsLoad = null;
let settingsAccess = null;

const cloneConfig = (value) => JSON.parse(JSON.stringify(value));

const settingsDirty = () =>
  savedConfig != null && JSON.stringify(config) !== JSON.stringify(savedConfig);

function updateSettingsActions() {
  const dirty = settingsDirty();
  $("btn-save-settings").disabled = offline || settingsSaving || !dirty;
  $("btn-reset-settings").disabled = settingsSaving || !dirty;
}

// Edits are held in memory until Save; a reload discards them.
addEventListener("beforeunload", (event) => {
  if (settingsDirty()) event.preventDefault();
});

// Native language names and product names are not translated — they read the
// same in every locale.
const LANGS = [["en", "English"], ["it", "Italiano"], ["es", "Español"], ["fr", "Français"], ["de", "Deutsch"], ["zh", "中文"]];
const ENCODERS = [["SvtAv1", "SVT-AV1 (Software)"], ["Nvenc", "NVENC (NVIDIA)"], ["Qsv", "Quick Sync (Intel)"], ["Amf", "AMF (AMD)"]];
const NVENC_PRESETS = ["p1", "p2", "p3", "p4", "p5", "p6", "p7"].map((p) => [p, p]);

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
  const local = Boolean(settingsAccess?.local);
  const localHint = local ? "" : tr("local_only_note");
  const restartHint = tr("restart_required");
  const serviceHint = settingsAccess?.autostart_supported
    ? localHint
    : tr("autostart_unsupported");
  const fields = [
    { group: tr("group_general") },
    { path: "language", label: tr("cfg_language"), type: "select", options: LANGS },
    { path: "encoder", label: tr("cfg_encoder"), type: "select", options: ENCODERS, rebuild: true },
    { path: "quality_preset", label: tr("cfg_quality_preset"), type: "select", options: presets(), rebuild: true },
    { group: tr("group_quality") },
    { path: "quality.vmaf_enabled", label: tr("cfg_vmaf_enabled"), type: "checkbox", rebuild: true },
    { path: "quality.vmaf_threshold", label: tr("cfg_vmaf_threshold"), type: "number", min: 0, max: 100, step: 0.1, disabled: !cfg.quality.vmaf_enabled },
    { path: "quality.delete_source_on_success", label: tr("cfg_delete_source"), type: "checkbox", disabled: !cfg.quality.vmaf_enabled, warning: tr("delete_source_warning") },
    { group: tr("group_performance") },
    { path: "performance.svt_preset", label: tr("cfg_svt_preset"), type: "number", min: 0, max: 13 },
    { path: "performance.nvenc_preset", label: tr("cfg_nvenc_preset"), type: "select", options: NVENC_PRESETS },
  ];
  fields.push({ group: tr("group_rate_factors") });
  const presetMetrics = [
    ["crf", "CRF", 63],
    ["film_grain", tr("cfg_film_grain"), 50],
    ["nvenc_cq", "NVENC CQ", 51],
    ["qsv_quality", "QSV Quality", 51],
    ["amf_quality", "AMF Quality", 51],
  ];
  for (const [tier, tierLabel] of rfTiers()) {
    for (const [metric, metricLabel, maximum] of presetMetrics) {
      fields.push({
        path: `presets.${tier}.${metric}`,
        label: `${tierLabel} · ${metricLabel}`,
        type: "number",
        min: 0,
        max: maximum,
        disabled: cfg.quality_preset !== "custom",
      });
    }
  }
  fields.push(
    { group: tr("group_output") },
    { path: "output.suffix", label: tr("cfg_output_suffix"), type: "text" },
    { path: "output.container", label: tr("cfg_output_container"), type: "text" },
    { path: "output.same_directory", label: tr("cfg_same_directory"), type: "checkbox", rebuild: true },
    { path: "output.output_directory", label: tr("cfg_output_directory"), type: "text", nullable: true, disabled: cfg.output.same_directory, required: !cfg.output.same_directory, browse: true },
    { group: tr("group_tracks") },
    { path: "tracks.preferred_audio_languages", label: tr("cfg_audio_languages"), type: "list" },
    { path: "tracks.preferred_subtitle_languages", label: tr("cfg_subtitle_languages"), type: "list" },
    { path: "tracks.select_all_fallback", label: tr("cfg_select_all_fallback"), type: "checkbox" },
    { group: tr("group_audio") },
    { path: "audio.default_mode", label: tr("cfg_audio_default"), type: "select", options: audioModes() },
    { path: "audio.opus_bitrate_per_channel", label: tr("cfg_opus_bitrate"), type: "number", min: 16, max: 256 },
    { path: "audio.skip_already_opus", label: tr("cfg_skip_already_opus"), type: "checkbox" },
    { group: tr("group_daemon") },
    { note: tr("daemon_note") },
    { path: "daemon.enabled", label: tr("cfg_daemon_enabled"), type: "checkbox", disabled: !local, hint: [localHint, restartHint].filter(Boolean).join(" ") },
    { path: "_service.autostart", label: tr("cfg_daemon_autostart"), type: "checkbox", disabled: !local || !settingsAccess?.autostart_supported, immediateService: true, hint: serviceHint },
    { path: "daemon.bind_address", label: tr("cfg_daemon_bind_address"), type: "text", disabled: !local, required: true, hint: [localHint, restartHint].filter(Boolean).join(" ") },
    { path: "daemon.port", label: tr("cfg_daemon_port"), type: "number", min: 1, max: 65535, disabled: !local, hint: [localHint, restartHint].filter(Boolean).join(" ") },
    { path: "daemon.browse_root", label: tr("cfg_daemon_browse_root"), type: "text", disabled: !local, browse: true, hint: localHint },
    { path: "daemon.auth_token", label: tr("cfg_daemon_auth_token"), type: "password", disabled: !local, minLength: 32, placeholder: settingsAccess?.auth_token_set ? "••••••••" : "", hint: [localHint, tr("token_hint")].filter(Boolean).join(" ") },
    { group: tr("group_disc") },
    { path: "disc.makemkvcon_path", label: tr("cfg_makemkvcon_path"), type: "text", nullable: true, disabled: !local, hint: localHint },
    { path: "disc.staging_directory", label: tr("cfg_staging_directory"), type: "text", nullable: true, disabled: !local, browse: true, hint: localHint },
  );
  return fields;
}

function verifySettingsCoverage(cfg, paths) {
  const represented = new Set(
    settingsFields(cfg).filter((field) => field.path).map((field) => field.path),
  );
  const missing = paths.filter((path) => !represented.has(path));
  if (missing.length) throw new Error(`Settings form is missing: ${missing.join(", ")}`);
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
  // Runs on every change to a field with dependants. Ids come from the config
  // path and are stable across the rebuild; focus is restored by id.
  const focused = document.activeElement?.id;
  form.textContent = "";
  // Each group entry opens a <fieldset><legend>; later fields append to it.
  let group = form;
  for (const field of settingsFields(config)) {
    if (field.group) {
      group = document.createElement("fieldset");
      group.className = "field-group";
      const legend = document.createElement("legend");
      legend.textContent = field.group;
      group.appendChild(legend);
      form.appendChild(group);
      continue;
    }
    if (field.note) {
      const note = document.createElement("div");
      note.className = "muted";
      note.textContent = field.note;
      group.appendChild(note);
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
      input.type = ["number", "password"].includes(field.type) ? field.type : "text";
      if (field.min != null) input.min = field.min;
      if (field.max != null) input.max = field.max;
      if (field.step != null) input.step = field.step;
      if (field.minLength != null) input.minLength = field.minLength;
      if (field.placeholder != null) input.placeholder = field.placeholder;
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

    input.addEventListener("change", async () => {
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
      if (field.immediateService) {
        input.disabled = true;
        try {
          const result = await post("/api/settings/service", { enabled: parsed });
          setPath(config, field.path, result.enabled);
          setPath(savedConfig, field.path, result.enabled);
          config.daemon.enabled = result.daemon_enabled;
          savedConfig.daemon.enabled = result.daemon_enabled;
          const daemonEnabled = $("set-daemon-enabled");
          if (daemonEnabled) daemonEnabled.checked = result.daemon_enabled;
          input.checked = result.enabled;
        } catch (error) {
          const previous = getPath(savedConfig, field.path);
          setPath(config, field.path, previous);
          input.checked = previous;
          toast(error.message, true);
        } finally {
          input.disabled = Boolean(field.disabled);
          updateSettingsActions();
        }
        return;
      }
      if (field.rebuild) buildSettingsForm();
      updateSettingsActions();
    });

    if (field.browse) {
      const wrap = document.createElement("div");
      wrap.className = "field-browse";
      const browse = document.createElement("button");
      browse.type = "button";
      browse.className = "iconbtn";
      // Picking rebuilds the form; focus is restored by this id.
      browse.id = `${inputId}-browse`;
      browse.textContent = tr("browse");
      browse.disabled = Boolean(field.disabled);
      browse.setAttribute("aria-label", `${tr("browse")}: ${field.label}`);
      browse.addEventListener("click", () => openBrowser("folder", (path) => {
        setPath(config, field.path, path);
        buildSettingsForm();
        updateSettingsActions();
      }, input.value.trim()));
      wrap.append(input, browse);
      row.appendChild(wrap);
    } else {
      row.appendChild(input);
    }
    if (field.warning) {
      const warning = document.createElement("div");
      warning.id = `${inputId}-warning`;
      warning.className = "field-warning";
      warning.textContent = field.warning;
      input.setAttribute("aria-describedby", warning.id);
      row.appendChild(warning);
    }
    if (field.hint) {
      const hint = document.createElement("div");
      hint.id = `${inputId}-hint`;
      hint.className = "field-hint";
      hint.textContent = field.hint;
      const describedBy = input.getAttribute("aria-describedby");
      input.setAttribute("aria-describedby", [describedBy, hint.id].filter(Boolean).join(" "));
      row.appendChild(hint);
    }
    group.appendChild(row);
  }
  if (focused) $(focused)?.focus();
  updateSettingsActions();
}

function loadSettings() {
  if (!settingsLoad) {
    settingsLoad = loadSettingsNow().finally(() => { settingsLoad = null; });
  }
  return settingsLoad;
}

async function loadSettingsNow() {
  try {
    const [loadedConfig, access] = await Promise.all([
      api("/api/settings"),
      api("/api/settings/access"),
    ]);
    config = loadedConfig;
    settingsAccess = access;
    config._service = { autostart: access.autostart };
    verifySettingsCoverage(config, access.setting_paths);
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
  const replacementToken = config.daemon.auth_token.trim();
  const serviceState = cloneConfig(config._service);
  const submittedConfig = cloneConfig(config);
  delete submittedConfig._service;
  const save = $("btn-save-settings");
  save.textContent = tr("saving");
  try {
    config = await post("/api/settings", submittedConfig);
    config._service = serviceState;
    if (replacementToken) {
      token = replacementToken;
      try { sessionStorage.setItem("av1c_token", token); } catch { /* memory only */ }
    }
    savedConfig = cloneConfig(config);
    if (languageChanged) {
      try {
        strings = await api("/api/strings");
        applyStrings();
        document.documentElement.lang = strings.html_lang ?? document.documentElement.lang;
      } catch (e) {
        toast(`${tr("saved_exclaim")} ${e.message}`, true);
      }
    }
    buildSettingsForm();
    updateSettingsActions();
    // Save and Discard disabling is the on-screen confirmation; the announcement
    // carries it to a screen reader.
    announce(tr("saved_exclaim"));
  } catch (e) { toast(e.message, true); }
  finally {
    settingsSaving = false;
    form.inert = false;
    form.removeAttribute("aria-busy");
    save.textContent = tr("save_settings");
    updateSettingsActions();
  }
});

$("btn-reset-settings").addEventListener("click", () => {
  config = cloneConfig(savedConfig);
  buildSettingsForm();
  updateSettingsActions();
});
