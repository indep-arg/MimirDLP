// What the page knows: the app's latest snapshot, which screen is showing,
// and the two activity logs. The app holds the real state; this only
// mirrors it and decides what is on screen.

import { computed, reactive, watch } from "vue";
import { api, type Fixed, type LogLine, type Snapshot } from "./api";
import { setCatalog } from "./i18n";

export type Page = "download" | "setup" | "settings" | "about";

/** As many lines as the app itself keeps (app/log.rs). */
const MAX_LOG = 2000;

interface Log {
  lines: LogLine[];
  /** The revision the lines were last brought up to. */
  revision: number;
  loading: boolean;
}

interface Store {
  snap: Snapshot | null;
  fixed: Fixed | null;
  page: Page;
  /** The app opened on Setup and moves on to Download by itself once
   *  everything is installed, unless the user went somewhere first. */
  leaveSetupWhenReady: boolean;
  /** What is typed in the link field. */
  input: string;
  logs: { queue: Log; setup: Log };
  /** The system's own light or dark mode. */
  systemDark: boolean;
  /** Debug builds only: open with the menu or the advanced settings
   *  showing (see dev_hints in commands.rs). */
  dev: { menu: boolean; advanced: boolean };
}

const emptyLog = (): Log => ({ lines: [], revision: 0, loading: false });

export const store = reactive<Store>({
  snap: null,
  fixed: null,
  page: "download",
  leaveSetupWhenReady: false,
  input: "",
  logs: { queue: emptyLog(), setup: emptyLog() },
  systemDark: window.matchMedia("(prefers-color-scheme: dark)").matches,
  dev: { menu: false, advanced: false },
});

export const dark = computed(() => {
  const choice = store.snap?.settings.values.theme ?? "system";
  return choice === "system" ? store.systemDark : choice === "dark";
});

/** Brings a log up to date: only the lines the page doesn't have yet. */
async function refreshLog(which: "queue" | "setup", revision: number) {
  const log = store.logs[which];
  if (log.loading || log.revision === revision) return;
  log.loading = true;
  try {
    // From the last line it has, which may have been repeated since and
    // then carries a count now.
    const from = log.lines.length ? log.lines[log.lines.length - 1].seq : 0;
    const fresh = await api.logSince(which, from);
    if (fresh.length) {
      const kept = log.lines.filter((line) => line.seq < fresh[0].seq);
      log.lines = kept.concat(fresh).slice(-MAX_LOG);
    }
    log.revision = revision;
  } finally {
    log.loading = false;
  }
  // More may have arrived while that was fetched.
  const latest = which === "queue" ? store.snap?.queue.logRevision : store.snap?.setup.logRevision;
  if (latest !== undefined && latest !== log.revision) void refreshLog(which, latest);
}

function take(snapshot: Snapshot) {
  const before = store.snap;
  store.snap = snapshot;
  // The first inspection found a tool that is there but doesn't run:
  // Setup repairs it by itself, then comes back to Download.
  if (before?.confirmingTools && !snapshot.confirmingTools) {
    if (!snapshot.setup.ready && store.page === "download") {
      store.page = "setup";
      store.leaveSetupWhenReady = true;
    }
  }
  if (store.leaveSetupWhenReady && snapshot.setup.ready && snapshot.setup.job === null) {
    store.leaveSetupWhenReady = false;
    if (store.page === "setup") store.page = "download";
  }
  void refreshLog("queue", snapshot.queue.logRevision);
  void refreshLog("setup", snapshot.setup.logRevision);
}

/** What only changes with the language, the translations among it. */
export async function loadFixed() {
  const fixed = await api.fixed();
  setCatalog(fixed.catalog);
  store.fixed = fixed;
  document.documentElement.lang = fixed.language;
}

export async function start() {
  window
    .matchMedia("(prefers-color-scheme: dark)")
    .addEventListener("change", (event) => (store.systemDark = event.matches));
  watch(
    dark,
    (isDark) => (document.documentElement.dataset.theme = isDark ? "dark" : "light"),
    { immediate: true },
  );

  await api.onState(take);
  const [snapshot, hints] = await Promise.all([api.snapshot(), api.devHints(), loadFixed()]);
  store.dev = { menu: hints.menu, advanced: hints.advanced };
  // With every tool there, open on Download; a first launch, or a folder
  // with something missing, opens on Setup.
  store.page = snapshot.toolsPresent ? "download" : "setup";
  store.leaveSetupWhenReady = store.page === "setup";
  take(snapshot);

  if (hints.page && ["download", "setup", "settings", "about"].includes(hints.page)) {
    store.page = hints.page as Page;
    store.leaveSetupWhenReady = false;
  }

  // A link copied elsewhere is waiting in the link field when the window
  // comes back to the front. It is only queued when Add is pressed.
  window.addEventListener("focus", () => void offerClipboard());
  void offerClipboard();
}

async function offerClipboard() {
  if (store.input.trim() !== "") return;
  const offered = await api.offerClipboard(store.input);
  if (offered && store.input.trim() === "") store.input = offered;
}

export async function addLinks() {
  // What couldn't be queued stays in the field, to be fixed; why is said
  // next to the Download button (the snapshot's queue.problem).
  const field = await api.addLinks(store.input);
  store.input = field.input;
}

/** Whether `target` can be shown right now. */
export function canOpen(target: Page): boolean {
  const snap = store.snap;
  if (!snap) return false;
  if (target === "download") return snap.canDownload;
  // yt-dlp is running: what is in bin/ must be left alone.
  if (target === "setup") return !snap.queue.usesTools;
  return true;
}

export function navigate(target: Page) {
  store.leaveSetupWhenReady = false;
  if (canOpen(target)) store.page = target;
}

/** From the "yt-dlp is old" notice: go to Setup and check for updates. */
export function checkForUpdates() {
  const snap = store.snap;
  if (!snap || snap.queue.usesTools || snap.setup.job !== null) return;
  store.leaveSetupWhenReady = false;
  store.page = "setup";
  void api.setup("check_from_notice");
}

export async function setLanguage(code: string) {
  await api.setSetting("language", code);
  await loadFixed();
}

/** A setting's value, by its key in the settings file. */
export function setting(key: string): string {
  return store.snap?.settings.values[key] ?? "";
}

export function isOn(key: string): boolean {
  return setting(key) === "yes";
}

export function choices(key: string) {
  return store.fixed?.choices[key] ?? [];
}

/** The label of a choice's option, in the interface's language. */
export function choiceLabel(key: string, option: string): string {
  return choices(key).find((choice) => choice.key === option)?.label ?? option;
}
