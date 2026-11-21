<script setup lang="ts">
// The versions a bug report asks for, and where to find the project or
// report a problem.
import { ref } from "vue";
import { Bug, Globe } from "@lucide/vue";
import { api, type Snapshot } from "../lib/api";
import { t } from "../lib/i18n";
import { store } from "../lib/store";
import logo from "../assets/logo.svg";

defineProps<{ snap: Snapshot }>();

// Said here when no browser opened the page, with its address to copy.
const linkProblem = ref<string | null>(null);
async function open(which: "website" | "report") {
  linkProblem.value = await api.openLink(which);
}
</script>

<template>
  <div class="about page">
    <img class="logo" :src="logo" alt="" draggable="false" />
    <h1>Osprey</h1>
    <div class="versions">
      <p class="version selectable">
        {{ t("Version {version}", { version: store.fixed?.version ?? "" }) }}
      </p>
      <p v-if="snap.setup.ytdlpVersion" class="muted selectable">
        {{ t("Using yt-dlp {version}", { version: snap.setup.ytdlpVersion }) }}
      </p>
    </div>
    <p>{{ t("Download videos and music from YouTube and many other sites.") }}</p>
    <div class="links">
      <button class="btn" @click="open('website')"><Globe /> {{ t("Website") }}</button>
      <button class="btn" @click="open('report')">
        <Bug /> {{ t("Report a problem") }}
      </button>
    </div>
    <p v-if="linkProblem" class="caption danger-text selectable">{{ linkProblem }}</p>
    <p class="caption muted">{{ t("Free software under the AGPL-3.0 license.") }}</p>
  </div>
</template>

<style scoped>
.about {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 18px;
  min-height: 100%;
  text-align: center;
}
.logo {
  width: 104px;
  height: 104px;
}
h1 {
  font-size: 30px;
  font-weight: 700;
  letter-spacing: -0.02em;
}
.versions {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.version {
  font-size: 16px;
}
.links {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 10px;
}
</style>
