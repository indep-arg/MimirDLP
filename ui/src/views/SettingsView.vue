<script setup lang="ts">
// The Settings screen. Every row edits one setting by the key it has in
// the settings file; what can't apply right now isn't shown at all.
import { computed, ref, watch } from "vue";
import { ChevronDown } from "@lucide/vue";
import { api, type Snapshot } from "../lib/api";
import { t, tn } from "../lib/i18n";
import { choices, isOn, setting, store } from "../lib/store";
import SettingField from "../components/SettingField.vue";
import UiSelect from "../components/UiSelect.vue";
import UiSwitch from "../components/UiSwitch.vue";

const props = defineProps<{ snap: Snapshot }>();

const set = (key: string, value: string) => api.setSetting(key, value);
const toggle = (key: string) => set(key, isOn(key) ? "no" : "yes");

const fixed = computed(() => store.fixed);
const is = (key: string, value: string) => setting(key) === value;

// yt-dlp's --audio-quality scale.
const audioQualities = Array.from({ length: 11 }, (_, n) => ({ key: String(n), label: String(n) }));
// Lossless formats ignore the quality scale, so it isn't asked about.
const lossless = computed(() => ["flac", "alac", "wav"].includes(setting("audio_format")));

// The less common sections are folded away until asked for. Not saved:
// every launch starts with the short page.
const showAdvanced = ref(store.dev.advanced);

// The history's own entries, shown one by one while this is a list, and
// the two step way to throw it away.
const historyEntries = ref<string[] | null>(null);
const confirmingClear = ref(false);

async function toggleEntries() {
  historyEntries.value = historyEntries.value ? null : ((await api.historyLines()) ?? []);
}
async function clearHistory() {
  confirmingClear.value = false;
  historyEntries.value = null;
  await api.clearHistory();
}
// A finished download may have added to the history.
watch(
  () => props.snap.settings.history,
  async () => {
    if (historyEntries.value) historyEntries.value = (await api.historyLines()) ?? [];
  },
);

const historySummary = computed(() => {
  const count = props.snap.settings.history;
  return count === null
    ? t("The history file doesn't exist yet.")
    : tn("{n} video in the history", "{n} videos in the history", count);
});
</script>

