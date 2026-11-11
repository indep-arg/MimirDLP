// Translations, the gettext way, shared with the Rust side: every text is
// written here in English inside t("...") or tn("one", "many", n), and the
// app hands over the chosen language's translations from its .po file.
// tests/translations.rs in crates/core finds these calls, so the text must
// be a plain string literal.

import { shallowRef } from "vue";

export interface Catalog {
  /** gettext's Plural-Forms expression, C syntax over n. */
  plural: string;
  single: Record<string, string>;
  plurals: Record<string, string[]>;
}

type Params = Record<string, string | number>;

// Read by every t() while rendering, so choosing another language redraws
// what is on screen.
const catalog = shallowRef<Catalog | null>(null);
let pluralIndex: (n: number) => number = (n) => (n === 1 ? 0 : 1);

export function setCatalog(next: Catalog | null) {
  pluralIndex = next ? compilePlural(next.plural) : (n) => (n === 1 ? 0 : 1);
  catalog.value = next;
}

function fill(text: string, params?: Params): string {
  if (!params) return text;
  return text.replace(/\{(\w+)\}/g, (whole, name: string) =>
    name in params ? String(params[name]) : whole,
  );
}

export function t(text: string, params?: Params): string {
  return fill(catalog.value?.single[text] ?? text, params);
}

/** A text that depends on a number, which fills {n}. */
export function tn(one: string, many: string, n: number, params?: Params): string {
  const forms = catalog.value?.plurals[one];
  const text = forms?.[pluralIndex(n)] ?? (n === 1 ? one : many);
  return fill(text, { n, ...params });
}

// gettext's plural rules are C expressions such as
//   n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 ? 1 : 2
// The page's content security policy rules out eval, so they are parsed.
type Expr = (n: number) => number;

const LEVELS: [string, (a: number, b: number) => number][][] = [
  [["||", (a, b) => Number(a !== 0 || b !== 0)]],
  [["&&", (a, b) => Number(a !== 0 && b !== 0)]],
  [
    ["==", (a, b) => Number(a === b)],
    ["!=", (a, b) => Number(a !== b)],
  ],
  [
    ["<=", (a, b) => Number(a <= b)],
    [">=", (a, b) => Number(a >= b)],
    ["<", (a, b) => Number(a < b)],
    [">", (a, b) => Number(a > b)],
  ],
  [
    ["+", (a, b) => a + b],
    ["-", (a, b) => a - b],
  ],
  [
    ["*", (a, b) => a * b],
    ["/", (a, b) => (b === 0 ? 0 : Math.floor(a / b))],
    ["%", (a, b) => (b === 0 ? 0 : a % b)],
  ],
];

export function compilePlural(source: string): Expr {
  const tokens = source.match(/\d+|n|\|\||&&|==|!=|<=|>=|[<>+\-*/%!?:()]/g) ?? [];
  let at = 0;
  const eat = (symbol: string) => {
    if (tokens[at] !== symbol) return false;
    at += 1;
    return true;
  };
  const ternary = (): Expr => {
    const condition = binary(0);
    if (!eat("?")) return condition;
    const yes = ternary();
    if (!eat(":")) throw new Error("a ? without its :");
    const no = ternary();
    return (n) => (condition(n) !== 0 ? yes(n) : no(n));
  };
  const binary = (level: number): Expr => {
    if (level >= LEVELS.length) return unary();
    let left = binary(level + 1);
    for (;;) {
      const found = LEVELS[level].find(([symbol]) => eat(symbol));
      if (!found) return left;
      const [, apply] = found;
      const [a, b] = [left, binary(level + 1)];
      left = (n) => apply(a(n), b(n));
    }
  };
  const unary = (): Expr => {
    if (eat("!")) {
      const inner = unary();
      return (n) => Number(inner(n) === 0);
    }
    if (eat("(")) {
      const inner = ternary();
      if (!eat(")")) throw new Error("a ( without its )");
      return inner;
    }
    const token = tokens[at++];
    if (token === "n") return (n) => n;
    if (token !== undefined && /^\d+$/.test(token)) {
      const value = Number(token);
      return () => value;
    }
    throw new Error(`expected n or a number in ${source}`);
  };
  try {
    const rule = ternary();
    if (at !== tokens.length) throw new Error(`unexpected ${tokens[at]} in ${source}`);
    return rule;
  } catch {
    // A rule that can't be read falls back to English's, like a missing one.
    return (n) => (n === 1 ? 0 : 1);
  }
}
