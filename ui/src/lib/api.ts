// The app behind the window: what it tells the page (the types mirror
// crates/core/src/app/view.rs) and what the page can ask of it (the
// commands of crates/desktop/src/commands.rs).

import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Catalog } from "./i18n";

export interface Choice {
  key: string;
  label: string;
}

export interface Progress {
  downloaded: number;
  total: number | null;
  speed: number | null;
  eta: number | null;
  processing: boolean;
}

export interface Item {
  id: number;
  url: string;
  title: string | null;
  lookup: "fetching" | "ready" | "failed";
  lookupError: string | null;
  playlist: boolean;
  playlistCount: number | null;
  duration: number | null;
  size: number | null;
  thumbnail: boolean;
  kind: string;
  quality: string;
  status: "queued" | "downloading" | "completed" | "failed";
  progress: Progress | null;
  path: string | null;
  error: string | null;
  live: boolean;
}

export interface QueueState {
  items: Item[];
  input: string;
  problem: string | null;
  running: boolean;
  recording: boolean;
  usesTools: boolean;
  fraction: number;
  done: number;
  hasQueued: boolean;
  hasFinished: boolean;
  logRevision: number;
}

export interface Component {
  name: string;
  status: "checking" | "missing" | "broken" | "installed";
  version: string | null;
  update: "current" | "outdated" | "unknown" | "failed" | null;
  staleAge: number | null;
}

export interface Transfer {
  file: string;
  received: number;
  total: number | null;
}

export interface SetupState {
  components: Component[];
  channel: string;
  job: "inspecting" | "installing" | "checking" | null;
  ready: boolean;
  needsInstall: boolean;
  bundled: boolean;
  installed: boolean;
  binDir: string;
  confirmingReinstall: boolean;
  transfer: Transfer | null;
  welcoming: boolean;
  firstLaunch: boolean;
  welcomeStep: number;
  welcomeFraction: number;
  ytdlpVersion: string | null;
  ytdlpAge: number | null;
  lastError: string | null;
  logRevision: number;
}

export interface SettingsState {
  values: Record<string, string>;
  videoPassword: boolean;
  history: number | null;
  autoSubsProblem: string | null;
  desktopEntry: boolean;
}

export interface Snapshot {
  appUpdate: string | null;
  canDownload: boolean;
  toolsPresent: boolean;
  confirmingTools: boolean;
  queue: QueueState;
  setup: SetupState;
  settings: SettingsState;
}

export interface Fixed {
  version: string;
  language: string;
  languages: Choice[];
  choices: Record<string, Choice[]>;
  channels: Choice[];
  linux: boolean;
  installed: boolean;
  bundled: boolean;
  defaultFolder: string;
  catalog: Catalog | null;
}

export interface LogLine {
  seq: number;
  level: "info" | "install" | "download" | "verify" | "ok" | "warn" | "error" | "success";
  text: string;
}

/** Debug builds only: how to open (see dev_hints in commands.rs). */
export interface DevHints {
  page: string | null;
  menu: boolean;
  advanced: boolean;
}

export interface LinkField {
  input: string;
  problem: string | null;
}

export type QueueAction =
  | "start"
  | "stop"
  | "clear"
  | "folder"
  | "remove"
  | "retry"
  | "kind"
  | "quality"
  | "copy"
  | "open"
  | "reveal";

export type SetupAction =
  | "install"
  | "check"
  | "update"
  | "ask_reinstall"
  | "confirm_reinstall"
  | "cancel_reinstall"
  | "details"
  | "channel"
  | "diagnostics"
  | "check_from_notice";

export const api = {
  snapshot: () => invoke<Snapshot>("snapshot"),
  fixed: () => invoke<Fixed>("fixed"),
  onState: (handler: (snapshot: Snapshot) => void) =>
    listen<Snapshot>("state", (event) => handler(event.payload)),
  logSince: (which: "queue" | "setup", seq: number) =>
    invoke<LogLine[]>("log_since", { which, seq }),
  windowReady: () => invoke<void>("window_ready"),
  devHints: () => invoke<DevHints>("dev_hints"),
  addLinks: (input: string) => invoke<LinkField>("add_links", { input }),
  offerClipboard: (input: string) => invoke<string | null>("offer_clipboard", { input }),
  queue: (action: QueueAction, id?: number, value?: string) =>
    invoke<void>("queue_do", { action, id: id ?? null, value: value ?? null }),
  setup: (action: SetupAction, value?: string) =>
    invoke<void>("setup_do", { action, value: value ?? null }),
  setSetting: (key: string, value: string) => invoke<void>("set_setting", { key, value }),
  setDesktopEntry: (wanted: boolean) => invoke<void>("set_desktop_entry", { wanted }),
  chooseFolder: () => invoke<void>("choose_folder"),
  historyLines: () => invoke<string[] | null>("history_lines"),
  clearHistory: () => invoke<void>("clear_history"),
  dismissUpdate: () => invoke<void>("dismiss_update"),
  openLink: (which: "website" | "report" | "releases") => invoke<void>("open_link", { which }),
  /** Where the page finds the thumbnail of the queue item `id`. */
  thumbnail: (id: number) => convertFileSrc(String(id), "thumb"),
};
