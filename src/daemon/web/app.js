"use strict";

// ── Helpers ─────────────────────────────────────────────────────────

const $ = (id) => document.getElementById(id);

// Same rounding as format_file_size on the server, so both kinds of size on
// the page read alike.
function fmtBytes(n) {
  if (n == null) return "—";
  const KIB = 1024, MIB = 1024 * KIB, GIB = 1024 * MIB;
  if (n >= GIB) {
    const h = Math.floor((n * 100) / GIB);
    return `${Math.floor(h / 100)}.${String(h % 100).padStart(2, "0")} GiB`;
  }
  if (n >= MIB) {
    const h = Math.floor((n * 10) / MIB);
    return `${Math.floor(h / 10)}.${h % 10} MiB`;
  }
  if (n >= KIB) return `${Math.floor(n / KIB)} KiB`;
  return `${n} B`;
}

function fmtDuration(secs) {
  if (secs == null) return "—";
  const h = Math.floor(secs / 3600), m = Math.floor((secs % 3600) / 60), s = Math.floor(secs % 60);
  return h > 0 ? trf("duration_hm", { h, m }, "{h}h {m}m")
    : m > 0 ? trf("duration_ms", { m, s }, "{m}m {s}s")
    : trf("duration_s", { s }, "{s}s");
}

// Writes into one of the two live regions in index.html. Their role and
// aria-live are fixed in the markup and never reassigned.
function announce(message, isError) {
  const region = $(isError ? "live-assertive" : "live-polite");
  // A repeat of the current text gets a trailing space; the region content
  // then differs and the announcement fires again.
  region.textContent = region.textContent === message ? `${message} ` : message;
}

// Popover support, absent in Safari before 17; there the toast is shown and
// hidden by its class alone.
const popovers = typeof HTMLElement.prototype.showPopover === "function";

function closePopover(el) {
  if (popovers && el.matches(":popover-open")) el.hidePopover();
}

function toast(message, isError) {
  const el = $("toast");
  const unreadError = el.classList.contains("error") && !el.classList.contains("hidden");
  // An error waits for its dismissal; a later success only announces itself.
  if (!isError && unreadError) {
    announce(message, false);
    return;
  }
  $("toast-message").textContent = message;
  el.classList.toggle("error", Boolean(isError));
  announce(message, isError);
  el.classList.remove("hidden");
  // Popovers share the browser's top layer with dialogs. Reopening moves an
  // existing toast above a modal, so failures are never hidden by its backdrop.
  closePopover(el);
  if (popovers) el.showPopover();
  clearTimeout(toast.timer);
  if (!isError) toast.timer = setTimeout(hideToast, 4000);
}

function hideToast() {
  clearTimeout(toast.timer);
  const el = $("toast");
  closePopover(el);
  el.classList.add("hidden");
  el.classList.remove("error");
}

$("toast-close").addEventListener("click", hideToast);

// URL fragments never reach the HTTP server, proxy logs or Referer headers.
// Storage can be disabled by privacy settings, so it is strictly best-effort.
// Reads #token=… from the URL, stores it and strips it from the address bar.
function tokenFromHash() {
  const fromUrl = new URLSearchParams(location.hash.slice(1)).get("token");
  if (!fromUrl) return null;
  try { sessionStorage.setItem("av1c_token", fromUrl); } catch { /* memory only */ }
  history.replaceState(null, "", location.pathname + location.search);
  return fromUrl;
}

let token = tokenFromHash() ?? (() => {
  try { return sessionStorage.getItem("av1c_token") || ""; } catch { return ""; }
})();

