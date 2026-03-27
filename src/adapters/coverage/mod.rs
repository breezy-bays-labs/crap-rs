//! LCOV coverage parser adapter.
//!
//! Parses `cargo-llvm-cov --lcov` output into per-file, per-line hit data.
//! Only uses SF (source file) and DA (line data) records — FN/FNDA records
//! are ignored because function matching uses line ranges from syn, not
//! LCOV function names (which are mangled Rust symbols).

use crate::domain::matching::LineCoverage;
use crate::domain::types::{CrapError, ParseDiagnostic};
use crate::ports::{CoveragePort, ParseOutput};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// Parses LCOV format coverage data.
///
/// Uses a single-pass block accumulator: iterates lines once,
/// SF: starts a new block, DA: accumulates into a BTreeMap per block,
/// and blocks are flushed at the next SF: or end of input.
pub struct LcovParser {
    root_path: PathBuf,
}

impl LcovParser {
    pub fn new(root_path: PathBuf) -> Self {
        Self { root_path }
    }

    fn normalize_path(&self, path: &str) -> String {
        let fwd = path.replace('\\', "/");
        let root_fwd = self.root_path.to_string_lossy().replace('\\', "/");
        let p = Path::new(&fwd);
        let root = Path::new(&root_fwd);
        p.strip_prefix(root)
            .unwrap_or(p)
            .to_string_lossy()
            .into_owned()
    }
}

impl CoveragePort for LcovParser {
    fn parse(&self, data: &str) -> Result<ParseOutput, CrapError> {
        let mut coverage: HashMap<String, Vec<LineCoverage>> = HashMap::new();
        let mut diagnostics: Vec<ParseDiagnostic> = Vec::new();
        let mut current_path: Option<String> = None;
        let mut current_lines: BTreeMap<usize, u64> = BTreeMap::new();

        for (line, line_number) in data.lines().zip(1usize..) {
            if let Some(path) = line.strip_prefix("SF:") {
                flush_block(&mut coverage, current_path.as_deref(), &mut current_lines);

                if path.is_empty() {
                    diagnostics.push(ParseDiagnostic::EmptySourceFile { line_number });
                    current_path = None;
                } else {
                    current_path = Some(self.normalize_path(path));
                }
            } else if let Some(da_rest) = line.strip_prefix("DA:")
                && current_path.is_some()
            {
                match parse_da(da_rest) {
                    Ok((line_no, hits)) => {
                        current_lines
                            .entry(line_no)
                            .and_modify(|h| *h = h.saturating_add(hits))
                            .or_insert(hits);
                    }
                    Err(_) => {
                        diagnostics.push(ParseDiagnostic::MalformedRecord {
                            line_number,
                            content: line.to_string(),
                        });
                    }
                }
            }
        }

        flush_block(&mut coverage, current_path.as_deref(), &mut current_lines);

        Ok(ParseOutput {
            coverage,
            diagnostics,
        })
    }
}

/// Parse a DA record value (after "DA:" prefix).
/// Line 0 is treated as malformed (LCOV is 1-based).
fn parse_da(da: &str) -> Result<(usize, u64), ()> {
    let (line_str, rest) = da.split_once(',').ok_or(())?;
    // DA format: line,hits[,checksum] — ignore optional checksum field
    let hits_str = rest.split(',').next().ok_or(())?;
    let line_no: usize = line_str.parse().map_err(|_| ())?;
    if line_no == 0 {
        return Err(());
    }
    let hits: u64 = hits_str.parse().map_err(|_| ())?;
    Ok((line_no, hits))
}

