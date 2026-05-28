/**
 * Complexity-squared at full coverage. Isolates the `c²` term of the
 * CRAP formula: a moderately-complex function lands in the Acceptable
 * band purely on its complexity coefficient.
 *
 * `parseRecord` is a minimal RFC-4180-flavored CSV record parser —
 * quoted fields and comma + newline handling. The test suite covers
 * every branch, so the CRAP score reflects the complexity term alone
 * (no coverage penalty).
 */

export interface Record {
  fields: string[];
}

export interface ParseResult {
  record: Record;
  consumed: number;
}

export function parseRecord(input: string): ParseResult {
  const fields: string[] = [];
  let current = "";
  let inQuotes = false;
  let i = 0;

  while (i < input.length) {
    const ch = input[i];
    if (inQuotes) {
      if (ch === '"') {
        inQuotes = false;
      } else {
        current += ch;
      }
      i += 1;
    } else if (ch === '"') {
      inQuotes = true;
      i += 1;
    } else if (ch === ",") {
      fields.push(current);
      current = "";
      i += 1;
    } else if (ch === "\n") {
      fields.push(current);
      return { record: { fields }, consumed: i + 1 };
    } else {
      current += ch;
      i += 1;
    }
  }

  if (inQuotes) {
    throw new Error("unterminated quoted field");
  }
  fields.push(current);
  return { record: { fields }, consumed: i };
}

export function parseAll(input: string): Record[] {
  const records: Record[] = [];
  let cursor = 0;
  while (cursor < input.length) {
    const slice = input.slice(cursor);
    const { record, consumed } = parseRecord(slice);
    records.push(record);
    cursor += consumed;
  }
  return records;
}
