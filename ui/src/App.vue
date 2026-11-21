<script setup lang="ts">
// The window: a header bar and one screen at a time.
import { nextTick, onMounted } from "vue";
import { api } from "./lib/api";
import { start, store } from "./lib/store";
import AppHeader from "./components/AppHeader.vue";
import AboutView from "./views/AboutView.vue";
import DownloadView from "./views/DownloadView.vue";
import SettingsView from "./views/SettingsView.vue";
import SetupView from "./views/SetupView.vue";

onMounted(async () => {
  try {
    await start();
  } finally {
    // The window is hidden until the page has drawn itself, so it never
    // shows up empty: wait for that frame, then ask to be shown.
    await nextTick();
    requestAnimationFrame(() => requestAnimationFrame(() => void api.windowReady()));
  }
});
</script>

<template>
  <template v-if="store.snap">
    <AppHeader />
    <main class="screen" :class="store.page">
      <Transition name="page" mode="out-in">
        <DownloadView v-if="store.page === 'download'" :snap="store.snap" />
        <SetupView v-else-if="store.page === 'setup'" :snap="store.snap" />
        <SettingsView v-else-if="store.page === 'settings'" :snap="store.snap" />
        <AboutView v-else :snap="store.snap" />
      </Transition>
    </main>
  </template>
</template>

<style>
#app {
  display: flex;
  flex-direction: column;
}
.screen {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  scrollbar-gutter: stable both-edges;
}
/* The Download screen scrolls its own list, and keeps its buttons in view. */
.screen.download {
  overflow: hidden;
}
</style>
