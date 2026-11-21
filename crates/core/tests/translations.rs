//! The translation template and the translations themselves.
//!
//! `translations/osprey.pot` lists every text the interface shows, found in the
//! source: in Rust each `tr!(...)` (with its plural and context, if any)
//! and each label of `config.rs`'s `keyed!` tables, and in the window's
//! own code (`ui/src`) each `t("...")` and `tn("one", "many", n)`. After
//! changing a text, regenerate it with
//!
//! ```text
//! OSPREY_UPDATE_POT=1 cargo test -p osprey-core --test translations
//! ```
//!
//! which also brings every `translations/<code>.po` in line with it: translations
//! are kept, new texts are added empty and the ones no longer in the
//! source are dropped. CI fails until that is done, so neither the
//! template nor a translation falls behind the source.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use proc_macro2::{Delimiter, TokenStream, TokenTree};

/// The tests run from the crate's folder; everything here is named from
/// the repository's.
const ROOT: &str = "../..";
const POT: &str = "crates/core/translations/osprey.pot";

fn root(path: &str) -> PathBuf {
    Path::new(ROOT).join(path)
}

#[derive(Debug, Clone, PartialEq)]
struct Message {
    context: Option<String>,
    id: String,
    plural: Option<String>,
    files: BTreeSet<String>,
}

#[test]
fn the_template_lists_every_text_in_the_source() {
    let messages = extract();
    let expected = template(&messages);
    if std::env::var_os("OSPREY_UPDATE_POT").is_some() {
        fs::write(root(POT), &expected).unwrap();
        for path in po_files() {
            let current = fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
            fs::write(&path, merged(&current, &messages)).unwrap();
        }
        return;
    }
    // A Windows checkout may have turned LF into CRLF (.gitattributes keeps
    // the .po files as LF, but a clone with other settings shouldn't fail here).
    let current = fs::read_to_string(root(POT))
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(current == expected, "{POT} is out of date. {REGENERATE}");
}

const REGENERATE: &str = "Regenerate it with:\n\
    OSPREY_UPDATE_POT=1 cargo test -p osprey-core --test translations";

/// A translation lists exactly the texts of the source, in the template's
/// order, so what is left to translate is whatever has an empty `msgstr`.
#[test]
fn every_translation_lists_exactly_the_texts_in_the_source() {
    let messages = extract();
    for path in po_files() {
        let name = path.display().to_string();
        let current = fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
        assert!(
            current == merged(&current, &messages),
            "{name} doesn't match the source's texts. {REGENERATE}"
        );
    }
}

/// Spanish is the language the app is kept complete in.
#[test]
fn the_spanish_translation_is_complete() {
    let text = fs::read_to_string(root("crates/core/translations/es.po")).unwrap();
    let file = rspolib::pofile(text.as_str()).unwrap();
    let missing: Vec<&str> = file
        .entries
        .iter()
        .filter(|e| !e.obsolete && !e.msgid.is_empty())
        .filter(|e| match &e.msgid_plural {
            Some(_) => e.msgstr_plural.is_empty() || e.msgstr_plural.iter().any(String::is_empty),
            None => e.msgstr.as_deref().unwrap_or_default().is_empty(),
        })
        .map(|e| e.msgid.as_str())
        .collect();
    assert!(
        missing.is_empty(),
        "not translated in translations/es.po: {missing:#?}"
    );
}

