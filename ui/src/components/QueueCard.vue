<script setup lang="ts">
// One link of the queue: its thumbnail (a placeholder while it loads, or
// when the lookup failed), title and details, the choices it downloads
// with, where it is at, and the actions it allows right now.
import { computed, ref } from "vue";
import {
  Check,
  CircleCheck,
  CircleX,
  Clock,
  Copy,
  Download,
  FileVideo,
  FolderOpen,
  LoaderCircle,
  Play,
  Radio,
  RotateCcw,
  X,
} from "@lucide/vue";
import { api, type Item } from "../lib/api";
import { amount, clock, duration, fraction, size, speed } from "../lib/format";
import { t, tn } from "../lib/i18n";
import { choiceLabel, choices } from "../lib/store";
import ProgressBar from "./ProgressBar.vue";
import UiSelect from "./UiSelect.vue";

const props = defineProps<{ item: Item }>();

const title = computed(() => props.item.title ?? props.item.url);

const details = computed(() => {
  const item = props.item;
  const parts: string[] = [];
  if (item.playlist) {
    parts.push(
      item.playlistCount === null
        ? t("Playlist")
        : tn("Playlist, {n} video", "Playlist, {n} videos", item.playlistCount),
    );
  }
  if (item.duration !== null) parts.push(duration(item.duration));
  if (item.size !== null) parts.push(`~${size(item.size)}`);
  return parts.join("  ·  ");
});

const progressDetails = computed(() => {
  const progress = props.item.progress;
  if (!progress) return "";
  const parts = [amount(progress.downloaded, progress.total)];
  if (progress.speed !== null) parts.push(t("at {speed} MB/s", { speed: speed(progress.speed) }));
  if (progress.eta !== null && !progress.processing) {
    parts.push(t("{time} left", { time: duration(progress.eta) }));
  }
  return parts.join("   ");
});

// The copy button answers for a moment, so the click is seen to have worked.
const copied = ref(false);
async function copy() {
  await api.queue("copy", props.item.id);
  copied.value = true;
  setTimeout(() => (copied.value = false), 1400);
}
</script>

<template>
  <article class="card item" :class="item.status">
    <div class="thumb">
      <img v-if="item.thumbnail" :src="api.thumbnail(item.id)" alt="" draggable="false" />
      <div v-else class="placeholder">
        <LoaderCircle v-if="item.lookup === 'fetching'" class="spin" />
        <FileVideo v-else />
      </div>
      <span v-if="item.duration !== null && item.thumbnail" class="length">
        {{ clock(item.duration) }}
      </span>
      <span v-if="item.live" class="live"><Radio /> LIVE</span>
    </div>

    <div class="body">
      <h3 class="title selectable" :title="title">{{ title }}</h3>

      <p v-if="item.lookup === 'fetching'" class="caption muted">
        {{ t("Looking up this link...") }}
      </p>
      <p v-else-if="item.lookup === 'failed'" class="caption warning-text selectable">
        {{ t("Could not look this link up: {e}", { e: item.lookupError ?? "" }) }}
      </p>
      <p v-else-if="details" class="caption muted">{{ details }}</p>

      <!-- The same two controls in every state: dropdowns while the link
           waits, the same shapes without an arrow once it can't change.
           Quality means nothing for audio. -->
      <div class="picks">
        <template v-if="item.status === 'queued'">
          <UiSelect
            small
            :model-value="item.kind"
            :options="choices('kind')"
            :label="t('Download')"
            @update:model-value="api.queue('kind', item.id, $event)"
          />
          <UiSelect
            v-if="item.kind === 'video'"
            small
            :model-value="item.quality"
            :options="choices('quality')"
            :label="t('Quality')"
            @update:model-value="api.queue('quality', item.id, $event)"
          />
        </template>
        <template v-else>
          <span class="chip">{{ choiceLabel("kind", item.kind) }}</span>
          <span v-if="item.kind === 'video'" class="chip">
            {{ choiceLabel("quality", item.quality) }}
          </span>
        </template>
      </div>

      <div v-if="item.status === 'queued'" class="status muted"><Clock /> {{ t("Queued") }}</div>

      <div v-else-if="item.status === 'downloading' && item.progress" class="running">
        <div class="running-line">
          <span class="status" :class="item.live ? 'danger-text' : 'accent'">
            <Radio v-if="item.live" />
            <Download v-else />
            <span class="running-label">
              {{
                item.progress.processing
                  ? t("Processing")
                  : item.live
                    ? t("Recording live")
                    : t("Downloading")
              }}
            </span>
          </span>
          <span class="caption muted numbers">{{ progressDetails }}</span>
        </div>
        <ProgressBar
          :value="fraction(item.progress.downloaded, item.progress.total)"
          :unknown="item.progress.processing || !item.progress.total"
        />
      </div>

      <!-- No file: yt-dlp skipped it (already in the history, or the file
           is already there). -->
      <div v-else-if="item.status === 'completed' && !item.path" class="status muted">
        <CircleCheck /> {{ t("Nothing new to download") }}
      </div>
      <div v-else-if="item.status === 'completed'" class="status success">
        <CircleCheck /> {{ t("Completed") }}
      </div>

      <!-- The reason in ordinary text under a short red heading, and the
           way out right under it. -->
      <div v-else-if="item.status === 'failed'" class="failure">
        <div class="status danger-text"><CircleX /> {{ t("Failed") }}</div>
        <p class="caption selectable">{{ item.error }}</p>
        <button class="btn btn-small" @click="api.queue('retry', item.id)">
          <RotateCcw /> {{ t("Retry") }}
        </button>
      </div>
    </div>

    <!-- Only what applies is shown, rather than greyed-out buttons. -->
    <div class="actions">
      <template v-if="item.status === 'completed' && item.path">
        <button
          class="icon-btn"
          :title="t('Open the file')"
          :aria-label="t('Open the file')"
          @click="api.queue('open', item.id)"
        >
          <Play />
        </button>
        <button
          class="icon-btn"
          :title="t('Show in folder')"
          :aria-label="t('Show in folder')"
          @click="api.queue('reveal', item.id)"
        >
          <FolderOpen />
        </button>
      </template>
      <button class="icon-btn" :title="t('Copy link')" :aria-label="t('Copy link')" @click="copy">
        <Check v-if="copied" class="success" />
        <Copy v-else />
      </button>
      <button
        v-if="item.status !== 'downloading'"
        class="icon-btn"
        :title="t('Remove')"
        :aria-label="t('Remove')"
        @click="api.queue('remove', item.id)"
      >
        <X />
      </button>
    </div>
  </article>
