//! Translations, the gettext way (like unrud/video-downloader): every text
//! a user sees is written in English inside `tr!("...")`, `po/mimirdlp.pot`
//! lists them all (`tests/translations.rs` keeps it current), and each
//! language is a `po/<code>.po` file, translated on Weblate.
//!
//! The `.po` files are compiled into the binary, so the application stays
//! one portable file. A text without a translation (missing, empty, or
//! marked fuzzy, which is how Weblate flags "needs editing") shows in
//! English. The `tr` crate supplies the `tr!` macro and the registry it
//! reads from; loading and plural rules are done here, since its own `.po`
//! loader reads nothing from memory and keeps empty and fuzzy entries.

use std::borrow::Cow;
use std::collections::HashMap;

/// A language the interface can be shown in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Language {
    /// The `.po` file's name: a language (`es`), or a language and region
    /// (`pt_BR`) when the two differ enough.
    pub code: &'static str,
    /// The language's name in that language, as the picker lists it.
    pub name: &'static str,
    /// `None` for English, which is the source text itself.
    po: Option<&'static str>,
}

/// Every language a `.po` file exists for, plus English. Adding one is a
/// line here and its file in `po/`.
pub const LANGUAGES: &[Language] = &[
    Language {
        code: "en",
        name: "English",
        po: None,
    },
    Language {
        code: "es",
        name: "Español",
        po: Some(include_str!("../po/es.po")),
    },
];

/// The language for a saved choice: a code from [`LANGUAGES`], or empty
/// for the system's language (English when the system's has no
/// translation).
pub fn resolve(choice: &str) -> &'static Language {
    let english = &LANGUAGES[0];
    if !choice.is_empty() {
        return LANGUAGES
            .iter()
            .find(|l| l.code == choice)
            .unwrap_or(english);
    }
    sys_locale::get_locales()
        .find_map(|tag| best_match(&tag))
        .unwrap_or(english)
}

/// Shows the interface in `choice` (see [`resolve`]) from now on. Texts
/// already shown, such as earlier log lines, keep their language.
pub fn apply(choice: &str) {
    let language = resolve(choice);
    match language.po.map(Catalog::parse) {
        Some(Ok(catalog)) => tr::set_translator!(catalog),
        // A broken file can't reach a release (tests/translations.rs
        // parses every one), so English is a fine fallback.
        Some(Err(_)) | None => tr::unset_translator!(),
    }
}

/// `pt-BR`, `pt_BR.UTF-8` or `es-419` from the system, matched to a file:
/// the exact language and region first, then the language alone.
fn best_match(tag: &str) -> Option<&'static Language> {
    let tag = tag
        .split(['.', '@'])
        .next()
        .unwrap_or(tag)
        .replace('-', "_");
    let language = tag.split('_').next().unwrap_or(&tag);
    LANGUAGES
        .iter()
        .find(|l| l.code.eq_ignore_ascii_case(&tag))
        .or_else(|| {
            LANGUAGES
                .iter()
                .find(|l| l.code.eq_ignore_ascii_case(language))
        })
}

/// Translates a text that isn't a literal at the call site (the labels of
/// the settings' choices, written once in `config.rs`'s `keyed!` tables,
/// which `tests/translations.rs` extracts too).
pub fn label(text: &'static str) -> String {
    tr::tr!(text)
}

/// One language's translations.
pub struct Catalog {
    single: HashMap<(Option<String>, String), String>,
    plural: HashMap<(Option<String>, String), Vec<String>>,
    plural_rule: plural::Expr,
}

impl Catalog {
    pub fn parse(po: &str) -> Result<Catalog, String> {
        let file = rspolib::pofile(po).map_err(|e| e.to_string())?;
        let rule = match file.metadata.get("Plural-Forms") {
            Some(forms) => plural::rule_from_header(forms)?,
            None => plural::parse("n != 1")?,
        };
        let mut catalog = Catalog {
            single: HashMap::new(),
            plural: HashMap::new(),
            plural_rule: rule,
        };
        for entry in file.entries {
            if entry.obsolete || entry.flags.iter().any(|f| f == "fuzzy") {
                continue;
            }
            let key = (entry.msgctxt, entry.msgid);
            if entry.msgid_plural.is_some() {
                if !entry.msgstr_plural.is_empty()
                    && entry.msgstr_plural.iter().all(|s| !s.is_empty())
                {
                    catalog.plural.insert(key, entry.msgstr_plural);
                }
            } else if let Some(text) = entry.msgstr.filter(|s| !s.is_empty()) {
                catalog.single.insert(key, text);
            }
        }
        Ok(catalog)
    }

