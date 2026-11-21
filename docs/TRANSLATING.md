# Translating Osprey

Osprey is written in English and translated the way most GNOME apps are, with gettext `.po` files. They live in [`crates/core/translations/`](../crates/core/translations/), and a translation is contributed like any other change: as a pull request.

- `crates/core/translations/osprey.pot` lists every text in the interface, in English. It is generated from the source code; don't edit it by hand.
- Each language has its own file, `crates/core/translations/<code>.po` (for example `es.po`), with the English text (`msgid`) and its translation (`msgstr`).
- A text without a translation is shown in English, so a partial translation is fine. Spanish is the exception: it is kept complete, and the tests fail when a text is missing from it.

## Translating a language that already exists

Edit its `crates/core/translations/<code>.po` with any PO editor ([Poedit](https://poedit.net), GNOME Translation Editor, Lokalize) or a plain text editor, and open a pull request. What is left to translate is whatever has an empty `msgstr`.

Keep every `{placeholder}` exactly as it is, since the app fills them in (`{n}` is always a number). Where the English has a singular and a plural (`msgid_plural`), fill in every `msgstr[N]` your language needs.

## Adding a new language

1. Copy `osprey.pot` to `<code>.po` in the same folder, using the language code (`fr`, `de`, `pt_BR`...).
2. In its header, set `Language:` and add the `Plural-Forms:` line for your language (the [gettext manual](https://www.gnu.org/software/gettext/manual/html_node/Plural-forms.html) lists them). If your language has more or fewer than two plural forms, run the command under "For developers" once: it gives every plural text the right number of `msgstr[N]` lines.
3. Add a line for it to `LANGUAGES` in `crates/core/src/i18n.rs`, with the language's name written in that language.
4. Translate, then run `cargo test -p osprey-core --test translations` to check the file.

## For developers

Every text a user sees is written in English at the place it is shown, and marked so it can be found:

- in Rust (`crates/core`), `tr!("...")`, or `tr!("{n} file" | "{n} files" % count)` for a text that depends on a number;
- in the window's code (`ui/src`), `t("...")` and `tn("{n} file", "{n} files", count)`. The text has to be a plain string literal.

After adding or changing a text, regenerate the template:

```
OSPREY_UPDATE_POT=1 cargo test -p osprey-core --test translations
```

That also brings every `crates/core/translations/<code>.po` in line with it: translations are kept, new texts are added empty and texts no longer in the source are dropped. Then fill in the Spanish for what is new. CI fails while the template or a translation is out of date, or the Spanish is incomplete.