// A token added to the URL of an open page replaces the current one.
addEventListener("hashchange", () => {
  const next = tokenFromHash();
  if (!next) return;
  token = next;
  poll();
});

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
  // Dynamic nodes are not marked data-i18n; refresh them from live state.
  if (typeof updateWorkButtons === "function") updateWorkButtons();
  if (typeof setBrowserMode === "function" && typeof browser !== "undefined" && browser?.mode) {
    setBrowserMode(browser.mode, browser.onPick);
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
  if (!response.ok) {
    const error = new Error(body.error || `HTTP ${response.status}`);
    error.status = response.status;
    error.needsConfirm = body.needs_confirm === true;
    throw error;
  }
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
    if (activeTab === "settings" && !settingsDirty()) loadSettings();
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
// A status or queue read that takes longer than this counts as offline.
const POLL_TIMEOUT_MS = 10000;
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
let pollSkipped = false;
// Sequence number of the most recently started poll.
let pollSeq = 0;
let lastWork = { encoding: false, ripping: false, analyzing: false };
// The last /api/status answer.
let lastStatus = null;

async function poll() {
  if (pollInFlight) {
    pollSkipped = true;
    return;
  }
  pollInFlight = true;
  const seq = ++pollSeq;
  try {
    let s;
    try {
      s = await api("/api/status", { signal: AbortSignal.timeout(POLL_TIMEOUT_MS) });
    } catch (e) {
      // A refused token keeps its own message; every other failure reads as
      // an unreachable daemon. The text is set before setOffline() reads it
      // for the announcement.
      setText(
        $("offline-banner"),
        e.unauthorized ? e.message : tr("offline", "Daemon unreachable — retrying…"),
      );
      setOffline(true);
      return;
    }
    setOffline(false);
    if (Object.keys(strings).length === 0) loadStrings().catch(() => {});

    const pill = $("status-pill");
    const kind = s.current?.status.kind;
    const statusKey = kind === "ripping" ? "status_ripping"
      : kind === "verifying" ? "verifying_vmaf"
      : kind === "encoding" || s.encoding_active ? "status_encoding"
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
        const progress = Number(st.progress);
        setProgress("current", Number.isFinite(progress) ? progress : 0);
        setText($("current-pct"), Number.isFinite(progress) ? `${progress.toFixed(1)}%` : "");
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

    const overall = Number(s.overall_progress);
    setProgress("overall", Number.isFinite(overall) ? overall : 0);
    setText($("overall-pct"), Number.isFinite(overall) ? `${overall.toFixed(1)}%` : "");
    setText($("eta"), [
      s.counts.active > 0 && s.elapsed_secs != null
        ? `${tr("elapsed")} ${fmtDuration(s.elapsed_secs)}`
        : "",
      s.eta_secs != null ? `${tr("eta")} ${fmtDuration(s.eta_secs)}` : "",
    ].filter(Boolean).join(" · "));

    setText($("stat-total"), String(s.counts.total));
    setText($("stat-saved"), s.total_space_saved.human);
    lastStatus = s;
    setText($("host-status"), hostStatusText(s));

    lastWork.encoding = Boolean(s.encoding_active);
    lastWork.ripping = Boolean(s.disc?.active);
    lastWork.analyzing = s.counts.analyzing > 0;
    updateWorkButtons();

    updateSummary(s);
    onDiscStatus(s.disc, seq);

    if (activeTab === "queue") await refreshQueue();
  } finally {
    pollInFlight = false;
    if (pollSkipped) {
      pollSkipped = false;
      poll();
    }
  }
}

function updateWorkButtons() {
  const ripping = lastWork.ripping;
  const encoding = lastWork.encoding;
  const analyzing = lastWork.analyzing;
  $("btn-cancel").disabled = offline || (!encoding && !analyzing);
  setText(
    $("btn-cancel"),
    encoding ? tr("cancel_encoding")
      : analyzing ? tr("cancel_analysis")
      : tr("cancel_encoding"),
  );
  $("btn-cancel-analysis").hidden = !(encoding && analyzing);
  $("btn-cancel-analysis").disabled = offline;
  // A rip runs alongside an encode and is cancelled on its own.
  $("btn-cancel-disc").hidden = !ripping;
  $("btn-cancel-disc").disabled = offline;
  $("btn-add-disc").disabled = offline || ripping;
  $("btn-add-disc").title = ripping ? tr("status_ripping") : "";
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

  const finished = s.counts.active === 0 && converted + skipped + errors + cancelled > 0;
  if (wasActive && finished) sawCompletion = true;

  const summary = $("summary");
  const show = finished && sawCompletion && !summaryDismissed;
  summary.classList.toggle("hidden", !show);

  if (show) {
    $("summary-converted").textContent = converted;
    $("summary-skipped").textContent = skipped;
    $("summary-cancelled").textContent = cancelled;
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
    $("summary-cancelled-group").classList.toggle("hidden", cancelled === 0);
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
      `${tr("badge_skipped")}: ${skipped}` +
      (cancelled > 0 ? `, ${tr("summary_cancelled")}: ${cancelled}` : "") +
      `, ${tr("summary_errors")}: ${errors}`,
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

// Fetches and applies the string map. Concurrent callers share one request.
function loadStrings() {
  loadStrings.pending ??= api("/api/strings")
    .then((map) => {
      strings = map;
      applyStrings();
      document.documentElement.lang = strings.html_lang ?? document.documentElement.lang;
    })
    .finally(() => { loadStrings.pending = null; });
  return loadStrings.pending;
}

// Strings first, so nothing renders in English and then flips a moment later.
// If the fetch fails the page keeps the English in index.html and carries on:
// an unreachable or unauthorized daemon is already reported by the poll, and
// an untranslated UI beats a blank one. A later successful poll retries it.
(async () => {
  try {
    await loadStrings();
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

// Reasons arrive as fixed English strings; the known ones translate, anything
// else shows as sent.
const REASON_KEY = {
  "Cancelled": "reason_cancelled",
  "interrupted by a daemon restart": "reason_restart_interrupted",
};

function trReason(reason) {
  const key = REASON_KEY[reason];
  return key ? tr(key, reason) : reason;
}

function badgeText(st) {
  const num = (value) => (value == null ? NaN : Number(value));
  const pct = (value) => (Number.isFinite(num(value)) ? num(value).toFixed(1) : "?");
  switch (st.kind) {
    case "encoding": return `${tr("status_encoding")} ${pct(st.progress)}%`;
    case "ripping": return `${tr("status_ripping")} ${pct(st.progress)}%`;
    case "done_vmaf": return `${tr("badge_done")} · VMAF ${pct(st.vmaf)}`;
    case "done_vmaf_failed": return `${tr("badge_done")} · ${tr("badge_vmaf_failed")}`;
    case "quality_warning": {
      const min = Number.isFinite(num(st.min_score))
        ? ` (min ${pct(st.min_score)})`
        : "";
      const threshold = Number.isFinite(num(st.threshold))
        ? ` < ${num(st.threshold)} ${tr("threshold_label")}`
        : "";
      return `${tr("badge_low_vmaf")} ${pct(st.vmaf)}${min}${threshold}`;
    }
    case "skipped": return `${tr("badge_skipped")} · ${trReason(st.reason)}`;
    default: return tr(BADGE_KEY[st.kind] ?? "", st.kind);
  }
}

// The VMAF score that failed a quality_warning job's threshold: the mean, or
// the minimum when only the minimum is below it.
function failedVmafText(st) {
  if (st.kind !== "quality_warning") return "";
  const mean = Number(st.vmaf);
  const threshold = Number(st.threshold);
  if (!Number.isFinite(threshold)) return "";
  const score = Number.isFinite(mean) && mean < threshold
    ? `VMAF ${mean.toFixed(1)}`
    : `VMAF min ${Number.isFinite(Number(st.min_score)) ? Number(st.min_score).toFixed(1) : "?"}`;
  return `${score} < ${threshold}`;
}

// Rows are kept and updated in place, keyed by job id. Rebuilding the table on
// every poll would throw away hover, focus and any text the user has selected,
// once a second, for the whole length of an encode.
const rows = new Map();
const promptedTrackJobs = new Set();
let openingTracks = false;
// Highest job id seen in the queue, and the id at or below which jobs are not
// auto-prompted: the highest id seen when a tracks dialog that closed without
// saving was opened.
let maxJobId = 0;
let trackPromptFloor = 0;

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
  size.className = "size-cell";
  const saved = row.insertCell();

  const moveUp = document.createElement("button");
  moveUp.className = "iconbtn";
  moveUp.textContent = "↑";
  moveUp.addEventListener("click", async () => {
    if (moveUp.getAttribute("aria-disabled") === "true") return;
    moveUp.setAttribute("aria-busy", "true");
    entry.applyDisabled();
    try {
      await post("/api/queue/move_up", { id: job.id });
    } catch (e) { toast(e.message, true); }
    finally {
      moveUp.removeAttribute("aria-busy");
      entry.applyDisabled();
      forceRefreshQueue();
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
    if (entry.temporary && !await askConfirm(tr("remove_rip_prompt"))) return;
    remove.disabled = true;
    remove.setAttribute("aria-busy", "true");
    try {
      await post("/api/queue/remove", { id: job.id });
    } catch (e) { toast(e.message, true); }
    finally {
      remove.removeAttribute("aria-busy");
      forceRefreshQueue();
    }
  });
  row.insertCell().appendChild(remove);

  const entry = { tr: row, name, sub, source, badge, confirm, detail, bar, size, saved, moveUp, tracks, remove, kind: job.status.kind, tracksEditable: job.tracks_editable, canMoveUp: job.can_move_up };
  entry.applyDisabled = () => {
    // aria-disabled keeps the button focusable while its row is reordered.
    entry.moveUp.setAttribute("aria-disabled", String(offline
      || entry.moveUp.getAttribute("aria-busy") === "true"
      || !entry.canMoveUp));
    entry.remove.disabled = offline
      || entry.remove.getAttribute("aria-busy") === "true"
      || ["encoding", "verifying", "ripping"].includes(entry.kind);
    entry.tracks.disabled = offline || !entry.tracksEditable;
    entry.confirm.disabled = offline;
  };
  return entry;
}

function updateRow(row, job) {
  setText(row.name, job.filename);
  row.name.title = job.output_name ? `→ ${job.output_name}` : "";
  setText(row.sub, [
    job.crf != null ? `CRF ${job.crf}` : "",
    job.remux_only ? tr("tag_remux") : "",
    job.source_deleted ? tr("tag_source_deleted") : "",
    job.source_kept_vmaf != null || job.source_kept_reason ? tr("tag_source_kept") : "",
  ].filter(Boolean).join(" · "));
  row.sub.title = job.source_kept_reason
    ? `${tr("tag_source_kept")}: ${job.source_kept_reason}`
    : job.source_kept_vmaf != null ? failedVmafText(job.status) : "";
  // Resolution and HDR are null until the probe has run.
  setText(row.source, [job.resolution, job.hdr].filter(Boolean).join(" ") || "—");

  row.badge.className = `badge ${BADGE_CLASS[job.status.kind] || ""}`;
  setText(row.badge, job.quality
    ? `${badgeText(job.status)} (${job.quality})`
    : badgeText(job.status));
  const awaitingTracks = job.status.kind === "awaiting_config";
  row.badge.classList.toggle("hidden", awaitingTracks);
  row.confirm.classList.toggle("hidden", !awaitingTracks);
  setText(row.confirm, tr("confirm_tracks"));
  row.confirm.setAttribute("aria-label", `${tr("confirm_tracks")}: ${job.filename}`);
  const detail = job.status.kind === "error" ? job.status.message
    : job.status.kind === "done_vmaf_failed" ? trReason(job.status.reason)
    : "";
  setText(row.detail, detail);
  row.detail.classList.toggle("hidden", !detail);
  if (detail) row.badge.setAttribute("aria-describedby", row.detail.id);
  else row.badge.removeAttribute("aria-describedby");

  // A rip fills the same bar as an encode: same shape of work, same row.
  const live = ["encoding", "ripping"].includes(job.status.kind);
  row.bar.classList.toggle("hidden", !live);
  if (live) {
    row.bar.value = job.status.progress;
    row.bar.setAttribute(
      "aria-label",
      `${job.status.kind === "ripping" ? tr("status_ripping") : tr("status_encoding")}: ${job.filename}`,
    );
  }

  setText(row.size, job.output_size != null
    ? `${fmtBytes(job.source_size)} → ${fmtBytes(job.output_size)}`
    : fmtBytes(job.source_size));

  if (job.saved_percent == null) {
    setText(row.saved, "");
    row.saved.className = "";
  } else {
    const percent = Math.round(Math.abs(job.saved_percent));
    setText(row.saved, percent === 0
      ? "0%"
      : `${job.saved_percent < 0 ? "+" : "−"}${percent}%`);
    row.saved.className = job.saved_percent < 0 ? "grew" : "";
  }

  row.kind = job.status.kind;
  row.temporary = Boolean(job.temporary);
  row.tracksEditable = job.tracks_editable;
  row.canMoveUp = job.can_move_up;
  row.applyDisabled();
  row.moveUp.title = tr("move_up");
  row.moveUp.setAttribute("aria-label", `${tr("move_up")}: ${job.filename}`);
  row.remove.title = tr("remove_from_queue");
  row.remove.setAttribute("aria-label", `${tr("remove_from_queue")}: ${job.filename}`);
  setText(row.tracks, tr("tracks_title"));
  row.tracks.title = tr("tracks_hint");
  row.tracks.setAttribute("aria-label", `${tr("tracks_hint")}: ${job.filename}`);
  // The Tracks button is the accented action on a row that awaits config.
  row.tracks.classList.toggle("primary", job.status.kind === "awaiting_config");
}

let queueRefresh = null;
let clearingFinished = false;
let hasFinishedJobs = false;
// Whether any finished job is a ripped disc title; clearing it deletes the rip.
let hasTemporaryFinished = false;
$("btn-clear").disabled = true;

function updateClearFinished() {
  $("btn-clear").disabled = offline || clearingFinished || !hasFinishedJobs;
}

function refreshQueue() {
  if (queueRefresh) return queueRefresh;
  queueRefresh = refreshQueueNow().finally(() => { queueRefresh = null; });
  return queueRefresh;
}

// A fetch that started before a mutation committed carries the old order;
// this waits it out and fetches again.
function forceRefreshQueue() {
  const pending = queueRefresh ?? Promise.resolve();
  return pending.then(() => refreshQueue());
}

async function refreshQueueNow() {
  let data;
  try {
    data = await api("/api/queue", { signal: AbortSignal.timeout(POLL_TIMEOUT_MS) });
  } catch {
    return; // the status poll owns the offline banner
  }
  const tbody = $("queue-body");
  $("queue-empty").classList.toggle("hidden", data.jobs.length > 0);
  const finished = data.jobs.filter((job) => [
    "done", "done_vmaf", "done_vmaf_failed", "skipped", "error", "quality_warning",
  ].includes(job.status.kind));
  hasFinishedJobs = finished.length > 0;
  hasTemporaryFinished = finished.some((job) => job.temporary);
  updateClearFinished();

  // Moving a row with insertBefore drops focus from the control inside it.
  const focused = tbody.contains(document.activeElement) ? document.activeElement : null;
  const seen = new Set();
  for (const [position, job] of data.jobs.entries()) {
    seen.add(job.id);
    maxJobId = Math.max(maxJobId, job.id);
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
  if (focused?.isConnected && document.activeElement !== focused) focused.focus();
  if (trackEditor?.editable && rows.get(trackEditor.id)?.tracksEditable === false) lockTracks();

  const next = !data.jobs.some((job) => job.status.kind === "analyzing")
    && data.jobs.find((job) =>
      job.status.kind === "awaiting_config" && job.id > trackPromptFloor
      && !promptedTrackJobs.has(job.id));
  const settingsOpen = activeTab === "settings" && settingsDirty();
  if (next && !openingTracks && !settingsOpen && !document.querySelector("dialog[open]")) {
    promptedTrackJobs.add(next.id);
    // The status poll does not wait for the dialog's requests.
    openTracks(next.id).then((opened) => {
      if (!opened) promptedTrackJobs.delete(next.id);
    });
  }
}

// Answers no, without asking, while another confirmation is open.
function askConfirm(message) {
  if ($("confirm-modal").open) return Promise.resolve(false);
  return new Promise((resolve) => {
    $("confirm-body").textContent = message;
    const modal = $("confirm-modal");
    let done = false;
    const finish = (ok) => {
      if (done) return;
      done = true;
      $("confirm-yes").onclick = null;
      $("confirm-no").onclick = null;
      modal.onclose = null;
      if (modal.open) modal.close();
      resolve(ok);
    };
    $("confirm-yes").onclick = () => finish(true);
    $("confirm-no").onclick = () => finish(false);
    modal.onclose = () => finish(false);
    modal.showModal();
  });
}

$("btn-cancel").addEventListener("click", async () => {
  const encoding = lastWork.encoding;
  const analyzing = lastWork.analyzing;
  if (!encoding && !analyzing) return;
  const prompt = encoding ? tr("cancel_encoding_prompt") : tr("cancel_analysis_prompt");
  if (!await askConfirm(prompt)) return;
  // Nothing is cancelled when the work that was running changed during the prompt.
  const unchanged = encoding ? lastWork.encoding : !lastWork.encoding && lastWork.analyzing;
  if (!unchanged) return;
  try {
    if (encoding) await post("/api/queue/cancel");
    else await post("/api/queue/cancel_analysis");
    toast(tr("cancelling"));
    forceRefreshQueue();
  } catch (e) { toast(e.message, true); }
});

$("btn-cancel-analysis").addEventListener("click", async () => {
  if (!lastWork.analyzing) return;
  if (!await askConfirm(tr("cancel_analysis_prompt")) || !lastWork.analyzing) return;
  try {
    await post("/api/queue/cancel_analysis");
    toast(tr("cancelling"));
    forceRefreshQueue();
  } catch (e) { toast(e.message, true); }
});

$("btn-cancel-disc").addEventListener("click", async () => {
  if (!lastWork.ripping) return;
  if (!await askConfirm(tr("cancel_disc_prompt")) || !lastWork.ripping) return;
  try {
    await post("/api/discs/cancel");
    toast(tr("cancelling"));
    forceRefreshQueue();
  } catch (e) { toast(e.message, true); }
});

$("btn-clear").addEventListener("click", async () => {
  if (clearingFinished) return;
  let confirm = hasTemporaryFinished;
  if (confirm && !await askConfirm(tr("clear_finished_rip_prompt"))) return;
  const button = $("btn-clear");
  clearingFinished = true;
  button.setAttribute("aria-busy", "true");
  updateClearFinished();
  try {
    let r;
    try {
      r = await post("/api/queue/clear_finished", { confirm });
    } catch (e) {
      // The server found a finished rip this page had not seen yet.
      if (!e.needsConfirm) throw e;
      if (!await askConfirm(tr("clear_finished_rip_prompt"))) return;
      r = await post("/api/queue/clear_finished", { confirm: true });
    }
    toast(trf("removed_finished", { n: r.removed }));
    forceRefreshQueue();
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

$("tracks-close").addEventListener("click", () => closeTracks());
$("tracks-back").addEventListener("click", () => closeTracks());
$("tracks-modal").addEventListener("close", () => {
  if (trackEditor && !trackEditor.saved) {
    trackPromptFloor = Math.max(trackPromptFloor, trackEditor.promptFloor);
  }
  trackEditor = null;
});
$("tracks-modal").addEventListener("cancel", (event) => {
  if (!tracksDirty()) return;
  event.preventDefault();
  closeTracks();
});

function tracksDirty() {
  if (!trackEditor?.editable || !trackEditor.snapshot) return false;
  return tracksSnapshot(trackEditor) !== trackEditor.snapshot;
}

function tracksSnapshot(editor) {
  return JSON.stringify({
    audio: editor.audio.map((t) => ({ index: t.index, mode: t.mode })),
    subtitles: editor.subtitles.map((t) => ({ index: t.index, selected: t.selected })),
    remuxOnly: editor.remuxOnly,
    dvMode: editor.dvMode,
  });
}

// The open track dialog's job has started encoding: its tracks turn read-only.
function lockTracks() {
  trackEditor.editable = false;
  $("tracks-note").textContent = tr("tracks_locked");
  updateModalActions();
  renderTracks();
}

async function closeTracks() {
  if (tracksDirty() && !await askConfirm(tr("abandon_tracks_prompt"))) return;
  $("tracks-modal").close();
}

async function openTracks(id) {
  if (openingTracks || $("tracks-modal").open) return false;
  openingTracks = true;
  // The projected Opus bitrate comes from the audio settings, which are
  // reloaded here unless the settings tab holds unsaved edits.
  const promptFloor = maxJobId;
  try {
    if (!settingsDirty()) await loadSettings();
    const data = await api(`/api/job/tracks?id=${id}`);
    const editor = trackEditor = {
      id,
      promptFloor,
      audio: data.audio.map((t) => ({ ...t, mode: t.selected ? (t.opus ? "opus" : "copy") : "off" })),
      subtitles: data.subtitles.map((t) => ({ ...t })),
      editable: data.editable,
      remuxOnly: data.remux_only,
      dv: data.dv,
      dvMode: data.dv ? data.dv.mode : null,
    };
    editor.snapshot = tracksSnapshot(editor);
    editor.filename = data.filename;
    editor.outputNames = data.output_names ?? [];
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
    if (trackEditor !== editor || document.querySelector("dialog[open]")) {
      if (trackEditor === editor) trackEditor = null;
      return false;
    }
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

// The encoder and VMAF lines of the TUI's Home screen.
function hostStatusText(s) {
  if (!s.deps) return "";
  const encoder = `${tr("cfg_encoder")}: ${s.encoder}${s.deps.encoder ? "" : ` ⚠ ${tr("encoder_unavailable")}`}`;
  const vmaf = !s.vmaf_enabled ? tr("vmaf_disabled")
    : s.deps.ffmpeg && s.deps.vmaf ? `${tr("vmaf_enabled_open")}${Math.round(s.vmaf_threshold)})`
    : `⚠ ${tr("deps_missing")}`;
  const queue = s.unreadable_queue
    ? ` · ⚠ ${trf("queue_unreadable", { path: s.unreadable_queue })}`
    : "";
  return `${encoder} · ${vmaf}${queue}`;
}

function renderTracks() {
  const body = $("tracks-body");
  body.textContent = "";
  const { audio, subtitles, editable } = trackEditor;
  const outputName = trackEditor.outputNames?.[trackEditor.remuxOnly ? 1 : 0];
  $("tracks-filename").textContent = outputName
    ? ` — ${trackEditor.filename} → ${outputName}`
    : ` — ${trackEditor.filename}`;

  body.appendChild(groupHeading(tr("options")));
  body.appendChild(remuxRow());
  if (trackEditor.dv) body.appendChild(dvRow());

  // Toggles, not select-all buttons: when everything is already selected the
  // second press clears it, matching the TUI's 'a' and 's' keys.
  const selectedAudio = audio.filter((t) => t.mode !== "off");
  const opusMissing = lastStatus?.deps && !lastStatus.deps.opus
    && audio.some((t) => t.mode !== "off"
      && (t.container_opus_kbps?.[outputMode()] != null
        || (t.mode === "opus" && !isAlreadyOpus(t))));
  const audioHeading = opusMissing
    ? `${tr("heading_audio")} ⚠ ${tr("opus_unavailable")}`
    : tr("heading_audio");
  body.appendChild(groupHeading(audioHeading, audio.length > 0 && toggleAllButton(
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
  ), selectedAudio.length > 0 && toggleAllButton(
    "toggle-all-opus",
    selectedAudio.every((t) => t.mode === "opus"),
    (toOpus) => {
      for (const track of selectedAudio) track.mode = toOpus ? "opus" : "copy";
    },
    { labels: [tr("all_opus"), tr("track_copy")] },
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
    const target = document.createElement("span");
    target.className = "track-target";
    const setDropped = () => {
      target.textContent = track.selected && track.container_drops?.[outputMode()]
        ? tr("subtitle_not_included")
        : "";
    };
    box.addEventListener("change", () => {
      track.selected = box.checked;
      setDropped();
    });
    setDropped();

    row.append(info, target, box);
    body.appendChild(row);
  }
}

// Selects the per-mode container projections the server sends for each track.
const outputMode = () => (trackEditor.remuxOnly ? "remux" : "encode");

function setTarget(node, track) {
  const forcedKbps = track.container_opus_kbps?.[outputMode()];
  if (track.mode !== "off" && forcedKbps != null) {
    node.textContent = `→ Opus ${forcedKbps}k`;
  } else if (track.mode !== "opus") {
    node.textContent = "";
  } else if (isAlreadyOpus(track)) {
    node.textContent = tr("already_opus_copied");
  } else {
    node.textContent = `→ Opus ${projectedKbps(track)}k`;
  }
}

function groupHeading(text, ...controls) {
  const node = document.createElement("h3");
  node.className = "track-group";
  const label = document.createElement("span");
  label.textContent = text;
  node.appendChild(label);
  for (const control of controls) {
    if (control) node.appendChild(control);
  }
  return node;
}

function toggleAllButton(id, allSelected, apply, options = {}) {
  const {
    enabled = trackEditor?.editable ?? true,
    rerender = renderTracks,
    labels = [tr("select_all"), tr("clear_all")],
  } = options;
  const button = document.createElement("button");
  button.id = id;
  button.type = "button";
  button.className = "iconbtn";
  button.textContent = allSelected ? labels[1] : labels[0];
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
    option.textContent = value === dv.recommended ? `${text} (${tr("dv_recommended")})` : text;
    option.disabled = value === "keep" && !dv.can_keep;
    select.appendChild(option);
  }
  select.value = trackEditor.dvMode;
  select.disabled = !editable || remuxOnly;
  select.addEventListener("change", () => {
    trackEditor.dvMode = select.value;
    renderTracks();
    $("opt-dolby-vision")?.focus();
  });

  const profile = dv.profile == null ? "" : trf("dv_profile", { n: dv.profile });
  const source = trf("dv_source_hint", { profile });
  const choice = tr(select.value === "keep" ? "dv_keep_desc" : "dv_hdr10_desc");
  const warning = dv.profile === 5 ? ` ⚠ ${tr("dv_p5_warning")}` : "";
  const hint = remuxOnly
    ? tr("dv_remux_hint")
    : dv.can_keep ? `${source} ${choice}.${warning}` : `${source} ${tr("dv_requires_svt")}${warning}`;
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
  const sent = tracksSnapshot(editor);
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
    editor.snapshot = sent;
    editor.saved = true;
    if (trackEditor === editor) closeTracks();
    toast(r.applied > 1
      ? trf("tracks_applied", { n: r.applied })
      : tr("tracks_updated"));
    forceRefreshQueue();
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

// Join a directory and an entry name without doubling the separator.
const pathSep = (path) => (path.includes("\\") && !path.includes("/") ? "\\" : "/");
const joinPath = (dir, name) => {
  if (!dir) return name;
  const sep = pathSep(dir);
  return dir.endsWith("/") || dir.endsWith("\\") ? `${dir}${name}` : `${dir}${sep}${name}`;
};

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
  if (dialog.id === "tracks-modal" || dialog.id === "confirm-modal") continue;
  // Closes on a click that both starts and ends on the backdrop.
  let pressedBackdrop = false;
  dialog.addEventListener("pointerdown", (event) => {
    pressedBackdrop = event.target === dialog;
  });
  dialog.addEventListener("click", (event) => {
    if (event.target === dialog && pressedBackdrop) dialog.close();
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
  browser.loaded = false;
  $("browser-list").textContent = "";
  $("browser-path").textContent = "";
  setBrowserMode(mode, onPick);
  $("browser").showModal();
  loadDir(startPath || browser.path, true);
}

function updateBrowserChoose() {
  // browser.loaded: a listing for browser.path arrived in this session.
  $("browser-choose").disabled = offline
    || !browser.loaded
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
    if (request === loadDir.request && session === browser.session) {
      toast(e.message, true);
      // A start path that fails to load falls back to the default directory.
      if (!browser.loaded && path) loadDir("", takeFocus);
    }
    return;
  } finally {
    if (request === loadDir.request && session === browser.session) {
      $("browser").removeAttribute("aria-busy");
      $("browser-busy").classList.add("hidden");
    }
  }
  if (request !== loadDir.request || session !== browser.session) return;

  browser.path = data.path;
  browser.loaded = true;
  updateBrowserChoose();
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
    } else if (f.is_iso && browser.mode === "disc") {
      addEntry(">", f.name, tr("kind_disc_image"), () => {
        $("browser").close();
        scanDiscFolder(filePath);
      }, fmtBytes(f.size));
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

// One button per segment of the current path. Roots like "/" or "C:\" are
// shown as the first crumb; Windows paths keep their backslash separators.
function renderCrumbs(path) {
  const crumbs = $("browser-path");
  crumbs.textContent = "";
  const sep = pathSep(path);
  crumbs.dataset.sep = sep;
  const unc = path.startsWith("\\\\");
  const drive = /^[A-Za-z]:/.exec(path);
  let root;
  let rest;
  if (unc) {
    const parts = path.replace(/^\\\\/, "").split(/\\+/).filter(Boolean);
    root = `\\\\${parts.slice(0, 2).join("\\")}`;
    rest = parts.slice(2);
  } else if (drive) {
    root = `${drive[0]}\\`;
    rest = path.slice(drive[0].length).split(/[/\\]+/).filter(Boolean);
  } else {
    root = "/";
    rest = path.split("/").filter(Boolean);
  }
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
  const atRoot = rest.length === 0;
  add(root === "/" ? "/" : root.replace(/[\\/]+$/, "") || root, root, atRoot);
  let walked = root.endsWith("\\") || root.endsWith("/") ? root.slice(0, -1) : root;
  rest.forEach((segment, index) => {
    walked += `${sep}${segment}`;
    add(segment, walked, index === rest.length - 1);
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
    // A file add leaves the browser open for the next file.
    if (mode !== "file" && browser.session === session && $("browser").open) {
      $("browser").close();
    }
    const skippedParts = [];
    if (r.already_queued) skippedParts.push(trf("already_queued", { n: r.already_queued }));
    if (r.skipped) skippedParts.push(trf("skipped_files", { n: r.skipped }));
    const skipped = skippedParts.length ? `, ${skippedParts.join(", ")}` : "";
    toast(r.added > 0
      ? `${trf("added_files", { n: r.added })}${skipped}`
      : `${tr("nothing_added")}${skipped}`);
    forceRefreshQueue();
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
$("disc-back").addEventListener("click", () => {
  if (!disc || discState.active) return;
  if (disc.scanPending) post("/api/discs/cancel").catch(() => {});
  disc.drive = null;
  disc.folder = null;
  disc.selected.clear();
  disc.error = null;
  disc.loading = false;
  disc.scanPending = false;
  discShapeRendered = null;
  renderDisc();
});
// Esc closes without going through the button, so the state is dropped on the
// close event: the one place every path passes through. A scan this dialog
// started and that is still running is called off.
$("disc-modal").addEventListener("close", () => {
  if (discBrowsing) return;
  // A rip closes this dialog on its way to the queue, where Cancel stops it.
  // This dialog's abandoned scan or still-running drive listing is called off
  // here; a scan another tab started is left running.
  if (disc && !disc.ripping && (disc.scanPending || disc.loading)) {
    post("/api/discs/cancel").catch(() => {});
  }
  disc = null;
  discShapeRendered = null;
});

async function openDisc() {
  if ($("disc-modal").open) return;
  if (discState.active) {
    toast(tr("status_ripping"), true);
    return;
  }
  const session = disc = {
    drives: [], drive: null, folder: null, selected: new Set(),
    error: null, loading: true, scanPending: false, statusAfter: Infinity,
  };
  renderDisc();
  $("disc-modal").showModal();
  try {
    const { drives } = await post("/api/discs/list", {});
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
  session.statusAfter = Infinity;
  updateBrowserChoose();
  try {
    await post("/api/discs/scan", { folder: path });
    session.statusAfter = pollSeq;
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
  session.statusAfter = Infinity;
  renderDisc();
  try {
    await post("/api/discs/scan", { drive: id });
    session.statusAfter = pollSeq;
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

function onDiscStatus(state, seq) {
  discState = state ?? discState;
  // A requested scan ends its loading state only on a poll that started after
  // the scan request returned. The daemon marks the scan as running before it
  // answers, so such a poll reports either the running scan or its result.
  if (disc?.scanPending && seq > disc.statusAfter) {
    disc.loading = false;
    if (!discState.scanning) disc.scanPending = false;
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
  const modal = $("disc-modal");
  const hadFocus = modal.contains(document.activeElement);
  const shape = discShape();
  if (shape !== discShapeRendered) {
    discShapeRendered = shape;
    renderDiscBody();
  }
  updateDiscFooter();
  // Focus lost from the dialog by the redraw moves to the first enabled control
  // in the body, or to the heading when there is none.
  if (hadFocus && modal.open && !modal.contains(document.activeElement)) {
    $("disc-title").tabIndex = -1;
    ($("disc-body").querySelector("button:not(:disabled), input:not(:disabled)")
      ?? $("disc-title")).focus();
  }
}

// Repaints the selected count and the Rip button. Leaves the title list.
function updateDiscFooter() {
  const listed = Boolean(disc) && !disc.loading && !discState.scanning
    && discState.titles.length > 0;
  $("disc-note").textContent = listed ? `${disc.selected.size} ${tr("selected")}` : "";
  $("disc-rip").disabled = offline || !listed || disc.selected.size === 0
    || discState.active || ripInFlight;
  $("disc-folder").disabled = offline || (Boolean(disc) && (discState.active || disc.loading));
  const sourced = Boolean(disc) && (disc.drive != null || disc.folder != null);
  $("disc-back").hidden = !sourced || discState.active;
}

function renderDiscBody() {
  const body = $("disc-body");
  body.textContent = "";
  if (!disc) return;

  // A failure from either side of the exchange reads the same way here. The
  // drive picker shows only its own errors.
  const sourced = disc.drive != null || disc.folder != null;
  const failure = disc.error ?? (sourced ? discState.error : null);
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
      title.chapters ? `${title.chapters} ${tr("disc_chapters")}` : "",
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

let ripInFlight = false;

$("disc-rip").addEventListener("click", async () => {
  if (!disc || disc.selected.size === 0 || ripInFlight) return;
  const session = disc;
  const button = $("disc-rip");
  ripInFlight = true;
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
    forceRefreshQueue();
  } catch (e) {
    toast(e.message, true);
    if (disc === session) updateDiscFooter();
  } finally {
    ripInFlight = false;
  }
});

// ── Settings ────────────────────────────────────────────────────────

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
  if (settingsDirty() || tracksDirty()) event.preventDefault();
});

// Native language names and product names are not translated — they read the
// same in every locale.
const LANGS = [["en", "English"], ["it", "Italiano"], ["es", "Español"], ["fr", "Français"], ["de", "Deutsch"], ["zh", "中文"]];
const ENCODERS = [["SvtAv1", "SVT-AV1 (Software)"], ["Nvenc", "NVENC (NVIDIA)"], ["Qsv", "Quick Sync (Intel)"], ["Amf", "AMF (AMD)"]];
const NVENC_PRESETS = ["p1", "p2", "p3", "p4", "p5", "p6", "p7"].map((p) => [p, p]);
const CONTAINERS = ["mkv", "mp4", "webm"].map((c) => [c, c]);

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
    ? [tr("autostart_hint"), localHint].filter(Boolean).join(" ")
    : tr("autostart_unsupported");
  const fields = [
    { group: tr("group_general") },
    { path: "language", label: tr("cfg_language"), type: "select", options: LANGS },
    { path: "encoder", label: tr("cfg_encoder"), type: "select", options: ENCODERS, rebuild: true },
    { path: "quality_preset", label: tr("cfg_quality_preset"), type: "select", options: presets(), rebuild: true },
    { group: tr("group_quality") },
    { path: "quality.vmaf_enabled", label: tr("cfg_vmaf_enabled"), type: "checkbox", rebuild: true },
    { path: "quality.vmaf_threshold", label: tr("cfg_vmaf_threshold"), type: "number", min: 0, max: 100, step: "any", disabled: !cfg.quality.vmaf_enabled },
    { path: "quality.delete_source_on_success", label: tr("cfg_delete_source"), type: "checkbox", disabled: !cfg.quality.vmaf_enabled, warning: tr("delete_source_warning") },
    { group: tr("group_performance") },
    { path: "performance.svt_preset", label: tr("cfg_svt_preset"), type: "number", min: 0, max: 13 },
    { path: "performance.nvenc_preset", label: tr("cfg_nvenc_preset"), type: "select", options: NVENC_PRESETS },
  ];
  fields.push({ group: tr("group_rate_factors") });
  const presetMetrics = [
    ["crf", "CRF", 63],
    ...(cfg.encoder === "SvtAv1" ? [["film_grain", tr("cfg_film_grain"), 50]] : []),
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
    { path: "output.container", label: tr("cfg_output_container"), type: "select", options: CONTAINERS },
    { path: "output.same_directory", label: tr("cfg_same_directory"), type: "checkbox", rebuild: true },
    { path: "output.output_directory", label: tr("cfg_output_directory"), type: "text", nullable: true, required: !cfg.output.same_directory, browse: true },
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
    { path: "daemon.allow_insecure_lan", label: tr("cfg_daemon_allow_insecure_lan"), type: "checkbox", disabled: !local, hint: [localHint, restartHint].filter(Boolean).join(" ") },
    { path: "daemon.behind_proxy", label: tr("cfg_daemon_behind_proxy"), type: "checkbox", disabled: !local, hint: localHint },
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
  // film_grain is SVT-AV1 only; the form omits those leaves for HW encoders.
  const expected = cfg.encoder === "SvtAv1"
    ? paths
    : paths.filter((path) => !path.endsWith(".film_grain"));
  const missing = expected.filter((path) => !represented.has(path));
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
      // A named quality preset rewrites the per-tier values, as it does when
      // the daemon loads or saves the configuration.
      if (field.path === "quality_preset") {
        const table = settingsAccess?.preset_tables?.[parsed];
        if (table) config.presets = cloneConfig(table);
      }
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
    // Edits made while the request was out are kept.
    if (settingsDirty()) return;
    config = loadedConfig;
    settingsAccess = access;
    config._service = { autostart: access.autostart };
    verifySettingsCoverage(config, access.setting_paths);
    savedConfig = cloneConfig(config);
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
    const saveWarning = config._warning;
    delete config._warning;
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
    if (saveWarning) toast(saveWarning, true);
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