    fn key(context: Option<&str>, msgid: &str) -> (Option<String>, String) {
        (context.map(str::to_owned), msgid.to_owned())
    }
}

impl tr::Translator for Catalog {
    fn translate<'a>(&'a self, string: &'a str, context: Option<&'a str>) -> Cow<'a, str> {
        match self.single.get(&Catalog::key(context, string)) {
            Some(text) => Cow::Borrowed(text),
            None => Cow::Borrowed(string),
        }
    }

    fn ntranslate<'a>(
        &'a self,
        n: u64,
        singular: &'a str,
        plural: &'a str,
        context: Option<&'a str>,
    ) -> Cow<'a, str> {
        let form = self
            .plural
            .get(&Catalog::key(context, singular))
            .and_then(|forms| forms.get(self.plural_rule.eval(n) as usize));
        match form {
            Some(text) => Cow::Borrowed(text),
            None => Cow::Borrowed(if n == 1 { singular } else { plural }),
        }
    }
}

/// gettext's `Plural-Forms` expressions: C syntax over `n`, such as
/// `n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2`.
pub mod plural {
    #[derive(Debug, Clone, PartialEq)]
    pub enum Expr {
        N,
        Num(u64),
        Not(Box<Expr>),
        Binary(Op, Box<Expr>, Box<Expr>),
        If(Box<Expr>, Box<Expr>, Box<Expr>),
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum Op {
        Or,
        And,
        Eq,
        Ne,
        Lt,
        Le,
        Gt,
        Ge,
        Add,
        Sub,
        Mul,
        Div,
        Rem,
    }

    impl Expr {
        pub fn eval(&self, n: u64) -> u64 {
            match self {
                Expr::N => n,
                Expr::Num(v) => *v,
                Expr::Not(e) => u64::from(e.eval(n) == 0),
                Expr::If(c, a, b) => {
                    if c.eval(n) != 0 {
                        a.eval(n)
                    } else {
                        b.eval(n)
                    }
                }
                Expr::Binary(op, a, b) => {
                    let (a, b) = (a.eval(n), b.eval(n));
                    match op {
                        Op::Or => u64::from(a != 0 || b != 0),
                        Op::And => u64::from(a != 0 && b != 0),
                        Op::Eq => u64::from(a == b),
                        Op::Ne => u64::from(a != b),
                        Op::Lt => u64::from(a < b),
                        Op::Le => u64::from(a <= b),
                        Op::Gt => u64::from(a > b),
                        Op::Ge => u64::from(a >= b),
                        Op::Add => a.wrapping_add(b),
                        Op::Sub => a.wrapping_sub(b),
                        Op::Mul => a.wrapping_mul(b),
                        Op::Div => a.checked_div(b).unwrap_or(0),
                        Op::Rem => a.checked_rem(b).unwrap_or(0),
                    }
                }
            }
        }
    }

    /// The `plural=...` part of a `Plural-Forms` header.
    pub fn rule_from_header(header: &str) -> Result<Expr, String> {
        let rule = header
            .split(';')
            .map(str::trim)
            .find_map(|part| part.strip_prefix("plural="))
            .ok_or_else(|| format!("no plural= in {header:?}"))?;
        parse(rule)
    }

