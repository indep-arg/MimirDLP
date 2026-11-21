<script setup lang="ts">
// The header bar's menu: the theme, the language and the screens besides
// Download.
import { computed, onBeforeUnmount, onMounted } from "vue";
import { Info, Monitor, Moon, Settings, Sun, Wrench } from "@lucide/vue";
import { api } from "../lib/api";
import { t } from "../lib/i18n";
import { canOpen, navigate, setLanguage, setting, store, type Page } from "../lib/store";
import UiSelect from "./UiSelect.vue";

const emit = defineEmits<{ close: [] }>();

const themes = computed(() => [
  { key: "system", label: t("System"), icon: Monitor },
  { key: "light", label: t("Light"), icon: Sun },
  { key: "dark", label: t("Dark"), icon: Moon },
]);

// Each translation by its own name, so it can be found by someone who
// can't read the current one.
const languages = computed(() => [
  { key: "", label: t("System language") },
  ...(store.fixed?.languages ?? []),
]);

function go(page: Page) {
  navigate(page);
  emit("close");
}

function escape(event: KeyboardEvent) {
  if (event.key === "Escape") emit("close");
}
onMounted(() => window.addEventListener("keydown", escape));
onBeforeUnmount(() => window.removeEventListener("keydown", escape));
</script>

<template>
  <!-- A click anywhere outside the menu closes it, and reaches nothing
       underneath. -->
  <!-- On the body rather than in the header bar: the bar's blur would
       otherwise make this layer only as large as the bar itself. -->
  <Teleport to="body">
    <div class="menu-layer" @mousedown.self="emit('close')">
    <div class="menu" role="menu">
      <!-- The menu stays open, so the other choices can be tried. -->
      <div class="themes" role="radiogroup" :aria-label="t('Theme')">
        <button
          v-for="theme in themes"
          :key="theme.key"
          class="theme"
          :class="{ chosen: setting('theme') === theme.key }"
          role="radio"
          :aria-checked="setting('theme') === theme.key"
          @click="api.setSetting('theme', theme.key)"
        >
          <component :is="theme.icon" />
          {{ theme.label }}
        </button>
      </div>
      <div class="language">
        <UiSelect
          :model-value="setting('language')"
          :options="languages"
          :label="t('Language')"
          @update:model-value="setLanguage"
        />
      </div>
      <hr />
      <button class="entry" role="menuitem" @click="go('settings')">
        <Settings /> {{ t("Settings") }}
      </button>
      <button class="entry" role="menuitem" :disabled="!canOpen('setup')" @click="go('setup')">
        <Wrench />
        {{ store.snap?.setup.bundled ? t("Components") : t("Setup and updates") }}
      </button>
      <button class="entry" role="menuitem" @click="go('about')">
        <Info /> {{ t("About Osprey") }}
      </button>
    </div>
    </div>
  </Teleport>
</template>

<style scoped>
.menu-layer {
  position: fixed;
  inset: 0;
  z-index: 50;
}
.menu {
  position: absolute;
  top: calc(var(--header-height) - 6px);
  right: 12px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  width: 290px;
  padding: 8px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-l);
  background: var(--surface);
  box-shadow: var(--shadow-pop);
  animation: drop 0.16s var(--ease);
}
@keyframes drop {
  from {
    opacity: 0;
    transform: translateY(-6px) scale(0.98);
  }
}
.themes {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 4px;
  padding: 4px;
  border-radius: var(--radius-m);
  background: var(--surface-2);
}
.theme {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  padding: 8px 4px;
  border: none;
  border-radius: 9px;
  background: transparent;
  color: var(--muted);
  font-size: 12.5px;
  cursor: pointer;
  transition:
    background 0.15s var(--ease),
    color 0.15s var(--ease);
}
.theme svg {
  width: 16px;
  height: 16px;
}
.theme:hover {
  color: var(--text);
}
.theme.chosen {
  background: var(--surface);
  color: var(--text);
  font-weight: 600;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.18);
}
.language {
  padding: 4px 0;
}
hr {
  width: 100%;
  margin: 2px 0;
  border: none;
  border-top: 1px solid var(--border);
}
.entry {
  display: flex;
  align-items: center;
  gap: 12px;
  width: 100%;
  padding: 10px 12px;
  border: none;
  border-radius: 10px;
  background: transparent;
  text-align: left;
  cursor: pointer;
  transition: background 0.12s var(--ease);
}
.entry svg {
  width: 17px;
  height: 17px;
  color: var(--muted);
}
.entry:hover:not(:disabled) {
  background: var(--surface-2);
}
.entry:disabled {
  opacity: 0.45;
  cursor: default;
}
</style>
