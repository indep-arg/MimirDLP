<script setup lang="ts">
// An activity log: what the app did, line by line. It follows the newest
// line unless the reader scrolled up to look at something.
import { nextTick, ref, watch } from "vue";
import type { LogLine } from "../lib/api";

const props = defineProps<{ lines: LogLine[]; empty: string }>();

const box = ref<HTMLElement>();
let following = true;

function scrolled() {
  const el = box.value;
  if (el) following = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
}

watch(
  () => [props.lines.length, props.lines[props.lines.length - 1]?.text],
  async () => {
    if (!following) return;
    await nextTick();
    if (box.value) box.value.scrollTop = box.value.scrollHeight;
  },
  { immediate: true },
);
</script>

<template>
  <div ref="box" class="log selectable" @scroll.passive="scrolled">
    <p v-if="lines.length === 0" class="nothing">{{ empty }}</p>
    <div v-for="line in lines" :key="line.seq" class="line" :class="line.level">
      <span v-if="line.text" class="tag">{{ line.level }}</span>
      <span class="text">{{ line.text || " " }}</span>
    </div>
  </div>
</template>

<style scoped>
.log {
  height: 100%;
  min-height: 0;
  overflow-y: auto;
  padding: 12px 14px;
  border: 1px solid var(--border);
  border-radius: var(--radius-m);
  background: var(--surface);
  font-family: var(--mono);
  font-size: 12px;
  line-height: 1.6;
}
.nothing {
  color: var(--faint);
  font-family: var(--font);
  font-size: 13px;
}
.line {
  display: flex;
  gap: 10px;
}
.tag {
  flex: none;
  width: 68px;
  color: var(--faint);
  text-transform: uppercase;
  font-size: 10.5px;
  letter-spacing: 0.04em;
  padding-top: 1px;
}
.text {
  min-width: 0;
  overflow-wrap: anywhere;
  white-space: pre-wrap;
  color: var(--muted);
}
.ok .tag,
.success .tag {
  color: var(--success);
}
.success .text {
  color: var(--success);
}
.warn .tag,
.warn .text {
  color: var(--warning);
}
.error .tag,
.error .text {
  color: var(--danger);
}
.download .tag,
.install .tag,
.verify .tag {
  color: var(--accent);
}
.download .text {
  color: var(--text);
}
</style>
