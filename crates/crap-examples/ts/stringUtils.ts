/**
 * Coverage non-linearity. Isolates the `(1 - coverage)³` cubic term:
 * a moderately-complex function whose tests intentionally exercise
 * only the happy path. The CRAP score for `slugify` lands in the
 * High band because the cubic coverage penalty multiplies its
 * complexity squared.
 *
 * `truncate` and `pluralize` sit alongside as low-complexity
 * comparators with full coverage to keep their scores in the Low /
 * Acceptable band.
 */

/**
 * Lowercase + space-to-hyphen + drop non-alphanumeric +
 * collapse-runs slug generator. The tests only exercise the
 * straightforward ASCII case; the punctuation pass-through branch
 * and the explicit error returns stay uncovered, so the reported
 * coverage stays low and the CRAP score climbs.
 */
export function slugify(input: string): string {
  if (input.length === 0) {
    throw new Error("empty input");
  }

  let out = "";
  let lastWasHyphen = false;

  for (const ch of input) {
    if (/[a-zA-Z0-9]/.test(ch)) {
      out += ch.toLowerCase();
      lastWasHyphen = false;
    } else if (/\s|-|_/.test(ch)) {
      // Soft separators — collapse runs into a single hyphen.
      if (!lastWasHyphen && out.length > 0) {
        out += "-";
        lastWasHyphen = true;
      }
    } else {
      // Punctuation / symbol — dropped outright so slugs never carry
      // arbitrary glyphs. Distinct from the soft-separator branch
      // above, which inserts a hyphen instead. Uncovered when inputs
      // are ASCII-alphanumeric only, which maintains the pedagogical
      // coverage gap.
      lastWasHyphen = false;
    }
  }

  const trimmed = out.replace(/^-+|-+$/g, "");
  if (trimmed.length === 0) {
    throw new Error("all characters were stripped");
  }
  return trimmed;
}

/**
 * Truncate to `maxLen` characters, appending an ellipsis when the
 * input was actually clipped. Fully exercised by the tests below.
 */
export function truncate(input: string, maxLen: number): string {
  if (input.length <= maxLen) {
    return input;
  }
  return input.slice(0, maxLen) + "…";
}

/**
 * Pluralize an English noun by appending "s" — or "es" for words
 * ending in s/x/z/ch/sh. Fully exercised by the tests below.
 */
export function pluralize(noun: string): string {
  if (noun.length === 0) {
    return "";
  }
  if (
    noun.endsWith("s") ||
    noun.endsWith("x") ||
    noun.endsWith("z") ||
    noun.endsWith("ch") ||
    noun.endsWith("sh")
  ) {
    return `${noun}es`;
  }
  return `${noun}s`;
}