</template>

<style scoped>
.item {
  display: flex;
  align-items: flex-start;
  gap: 16px;
  padding: 14px;
  transition:
    border-color 0.2s var(--ease),
    box-shadow 0.2s var(--ease);
}
.item:hover {
  border-color: var(--border-strong);
}
.item.downloading {
  border-color: color-mix(in srgb, var(--accent) 45%, transparent);
  box-shadow:
    var(--shadow),
    0 0 0 3px var(--accent-soft);
}

/* 16:9, like nearly every thumbnail. */
.thumb {
  position: relative;
  flex: none;
  width: 168px;
  aspect-ratio: 16 / 9;
  border-radius: 10px;
  overflow: hidden;
  background: var(--surface-2);
}
.thumb img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.placeholder {
  display: grid;
  place-items: center;
  width: 100%;
  height: 100%;
  color: var(--faint);
}
.placeholder svg {
  width: 26px;
  height: 26px;
}
.length,
.live {
  position: absolute;
  bottom: 6px;
  padding: 1px 6px;
  border-radius: 6px;
  background: rgba(0, 0, 0, 0.78);
  color: #fff;
  font-size: 11.5px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}
.length {
  right: 6px;
}
.live {
  left: 6px;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  background: var(--danger-solid);
  letter-spacing: 0.04em;
}
.live svg {
  width: 12px;
  height: 12px;
}

/* Takes what is left and wraps inside it, so a long error can't push the
   actions out of the card. */
.body {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
}
.title {
  display: -webkit-box;
  overflow: hidden;
  font-size: 15px;
  font-weight: 650;
  line-height: 1.3;
  letter-spacing: -0.005em;
  overflow-wrap: anywhere;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  line-clamp: 2;
}
.picks {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
.status {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  font-weight: 550;
}
.status svg {
  width: 16px;
  height: 16px;
  flex: none;
}
.accent {
  color: var(--accent);
}
.success {
  color: var(--success);
}
.running {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.running-line {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.running-label {
  color: var(--text);
}
.numbers {
  font-variant-numeric: tabular-nums;
  white-space: pre;
  text-align: right;
}
.failure {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 8px;
}
.failure p {
  overflow-wrap: anywhere;
}
.actions {
  display: flex;
  flex: none;
  gap: 2px;
}
</style>
