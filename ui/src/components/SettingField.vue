<script setup lang="ts">
// A setting that is typed. What is typed is saved a moment after the last
// key, and at once when the field is left or Enter is pressed; while the
// field has the focus, nothing from the app overwrites it.
import { onBeforeUnmount, ref, watch } from "vue";
import { api } from "../lib/api";
import { setting } from "../lib/store";

const props = defineProps<{
  /** The setting's key in the settings file. */
  name: string;
  placeholder?: string;
  label?: string;
  secret?: boolean;
  mono?: boolean;
}>();

// The one value the app never sends back: a secret starts empty.
const value = ref(props.secret ? "" : setting(props.name));
const focused = ref(false);
let saved = value.value;
let timer: number | undefined;

watch(
  () => setting(props.name),
  (current) => {
    if (props.secret || focused.value) return;
    value.value = current;
    saved = current;
  },
);

function save() {
  window.clearTimeout(timer);
  if (value.value === saved) return;
  saved = value.value;
  void api.setSetting(props.name, value.value);
}

function typed() {
  window.clearTimeout(timer);
  timer = window.setTimeout(save, 600);
}

function blur() {
  focused.value = false;
  save();
}

onBeforeUnmount(save);
</script>

<template>
  <input
    v-model="value"
    class="field"
    :class="{ mono }"
    :type="secret ? 'password' : 'text'"
    spellcheck="false"
    autocomplete="off"
    :placeholder="placeholder"
    :aria-label="label"
    @input="typed"
    @focus="focused = true"
    @blur="blur"
    @keydown.enter="save"
  />
</template>

<style scoped>
.mono {
  font-family: var(--mono);
  font-size: 12.5px;
}
</style>
