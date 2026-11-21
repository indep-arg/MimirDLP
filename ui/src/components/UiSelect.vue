<script setup lang="ts">
// A dropdown: the one every choice in the app is made with, so they all
// look and behave alike. The list is placed over the page rather than
// inside its card, so a card near the bottom can't cut it off.
import { computed, nextTick, onBeforeUnmount, ref } from "vue";
import { Check, ChevronDown } from "@lucide/vue";
import type { Choice } from "../lib/api";

const props = defineProps<{
  modelValue: string;
  options: Choice[];
  small?: boolean;
  disabled?: boolean;
  label?: string;
}>();
const emit = defineEmits<{ "update:modelValue": [value: string] }>();

const open = ref(false);
const active = ref(0);
const button = ref<HTMLButtonElement>();
const list = ref<HTMLElement>();
const place = ref({ left: "0px", top: "0px", minWidth: "0px", maxHeight: "0px" });

const current = computed(() => props.options.find((o) => o.key === props.modelValue));

function position() {
  const box = button.value?.getBoundingClientRect();
  if (!box) return;
  const gap = 6;
  const wanted = Math.min(props.options.length * 36 + 12, 320);
  const below = window.innerHeight - box.bottom - gap - 8;
  const above = box.top - gap - 8;
  // Below when it fits, or when there is more room there anyway.
  const down = below >= wanted || below >= above;
  const height = Math.max(120, Math.min(wanted, down ? below : above));
  place.value = {
    left: `${Math.max(8, Math.min(box.left, window.innerWidth - box.width - 8))}px`,
    top: down ? `${box.bottom + gap}px` : `${box.top - gap - height}px`,
    minWidth: `${box.width}px`,
    maxHeight: `${height}px`,
  };
}

function close() {
  if (!open.value) return;
  open.value = false;
  window.removeEventListener("resize", close);
  window.removeEventListener("scroll", close, true);
  window.removeEventListener("blur", close);
}

async function show() {
  if (props.disabled) return;
  active.value = Math.max(
    0,
    props.options.findIndex((o) => o.key === props.modelValue),
  );
  position();
  open.value = true;
  // Whatever moves the button leaves the list behind: close instead.
  window.addEventListener("resize", close);
  window.addEventListener("scroll", close, true);
  window.addEventListener("blur", close);
  await nextTick();
  list.value?.querySelector<HTMLElement>(".active")?.scrollIntoView({ block: "nearest" });
}

function pick(option: Choice) {
  close();
  button.value?.focus();
  if (option.key !== props.modelValue) emit("update:modelValue", option.key);
}

function step(by: number) {
  const count = props.options.length;
  active.value = (active.value + by + count) % count;
  void nextTick(() =>
    list.value?.querySelector<HTMLElement>(".active")?.scrollIntoView({ block: "nearest" }),
  );
}

function key(event: KeyboardEvent) {
  if (!open.value) {
    if (["ArrowDown", "ArrowUp", "Enter", " "].includes(event.key)) {
      event.preventDefault();
      void show();
    }
    return;
  }
  switch (event.key) {
    case "ArrowDown":
      event.preventDefault();
      step(1);
      break;
    case "ArrowUp":
      event.preventDefault();
      step(-1);
      break;
    case "Home":
      event.preventDefault();
      active.value = 0;
      break;
    case "End":
      event.preventDefault();
      active.value = props.options.length - 1;
      break;
    case "Enter":
    case " ":
      event.preventDefault();
      pick(props.options[active.value]);
      break;
    case "Escape":
    case "Tab":
      close();
      break;
  }
}

onBeforeUnmount(close);
</script>

<template>
  <button
    ref="button"
    type="button"
    class="select"
    :class="{ small, open }"
    :disabled="disabled"
    role="combobox"
    aria-haspopup="listbox"
    :aria-expanded="open"
    :aria-label="label"
    @click="open ? close() : show()"
    @keydown="key"
  >
    <span class="value">{{ current?.label ?? modelValue }}</span>
    <ChevronDown class="arrow" />
  </button>
  <Teleport to="body">
    <Transition name="pop">
      <div v-if="open" class="select-layer" @mousedown.self="close">
        <div ref="list" class="select-list" :style="place" role="listbox">
          <div
            v-for="(option, index) in options"
            :key="option.key"
            class="option"
            :class="{ active: index === active, chosen: option.key === modelValue }"
            role="option"
            :aria-selected="option.key === modelValue"
            @mouseenter="active = index"
            @click="pick(option)"
          >
            <span>{{ option.label }}</span>
            <Check v-if="option.key === modelValue" class="tick" />
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.select {
  display: inline-flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  width: 100%;
  height: 36px;
  padding: 0 10px 0 12px;
  border: 1px solid var(--border);
  border-radius: var(--radius-s);
  background: var(--surface-2);
  text-align: left;
  cursor: pointer;
  transition:
    border-color 0.15s var(--ease),
    box-shadow 0.15s var(--ease),
    background 0.15s var(--ease);
}
.select:hover:not(:disabled) {
  border-color: var(--border-strong);
}
.select.open {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-soft);
}
.select:disabled {
  opacity: 0.5;
  cursor: default;
}
.select.small {
  width: auto;
  height: 28px;
  padding: 0 6px 0 10px;
  gap: 6px;
  font-size: 12.5px;
}
.value {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.arrow {
  width: 16px;
  height: 16px;
  flex: none;
  color: var(--muted);
  transition: transform 0.18s var(--ease);
}
.open .arrow {
  transform: rotate(180deg);
}
</style>

<style>
/* Outside the component's own element (it is teleported to the body). */
.select-layer {
  position: fixed;
  inset: 0;
  z-index: 100;
}
.select-list {
  position: fixed;
  overflow-y: auto;
  padding: 6px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-m);
  background: var(--surface);
  box-shadow: var(--shadow-pop);
}
.select-list .option {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  min-height: 34px;
  padding: 6px 10px;
  border-radius: 8px;
  white-space: nowrap;
  cursor: pointer;
}
.select-list .option.active {
  background: var(--surface-2);
}
.select-list .option.chosen {
  color: var(--accent);
  font-weight: 600;
}
.select-list .tick {
  width: 16px;
  height: 16px;
  flex: none;
}
.pop-enter-active {
  transition: opacity 0.12s ease;
}
.pop-enter-active .select-list,
.pop-enter-active .menu {
  transition:
    transform 0.14s cubic-bezier(0.2, 0.7, 0.2, 1),
    opacity 0.12s ease;
}
.pop-leave-active {
  transition: opacity 0.1s ease;
}
.pop-enter-from,
.pop-leave-to {
  opacity: 0;
}
.pop-enter-from .select-list,
.pop-enter-from .menu {
  transform: translateY(-4px) scale(0.98);
}
</style>