#[test]
fn every_translation_keeps_the_placeholders_and_plural_forms() {
    let known: BTreeSet<String> = extract()
        .into_iter()
        .flat_map(|m| [Some(m.id), m.plural])
        .flatten()
        .collect();
    for path in po_files() {
        let name = path.display().to_string();
        let text = fs::read_to_string(&path).unwrap();
        let file = rspolib::pofile(text.as_str()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let forms = file
            .metadata
            .get("Plural-Forms")
            .and_then(|f| {
                f.split(';')
                    .find_map(|p| p.trim().strip_prefix("nplurals="))
            })
            .and_then(|n| n.trim().parse::<usize>().ok())
            .unwrap_or_else(|| panic!("{name}: no nplurals in Plural-Forms"));
        for entry in &file.entries {
            if entry.obsolete || entry.flags.iter().any(|f| f == "fuzzy") {
                continue;
            }
            let mut allowed = placeholders(&entry.msgid);
            if let Some(plural) = &entry.msgid_plural {
                allowed.extend(placeholders(plural));
            }
            let translations: Vec<&String> = match &entry.msgid_plural {
                Some(_) => entry.msgstr_plural.iter().collect(),
                None => entry.msgstr.iter().collect(),
            };
            for translation in translations.iter().filter(|t| !t.is_empty()) {
                let used = placeholders(translation);
                assert!(
                    used.is_subset(&allowed),
                    "{name}: {:?} uses {:?}, but the English text only has {:?}",
                    entry.msgid,
                    used,
                    allowed
                );
            }
            if entry.msgid_plural.is_some() && translations.iter().any(|t| !t.is_empty()) {
                assert_eq!(
                    entry.msgstr_plural.len(),
                    forms,
                    "{name}: {:?} needs {forms} plural forms",
                    entry.msgid
                );
            }
            assert!(
                known.contains(&entry.msgid) || entry.msgid.is_empty(),
                "{name}: {:?} is not in the source any more",
                entry.msgid
            );
        }
    }
}

#[test]
fn every_po_file_is_a_language_the_app_offers() {
    let offered: BTreeSet<&str> = osprey_core::i18n::LANGUAGES
        .iter()
        .map(|l| l.code)
        .filter(|code| *code != "en")
        .collect();
    let files: BTreeSet<String> = po_files()
        .iter()
        .map(|p| p.file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    let files: BTreeSet<&str> = files.iter().map(String::as_str).collect();
    assert_eq!(
        offered, files,
        "every translations/<code>.po needs its line in i18n::LANGUAGES, and the other way around"
    );
}

fn po_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(root("crates/core/translations"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "po"))
        .collect();
    files.sort();
    files
}

/// The `{name}` and `{}` placeholders of a text (`{{` is a literal brace).
fn placeholders(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        if rest[start..].starts_with("{{") {
            rest = &rest[start + 2..];
            continue;
        }
        match rest[start..].find('}') {
            Some(end) => {
                found.insert(rest[start..start + end + 1].to_string());
                rest = &rest[start + end + 1..];
            }
            None => break,
        }
    }
    found
}

/// Every translatable text of the core (`crates/core/src`) and of the
/// window (`ui/src`), in file order, each once.
fn extract() -> Vec<Message> {
    let mut files = Vec::new();
    collect(&root("crates/core/src"), &["rs"], &mut files);
    files.sort();
    let mut ui = Vec::new();
    collect(&root("ui/src"), &["vue", "ts"], &mut ui);
    // Where t() and tn() are defined and described: no text of its own.
    ui.retain(|path| !path.ends_with("lib/i18n.ts"));
    ui.sort();
    files.extend(ui);
    let mut messages: Vec<Message> = Vec::new();
    for path in files {
        let name = path
            .strip_prefix(ROOT)
            .unwrap()
            .display()
            .to_string()
            .replace('\\', "/");
        // CRLF from a Windows checkout would leave a \r in texts that are
        // continued over several lines with a trailing backslash.
        let source = fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
        let mut found = Vec::new();
        if name.ends_with(".rs") {
            let tokens = TokenStream::from_str(&source)
                .unwrap_or_else(|e| panic!("{name} does not tokenize: {e}"));
            walk(tokens, &mut found);
        } else {
            scan_ui(&source, &name, &mut found);
        }
        for (context, id, plural) in found {
            match messages
                .iter_mut()
                .find(|m| m.context == context && m.id == id)
            {
                Some(existing) => {
                    assert_eq!(
                        existing.plural, plural,
                        "{id:?} is used both with and without a plural, or with two"
                    );
                    existing.files.insert(name.clone());
                }
                None => messages.push(Message {
                    context,
                    id,
                    plural,
                    files: BTreeSet::from([name.clone()]),
                }),
            }
        }
    }
    messages
}

