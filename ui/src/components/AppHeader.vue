<script setup lang="ts">
// The header bar: a back button off the main screen, the screen's title
// (or the link field, on the Download screen) and the menu that leads
// everywhere else.
import { computed, ref } from "vue";
import { ChevronLeft, Link2, Menu, Plus } from "@lucide/vue";
import { t } from "../lib/i18n";
import { addLinks, canOpen, navigate, store } from "../lib/store";
import MainMenu from "./MainMenu.vue";

const menuOpen = ref(store.dev.menu);

const title = computed(() => {
  switch (store.page) {
    case "setup":
      // With the tools bundled, Setup only lists them.
      return store.snap?.setup.bundled ? t("Components") : t("Setup");
    case "settings":
      return t("Settings");
    case "about":
      return t("About");
    default:
      return "";
  }
});

// The first launch's welcome has its own title, and nowhere to go back to.
const welcoming = computed(() => store.page === "setup" && store.snap?.setup.welcoming);
const canGoBack = computed(() => store.page !== "download" && canOpen("download"));

// A text field drops line breaks from what is pasted, which would glue
// several links into one. They are separated by spaces instead.
function paste(event: ClipboardEvent) {
  const text = event.clipboardData?.getData("text");
  if (!text || !/[\r\n]/.test(text)) return;
  event.preventDefault();
  const field = event.target as HTMLInputElement;
  const [from, to] = [field.selectionStart ?? field.value.length, field.selectionEnd ?? field.value.length];
  const flat = text.trim().replace(/\s+/g, " ");
  store.input = field.value.slice(0, from) + flat + field.value.slice(to);
}
</script>

<template>
  <header class="header">
    <div class="side">
      <!-- Shown only where it leads somewhere, rather than greyed out. -->
      <button
        v-if="canGoBack"
        class="icon-btn"
        :title="t('Back')"
        :aria-label="t('Back')"
        @click="navigate('download')"
      >
        <ChevronLeft />
      </button>
    </div>

    <!-- As wide as the page's content below it, so the two line up. -->
    <div class="center">
      <form v-if="store.page === 'download'" class="link" @submit.prevent="addLinks">
        <label class="link-field">
          <Link2 />
          <input
            v-model="store.input"
            type="text"
            spellcheck="false"
            autocomplete="off"
            :placeholder="t('Paste a video or playlist link...')"
            :aria-label="t('Paste a video or playlist link...')"
            @paste="paste"
          />
        </label>
        <button type="submit" class="btn btn-primary"><Plus /> {{ t("Add") }}</button>
      </form>
      <h1 v-else-if="!welcoming" class="title">{{ title }}</h1>
    </div>

    <div class="side end">
      <button
        class="icon-btn"
        :class="{ active: menuOpen }"
        :title="t('Menu')"
        :aria-label="t('Menu')"
        aria-haspopup="menu"
        :aria-expanded="menuOpen"
        @click="menuOpen = !menuOpen"
      >
        <Menu />
      </button>
    </div>
    <MainMenu v-if="menuOpen" @close="menuOpen = false" />
  </header>
</template>

<style scoped>
.header {
  position: relative;
  z-index: 20;
  display: flex;
  align-items: center;
  flex: none;
  height: var(--header-height);
  padding: 0 12px;
  border-bottom: 1px solid var(--border);
  background: var(--header);
  backdrop-filter: blur(16px) saturate(1.4);
}
.side {
  display: flex;
  flex: none;
  width: 44px;
}
.side.end {
  justify-content: flex-end;
}
.center {
  flex: 1;
  min-width: 0;
  max-width: calc(var(--page-width) - 48px);
  margin: 0 auto;
  padding: 0 12px;
}
.title {
  font-size: 16px;
  font-weight: 650;
  text-align: center;
}
.link {
  display: flex;
  align-items: center;
  gap: 10px;
}
.link-field {
  display: flex;
  align-items: center;
  flex: 1;
  gap: 10px;
  min-width: 0;
  height: 38px;
  padding: 0 14px;
  border: 1px solid var(--border);
  border-radius: 999px;
  background: var(--surface-2);
  cursor: text;
  transition:
    border-color 0.15s var(--ease),
    box-shadow 0.15s var(--ease);
}
.link-field:hover {
  border-color: var(--border-strong);
}
.link-field:focus-within {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-soft);
}
.link-field svg {
  width: 16px;
  height: 16px;
  flex: none;
  color: var(--faint);
}
.link-field input {
  flex: 1;
  min-width: 0;
  border: none;
  outline: none;
  background: transparent;
  user-select: text;
}
.link-field input::placeholder {
  color: var(--faint);
}
</style>
