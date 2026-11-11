//! The Settings page's state and the settings themselves.
//!
//! `SettingsPage` owns [`Settings`], saves it, and keeps the history
//! file's count. Its values apply to links queued from then on: the queue
//! copies them into each item when it is added, so an item keeps what it
//! was queued with. `config::Settings`, `Settings::to_options` and
//! `Settings::problems` hold the model and its rules; this module only
//! edits it. The window reads and changes each setting by the key it has
//! in the settings file ([`Settings::pairs`], [`SettingsPage::set`]).

use tr::tr;

use crate::config::Settings;
use crate::log::Level;
use crate::provision::Paths;

/// A line for the activity log, which the queue owns.
pub type Note = (Level, String);

pub struct SettingsPage {
    paths: Paths,
    settings: Settings,
    /// How many videos the history holds, when it has been looked at.
    history: Option<usize>,
    /// Linux: the application menu has an entry for Osprey (see
    /// `crate::desktop`). Read from disk, not saved in the settings.
    desktop_entry: bool,
}

impl SettingsPage {
    pub fn new(paths: Paths, settings: Settings) -> Self {
        let mut page = SettingsPage {
            paths,
            settings,
            history: None,
            desktop_entry: crate::desktop::data_home()
                .is_some_and(|data| crate::desktop::installed(&data)),
        };
        page.refresh_history();
        page
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    /// How many videos the history holds: `None` while the history is off
    /// or its file doesn't exist yet.
    pub fn history(&self) -> Option<usize> {
        self.history
    }

    /// The history's own entries, one per line as yt-dlp wrote them.
    pub fn history_lines(&self) -> Option<Vec<String>> {
        self.settings.history_lines(&self.paths)
    }

    pub fn desktop_entry(&self) -> bool {
        self.desktop_entry
    }

    /// Changes one setting by its key in the settings file and saves.
    /// Returns what the activity log should say, if anything.
    pub fn set(&mut self, key: &str, value: &str) -> Option<Note> {
        if key == "video_password" {
            // Kept only while the app runs: never written anywhere.
            self.settings.video_password = value.to_string();
            return None;
        }
        self.settings.set(key, value);
        if key == "language" {
            // Shows the interface in the other language at once.
            crate::i18n::apply(&self.settings.language);
        }
        // Cheap, and it keeps the history count honest after the switch or
        // the file's name changes.
        self.refresh_history();
        self.save()
    }

    pub fn set_desktop_entry(&mut self, wanted: bool) -> Option<Note> {
        let Some(data) = crate::desktop::data_home() else {
            return Some((Level::Warn, tr!("Could not find the home folder.")));
        };
        let result = if wanted {
            std::env::current_exe().and_then(|exe| crate::desktop::install(&data, &exe))
        } else {
            crate::desktop::remove(&data)
        };
        self.desktop_entry = crate::desktop::installed(&data);
        match result {
            Ok(()) if wanted => Some((Level::Ok, tr!("Osprey is now in the applications menu."))),
            Ok(()) => Some((
                Level::Info,
                tr!("Osprey was removed from the applications menu."),
            )),
            Err(e) => Some((
                Level::Warn,
                tr!("Could not change the applications menu: {e}", e),
            )),
        }
    }

    /// Writes the settings. `None` when it worked.
    pub fn save(&mut self) -> Option<Note> {
        self.settings
            .save(&self.paths)
            .err()
            .map(|e| (Level::Warn, tr!("Could not save the settings: {e}", e)))
    }

    /// Re-reads the history file, which a finished download may have
    /// added to.
    pub fn refresh_history(&mut self) {
        self.history = self
            .settings
            .archive
            .then(|| self.settings.history_count(&self.paths))
            .flatten();
    }

    /// Throws the history away, and says how much went. The window asks
    /// first.
    pub fn clear_history(&mut self) -> Note {
        let entries = self.settings.history_count(&self.paths).unwrap_or(0);
        let path = self.settings.archive_path(&self.paths);
        // Emptied rather than removed, so a hard linked copy kept
        // elsewhere is cleared too.
        let cleared = if path.exists() {
            std::fs::write(&path, "")
        } else {
            Err(std::io::ErrorKind::NotFound.into())
        };
        self.refresh_history();
        match cleared {
            Ok(()) => (
                Level::Ok,
                tr!("History cleared ({n} entry)." | "History cleared ({n} entries)." % entries),
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                (Level::Info, tr!("There is no history file yet."))
            }
            Err(e) => (Level::Warn, tr!("Could not clear the history: {e}", e)),
        }
    }

    /// The default output folder, short enough for a placeholder.
    pub fn default_folder_hint(&self) -> String {
        short_path(&crate::config::default_output_dir(&self.paths))
    }
}

/// A path to show where there is little room: cut at the start, so the
/// end, the folder's own name, stays readable ("…/Osprey/downloads"),
/// rather than cut by the field at some letter in the middle.
fn short_path(path: &std::path::Path) -> String {
    let full = path.display().to_string();
    if full.chars().count() <= 40 {
        return full;
    }
    let last: Vec<String> = path
        .components()
        .rev()
        .take(2)
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let sep = std::path::MAIN_SEPARATOR;
    match last.as_slice() {
        [name, parent] => format!("\u{2026}{sep}{parent}{sep}{name}"),
        _ => full,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::tempdir;

    fn page_with_history(dir: &std::path::Path) -> (SettingsPage, std::path::PathBuf) {
        let paths = Paths::new(dir.to_path_buf());
        let settings = Settings {
            archive: true,
            ..Settings::default()
        };
        let history = settings.archive_path(&paths);
        std::fs::write(&history, "youtube a\nyoutube b\n").unwrap();
        (SettingsPage::new(paths, settings), history)
    }

    #[test]
    fn clearing_the_history_empties_it_and_says_how_much_went() {
        let dir = tempdir();
        let (mut page, history) = page_with_history(&dir);
        assert_eq!(page.history(), Some(2), "the count comes from the file");
        assert_eq!(
            page.history_lines(),
            Some(vec!["youtube a".to_string(), "youtube b".to_string()])
        );
        let note = page.clear_history();
        assert_eq!(std::fs::read_to_string(&history).unwrap(), "", "emptied");
        assert!(note.1.contains("(2 entries)"), "{note:?}");
        assert_eq!(page.history(), Some(0));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_history_is_only_counted_while_it_is_kept() {
        let dir = tempdir();
        let (mut page, _) = page_with_history(&dir);
        let _ = page.set("archive", "no");
        assert_eq!(page.history(), None);
        let _ = page.set("archive", "yes");
        assert_eq!(page.history(), Some(2));
        // Another file's name: nothing there yet.
        let _ = page.set("archive_file", "other.txt");
        assert_eq!(page.history(), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Every change is written at once, by the key the file uses.
    #[test]
    fn a_setting_is_changed_by_its_key_and_saved() {
        let dir = tempdir();
        let paths = Paths::new(dir.clone());
        let mut page = SettingsPage::new(paths.clone(), Settings::default());
        assert_eq!(page.set("output_dir", "/videos"), None);
        assert_eq!(Settings::load(&paths).output_dir, "/videos");
        let _ = page.set("verbose", "yes");
        let on_disk = Settings::load(&paths);
        assert!(on_disk.verbose);
        assert_eq!(on_disk.output_dir, "/videos", "the rest is kept");
        let _ = page.set("kind", "audio");
        assert_eq!(page.settings().kind, crate::config::Kind::AudioOnly);
        // Not one of the choice's keys, and not a setting at all.
        let _ = page.set("kind", "hologram");
        let _ = page.set("no_such_setting", "1");
        assert_eq!(page.settings().kind, crate::config::Kind::AudioOnly);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The video password is used for the links added next and never
    /// reaches the disk; changing another setting doesn't lose it.
    #[test]
    fn the_video_password_is_kept_in_memory_only() {
        let dir = tempdir();
        let paths = Paths::new(dir.clone());
        let mut page = SettingsPage::new(paths.clone(), Settings::default());
        let _ = page.set("video_password", "open sesame");
        let _ = page.set("verbose", "yes");
        assert_eq!(page.settings().video_password, "open sesame");
        let file = std::fs::read_to_string(dir.join(crate::config::FILE_NAME)).unwrap();
        assert!(!file.contains("sesame"), "{file}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A long default folder shows its end, the part that tells which
    /// folder it is; a short one is shown whole.
    #[test]
    fn a_long_folder_is_cut_at_the_start() {
        let sep = std::path::MAIN_SEPARATOR;
        let long = std::path::Path::new("/home/someone/Applications/portable/Osprey/downloads");
        assert_eq!(
            short_path(long),
            format!("\u{2026}{sep}Osprey{sep}downloads")
        );
        let short = std::path::Path::new("/opt/Osprey/downloads");
        assert_eq!(short_path(short), short.display().to_string());
    }

    /// Every key the file writes can be set back to the value it has, and
    /// to another one: no setting is read-only by accident.
    #[test]
    fn every_saved_setting_can_be_set_by_its_key() {
        let base = Settings::default();
        for (key, value) in base.pairs() {
            let mut s = base.clone();
            s.set(key, &value);
            assert_eq!(s, base, "{key} set to its own value changed something");
            let other = match value.as_str() {
                "yes" => "no".to_string(),
                "no" => "yes".to_string(),
                _ => match crate::config::choices().iter().find(|(k, _)| *k == key) {
                    Some((_, options)) => options
                        .iter()
                        .map(|(k, _)| *k)
                        .find(|k| *k != value)
                        .unwrap()
                        .to_string(),
                    None if key == "audio_quality" => "7".to_string(),
                    None => "marker".to_string(),
                },
            };
            s.set(key, &other);
            assert_ne!(s, base, "{key} could not be changed");
            let changed: Vec<&str> = s
                .pairs()
                .into_iter()
                .zip(base.pairs())
                .filter(|(a, b)| a.1 != b.1)
                .map(|(a, _)| a.0)
                .collect();
            assert_eq!(changed, [key], "{key} changed another setting too");
        }
    }
}
