<script setup lang="ts">
// The Setup screen: the tools Osprey needs (yt-dlp, FFmpeg, Deno), what
// is installed of them, and the actions available on them. A first launch
// sees a welcome with one progress bar instead, until everything runs.
import { computed, ref } from "vue";
import { Check, CircleCheck, CircleX, Copy, Download, LoaderCircle, TriangleAlert } from "@lucide/vue";
import { api, type Component, type Snapshot } from "../lib/api";
import { amount, fraction } from "../lib/format";
import { t, tn } from "../lib/i18n";
import { store } from "../lib/store";
import logo from "../assets/logo.svg";
import LogPanel from "../components/LogPanel.vue";
import ProgressBar from "../components/ProgressBar.vue";
import UiSelect from "../components/UiSelect.vue";

const props = defineProps<{ snap: Snapshot }>();

const setup = computed(() => props.snap.setup);
const busy = computed(() => setup.value.job !== null);
// yt-dlp is running: what is in bin/ must not be replaced underneath it.
const locked = computed(() => busy.value || props.snap.queue.usesTools);

const whereKept = computed(() => {
  if (setup.value.bundled) {
    return t(
      "yt-dlp, FFmpeg and Deno come with this package and are updated together with Osprey, from your software center or with flatpak update.",
    );
  }
  if (setup.value.installed) {
    return t("yt-dlp, FFmpeg and Deno are kept in {folder}, and updated from here.", {
      folder: setup.value.binDir,
    });
  }
  return t(
    "yt-dlp, FFmpeg and Deno are kept in the bin folder next to this app. Nothing is installed on your system.",
  );
});

const welcomeText = computed(() =>
  setup.value.installed
    ? t(
        "To download, Osprey needs yt-dlp, FFmpeg and Deno, about 200 MB. They are kept in your user folder.",
      )
    : t(
        "To download, Osprey needs yt-dlp, FFmpeg and Deno, about 200 MB. They are kept next to the app; nothing is installed on your system.",
      ),
);

type Tone = "good" | "warn" | "bad" | "neutral";

/** One line of the component list: how it is, and in which tone. */
function describe(component: Component): { tone: Tone; text: string } {
  const version = component.version ?? "";
  const bundled = setup.value.bundled;
  switch (component.status) {
    case "checking":
      return { tone: "neutral", text: t("Checking...") };
    case "missing":
      return {
        tone: "bad",
        text: bundled ? t("Not found in this package") : t("Not installed"),
      };
    case "broken":
      return { tone: "bad", text: t("Installed but not responding") };
  }
  // A yt-dlp weeks old is the usual reason a site stops working, so its
  // age is shown once it is stale.
  const age = component.staleAge;
  if (bundled) {
    return age !== null
      ? {
          tone: "warn",
          text: tn(
            "Version {version} ({n} day old, update Osprey)",
            "Version {version} ({n} days old, update Osprey)",
            age,
            { version },
          ),
        }
      : { tone: "good", text: t("Version {version} (included with Osprey)", { version }) };
  }
  if (component.update === null && age !== null) {
    return {
      tone: "warn",
      text: tn(
        "Version {version} ({n} day old, check for updates)",
        "Version {version} ({n} days old, check for updates)",
        age,
        { version },
      ),
    };
  }
  switch (component.update) {
    case null:
      return { tone: "good", text: t("Version {version} (installed)", { version }) };
    case "current":
      return { tone: "good", text: t("Version {version} (up to date)", { version }) };
    case "outdated":
      return { tone: "warn", text: t("Version {version} (update available)", { version }) };
    case "unknown":
      return {
        tone: "warn",
        text: t(
          "Version {version} (installed before update tracking, reinstall to enable it)",
          { version },
        ),
      };
    case "failed":
      return { tone: "warn", text: t("Version {version} (update check failed)", { version }) };
  }
}

const rows = computed(() => setup.value.components.map((c) => ({ ...c, ...describe(c) })));

// The contextual actions available, given what is installed right now.
const installed = (c: Component) => c.status === "installed";
const hasUpdates = computed(() =>
  setup.value.components.some((c) => installed(c) && c.update === "outdated"),
);
const hasUnchecked = computed(() =>
  setup.value.components.some((c) => installed(c) && (c.update === null || c.update === "failed")),
);
const hasInstalled = computed(() =>
  setup.value.components.some((c) => c.status === "installed" || c.status === "broken"),
);

