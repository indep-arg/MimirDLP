# Translating MimirDLP

MimirDLP is translated the way most GNOME apps are: with gettext `.po` files, on [Weblate](https://hosted.weblate.org/engage/mimirdlp/).

The easiest way to help is there: sign in, pick your language (or start a new one) and translate in the browser. Weblate sends the changes to this repository as pull requests. Everything below is only needed to work with the files directly.

- `po/mimirdlp.pot` lists every text in the interface, in English. It is generated from the source code; don't edit it by hand.
- Each language has its own file, `po/<code>.po` (for example `po/es.po`), with the English text (`msgid`) and its translation (`msgstr`).
- A text without a translation is shown in English, so a partial translation is fine.

## Translating a language that already exists

Edit its `po/<code>.po` with any PO editor (Poedit, GNOME Translation Editor, Lokalize) or a plain text editor, and open a pull request. Keep every `{placeholder}` exactly as it is, since the app fills them in (`{n}` is always a number). Where the English has a singular and a plural (`msgid_plural`), fill in every `msgstr[N]` your language needs.

## Adding a new language

1. Copy `po/mimirdlp.pot` to `po/<code>.po`, using the language code (`fr`, `de`, `pt_BR`...).
2. In its header, set `Language:` and the `Plural-Forms:` line for your language (the [gettext manual](https://www.gnu.org/software/gettext/manual/html_node/Plural-forms.html) lists them).
3. Add a line for it to `LANGUAGES` in `src/i18n.rs`, with the language's name written in that language.
4. Translate, then run `cargo test --test translations` to check the file.

## For developers

After adding or changing any text a user sees, wrap it in `tr!("...")` and regenerate the template:

```
MIMIRDLP_UPDATE_POT=1 cargo test --test translations
```

CI fails while the template is out of date.