fn collect(dir: &Path, extensions: &[&str], out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(&path, extensions, out);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| extensions.contains(&e))
        {
            out.push(path);
        }
    }
}

/// The window's code (Vue and TypeScript): `t("text", ...)` and
/// `tn("one", "many", n, ...)`, with the texts as plain string literals in
/// either kind of quotes. Anything else in their place (a variable, a
/// template string) can't be translated and fails here.
fn scan_ui(source: &str, name: &str, found: &mut Vec<Found>) {
    let bytes = source.as_bytes();
    let part_of_a_name = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b == b'.';
    let mut at = 0;
    while at < bytes.len() {
        if !source.is_char_boundary(at) {
            at += 1;
            continue;
        }
        let rest = &source[at..];
        let call = if rest.starts_with("tn(") {
            3
        } else if rest.starts_with("t(") {
            2
        } else {
            at += 1;
            continue;
        };
        // `emit(`, `split(` and the like end in t( without being t(), and
        // `function t(` is its own definition, in i18n.ts.
        if (at > 0 && part_of_a_name(bytes[at - 1]))
            || source[..at].trim_end().ends_with("function")
        {
            at += call;
            continue;
        }
        let line = source[..at].matches('\n').count() + 1;
        let mut cursor = at + call;
        let first = js_string(source, &mut cursor)
            .unwrap_or_else(|| panic!("{name}:{line}: t() needs a plain string literal"));
        if call == 2 {
            found.push((None, first, None));
        } else {
            let rest = source[cursor..].trim_start();
            cursor = source.len() - rest.len();
            assert!(rest.starts_with(','), "{name}:{line}: tn() needs two texts");
            cursor += 1;
            let plural = js_string(source, &mut cursor)
                .unwrap_or_else(|| panic!("{name}:{line}: tn() needs a plain plural literal"));
            found.push((None, first, Some(plural)));
        }
        at = cursor;
    }
}

/// The string literal starting at `cursor` (after any white space), and
/// `cursor` moved past it.
fn js_string(source: &str, cursor: &mut usize) -> Option<String> {
    let rest = source[*cursor..].trim_start();
    let start = source.len() - rest.len();
    let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let mut out = String::new();
    let mut chars = rest[1..].char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            c if c == quote => {
                *cursor = start + 1 + i + 1;
                return Some(out);
            }
            '\\' => match chars.next()?.1 {
                'n' => out.push('\n'),
                c @ ('\'' | '"' | '\\') => out.push(c),
                other => panic!("unsupported escape \\{other} in a translatable text"),
            },
            '\n' => return None,
            c => out.push(c),
        }
    }
    None
}

type Found = (Option<String>, String, Option<String>);

/// Looks through a token stream, macro bodies included (`column![...]`
/// holds most of the interface), for `tr!(...)` and `keyed!(...)`.
fn walk(tokens: TokenStream, found: &mut Vec<Found>) {
    let tokens: Vec<TokenTree> = tokens.into_iter().collect();
    for (i, token) in tokens.iter().enumerate() {
        if let TokenTree::Group(group) = token {
            let is_macro = |name: &str| {
                i >= 2
                    && matches!(&tokens[i - 1], TokenTree::Punct(p) if p.as_char() == '!')
                    && matches!(&tokens[i - 2], TokenTree::Ident(id) if id == name)
            };
            if is_macro("tr") && group.delimiter() == Delimiter::Parenthesis {
                if let Some(message) = tr_call(group.stream()) {
                    found.push(message);
                }
                continue;
            }
            if is_macro("keyed") {
                keyed_labels(group.stream(), found);
                continue;
            }
            walk(group.stream(), found);
        }
    }
}

