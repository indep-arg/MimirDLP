<script setup lang="ts">
// A progress bar. Without a known fraction it shows that something is
// going on instead of how far it is.
defineProps<{ value: number; unknown?: boolean; thin?: boolean }>();
</script>

<template>
  <div
    class="bar"
    :class="{ thin, unknown }"
    role="progressbar"
    :aria-valuenow="unknown ? undefined : Math.round(value * 100)"
    aria-valuemin="0"
    aria-valuemax="100"
  >
    <div class="fill" :style="unknown ? undefined : { width: `${Math.round(value * 1000) / 10}%` }" />
  </div>
</template>

<style scoped>
.bar {
  position: relative;
  height: 8px;
  border-radius: 999px;
  background: var(--surface-3);
  overflow: hidden;
}
.bar.thin {
  height: 6px;
}
.fill {
  height: 100%;
  min-width: 0;
  border-radius: inherit;
  background: var(--brand);
  transition: width 0.3s var(--ease);
}
.unknown .fill {
  position: absolute;
  width: 35%;
  animation: slide 1.4s ease-in-out infinite;
}
@keyframes slide {
  from {
    left: -35%;
  }
  to {
    left: 100%;
  }
}
</style>