fn flush_block(
    coverage: &mut HashMap<String, Vec<LineCoverage>>,
    current_path: Option<&str>,
    current_lines: &mut BTreeMap<usize, u64>,
) {
    if let Some(path) = current_path
        && !current_lines.is_empty()
    {
        let lines: Vec<LineCoverage> = current_lines
            .iter()
            .map(|(&line, &hits)| LineCoverage { line, hits })
            .collect();
        coverage.insert(path.to_owned(), lines);
    }
    current_lines.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn parser() -> LcovParser {
        LcovParser::new(PathBuf::from("/project"))
    }

    fn parse(input: &str) -> ParseOutput {
        parser().parse(input).unwrap()
    }

    #[test]
    fn empty_input_returns_empty_output() {
        let output = parse("");
        assert!(output.coverage.is_empty());
        assert!(output.diagnostics.is_empty());
    }

    #[test]
    fn single_file_single_da() {
        let output = parse("SF:/project/src/main.rs\nDA:1,5\nend_of_record\n");
        assert_eq!(output.coverage.len(), 1);
        let lines = &output.coverage["src/main.rs"];
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].line, 1);
        assert_eq!(lines[0].hits, 5);
    }

    #[test]
    fn multiple_da_lines() {
        let output = parse("SF:/project/src/lib.rs\nDA:1,3\nDA:2,0\nDA:3,7\nend_of_record\n");
        let lines = &output.coverage["src/lib.rs"];
        assert_eq!(lines.len(), 3);
        assert_eq!((lines[0].line, lines[0].hits), (1, 3));
        assert_eq!((lines[1].line, lines[1].hits), (2, 0));
        assert_eq!((lines[2].line, lines[2].hits), (3, 7));
    }

    #[test]
    fn multiple_source_files() {
        let input = "SF:/project/src/a.rs\nDA:1,1\nend_of_record\nSF:/project/src/b.rs\nDA:2,2\nend_of_record\n";
        let output = parse(input);
        assert_eq!(output.coverage.len(), 2);
        assert_eq!(output.coverage["src/a.rs"][0].hits, 1);
        assert_eq!(output.coverage["src/b.rs"][0].hits, 2);
    }

    #[test]
    fn hit_count_preservation() {
        let output = parse("SF:/project/src/main.rs\nDA:42,7\nDA:10,0\nend_of_record\n");
        let lines = &output.coverage["src/main.rs"];
        assert_eq!((lines[0].line, lines[0].hits), (10, 0));
        assert_eq!((lines[1].line, lines[1].hits), (42, 7));
    }

    #[test]
    fn path_normalization_strips_prefix() {
        let output = parse("SF:/project/src/main.rs\nDA:1,1\nend_of_record\n");
        assert!(output.coverage.contains_key("src/main.rs"));
    }

    #[test]
    fn non_matching_path_passes_through() {
        let output = parse("SF:/other/lib.rs\nDA:1,1\nend_of_record\n");
        assert!(output.coverage.contains_key("/other/lib.rs"));
    }

    #[test]
    fn backslash_normalized_to_forward_slash() {
        let p = LcovParser::new(PathBuf::from("C:\\project"));
        let output = p.parse("SF:C:\\project\\src\\main.rs\nDA:1,1\n").unwrap();
        assert!(output.coverage.contains_key("src/main.rs"));
    }

    #[test]
    fn path_boundary_not_confused_by_prefix_substring() {
        let output = parse("SF:/project-old/src/main.rs\nDA:1,1\nend_of_record\n");
        assert!(output.coverage.contains_key("/project-old/src/main.rs"));
    }

    #[test]
    fn duplicate_da_lines_sum_hits() {
        let output = parse("SF:/project/src/main.rs\nDA:42,3\nDA:42,1\nend_of_record\n");
        let lines = &output.coverage["src/main.rs"];
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].line, 42);
        assert_eq!(lines[0].hits, 4);
    }

    #[test]
    fn duplicate_da_multiple_lines_sum_correctly() {
        let output = parse(
            "SF:/project/src/main.rs\nDA:10,2\nDA:20,3\nDA:30,1\nDA:10,5\nDA:20,7\nDA:30,4\nend_of_record\n",
        );
        let lines = &output.coverage["src/main.rs"];
        assert_eq!(lines.len(), 3);
        assert_eq!((lines[0].line, lines[0].hits), (10, 7));
        assert_eq!((lines[1].line, lines[1].hits), (20, 10));
        assert_eq!((lines[2].line, lines[2].hits), (30, 5));
    }

    #[test]
    fn malformed_da_produces_diagnostic() {
        let output = parse("SF:/project/src/main.rs\nDA:not_a_number\nend_of_record\n");
        assert!(!output.coverage.contains_key("src/main.rs"));
        assert_eq!(output.diagnostics.len(), 1);
        match &output.diagnostics[0] {
            ParseDiagnostic::MalformedRecord {
                line_number,
                content,
            } => {
                assert_eq!(*line_number, 2);
                assert_eq!(content, "DA:not_a_number");
            }
            _ => panic!("Expected MalformedRecord"),
        }
    }

    #[test]
    fn malformed_da_does_not_stop_parsing() {
        let output = parse("SF:/project/src/main.rs\nDA:1,5\nDA:bad\nDA:3,7\nend_of_record\n");
        let lines = &output.coverage["src/main.rs"];
        assert_eq!(lines.len(), 2);
        assert_eq!(output.diagnostics.len(), 1);
    }

    #[test]
    fn da_missing_hit_count_is_malformed() {
        let output = parse("SF:/project/src/main.rs\nDA:42\nend_of_record\n");
        assert_eq!(output.diagnostics.len(), 1);
        assert!(matches!(
            &output.diagnostics[0],
            ParseDiagnostic::MalformedRecord { .. }
        ));
    }

    #[test]
    fn da_negative_hit_count_is_malformed() {
        let output = parse("SF:/project/src/main.rs\nDA:42,-1\nend_of_record\n");
        assert_eq!(output.diagnostics.len(), 1);
        assert!(matches!(
            &output.diagnostics[0],
            ParseDiagnostic::MalformedRecord { .. }
        ));
    }

    #[test]
    fn da_line_zero_is_malformed() {
        let output = parse("SF:/project/src/main.rs\nDA:0,5\nend_of_record\n");
        assert_eq!(output.diagnostics.len(), 1);
        match &output.diagnostics[0] {
            ParseDiagnostic::MalformedRecord {
                line_number,
                content,
            } => {
                assert_eq!(*line_number, 2);
                assert_eq!(content, "DA:0,5");
            }
            _ => panic!("Expected MalformedRecord for line 0"),
        }
    }

    #[test]
    fn end_of_record_is_ignored() {
        let with_eor = parse("SF:/project/src/main.rs\nDA:1,5\nend_of_record\n");
        let without_eor = parse("SF:/project/src/main.rs\nDA:1,5\n");
        assert_eq!(with_eor.coverage.len(), without_eor.coverage.len());
        assert_eq!(
            with_eor.coverage["src/main.rs"][0].hits,
            without_eor.coverage["src/main.rs"][0].hits
        );
    }

    #[test]
    fn unterminated_final_block_emits_data() {
        let output = parse("SF:/project/src/main.rs\nDA:1,5\nDA:2,3");
        assert_eq!(output.coverage.len(), 1);
        assert_eq!(output.coverage["src/main.rs"].len(), 2);
    }

    #[test]
    fn empty_sf_path_emits_diagnostic() {
        let output = parse("SF:\nDA:1,5\nend_of_record\n");
        assert!(output.coverage.is_empty());
        assert_eq!(output.diagnostics.len(), 1);
        assert!(matches!(
            &output.diagnostics[0],
            ParseDiagnostic::EmptySourceFile { line_number: 1 }
        ));
    }

    #[test]
    fn da_with_checksum_field_is_accepted() {
        let output = parse("SF:/project/src/main.rs\nDA:5,3,abc123\nend_of_record\n");
        let lines = &output.coverage["src/main.rs"];
        assert_eq!(lines.len(), 1);
        assert_eq!((lines[0].line, lines[0].hits), (5, 3));
        assert!(output.diagnostics.is_empty());
    }

    #[test]
    fn non_coverage_records_are_ignored() {
        let output = parse(
            "TN:\nSF:/project/src/main.rs\nFN:1,main\nFNDA:5,main\nBRDA:1,0,0,1\nDA:1,5\nLF:1\nLH:1\nend_of_record\n",
        );
        assert_eq!(output.coverage.len(), 1);
        assert_eq!(output.coverage["src/main.rs"].len(), 1);
        assert!(output.diagnostics.is_empty());
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_da_line() -> impl Strategy<Value = String> {
        (1..10000usize, 0..1000u64).prop_map(|(line, hits)| format!("DA:{},{}", line, hits))
    }

    fn arb_lcov_block(prefix: &'static str) -> impl Strategy<Value = String> {
        prop::collection::vec(arb_da_line(), 0..10).prop_map(move |das| {
            let mut block = format!("SF:/project/src/{}.rs\n", prefix);
            for da in das {
                block.push_str(&da);
                block.push('\n');
            }
            block.push_str("end_of_record\n");
            block
        })
    }

    fn arb_lcov_input() -> impl Strategy<Value = String> {
        (arb_lcov_block("a"), arb_lcov_block("b")).prop_map(|(a, b)| format!("{}{}", a, b))
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn no_panic_on_structured_input(input in arb_lcov_input()) {
            let parser = LcovParser::new(PathBuf::from("/project"));
            let _ = parser.parse(&input);
        }

        #[test]
        fn no_panic_on_arbitrary_input(input in ".*") {
            let parser = LcovParser::new(PathBuf::from("/project"));
            let _ = parser.parse(&input);
        }

        #[test]
        fn all_line_numbers_are_positive(input in arb_lcov_input()) {
            let parser = LcovParser::new(PathBuf::from("/project"));
            let output = parser.parse(&input).unwrap();
            for lines in output.coverage.values() {
                for lc in lines {
                    prop_assert!(lc.line > 0);
                }
            }
        }

        #[test]
        fn no_cross_file_leakage(
            a_lines in prop::collection::vec((1..100usize, 0..10u64), 1..5),
            b_lines in prop::collection::vec((200..300usize, 0..10u64), 1..5),
        ) {
            let mut input = String::from("SF:/project/src/a.rs\n");
            for (line, hits) in &a_lines {
                input.push_str(&format!("DA:{},{}\n", line, hits));
            }
            input.push_str("end_of_record\nSF:/project/src/b.rs\n");
            for (line, hits) in &b_lines {
                input.push_str(&format!("DA:{},{}\n", line, hits));
            }
            input.push_str("end_of_record\n");

            let parser = LcovParser::new(PathBuf::from("/project"));
            let output = parser.parse(&input).unwrap();

            if let Some(a_coverage) = output.coverage.get("src/a.rs") {
                for lc in a_coverage {
                    prop_assert!(lc.line < 200, "File A leaked line {} from file B range", lc.line);
                }
            }
            if let Some(b_coverage) = output.coverage.get("src/b.rs") {
                for lc in b_coverage {
                    prop_assert!(lc.line >= 200, "File B leaked line {} from file A range", lc.line);
                }
            }
        }
    }
}
