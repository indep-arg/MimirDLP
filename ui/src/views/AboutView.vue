<script setup lang="ts">
// The versions a bug report asks for, and where to find the project or
// report a problem.
import { Bug, Globe } from "@lucide/vue";
import { api, type Snapshot } from "../lib/api";
import { t } from "../lib/i18n";
import { store } from "../lib/store";
import logo from "../assets/logo.svg";

defineProps<{ snap: Snapshot }>();
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
      <button class="btn" @click="api.openLink('website')"><Globe /> {{ t("Website") }}</button>
      <button class="btn" @click="api.openLink('report')">
        <Bug /> {{ t("Report a problem") }}
      </button>
    </div>
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
  filter: drop-shadow(0 12px 28px rgba(47, 107, 234, 0.35));
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
