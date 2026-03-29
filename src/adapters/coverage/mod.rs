//! LCOV coverage parser adapter.
//!
//! Parses `cargo-llvm-cov --lcov` output into per-file, per-line hit data.
//! Uses SF (source file), DA (line data), and BRDA (branch data) records.
//! FN/FNDA records are ignored because function matching uses line ranges
//! from syn, not LCOV function names (which are mangled Rust symbols).

use crate::domain::types::{BranchCoverage, CrapError, LineCoverage, ParseDiagnostic};
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
        let mut branches: Option<HashMap<String, Vec<BranchCoverage>>> = None;
        let mut diagnostics: Vec<ParseDiagnostic> = Vec::new();
        let mut current_path: Option<String> = None;
        let mut current_lines: BTreeMap<usize, u64> = BTreeMap::new();
        let mut current_branches: BTreeMap<(usize, u32, u32), Option<u64>> = BTreeMap::new();

        for (line, line_number) in data.lines().zip(1usize..) {
            if let Some(path) = line.strip_prefix("SF:") {
                flush_block(
                    &mut coverage,
                    &mut branches,
                    current_path.as_deref(),
                    &mut current_lines,
                    &mut current_branches,
                );

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
            } else if let Some(brda_rest) = line.strip_prefix("BRDA:")
                && current_path.is_some()
            {
                match parse_brda(brda_rest) {
                    Ok((line_no, block, branch, taken)) => {
                        let key = (line_no, block, branch);
                        current_branches
                            .entry(key)
                            .and_modify(|existing| {
                                *existing = match (*existing, taken) {
                                    (Some(a), Some(b)) => Some(a.saturating_add(b)),
                                    (Some(a), None) => Some(a),
                                    (None, Some(b)) => Some(b),
                                    (None, None) => None,
                                };
                            })
                            .or_insert(taken);
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

        flush_block(
            &mut coverage,
            &mut branches,
            current_path.as_deref(),
            &mut current_lines,
            &mut current_branches,
        );

        Ok(ParseOutput {
            coverage,
            branches,
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

/// Parse a BRDA record value (after "BRDA:" prefix).
/// Format: line,block,branch,taken where taken is "-" or a non-negative integer.
/// Line 0 is treated as malformed (LCOV is 1-based).
fn parse_brda(brda: &str) -> Result<(usize, u32, u32, Option<u64>), ()> {
    let mut parts = brda.splitn(4, ',');
    let line_str = parts.next().ok_or(())?;
    let block_str = parts.next().ok_or(())?;
    let branch_str = parts.next().ok_or(())?;
    let taken_str = parts.next().ok_or(())?;

    let line_no: usize = line_str.parse().map_err(|_| ())?;
    if line_no == 0 {
        return Err(());
    }
    let block: u32 = block_str.parse().map_err(|_| ())?;
    let branch: u32 = branch_str.parse().map_err(|_| ())?;
    let taken = if taken_str == "-" {
        None
    } else {
        Some(taken_str.parse::<u64>().map_err(|_| ())?)
    };

    Ok((line_no, block, branch, taken))
}

fn flush_block(
    coverage: &mut HashMap<String, Vec<LineCoverage>>,
    branches: &mut Option<HashMap<String, Vec<BranchCoverage>>>,
    current_path: Option<&str>,
    current_lines: &mut BTreeMap<usize, u64>,
    current_branches: &mut BTreeMap<(usize, u32, u32), Option<u64>>,
) {
    if let Some(path) = current_path {
        if !current_lines.is_empty() {
            // Merge with any existing coverage for this file (handles repeated
            // SF blocks from concatenated tracefiles or TN: sections).
            let existing = coverage.entry(path.to_owned()).or_default();
            let mut merged: BTreeMap<usize, u64> = BTreeMap::new();
            for lc in existing.iter() {
                merged.insert(lc.line, lc.hits);
            }
            for (&line, &hits) in current_lines.iter() {
                merged
                    .entry(line)
                    .and_modify(|h| *h = h.saturating_add(hits))
                    .or_insert(hits);
            }
            *existing = merged
                .into_iter()
                .map(|(line, hits)| LineCoverage { line, hits })
                .collect();
        }

        if !current_branches.is_empty() {
            let branch_vec: Vec<BranchCoverage> = current_branches
                .iter()
                .map(|(&(line, _, _), &taken)| BranchCoverage { line, taken })
                .collect();
            branches
                .get_or_insert_with(HashMap::new)
                .entry(path.to_owned())
                .or_default()
                .extend(branch_vec);
        }
    }
    current_lines.clear();
    current_branches.clear();
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
    fn repeated_sf_blocks_merge_coverage() {
        let input = "\
SF:/project/src/main.rs\nDA:1,3\nDA:2,5\nend_of_record\n\
SF:/project/src/main.rs\nDA:2,2\nDA:3,7\nend_of_record\n";
        let output = parse(input);
        assert_eq!(output.coverage.len(), 1);
        let lines = &output.coverage["src/main.rs"];
        assert_eq!(lines.len(), 3);
        assert_eq!((lines[0].line, lines[0].hits), (1, 3));
        assert_eq!((lines[1].line, lines[1].hits), (2, 7)); // 5 + 2
        assert_eq!((lines[2].line, lines[2].hits), (3, 7));
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
        // BRDA record should now be parsed
        let branches = output.branches.as_ref().expect("branches should be Some");
        assert_eq!(branches["src/main.rs"].len(), 1);
        assert_eq!(branches["src/main.rs"][0].taken, Some(1));
    }

    // ── BRDA parsing tests ──────────────────────────────────────────

    #[test]
    fn brda_and_da_records_parsed() {
        let output =
            parse("SF:/project/src/lib.rs\nDA:1,5\nBRDA:1,0,0,3\nBRDA:1,0,1,0\nend_of_record\n");
        assert_eq!(output.coverage["src/lib.rs"].len(), 1);
        let branches = output.branches.as_ref().expect("branches should be Some");
        let file_branches = &branches["src/lib.rs"];
        assert_eq!(file_branches.len(), 2);
    }

    #[test]
    fn brda_dash_maps_to_none() {
        let output = parse("SF:/project/src/lib.rs\nBRDA:10,0,0,-\nend_of_record\n");
        let branches = output.branches.as_ref().expect("branches should be Some");
        assert_eq!(branches["src/lib.rs"][0].taken, None);
    }

    #[test]
    fn brda_numeric_maps_to_some() {
        let output = parse("SF:/project/src/lib.rs\nBRDA:10,0,0,5\nend_of_record\n");
        let branches = output.branches.as_ref().expect("branches should be Some");
        assert_eq!(branches["src/lib.rs"][0].taken, Some(5));
    }

    #[test]
    fn brda_zero_maps_to_some_zero() {
        let output = parse("SF:/project/src/lib.rs\nBRDA:10,0,0,0\nend_of_record\n");
        let branches = output.branches.as_ref().expect("branches should be Some");
        assert_eq!(branches["src/lib.rs"][0].taken, Some(0));
    }

    #[test]
    fn duplicate_brda_sum_taken() {
        let output = parse("SF:/project/src/lib.rs\nBRDA:10,0,0,3\nBRDA:10,0,0,7\nend_of_record\n");
        let branches = output.branches.as_ref().expect("branches should be Some");
        let file_branches = &branches["src/lib.rs"];
        // Duplicates merged by summing: 3 + 7 = 10
        assert_eq!(file_branches.len(), 1);
        assert_eq!(file_branches[0].taken, Some(10));
    }

    #[test]
    fn brda_dash_and_numeric_merge() {
        let output = parse("SF:/project/src/lib.rs\nBRDA:10,0,0,-\nBRDA:10,0,0,5\nend_of_record\n");
        let branches = output.branches.as_ref().expect("branches should be Some");
        // None + Some(5) = Some(5)
        assert_eq!(branches["src/lib.rs"][0].taken, Some(5));
    }

    #[test]
    fn malformed_brda_produces_diagnostic() {
        let output = parse("SF:/project/src/lib.rs\nBRDA:not,valid\nend_of_record\n");
        assert_eq!(output.diagnostics.len(), 1);
        match &output.diagnostics[0] {
            ParseDiagnostic::MalformedRecord {
                line_number,
                content,
            } => {
                assert_eq!(*line_number, 2);
                assert_eq!(content, "BRDA:not,valid");
            }
            _ => panic!("Expected MalformedRecord"),
        }
    }

    #[test]
    fn brda_missing_fields_is_malformed() {
        // Only 2 fields instead of 4
        let output = parse("SF:/project/src/lib.rs\nBRDA:10,0\nend_of_record\n");
        assert_eq!(output.diagnostics.len(), 1);
        assert!(matches!(
            &output.diagnostics[0],
            ParseDiagnostic::MalformedRecord { .. }
        ));
    }

    #[test]
    fn brda_missing_taken_is_malformed() {
        // Only 3 fields instead of 4
        let output = parse("SF:/project/src/lib.rs\nBRDA:10,0,0\nend_of_record\n");
        assert_eq!(output.diagnostics.len(), 1);
        assert!(matches!(
            &output.diagnostics[0],
            ParseDiagnostic::MalformedRecord { .. }
        ));
    }

    #[test]
    fn brda_line_zero_is_malformed() {
        let output = parse("SF:/project/src/lib.rs\nBRDA:0,0,0,5\nend_of_record\n");
        assert_eq!(output.diagnostics.len(), 1);
        assert!(matches!(
            &output.diagnostics[0],
            ParseDiagnostic::MalformedRecord { .. }
        ));
    }

    #[test]
    fn no_brda_produces_none_branches() {
        let output = parse("SF:/project/src/lib.rs\nDA:1,5\nend_of_record\n");
        assert!(output.branches.is_none());
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
        fn no_panic_on_arbitrary_brda(input in "BRDA:.*") {
            let lcov = format!("SF:/project/src/test.rs\n{input}\nend_of_record\n");
            let parser = LcovParser::new(PathBuf::from("/project"));
            let _ = parser.parse(&lcov);
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