<template>
  <div class="page stack settings">
    <p class="muted">
      {{
        t(
          "These apply to every link you queue from now on. Links already in the queue keep what they were queued with.",
        )
      }}
    </p>

    <section>
      <h2 class="section-title">{{ t("What to download") }}</h2>
      <div class="card rows">
        <!-- The same dropdown as on a queued link's card. -->
        <div class="row">
          <span class="row-label">{{ t("Download") }}</span>
          <div class="control">
            <UiSelect
              :model-value="setting('kind')"
              :options="choices('kind')"
              :label="t('Download')"
              @update:model-value="set('kind', $event)"
            />
          </div>
        </div>

        <template v-if="is('kind', 'video')">
          <div class="row">
            <span class="row-label">{{ t("Format") }}</span>
            <div class="control">
              <UiSelect
                :model-value="setting('video_format')"
                :options="choices('video_format')"
                :label="t('Format')"
                @update:model-value="set('video_format', $event)"
              />
            </div>
          </div>
          <div v-if="is('video_format', 'custom')" class="row">
            <span class="row-label">{{ t("Format string") }}</span>
            <div class="control wide">
              <SettingField
                name="custom_format"
                mono
                placeholder="bestvideo+bestaudio"
                :label="t('Format string')"
              />
            </div>
          </div>
          <div class="row">
            <span class="row-label">{{ t("Quality") }}</span>
            <div class="control">
              <UiSelect
                :model-value="setting('quality')"
                :options="choices('quality')"
                :label="t('Quality')"
                @update:model-value="set('quality', $event)"
              />
            </div>
          </div>
          <div class="row">
            <span class="row-label">{{ t("Container") }}</span>
            <div class="control">
              <UiSelect
                :model-value="setting('container')"
                :options="choices('container')"
                :label="t('Container')"
                @update:model-value="set('container', $event)"
              />
            </div>
          </div>
          <div class="row">
            <span class="row-label">{{ t("Convert the container") }}</span>
            <div class="control">
              <UiSelect
                :model-value="setting('convert')"
                :options="choices('convert')"
                :label="t('Convert the container')"
                @update:model-value="set('convert', $event)"
              />
            </div>
          </div>
          <div v-if="!is('convert', 'keep')" class="row">
            <span class="row-label">{{ t("Convert to") }}</span>
            <div class="control narrow">
              <SettingField name="convert_target" placeholder="mp4" :label="t('Convert to')" />
            </div>
          </div>
        </template>

        <template v-else>
          <div class="row clickable" @click="toggle('audio_convert')">
            <span class="row-label">{{ t("Convert it to another format") }}</span>
            <UiSwitch
              :model-value="isOn('audio_convert')"
              :label="t('Convert it to another format')"
              @click.stop
              @update:model-value="toggle('audio_convert')"
            />
          </div>
          <template v-if="isOn('audio_convert')">
            <div class="row">
              <span class="row-label">{{ t("Audio format") }}</span>
              <div class="control">
                <UiSelect
                  :model-value="setting('audio_format')"
                  :options="choices('audio_format')"
                  :label="t('Audio format')"
                  @update:model-value="set('audio_format', $event)"
                />
              </div>
            </div>
            <div v-if="!lossless" class="row">
              <span class="row-label">{{ t("Audio quality (0 best, 10 smallest)") }}</span>
              <div class="control narrow">
                <UiSelect
                  :model-value="setting('audio_quality')"
                  :options="audioQualities"
                  :label="t('Audio quality (0 best, 10 smallest)')"
                  @update:model-value="set('audio_quality', $event)"
                />
              </div>
            </div>
          </template>
          <p v-else class="row-note">
            {{ t("The audio is kept exactly as the site serves it.") }}
          </p>
        </template>
      </div>
    </section>

    <section>
      <h2 class="section-title">{{ t("Where to save") }}</h2>
      <div class="card">
        <div class="row">
          <SettingField
            name="output_dir"
            :placeholder="fixed?.defaultFolder"
            :label="t('Where to save')"
          />
          <button class="btn" @click="api.chooseFolder()">{{ t("Choose...") }}</button>
        </div>
        <p class="row-note flush">
          {{
            fixed?.installed
              ? t("Leave it empty to use your Downloads folder.")
              : t("Leave it empty to use the downloads folder next to this app.")
          }}
        </p>
      </div>
    </section>

    <section>
      <h2 class="section-title">{{ t("Extras") }}</h2>
      <div class="card rows">
        <div class="row clickable" @click="toggle('thumbnail')">
          <span class="row-label">{{ t("Embed the thumbnail") }}</span>
          <UiSwitch
            :model-value="isOn('thumbnail')"
            :label="t('Embed the thumbnail')"
            @click.stop
            @update:model-value="toggle('thumbnail')"
          />
        </div>
        <div v-if="isOn('thumbnail')" class="row">
          <span class="row-label">{{ t("Thumbnail format") }}</span>
          <div class="control">
            <UiSelect
              :model-value="setting('thumbnail_format')"
              :options="choices('thumbnail_format')"
              :label="t('Thumbnail format')"
              @update:model-value="set('thumbnail_format', $event)"
            />
          </div>
        </div>
        <div class="row clickable" @click="toggle('metadata')">
          <span class="row-label">{{ t("Embed title, artist and date") }}</span>
          <UiSwitch
            :model-value="isOn('metadata')"
            :label="t('Embed title, artist and date')"
            @click.stop
            @update:model-value="toggle('metadata')"
          />
        </div>
        <div class="row clickable" @click="toggle('chapters')">
          <span class="row-label">{{ t("Embed chapters") }}</span>
          <UiSwitch
            :model-value="isOn('chapters')"
            :label="t('Embed chapters')"
            @click.stop
            @update:model-value="toggle('chapters')"
          />
        </div>
        <div class="row">
          <span class="row-label">{{ t("Subtitles") }}</span>
          <div class="control">
            <UiSelect
              :model-value="setting('subtitles')"
              :options="choices('subtitles')"
              :label="t('Subtitles')"
              @update:model-value="set('subtitles', $event)"
            />
          </div>
        </div>
        <template v-if="!is('subtitles', 'off')">
          <div class="row">
            <span class="row-label">{{ t("Languages") }}</span>
            <div class="control">
              <SettingField
                name="subtitle_langs"
                :placeholder="t('all but live chat')"
                :label="t('Languages')"
              />
            </div>
          </div>
          <div class="row clickable" @click="toggle('subtitle_auto')">
            <span class="row-label">{{ t("Include auto-generated subtitles") }}</span>
            <UiSwitch
              :model-value="isOn('subtitle_auto')"
              :label="t('Include auto-generated subtitles')"
              @click.stop
              @update:model-value="toggle('subtitle_auto')"
            />
          </div>
          <p v-if="snap.settings.autoSubsProblem" class="row-note warning-text">
            {{ snap.settings.autoSubsProblem }}
          </p>
        </template>
        <div class="row">
          <!-- A name, the same in every language. -->
          <span class="row-label">SponsorBlock</span>
          <div class="control">
            <UiSelect
              :model-value="setting('sponsorblock')"
              :options="choices('sponsorblock')"
              label="SponsorBlock"
              @update:model-value="set('sponsorblock', $event)"
            />
          </div>
        </div>
        <template v-if="!is('sponsorblock', 'off')">
          <div class="row">
            <span class="row-label">{{ t("Segments") }}</span>
            <div class="control">
              <UiSelect
                :model-value="setting('sponsorblock_cats')"
                :options="choices('sponsorblock_cats')"
                :label="t('Segments')"
                @update:model-value="set('sponsorblock_cats', $event)"
              />
            </div>
          </div>
          <div v-if="is('sponsorblock_cats', 'custom')" class="row">
            <span class="row-label">{{ t("Categories") }}</span>
            <div class="control">
              <SettingField
                name="sponsorblock_custom"
                mono
                placeholder="sponsor,intro,outro"
                :label="t('Categories')"
              />
            </div>
          </div>
        </template>
        <div
          v-if="is('sponsorblock', 'remove')"
          class="row clickable"
          @click="toggle('sponsorblock_keyframes')"
        >
          <span class="row-label">
            {{ t("Cut exactly at keyframes (re-encodes, much slower)") }}
          </span>
          <UiSwitch
            :model-value="isOn('sponsorblock_keyframes')"
            :label="t('Cut exactly at keyframes (re-encodes, much slower)')"
            @click.stop
            @update:model-value="toggle('sponsorblock_keyframes')"
          />
        </div>
      </div>
    </section>

    <!-- Only Linux desktops build their menus from .desktop files, and an
         installed copy came with its own. -->
    <section v-if="fixed?.linux && !fixed.installed">
      <h2 class="section-title">{{ t("Desktop") }}</h2>
      <div class="card">
        <div class="row clickable" @click="api.setDesktopEntry(!snap.settings.desktopEntry)">
          <span class="row-label">{{ t("Show Osprey in the applications menu") }}</span>
          <UiSwitch
            :model-value="snap.settings.desktopEntry"
            :label="t('Show Osprey in the applications menu')"
            @click.stop
            @update:model-value="api.setDesktopEntry($event)"
          />
        </div>
        <p class="row-note flush">
          {{
            t(
              "Adds a launcher and the icon to your user folder, so Osprey can be started from the menu and shows its icon in the taskbar. Turning it off removes them.",
            )
          }}
        </p>
      </div>
    </section>

    <!-- The Flatpak is updated by Flathub, so it never looks for itself. -->
    <section v-if="!fixed?.bundled">
      <h2 class="section-title">{{ t("Updates") }}</h2>
      <div class="card">
        <div class="row clickable" @click="toggle('app_updates')">
          <span class="row-label">{{ t("Tell me when a new version of Osprey is out") }}</span>
          <UiSwitch
            :model-value="isOn('app_updates')"
            :label="t('Tell me when a new version of Osprey is out')"
            @click.stop
            @update:model-value="toggle('app_updates')"
          />
        </div>
        <p class="row-note flush">
          {{
            t(
              "Looks at Osprey's release page on GitHub once a day, when the app starts. Nothing about you or your downloads is sent, and nothing is installed by itself.",
            )
          }}
        </p>
      </div>
    </section>

    <button
      class="card expander"
      :class="{ open: showAdvanced }"
      :aria-expanded="showAdvanced"
      @click="showAdvanced = !showAdvanced"
    >
      <span class="row-label">
        <span class="expander-title">
          {{ showAdvanced ? t("Hide advanced options") : t("Advanced options") }}
        </span>
        <span class="caption muted">
          {{ t("Signing in, playlists, history, file names, speed limits") }}
        </span>
      </span>
      <ChevronDown />
    </button>

    <template v-if="showAdvanced">
      <section>
        <h2 class="section-title">{{ t("Signing in") }}</h2>
        <div class="card rows">
          <div>
            <div class="row">
              <span class="row-label">{{ t("Video password") }}</span>
              <div class="control">
                <SettingField
                  name="video_password"
                  secret
                  :placeholder="t('none')"
                  :label="t('Video password')"
                />
              </div>
            </div>
            <p class="row-note flush">
              {{
                t(
                  "For videos the site protects with a password, such as on Vimeo. It is used for the links you add next and is never saved: it is forgotten when the app closes.",
                )
              }}
            </p>
          </div>
          <!-- Browser cookies are left out of the Flatpak: its sandbox
               can't read the browsers' folders. -->
          <template v-if="!fixed?.bundled">
            <div class="row">
              <span class="row-label">{{ t("Use cookies from") }}</span>
              <div class="control">
                <UiSelect
                  :model-value="setting('cookies_browser')"
                  :options="choices('cookies_browser')"
                  :label="t('Use cookies from')"
                  @update:model-value="set('cookies_browser', $event)"
                />
              </div>
            </div>
            <div v-if="!is('cookies_browser', 'none')">
              <div class="row">
                <span class="row-label">{{ t("Profile (empty for the last used)") }}</span>
                <div class="control">
                  <SettingField
                    name="cookies_profile"
                    :placeholder="t('default')"
                    :label="t('Profile (empty for the last used)')"
                  />
                </div>
              </div>
              <p class="row-note flush">
                {{
                  t(
                    "Only for videos that need you signed in. Firefox works best: Chrome, Edge and other Chromium browsers may have to be closed first, and recent versions on Windows can't be read at all.",
                  )
                }}
              </p>
            </div>
          </template>
        </div>
      </section>

      <section>
        <h2 class="section-title">{{ t("Automation and naming") }}</h2>
        <div class="card rows">
          <div class="row">
            <span class="row-label">{{ t("Playlists") }}</span>
            <div class="control">
              <UiSelect
                :model-value="setting('playlist')"
                :options="choices('playlist')"
                :label="t('Playlists')"
                @update:model-value="set('playlist', $event)"
              />
            </div>
          </div>
          <template v-if="is('playlist', 'whole')">
            <div class="row">
              <span class="row-label">{{ t("Items (empty for all)") }}</span>
              <div class="control narrow">
                <SettingField
                  name="playlist_items"
                  placeholder="1-5,8"
                  :label="t('Items (empty for all)')"
                />
              </div>
            </div>
            <div class="row clickable" @click="toggle('playlist_reverse')">
              <span class="row-label">{{ t("Start from the end of the playlist") }}</span>
              <UiSwitch
                :model-value="isOn('playlist_reverse')"
                :label="t('Start from the end of the playlist')"
                @click.stop
                @update:model-value="toggle('playlist_reverse')"
              />
            </div>
            <div class="row clickable" @click="toggle('organize')">
              <span class="row-label">{{ t("One folder per playlist, numbered") }}</span>
              <UiSwitch
                :model-value="isOn('organize')"
                :label="t('One folder per playlist, numbered')"
                @click.stop
                @update:model-value="toggle('organize')"
              />
            </div>
          </template>

          <div class="row clickable" @click="toggle('archive')">
            <span class="row-label">
              {{ t("Keep a history and skip what was downloaded before") }}
            </span>
            <UiSwitch
              :model-value="isOn('archive')"
              :label="t('Keep a history and skip what was downloaded before')"
              @click.stop
              @update:model-value="toggle('archive')"
            />
          </div>
          <template v-if="isOn('archive')">
            <div class="row">
              <span class="row-label">{{ t("History file") }}</span>
              <div class="control">
                <SettingField
                  name="archive_file"
                  placeholder="download_archive.txt"
                  :label="t('History file')"
                />
              </div>
            </div>
            <div class="row clickable" @click="toggle('archive_stop')">
              <span class="row-label">
                {{ t("Stop as soon as a video is already in the history") }}
              </span>
              <UiSwitch
                :model-value="isOn('archive_stop')"
                :label="t('Stop as soon as a video is already in the history')"
                @click.stop
                @update:model-value="toggle('archive_stop')"
              />
            </div>
            <!-- What the history holds, and a two step way to throw it away. -->
            <div>
              <div v-if="confirmingClear" class="row">
                <span class="row-label warning-text">
                  {{ t("This deletes the history file for good.") }}
                </span>
                <button class="btn btn-small" @click="confirmingClear = false">
                  {{ t("Cancel") }}
                </button>
                <button class="btn btn-small btn-danger" @click="clearHistory">
                  {{ t("Clear") }}
                </button>
              </div>
              <div v-else class="row">
                <span class="row-label caption muted">{{ historySummary }}</span>
                <button
                  class="btn btn-small"
                  :disabled="snap.settings.history === null"
                  @click="toggleEntries"
                >
                  {{ historyEntries ? t("Hide entries") : t("View entries") }}
                </button>
                <button
                  class="btn btn-small"
                  :disabled="snap.settings.history === null"
                  @click="confirmingClear = true"
                >
                  {{ t("Clear history") }}
                </button>
              </div>
              <!-- Height capped, so a long history doesn't push the rest
                   of the card off the screen. -->
              <div v-if="historyEntries" class="entries selectable">
                <p v-if="historyEntries.length === 0" class="muted">
                  {{ t("The history is empty.") }}
                </p>
                <div v-for="(entry, index) in historyEntries" :key="index">{{ entry }}</div>
              </div>
            </div>
          </template>

          <div class="row">
            <span class="row-label">{{ t("Stop after this many files") }}</span>
            <div class="control narrow">
              <SettingField
                name="max_downloads"
                :placeholder="t('no limit')"
                :label="t('Stop after this many files')"
              />
            </div>
          </div>
          <div class="row">
            <span class="row-label">{{ t("File names") }}</span>
            <div class="control">
              <UiSelect
                :model-value="setting('template')"
                :options="choices('template')"
                :label="t('File names')"
                @update:model-value="set('template', $event)"
              />
            </div>
          </div>
          <div v-if="is('template', 'custom')" class="row">
            <span class="row-label">{{ t("Template") }}</span>
            <div class="control wide">
              <SettingField
                name="custom_template"
                mono
                placeholder="%(title)s [%(id)s].%(ext)s"
                :label="t('Template')"
              />
            </div>
          </div>
          <div class="row clickable" @click="toggle('restrict_filenames')">
            <span class="row-label">{{ t("Plain ASCII file names") }}</span>
            <UiSwitch
              :model-value="isOn('restrict_filenames')"
              :label="t('Plain ASCII file names')"
              @click.stop
              @update:model-value="toggle('restrict_filenames')"
            />
          </div>
        </div>
      </section>

      <section>
        <h2 class="section-title">{{ t("Network and other") }}</h2>
        <div class="card rows">
          <div class="row">
            <span class="row-label">{{ t("Speed limit") }}</span>
            <div class="control narrow">
              <SettingField
                name="limit_rate"
                :placeholder="t('no limit')"
                :label="t('Speed limit')"
              />
            </div>
          </div>
          <div class="row">
            <span class="row-label">{{ t("Parts downloaded at once") }}</span>
            <div class="control narrow">
              <SettingField
                name="fragments"
                placeholder="5"
                :label="t('Parts downloaded at once')"
              />
            </div>
          </div>
          <div class="row">
            <span class="row-label">{{ t("Seconds between requests") }}</span>
            <div class="control narrow">
              <SettingField name="sleep" placeholder="1.5" :label="t('Seconds between requests')" />
            </div>
          </div>
          <div class="row">
            <span class="row-label">{{ t("Retries after a network error") }}</span>
            <div class="control narrow">
              <SettingField
                name="retries"
                placeholder="10"
                :label="t('Retries after a network error')"
              />
            </div>
          </div>
          <div class="row clickable" @click="toggle('live_from_start')">
            <span class="row-label">{{ t("Record live streams from the start") }}</span>
            <UiSwitch
              :model-value="isOn('live_from_start')"
              :label="t('Record live streams from the start')"
              @click.stop
              @update:model-value="toggle('live_from_start')"
            />
          </div>
          <div class="row">
            <span class="row-label">{{ t("Wait for a scheduled stream (seconds)") }}</span>
            <div class="control narrow">
              <SettingField
                name="wait_for_video"
                placeholder="60-3600"
                :label="t('Wait for a scheduled stream (seconds)')"
              />
            </div>
          </div>
          <div class="row clickable" @click="toggle('preserve_mtime')">
            <span class="row-label">{{ t("Keep the original upload date on the file") }}</span>
            <UiSwitch
              :model-value="isOn('preserve_mtime')"
              :label="t('Keep the original upload date on the file')"
              @click.stop
              @update:model-value="toggle('preserve_mtime')"
            />
          </div>
          <div class="row clickable" @click="toggle('ignore_errors')">
            <span class="row-label">
              {{ t("Keep going when one video of a playlist fails") }}
            </span>
            <UiSwitch
              :model-value="isOn('ignore_errors')"
              :label="t('Keep going when one video of a playlist fails')"
              @click.stop
              @update:model-value="toggle('ignore_errors')"
            />
          </div>
          <div class="row clickable" @click="toggle('verbose')">
            <span class="row-label">{{ t("Verbose log (for troubleshooting)") }}</span>
            <UiSwitch
              :model-value="isOn('verbose')"
              :label="t('Verbose log (for troubleshooting)')"
              @click.stop
              @update:model-value="toggle('verbose')"
            />
          </div>
        </div>
      </section>
    </template>
  </div>
