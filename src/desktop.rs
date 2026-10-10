//! Linux only, and only when asked for: an entry in the desktop's
//! application menu (a `.desktop` file plus the icon, in the user's own
//! `~/.local/share`).
//!
//! On Wayland a window can't carry its own icon: the desktop looks up the
//! window's application id ([`APP_ID`]) among the installed
//! `.desktop` files, and shows a generic icon when none matches. Since the
//! application is portable it installs nothing by default; this is the
//! switch in Settings that does, and undoes it.
//!
//! Everything takes the data folder as an argument, so the tests write to a
//! temporary one instead of the real `~/.local/share`.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The application id, the same one Flathub would require (reverse DNS of
/// the GitHub account, `-` written `_`). The window carries it too.
pub const APP_ID: &str = "io.github.indep_arg.MimirDLP";
const ICON: &[u8] = include_bytes!("../branding/icon.png");

/// `$XDG_DATA_HOME`, or `~/.local/share` when it isn't set, as the XDG base
/// directory specification says.
pub fn data_home() -> Option<PathBuf> {
    match std::env::var_os("XDG_DATA_HOME") {
        Some(dir) if Path::new(&dir).is_absolute() => Some(PathBuf::from(dir)),
        _ => std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")),
    }
}

fn entry_file(data: &Path) -> PathBuf {
    data.join("applications").join(format!("{APP_ID}.desktop"))
}

fn icon_file(data: &Path) -> PathBuf {
    data.join("icons/hicolor/256x256/apps")
        .join(format!("{APP_ID}.png"))
}

/// The entry is in the application menu.
pub fn installed(data: &Path) -> bool {
    entry_file(data).is_file()
}

/// Adds the entry for the executable at `exe`, or rewrites it.
pub fn install(data: &Path, exe: &Path) -> io::Result<()> {
    let icon = icon_file(data);
    write(&icon, ICON)?;
    write(&entry_file(data), entry(exe, &icon).as_bytes())
}

/// Removes the entry and its icon. Nothing there is not an error.
pub fn remove(data: &Path) -> io::Result<()> {
    for file in [entry_file(data), icon_file(data)] {
        match fs::remove_file(&file) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
}

/// The application folder was moved since the entry was added: point the
/// entry at where the executable is now. Only ever touches an entry the
/// user asked for, and only when the program it starts is gone: a second
/// copy (an older one, or a new one being tried out) must not take over
/// the entry just by being opened.
pub fn refresh(data: &Path, exe: &Path) -> io::Result<()> {
    if !installed(data) {
        return Ok(());
    }
    let current = fs::read_to_string(entry_file(data)).unwrap_or_default();
    if current == entry(exe, &icon_file(data)) {
        return Ok(());
    }
    match exec_target(&current) {
        Some(target) if target != exe && target.is_file() => Ok(()),
        _ => install(data, exe),
    }
}

/// The program an entry starts, from its `Exec` line as [`exec_quote`]
/// writes it.
fn exec_target(entry: &str) -> Option<PathBuf> {
    let line = entry.lines().find_map(|l| l.strip_prefix("Exec="))?;
    let quoted = line.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = quoted.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(PathBuf::from(out)),
            '\\' => out.push(chars.next()?),
            '%' => {
                chars.next()?;
                out.push('%');
            }
            c => out.push(c),
        }
    }
    None
}

fn write(file: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = file.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, file)
}

/// The `.desktop` file. The icon is named by its full path, so it shows
/// without waiting for the desktop to rebuild its icon cache.
fn entry(exe: &Path, icon: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=MimirDLP\n\
         GenericName=Video Downloader\n\
         Comment=Download videos and music with yt-dlp\n\
         Exec={exec}\n\
         Icon={icon}\n\
         Terminal=false\n\
         Categories=AudioVideo;Video;\n\
         Keywords=yt-dlp;youtube;video;audio;download;\n\
         StartupWMClass={APP_ID}\n",
        exec = exec_quote(&exe.to_string_lossy()),
        icon = icon.display(),
    )
}

/// The desktop entry specification's quoting for `Exec`: the path in
/// double quotes, with `"`, `` ` ``, `$` and `\` escaped by a backslash, and
/// `%` doubled (a single one would start a field code).
fn exec_quote(path: &str) -> String {
    let mut out = String::from("\"");
    for c in path.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '%' => out.push_str("%%"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::tempdir;

    #[test]
    fn the_entry_is_added_pointed_at_the_executable_and_removed() {
        let data = tempdir();
        let exe = Path::new("/opt/MimirDLP/mimirdlp");
        assert!(!installed(&data));
        install(&data, exe).unwrap();
        assert!(installed(&data));
        let text = fs::read_to_string(entry_file(&data)).unwrap();
        assert!(text.contains("Exec=\"/opt/MimirDLP/mimirdlp\"\n"));
        assert!(text.contains(&format!("StartupWMClass={APP_ID}\n")));
        assert_eq!(fs::read(icon_file(&data)).unwrap(), ICON);
        remove(&data).unwrap();
        assert!(!installed(&data));
        assert!(!icon_file(&data).exists());
        // Removing what isn't there is fine.
        remove(&data).unwrap();
    }

    #[test]
    fn a_moved_folder_updates_an_entry_but_never_creates_one() {
        let data = tempdir();
        refresh(&data, Path::new("/somewhere/mimirdlp")).unwrap();
        assert!(
            !installed(&data),
            "refresh must not add an entry nobody asked for"
        );
        install(&data, Path::new("/old/place/mimirdlp")).unwrap();
        refresh(&data, Path::new("/new/place/mimirdlp")).unwrap();
        let text = fs::read_to_string(entry_file(&data)).unwrap();
        assert!(text.contains("Exec=\"/new/place/mimirdlp\""));
    }

    #[test]
    fn another_copy_that_is_opened_leaves_the_entry_alone() {
        let data = tempdir();
        let first = data.join("first/mimirdlp");
        fs::create_dir_all(first.parent().unwrap()).unwrap();
        fs::write(&first, "").unwrap();
        install(&data, &first).unwrap();
        refresh(&data, Path::new("/second/copy/mimirdlp")).unwrap();
        let text = fs::read_to_string(entry_file(&data)).unwrap();
        assert!(text.contains(&exec_quote(&first.to_string_lossy())));
        // Once the first copy is gone, the one that is opened takes over.
        fs::remove_file(&first).unwrap();
        refresh(&data, Path::new("/second/copy/mimirdlp")).unwrap();
        let text = fs::read_to_string(entry_file(&data)).unwrap();
        assert!(text.contains("Exec=\"/second/copy/mimirdlp\""));
    }

    #[test]
    fn exec_paths_are_quoted_the_way_the_specification_asks() {
        assert_eq!(exec_quote("/a b/mimirdlp"), r#""/a b/mimirdlp""#);
        assert_eq!(
            exec_quote(r#"/x"y`z$w\v/100%/m"#),
            r#""/x\"y\`z\$w\\v/100%%/m""#
        );
        for path in ["/a b/mimirdlp", r#"/x"y`z$w\v/100%/m"#] {
            let entry = format!("[Desktop Entry]\nExec={}\n", exec_quote(path));
            assert_eq!(exec_target(&entry), Some(PathBuf::from(path)));
        }
    }
}