const busyText = computed(() => {
  switch (setup.value.job) {
    case "inspecting":
      return t("Looking at what is installed...");
    case "installing":
      return t("Working...");
    case "checking":
      return t("Checking for updates...");
    default:
      return "";
  }
});

const welcomeStep = computed(() =>
  t("Step {step} of {total}: getting {component}...", {
    step: setup.value.welcomeStep,
    total: setup.value.components.length,
    component: setup.value.components[setup.value.welcomeStep - 1]?.name ?? "",
  }),
);

// Copying the diagnostics answers for a moment, like copying a link.
const copied = ref(false);
async function copyDiagnostics() {
  await api.setup("diagnostics");
  copied.value = true;
  setTimeout(() => (copied.value = false), 1400);
}
</script>

<template>
  <!-- What a first launch sees: what the app needs and why, one bar for
       the whole install, and what went wrong if it did. -->
  <div v-if="setup.welcoming" class="welcome page">
    <img class="logo" :src="logo" alt="" draggable="false" />
    <h1>{{ t("Welcome to Osprey") }}</h1>
    <p class="muted text">{{ welcomeText }}</p>

    <div class="welcome-progress">
      <template v-if="setup.job === 'installing'">
        <div class="line">
          <span class="caption">{{ welcomeStep }}</span>
          <span v-if="setup.transfer" class="caption muted numbers">
            {{ amount(setup.transfer.received, setup.transfer.total) }}
          </span>
        </div>
        <ProgressBar :value="setup.welcomeFraction" />
      </template>
      <p v-else-if="setup.job" class="caption muted">{{ t("Getting ready...") }}</p>
      <template v-else>
        <p v-if="setup.lastError" class="caption danger-text selectable">
          {{ t("Not everything could be installed: {error}", { error: setup.lastError }) }}
        </p>
        <p v-else class="caption muted">{{ t("Nothing is installed yet.") }}</p>
        <button class="btn btn-primary" @click="api.setup('install')">
          <Download /> {{ setup.lastError ? t("Try again") : t("Install") }}
        </button>
      </template>
    </div>

    <button class="btn btn-ghost btn-small" @click="api.setup('details')">
      {{ t("Show details") }}
    </button>
  </div>

  <div v-else class="setup page">
    <div class="intro">
      <p class="muted">{{ whereKept }}</p>
      <!-- Opened from the first launch's welcome: the way back to it. -->
      <button v-if="setup.firstLaunch" class="btn btn-ghost btn-small" @click="api.setup('details')">
        {{ t("Hide details") }}
      </button>
    </div>

    <section>
      <h2 class="section-title">{{ t("Components") }}</h2>
      <div class="card rows">
        <div v-for="row in rows" :key="row.name" class="row component" :class="row.tone">
          <span class="mark">
            <CircleCheck v-if="row.tone === 'good'" />
            <TriangleAlert v-else-if="row.tone === 'warn'" />
            <CircleX v-else-if="row.tone === 'bad'" />
            <LoaderCircle v-else class="spin" />
          </span>
          <span class="name">{{ row.name }}</span>
          <span class="detail selectable">{{ row.text }}</span>
        </div>
        <!-- The package brings its own yt-dlp, so there is nothing to choose. -->
        <template v-if="!setup.bundled">
          <div class="row">
            <span class="row-label">{{ t("yt-dlp builds") }}</span>
            <div class="control">
              <UiSelect
                :model-value="setup.channel"
                :options="store.fixed?.channels ?? []"
                :disabled="locked"
                :label="t('yt-dlp builds')"
                @update:model-value="api.setup('channel', $event)"
              />
            </div>
          </div>
          <p class="row-note">
            {{
              t(
                "Nightly builds get fixes for sites that changed within a day. Stable releases are tested for longer but come weeks apart.",
              )
            }}
          </p>
        </template>
      </div>
    </section>

    <!-- None for tools that came with the package, only what is going on. -->
    <div v-if="setup.bundled" class="actions">
      <span v-if="busy" class="muted busy"><LoaderCircle class="spin" /> {{ busyText }}</span>
    </div>
    <div v-else-if="setup.confirmingReinstall" class="actions confirm">
      <span class="warning-text spacer">
        {{ t("This will re-download and reinstall ALL components.") }}
      </span>
      <button class="btn" @click="api.setup('cancel_reinstall')">{{ t("Cancel") }}</button>
      <button class="btn btn-danger" @click="api.setup('confirm_reinstall')">
        {{ t("Reinstall") }}
      </button>
    </div>
    <!-- Longer labels (Spanish, say) go on to a second line rather than
         squeezing each other. -->
    <div v-else class="actions">
      <button
        v-if="setup.needsInstall"
        class="btn btn-primary"
        :disabled="busy"
        @click="api.setup('install')"
      >
        {{ t("Install missing components") }}
      </button>
      <button
        v-if="hasUpdates"
        class="btn btn-primary"
        :disabled="locked"
        @click="api.setup('update')"
      >
        {{ t("Update outdated components") }}
      </button>
      <button
        v-if="hasUnchecked"
        class="btn"
        :class="{ 'btn-primary': !setup.needsInstall && !hasUpdates }"
        :disabled="busy"
        @click="api.setup('check')"
      >
        {{ t("Check for updates") }}
      </button>
      <button v-if="hasInstalled" class="btn" :disabled="locked" @click="api.setup('ask_reinstall')">
        {{ t("Reinstall all") }}
      </button>
      <span v-if="busy" class="muted busy"><LoaderCircle class="spin" /> {{ busyText }}</span>
    </div>

    <div v-if="setup.transfer" class="transfer">
      <div class="line">
        <span class="caption file selectable">{{ setup.transfer.file }}</span>
        <span class="caption muted numbers">
          {{ amount(setup.transfer.received, setup.transfer.total) }}
        </span>
      </div>
      <ProgressBar
        :value="fraction(setup.transfer.received, setup.transfer.total)"
        :unknown="!setup.transfer.total"
      />
    </div>

    <!-- Copying the diagnostics changes nothing and works at any time, so
         it sits with the activity rather than among the actions. -->
    <section class="activity">
      <div class="activity-title">
        <h2 class="section-title">{{ t("Activity") }}</h2>
        <button class="btn btn-small" @click="copyDiagnostics">
          <Check v-if="copied" />
          <Copy v-else />
          {{ t("Copy diagnostic information") }}
        </button>
      </div>
      <div class="activity-log">
        <LogPanel
          :lines="store.logs.setup.lines"
          :empty="t('Nothing yet. Each download and check will be listed here.')"
        />
      </div>
    </section>
  </div>
