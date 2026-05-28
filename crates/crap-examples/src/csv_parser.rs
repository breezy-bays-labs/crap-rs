//! Complexity-squared at full coverage. Isolates the `c²` term of
//! the CRAP formula: even with every branch exercised by tests, a
//! moderately-complex function lands in the Acceptable band purely
//! on its complexity coefficient.
//!
//! `parse_record` is a minimal RFC-4180-flavored CSV record parser —
//! quoted fields and comma + newline handling. The exhaustive test
//! suite covers every branch, so the CRAP score reflects the
//! complexity term alone (no coverage penalty).

use anyhow::{Result, anyhow};

#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub fields: Vec<String>,
}

/// Parse one CSV record. Returns the record plus the number of input
/// bytes consumed (so the caller can slice into the next record).
pub fn parse_record(input: &str) -> Result<(Record, usize)> {
    let mut fields: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut consumed = 0;

    for (i, ch) in input.char_indices() {
        consumed = i + ch.len_utf8();
        if in_quotes {
            if ch == '"' {
                in_quotes = false;
            } else {
                current.push(ch);
            }
        } else if ch == '"' {
            in_quotes = true;
        } else if ch == ',' {
            fields.push(std::mem::take(&mut current));
        } else if ch == '\n' {
            fields.push(std::mem::take(&mut current));
            return Ok((Record { fields }, consumed));
        } else {
            current.push(ch);
        }
    }

    if in_quotes {
        return Err(anyhow!("unterminated quoted field"));
    }
    fields.push(current);
    Ok((Record { fields }, consumed))
}

/// Convenience wrapper: parse the whole input as a sequence of
/// records.
pub fn parse_all(input: &str) -> Result<Vec<Record>> {
    let mut out: Vec<Record> = Vec::new();
    let mut cursor = 0;
    while cursor < input.len() {
        let slice = &input[cursor..];
        let (record, consumed) = parse_record(slice)?;
        out.push(record);
        cursor += consumed;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_yields_empty_record() {
        let (rec, _) = parse_record("").unwrap();
        assert_eq!(rec.fields, vec![""]);
    }

    #[test]
    fn three_unquoted_fields() {
        let (rec, n) = parse_record("a,b,c").unwrap();
        assert_eq!(rec.fields, vec!["a", "b", "c"]);
        assert_eq!(n, 5);
    }

    #[test]
    fn newline_terminates_record() {
        let (rec, n) = parse_record("a,b\nrest").unwrap();
        assert_eq!(rec.fields, vec!["a", "b"]);
        assert_eq!(n, 4);
    }

    #[test]
    fn quoted_field_strips_quotes() {
        let (rec, _) = parse_record("\"hello\",world").unwrap();
        assert_eq!(rec.fields, vec!["hello", "world"]);
    }

    #[test]
    fn embedded_comma_inside_quotes() {
        let (rec, _) = parse_record("\"a,b\",c").unwrap();
        assert_eq!(rec.fields, vec!["a,b", "c"]);
    }

    #[test]
    fn unterminated_quote_errors() {
        let err = parse_record("\"abc").unwrap_err();
        assert!(err.to_string().contains("unterminated"));
    }

    #[test]
    fn parse_all_returns_multiple_records() {
        let records = parse_all("a,b\nc,d\ne,f\n").unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].fields, vec!["a", "b"]);
    }

    #[test]
    fn parse_all_handles_trailing_record_without_newline() {
        let records = parse_all("a,b\nc,d").unwrap();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn multibyte_utf8_round_trips_intact() {
        let (rec, n) = parse_record("café,naïve,日本語").unwrap();
        assert_eq!(rec.fields, vec!["café", "naïve", "日本語"]);
        assert_eq!(n, "café,naïve,日本語".len());
    }
}