/// `tr!("text", args)`, `tr!("context" => "text")` or
/// `tr!("one" | "many" % n, args)`. A first token that isn't a string
/// (`tr!(text)` in `i18n::label`) translates a text extracted elsewhere.
fn tr_call(stream: TokenStream) -> Option<Found> {
    let tokens: Vec<TokenTree> = stream.into_iter().collect();
    let first = string(tokens.first()?)?;
    let punct = |at: usize, c: char| matches!(tokens.get(at), Some(TokenTree::Punct(p)) if p.as_char() == c);
    if punct(1, '=') && punct(2, '>') {
        let id = string(tokens.get(3)?).expect("tr!(context => ...) needs a literal text");
        return Some((Some(first), id, None));
    }
    if punct(1, '|') {
        let plural = string(tokens.get(2)?).expect("tr!(one | many % n) needs a literal plural");
        return Some((None, first, Some(plural)));
    }
    Some((None, first, None))
}

/// `Name { Variant => "key", "Label", ... }`: the labels are what the
/// settings' choices show.
fn keyed_labels(stream: TokenStream, found: &mut Vec<Found>) {
    for token in stream {
        let TokenTree::Group(body) = token else {
            continue;
        };
        let tokens: Vec<TokenTree> = body.stream().into_iter().collect();
        for (i, token) in tokens.iter().enumerate() {
            let arrow = matches!(token, TokenTree::Punct(p) if p.as_char() == '>')
                && i >= 1
                && matches!(&tokens[i - 1], TokenTree::Punct(p) if p.as_char() == '=');
            // `"Firefox" as name` is a proper name, shown as written.
            let proper = matches!(tokens.get(i + 4), Some(TokenTree::Ident(id)) if id == "as");
            if arrow
                && !proper
                && let Some(label) = tokens.get(i + 3).and_then(string)
            {
                found.push((None, label, None));
            }
        }
    }
}

fn string(token: &TokenTree) -> Option<String> {
    let TokenTree::Literal(literal) = token else {
        return None;
    };
    syn::parse_str::<syn::LitStr>(&literal.to_string())
        .ok()
        .map(|s| s.value())
}

fn template(messages: &[Message]) -> String {
    let mut out = String::from(
        "# The texts of Osprey's interface, for translators.\n\
         # Generated by crates/core/tests/translations.rs; do not edit by hand.\n\
         msgid \"\"\n\
         msgstr \"\"\n\
         \"Project-Id-Version: Osprey\\n\"\n\
         \"MIME-Version: 1.0\\n\"\n\
         \"Content-Type: text/plain; charset=UTF-8\\n\"\n\
         \"Content-Transfer-Encoding: 8bit\\n\"\n",
    );
    for message in messages {
        out.push('\n');
        out += &entry(message, None, &[String::new(), String::new()]);
    }
    out
}

/// One entry of a template or of a translation: where the text is, the
/// text, and its translation (`single`, or `plural` for a text with one).
fn entry(message: &Message, single: Option<&str>, plural: &[String]) -> String {
    // One line, as gettext's own tools write it.
    let files: Vec<&str> = message.files.iter().map(String::as_str).collect();
    let mut out = format!("#: {}\n", files.join(" "));
    let braces = |t: &str| !placeholders(t).is_empty();
    if braces(&message.id) || message.plural.as_deref().is_some_and(braces) {
        // Translation editors then check that every {name} is kept.
        out += "#, python-brace-format\n";
    }
    if let Some(context) = &message.context {
        out += &format!("msgctxt {}\n", quote(context));
    }
    out += &format!("msgid {}\n", quote(&message.id));
    match &message.plural {
        Some(text) => {
            out += &format!("msgid_plural {}\n", quote(text));
            for (n, form) in plural.iter().enumerate() {
                out += &format!("msgstr[{n}] {}\n", quote(form));
            }
        }
        None => out += &format!("msgstr {}\n", quote(single.unwrap_or_default())),
    }
    out
}