</template>

<style scoped>
.settings {
  padding-bottom: 40px;
}
/* Every dropdown and field the same width, lined up on the right. */
.control {
  flex: none;
  width: 280px;
}
.control.narrow {
  width: 160px;
}
.control.wide {
  width: 340px;
}
.clickable {
  cursor: pointer;
}
.row-note.flush {
  padding-top: 0;
}
.rows > .row-note {
  border-top: none;
  padding-top: 0;
}
.expander {
  display: flex;
  align-items: center;
  gap: 16px;
  width: 100%;
  padding: 14px 18px;
  text-align: left;
  cursor: pointer;
  transition:
    border-color 0.15s var(--ease),
    background 0.15s var(--ease);
}
.expander:hover {
  border-color: var(--border-strong);
}
.expander .row-label {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.expander-title {
  font-weight: 600;
}
.expander svg {
  width: 18px;
  height: 18px;
  color: var(--muted);
  transition: transform 0.2s var(--ease);
}
.expander.open svg {
  transform: rotate(180deg);
}
.entries {
  max-height: 150px;
  margin: 0 18px 14px;
  padding: 10px 12px;
  overflow-y: auto;
  border: 1px solid var(--border);
  border-radius: var(--radius-s);
  background: var(--surface-2);
  font-family: var(--mono);
  font-size: 12px;
  line-height: 1.7;
}
.entries p {
  font-family: var(--font);
}
</style>