</template>

<style scoped>
.welcome {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 20px;
  height: 100%;
  text-align: center;
}
.logo {
  width: 104px;
  height: 104px;
  filter: drop-shadow(0 12px 28px rgba(47, 107, 234, 0.35));
}
.welcome h1 {
  font-size: 26px;
  font-weight: 700;
  letter-spacing: -0.015em;
}
.welcome .text {
  max-width: 560px;
}
.welcome-progress {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  width: 100%;
  max-width: 480px;
  min-height: 72px;
}
.welcome-progress > :deep(.bar) {
  width: 100%;
}
.line {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 12px;
  width: 100%;
  text-align: left;
}
.numbers {
  font-variant-numeric: tabular-nums;
  white-space: pre;
}

.setup {
  display: flex;
  flex-direction: column;
  gap: 20px;
  height: 100%;
  min-height: 560px;
}
.intro {
  display: flex;
  align-items: center;
  gap: 16px;
}
.intro p {
  flex: 1;
}
.component {
  min-height: 52px;
}
.mark {
  display: grid;
  place-items: center;
  flex: none;
  width: 22px;
  color: var(--faint);
}
.mark svg {
  width: 18px;
  height: 18px;
}
.name {
  flex: none;
  width: 80px;
  font-weight: 600;
}
.detail {
  color: var(--muted);
}
.good .mark {
  color: var(--success);
}
.warn .mark,
.warn .detail {
  color: var(--warning);
}
.bad .mark,
.bad .detail {
  color: var(--danger);
}
.control {
  flex: none;
  width: 260px;
}
.actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px;
}
.actions:empty {
  display: none;
}
.busy {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  margin-left: 4px;
}
.busy svg {
  width: 16px;
  height: 16px;
}
.transfer {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.file {
  font-family: var(--mono);
}
.activity {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 160px;
}
.activity-title {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 10px;
}
.activity-title .section-title {
  margin-bottom: 0;
}
.activity-log {
  flex: 1;
  min-height: 0;
}
</style>