/// A translation brought in line with the source: its own header, then
/// the template's entries in the template's order, each with the
/// translation the file already had. A text that changed in the source is
/// a new text: its old translation goes with the old text.
fn merged(po: &str, messages: &[Message]) -> String {
    let file = rspolib::pofile(po).unwrap();
    let forms = file
        .metadata
        .get("Plural-Forms")
        .and_then(|f| {
            f.split(';')
                .find_map(|p| p.trim().strip_prefix("nplurals="))
        })
        .and_then(|n| n.trim().parse::<usize>().ok())
        .unwrap_or(2);
    // Everything up to the first blank line: the comments and the header.
    let header = po.split("\n\n").next().unwrap_or_default();
    let mut out = format!("{header}\n");
    for message in messages {
        let known = file.entries.iter().find(|e| {
            !e.obsolete
                && e.msgid == message.id
                && e.msgctxt == message.context
                && e.msgid_plural == message.plural
        });
        let single = known.and_then(|e| e.msgstr.as_deref());
        let plural: Vec<String> = match known {
            Some(e) if e.msgstr_plural.len() == forms => e.msgstr_plural.clone(),
            _ => vec![String::new(); forms],
        };
        out.push('\n');
        out += &entry(message, single, &plural);
    }
    out
}

fn quote(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out += "\\\"",
            '\\' => out += "\\\\",
            '\n' => out += "\\n",
            '\t' => out += "\\t",
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[test]
fn the_extractor_reads_every_form_of_tr() {
    let source = r#"
        fn view() {
            column![
                text(tr!("Plain")),
                text(tr!("Hello {name}", name = who)),
                text(tr!("menu" => "Open")),
                text(tr!("{n} file" | "{n} files" % count, n = count)),
                text(tr!("Line one\nwith \"quotes\"")),
            ];
            let label = tr!(text);
        }
        keyed!(Subs {
            Off => "off", "Don't download",
            Embed => "embed", "Embed in the video",
        });
        keyed!(Browser {
            None => "none", "Don't use cookies",
            Firefox => "firefox", "Firefox" as name,
        });
    "#;
    let mut found = Vec::new();
    walk(TokenStream::from_str(source).unwrap(), &mut found);
    let s = |t: &str| t.to_string();
    assert_eq!(
        found,
        vec![
            (None, s("Plain"), None),
            (None, s("Hello {name}"), None),
            (Some(s("menu")), s("Open"), None),
            (None, s("{n} file"), Some(s("{n} files"))),
            (None, s("Line one\nwith \"quotes\""), None),
            (None, s("Don't download"), None),
            (None, s("Embed in the video"), None),
            (None, s("Don't use cookies"), None),
        ]
    );
    let ui = r#"
        <button :title="t('Copy link')" @click="emit('close')">{{ t("Don't stop") }}</button>
        const a = tn("{n} file", "{n} files", count, { extra });
        const b = t(
          "A long text, with a comma",
          { name },
        );
        const c = text.split("x") + list.at(0) + i18n.t("not ours");
        export function t(text: string) {}
        const d = t('It\'s "here"');
    "#;
    let mut found = Vec::new();
    scan_ui(ui, "example.vue", &mut found);
    assert_eq!(
        found,
        vec![
            (None, s("Copy link"), None),
            (None, s("Don't stop"), None),
            (None, s("{n} file"), Some(s("{n} files"))),
            (None, s("A long text, with a comma"), None),
            (None, s("It's \"here\""), None),
        ]
    );
    assert_eq!(quote("a \"b\"\n\\"), r#""a \"b\"\n\\""#);
    assert_eq!(
        placeholders("{n} of {total}, {{literal}}, {}"),
        BTreeSet::from([s("{n}"), s("{total}"), s("{}")])
    );
}