    pub fn parse(source: &str) -> Result<Expr, String> {
        let tokens = tokenize(source)?;
        let mut parser = Parser { tokens, at: 0 };
        let expr = parser.ternary()?;
        match parser.tokens.get(parser.at) {
            None => Ok(expr),
            Some(t) => Err(format!("unexpected {t:?} in {source:?}")),
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    enum Token {
        N,
        Num(u64),
        Sym(&'static str),
    }

    fn tokenize(source: &str) -> Result<Vec<Token>, String> {
        const SYMBOLS: &[&str] = &[
            "||", "&&", "==", "!=", "<=", ">=", "<", ">", "+", "-", "*", "/", "%", "!", "?", ":",
            "(", ")",
        ];
        let mut tokens = Vec::new();
        let mut rest = source.trim();
        while !rest.is_empty() {
            if let Some(after) = rest.strip_prefix('n') {
                tokens.push(Token::N);
                rest = after;
            } else if rest.starts_with(|c: char| c.is_ascii_digit()) {
                let end = rest
                    .find(|c: char| !c.is_ascii_digit())
                    .unwrap_or(rest.len());
                let value = rest[..end].parse().map_err(|e| format!("{e}"))?;
                tokens.push(Token::Num(value));
                rest = &rest[end..];
            } else if let Some(symbol) = SYMBOLS.iter().find(|s| rest.starts_with(**s)) {
                tokens.push(Token::Sym(symbol));
                rest = &rest[symbol.len()..];
            } else {
                return Err(format!("unexpected text {rest:?}"));
            }
            rest = rest.trim_start();
        }
        Ok(tokens)
    }

    struct Parser {
        tokens: Vec<Token>,
        at: usize,
    }

    /// From the loosest binding to the tightest, as in C.
    const LEVELS: &[&[(&str, Op)]] = &[
        &[("||", Op::Or)],
        &[("&&", Op::And)],
        &[("==", Op::Eq), ("!=", Op::Ne)],
        &[("<=", Op::Le), (">=", Op::Ge), ("<", Op::Lt), (">", Op::Gt)],
        &[("+", Op::Add), ("-", Op::Sub)],
        &[("*", Op::Mul), ("/", Op::Div), ("%", Op::Rem)],
    ];

    impl Parser {
        fn eat(&mut self, symbol: &str) -> bool {
            if matches!(self.tokens.get(self.at), Some(Token::Sym(s)) if *s == symbol) {
                self.at += 1;
                true
            } else {
                false
            }
        }

        fn ternary(&mut self) -> Result<Expr, String> {
            let condition = self.binary(0)?;
            if !self.eat("?") {
                return Ok(condition);
            }
            let yes = self.ternary()?;
            if !self.eat(":") {
                return Err("a ? without its :".into());
            }
            let no = self.ternary()?;
            Ok(Expr::If(Box::new(condition), Box::new(yes), Box::new(no)))
        }

        fn binary(&mut self, level: usize) -> Result<Expr, String> {
            let Some(ops) = LEVELS.get(level) else {
                return self.unary();
            };
            let mut left = self.binary(level + 1)?;
            'outer: loop {
                for (symbol, op) in ops.iter() {
                    if self.eat(symbol) {
                        let right = self.binary(level + 1)?;
                        left = Expr::Binary(*op, Box::new(left), Box::new(right));
                        continue 'outer;
                    }
                }
                return Ok(left);
            }
        }

        fn unary(&mut self) -> Result<Expr, String> {
            if self.eat("!") {
                return Ok(Expr::Not(Box::new(self.unary()?)));
            }
            if self.eat("(") {
                let inner = self.ternary()?;
                if !self.eat(")") {
                    return Err("a ( without its )".into());
                }
                return Ok(inner);
            }
            let token = self.tokens.get(self.at).cloned();
            self.at += 1;
            match token {
                Some(Token::N) => Ok(Expr::N),
                Some(Token::Num(v)) => Ok(Expr::Num(v)),
                other => Err(format!("expected n or a number, got {other:?}")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tr::Translator;

    const SAMPLE: &str = r#"msgid ""
msgstr ""
"Content-Type: text/plain; charset=UTF-8\n"
"Plural-Forms: nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);\n"

msgid "Done"
msgstr "Gotovo"

msgid "Empty"
msgstr ""

#, fuzzy
msgid "Needs editing"
msgstr "Stale"

msgctxt "button"
msgid "Open"
msgstr "Otkryt"

msgid "{n} file"
msgid_plural "{n} files"
msgstr[0] "{n} fajl"
msgstr[1] "{n} fajla"
msgstr[2] "{n} fajlov"

msgid "{n} link"
msgid_plural "{n} links"
msgstr[0] ""
msgstr[1] ""
msgstr[2] ""

#~ msgid "Gone"
#~ msgstr "Ushlo"
"#;

    #[test]
    fn translations_are_found_and_the_rest_stays_english() {
        let catalog = Catalog::parse(SAMPLE).unwrap();
        assert_eq!(catalog.translate("Done", None), "Gotovo");
        assert_eq!(catalog.translate("Missing", None), "Missing");
        // Empty, fuzzy and obsolete entries are not translations.
        assert_eq!(catalog.translate("Empty", None), "Empty");
        assert_eq!(catalog.translate("Needs editing", None), "Needs editing");
        assert_eq!(catalog.translate("Gone", None), "Gone");
        // A context keeps two uses of one English word apart.
        assert_eq!(catalog.translate("Open", Some("button")), "Otkryt");
        assert_eq!(catalog.translate("Open", None), "Open");
    }

    #[test]
    fn plurals_follow_the_language_s_own_rule() {
        let catalog = Catalog::parse(SAMPLE).unwrap();
        let file = |n| catalog.ntranslate(n, "{n} file", "{n} files", None);
        assert_eq!(file(1), "{n} fajl");
        assert_eq!(file(21), "{n} fajl");
        assert_eq!(file(3), "{n} fajla");
        assert_eq!(file(5), "{n} fajlov");
        assert_eq!(file(11), "{n} fajlov");
        let link = |n| catalog.ntranslate(n, "{n} link", "{n} links", None);
        assert_eq!(link(1), "{n} link");
        assert_eq!(link(4), "{n} links");
    }

    #[test]
    fn plural_rules_evaluate_like_c() {
        let cases: &[(&str, &[(u64, u64)])] = &[
            ("n != 1", &[(0, 1), (1, 0), (2, 1)]),
            ("n > 1", &[(0, 0), (1, 0), (2, 1)]),
            ("0", &[(1, 0), (7, 0)]),
            // Arabic.
            (
                "n==0 ? 0 : n==1 ? 1 : n==2 ? 2 : n%100>=3 && n%100<=10 ? 3 : n%100>=11 ? 4 : 5",
                &[(0, 0), (1, 1), (2, 2), (5, 3), (111, 4), (100, 5)],
            ),
            // Polish.
            (
                "(n==1 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2)",
                &[(1, 0), (2, 1), (12, 2), (22, 1), (25, 2)],
            ),
            ("!(n == 1)", &[(1, 0), (3, 1)]),
            ("n / 0 + n % 0", &[(5, 0)]),
        ];
        for (rule, expected) in cases {
            let expr = plural::parse(rule).unwrap();
            for (n, form) in *expected {
                assert_eq!(expr.eval(*n), *form, "{rule} for n={n}");
            }
        }
        assert!(plural::parse("n ==").is_err());
        assert!(plural::parse("(n").is_err());
        assert!(plural::parse("n ? 1").is_err());
        assert!(plural::parse("x").is_err());
    }

    #[test]
    fn the_system_language_is_matched_to_a_file() {
        assert_eq!(best_match("es-AR").map(|l| l.code), Some("es"));
        assert_eq!(best_match("es_ES.UTF-8").map(|l| l.code), Some("es"));
        assert_eq!(best_match("es").map(|l| l.code), Some("es"));
        assert_eq!(best_match("en-US").map(|l| l.code), Some("en"));
        assert_eq!(best_match("xx-YY"), None);
        assert_eq!(resolve("es").code, "es");
        assert_eq!(resolve("not-a-language").code, "en");
    }

    #[test]
    fn the_spanish_file_translates_plain_texts_and_plurals() {
        let catalog = Catalog::parse(resolve("es").po.unwrap()).unwrap();
        assert_eq!(catalog.translate("Download", None), "Descargar");
        let videos = |n| {
            catalog.ntranslate(
                n,
                "{n} video in the history",
                "{n} videos in the history",
                None,
            )
        };
        assert_eq!(videos(1), "{n} video en el historial");
        assert_eq!(videos(2), "{n} videos en el historial");
    }

    #[test]
    fn every_bundled_translation_parses() {
        for language in LANGUAGES {
            if let Some(po) = language.po {
                Catalog::parse(po).unwrap_or_else(|e| panic!("{}: {e}", language.code));
            }
        }
    }
}
