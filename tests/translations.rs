//! The translation template and the translations themselves.
//!
//! `po/mimirdlp.pot` lists every text the interface shows, found in the
//! source: each `tr!(...)` (with its plural and context, if any) and each
//! label of `config.rs`'s `keyed!` tables. Weblate reads it to know what is
//! left to translate. After changing a text, regenerate it with
//!
//! ```text
//! MIMIRDLP_UPDATE_POT=1 cargo test --test translations
//! ```
//!
//! and CI fails until that is done, so the template never falls behind.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use proc_macro2::{Delimiter, TokenStream, TokenTree};

const POT: &str = "po/mimirdlp.pot";

#[derive(Debug, Clone, PartialEq)]
struct Message {
    context: Option<String>,
    id: String,
    plural: Option<String>,
    files: BTreeSet<String>,
}

#[test]
fn the_template_lists_every_text_in_the_source() {
    let expected = template(&extract());
    if std::env::var_os("MIMIRDLP_UPDATE_POT").is_some() {
        fs::write(POT, &expected).unwrap();
        return;
    }
    // A Windows checkout may have turned LF into CRLF (.gitattributes keeps
    // po/ as LF, but a clone with other settings shouldn't fail here).
    let current = fs::read_to_string(POT)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        current == expected,
        "{POT} is out of date. Regenerate it with:\n\
         MIMIRDLP_UPDATE_POT=1 cargo test --test translations"
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
            // A text that was translated but no longer exists in the source
            // is harmless, but worth knowing about when it is a typo.
            if !known.contains(&entry.msgid) && !entry.msgid.is_empty() {
                eprintln!("{name}: {:?} is not in the source any more", entry.msgid);
            }
        }
    }
}

#[test]
fn every_po_file_is_a_language_the_app_offers() {
    let offered: BTreeSet<&str> = mimirdlp::i18n::LANGUAGES
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
        "every po/<code>.po needs its line in i18n::LANGUAGES, and the other way around"
    );
}

fn po_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir("po")
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

/// Every translatable text in `src/`, in file order, each once.
fn extract() -> Vec<Message> {
    let mut files = Vec::new();
    collect_rs(Path::new("src"), &mut files);
    files.sort();
    let mut messages: Vec<Message> = Vec::new();
    for path in files {
        let name = path.display().to_string().replace('\\', "/");
        // CRLF from a Windows checkout would leave a \r in texts that are
        // continued over several lines with a trailing backslash.
        let source = fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
        let tokens = TokenStream::from_str(&source)
            .unwrap_or_else(|e| panic!("{name} does not tokenize: {e}"));
        let mut found = Vec::new();
        walk(tokens, &mut found);
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

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
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
            if arrow && let Some(label) = tokens.get(i + 3).and_then(string) {
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
        "# The texts of MimirDLP's interface, for translators.\n\
         # Generated by tests/translations.rs; do not edit by hand.\n\
         msgid \"\"\n\
         msgstr \"\"\n\
         \"Project-Id-Version: MimirDLP\\n\"\n\
         \"MIME-Version: 1.0\\n\"\n\
         \"Content-Type: text/plain; charset=UTF-8\\n\"\n\
         \"Content-Transfer-Encoding: 8bit\\n\"\n",
    );
    for message in messages {
        out.push('\n');
        // One line, as gettext's own tools write it (Weblate's msgmerge
        // rewrites every .po that way).
        let files: Vec<&str> = message.files.iter().map(String::as_str).collect();
        out += &format!("#: {}\n", files.join(" "));
        let braces = |t: &str| !placeholders(t).is_empty();
        if braces(&message.id) || message.plural.as_deref().is_some_and(braces) {
            // Weblate then checks that a translation keeps every {name}.
            out += "#, python-brace-format\n";
        }
        if let Some(context) = &message.context {
            out += &format!("msgctxt {}\n", quote(context));
        }
        out += &format!("msgid {}\n", quote(&message.id));
        match &message.plural {
            Some(plural) => {
                out += &format!("msgid_plural {}\n", quote(plural));
                out += "msgstr[0] \"\"\nmsgstr[1] \"\"\n";
            }
            None => out += "msgstr \"\"\n",
        }
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
        ]
    );
    assert_eq!(quote("a \"b\"\n\\"), r#""a \"b\"\n\\""#);
    assert_eq!(
        placeholders("{n} of {total}, {{literal}}, {}"),
        BTreeSet::from([s("{n}"), s("{total}"), s("{}")])
    );
}
