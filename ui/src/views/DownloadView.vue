<script setup lang="ts">
// The Download screen: the queue, how far it is, and its activity log.
import { computed, ref } from "vue";
import {
  Download,
  FolderOpen,
  ListTree,
  Sparkles,
  Square,
  Trash2,
  TriangleAlert,
} from "@lucide/vue";
import { api, type Snapshot } from "../lib/api";
import { t, tn } from "../lib/i18n";
import { checkForUpdates, store } from "../lib/store";
import LogPanel from "../components/LogPanel.vue";
import ProgressBar from "../components/ProgressBar.vue";
import QueueCard from "../components/QueueCard.vue";

const props = defineProps<{ snap: Snapshot }>();

// The activity log is folded away until asked for.
const showLog = ref(false);

const queue = computed(() => props.snap.queue);
const total = computed(() => queue.value.items.length);
const compact = computed(() => total.value >= 5);

// A failed link is finished, but not done: it is counted on its own, so a
// queue where nothing worked never reads "All done.".
const summary = computed(() => {
  const failed = queue.value.failed;
  const counts = { done: queue.value.done - failed, total: total.value };
  if (queue.value.running) {
    return failed
      ? tn(
          "Downloading: {done} of {total} complete, {n} failed",
          "Downloading: {done} of {total} complete, {n} failed",
          failed,
          counts,
        )
      : t("Downloading: {done} of {total} complete", counts);
  }
  if (queue.value.done === total.value) {
    return failed
      ? tn(
          "Queue finished with {n} failed download.",
          "Queue finished with {n} failed downloads.",
          failed,
        )
      : t("All done.");
  }
  return failed
    ? tn(
        "Queue ready: {done} of {total} complete, {n} failed",
        "Queue ready: {done} of {total} complete, {n} failed",
        failed,
        counts,
      )
    : t("Queue ready: {done} of {total} complete", counts);
});

// Sites change their pages all the time and yt-dlp follows within days, so
// an old one is the usual reason a download suddenly fails. A bundled one
// comes with the package, so the way out is updating Osprey itself.
const staleNotice = computed(() => {
  const age = props.snap.setup.ytdlpAge;
  if (age === null) return null;
  return props.snap.setup.bundled
    ? tn(
        "The installed version of yt-dlp is {n} day old. Sites change often; if a download fails, update Osprey using the same method you used to install it.",
        "The installed version of yt-dlp is {n} days old. Sites change often; if a download fails, update Osprey using the same method you used to install it.",
        age,
      )
    : tn(
        "The installed version of yt-dlp is {n} day old. Sites change often; if a download fails, update yt-dlp first.",
        "The installed version of yt-dlp is {n} days old. Sites change often; if a download fails, update yt-dlp first.",
        age,
      );
});
const summaryTone = computed(() => {
  if (queue.value.failed === 0) return "muted";
  return queue.value.failed === total.value ? "danger-text" : "warning-text";
});
const canUpdateTools = computed(() => !queue.value.usesTools && props.snap.setup.job === null);
</script>

<template>
  <div class="download page">
    <!-- The notices sit in the page, lined up with what is below. -->
    <div v-if="snap.appUpdate" class="notice update">
      <Sparkles />
      <span class="notice-text">
        {{ t("Osprey {version} is available.", { version: snap.appUpdate }) }}
      </span>
      <button class="btn btn-small btn-primary" @click="api.openLink('releases')">
        {{ t("What's new") }}
      </button>
      <button class="btn btn-small" @click="api.dismissUpdate()">{{ t("Later") }}</button>
    </div>
    <div v-if="staleNotice" class="notice stale">
      <TriangleAlert />
      <span class="notice-text">{{ staleNotice }}</span>
      <button
        v-if="!snap.setup.bundled"
        class="btn btn-small"
        :disabled="!canUpdateTools"
        @click="checkForUpdates"
      >
        {{ t("Check for updates") }}
      </button>
    </div>

    <div v-if="total === 0" class="empty">
      <h2>{{ t("Add a link to get started") }}</h2>
      <p class="muted">{{ t("Paste a YouTube or other supported link in the box above.") }}</p>
      <p class="caption muted">
        {{
          t(
            "Copy a link and return to Osprey: it will appear in the field above. You can also paste with Ctrl+V.",
          )
        }}
      </p>
    </div>
    <div v-else class="list-scroll">
      <TransitionGroup name="list" tag="div" class="list" :class="{ compact }">
        <QueueCard v-for="item in queue.items" :key="item.id" :item="item" :compact="compact" />
      </TransitionGroup>
    </div>

    <footer class="footer">
      <div v-if="total > 0" class="progress">
        <span class="caption" :class="summaryTone">{{ summary }}</span>
        <!-- An empty bar under a queue nobody started yet says nothing. -->
        <ProgressBar
          v-if="queue.running || queue.done > 0"
          thin
          :danger="queue.failed > 0 && queue.failed === total"
          :value="queue.fraction"
        />
      </div>
      <div class="actions">
        <button v-if="queue.running" class="btn btn-danger" @click="api.queue('stop')">
          <Square /> {{ queue.recording ? t("Stop and save") : t("Stop") }}
        </button>
        <!-- Greyed out until there is something to download. -->
        <button
          v-else
          class="btn btn-primary"
          :disabled="!queue.hasQueued"
          @click="api.queue('start')"
        >
          <Download /> {{ t("Download") }}
        </button>
        <button class="btn" @click="api.queue('folder')">
          <FolderOpen /> {{ t("Open downloads") }}
        </button>
        <button v-if="queue.hasFinished" class="btn" @click="api.queue('clear')">
          <Trash2 /> {{ t("Remove finished items") }}
        </button>
        <span v-if="queue.problem" class="problem danger-text">{{ queue.problem }}</span>
        <span class="spacer" />
        <button class="btn btn-ghost" :class="{ active: showLog }" @click="showLog = !showLog">
          <ListTree /> {{ showLog ? t("Hide activity") : t("Show activity") }}
        </button>
      </div>
      <div v-if="showLog" class="log">
        <LogPanel
          :lines="store.logs.queue.lines"
          :empty="t('No activity yet. Download steps will appear here.')"
        />
      </div>
    </footer>
  </div>
</template>

<style scoped>
.download {
  display: flex;
  flex-direction: column;
  gap: 14px;
  height: 100%;
  padding-bottom: 20px;
}
.update {
  --tone: var(--accent);
}
.stale {
  --tone: var(--warning);
}
.empty {
  display: flex;
  flex: 1;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  text-align: center;
}
.empty h2 {
  font-size: 22px;
  font-weight: 600;
  letter-spacing: -0.01em;
}
.empty .caption {
  max-width: 420px;
}
/* Its own lane, so the scrollbar never covers a card's edge. */
.list-scroll {
  flex: 1;
  min-height: 0;
  margin-right: -14px;
  padding-right: 4px;
  overflow-y: auto;
  scrollbar-gutter: stable;
}
.list {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 2px 0 4px;
}
.list.compact {
  gap: 6px;
}
.footer {
  display: flex;
  flex: none;
  flex-direction: column;
  gap: 14px;
}
.progress {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px;
}
.problem {
  font-size: 13px;
}
.log {
  height: 170px;
}
</style>
