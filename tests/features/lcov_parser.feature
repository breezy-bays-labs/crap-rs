Feature: LCOV coverage parsing

  The LCOV parser extracts per-file, per-line hit counts from
  cargo-llvm-cov output. It produces structured coverage data
  alongside diagnostics for any issues encountered during parsing.

  # --- Valid parsing ---

  Scenario: Single source file with line coverage
    Given LCOV data for one source file with 3 covered lines
    When the coverage is parsed
    Then the result contains 1 file
    And that file has 3 line coverage entries
    And no diagnostics are reported

  Scenario: Multiple source files in one LCOV report
    Given LCOV data for 3 source files
    When the coverage is parsed
    Then the result contains 3 files
    And each file has its own line coverage entries

  Scenario: Line hit counts are preserved
    Given a DA record "DA:42,7"
    When the coverage is parsed
    Then line 42 has 7 hits

  Scenario: Zero-hit lines are included
    Given a DA record "DA:10,0"
    When the coverage is parsed
    Then line 10 has 0 hits

  # --- Path normalization ---

  Scenario: Absolute paths are stripped to project-relative
    Given a root path of "/Users/dev/project"
    And an SF record "SF:/Users/dev/project/src/main.rs"
    When the coverage is parsed
    Then the file key is "src/main.rs"

  Scenario: Non-matching paths pass through unchanged
    Given a root path of "/Users/dev/project"
    And an SF record "SF:/other/path/lib.rs"
    When the coverage is parsed
    Then the file key is "/other/path/lib.rs"

  Scenario: Path separators are normalized to forward slashes
    Given a root path with platform-specific separators
    And an SF record with a matching path
    When the coverage is parsed
    Then the file key uses forward slashes only

  # --- Duplicate DA lines ---

  Scenario: Duplicate DA lines for the same line number are summed
    Given LCOV data with two DA records for line 42
      | record   |
      | DA:42,3  |
      | DA:42,1  |
    When the coverage is parsed
    Then line 42 has 4 hits

  Scenario: Multiple duplicates across many lines are all summed
    Given LCOV data with duplicate DA records for lines 10, 20, and 30
    When the coverage is parsed
    Then each line's hits equal the sum of all its DA entries

  # --- Malformed DA handling ---

  Scenario: Malformed DA line produces a diagnostic
    Given LCOV data containing "DA:not_a_number"
    When the coverage is parsed
    Then a MalformedRecord diagnostic is reported
    And the diagnostic includes the line number in the input
    And the diagnostic includes the raw content "DA:not_a_number"

  Scenario: Malformed DA line does not prevent parsing other lines
    Given LCOV data with a valid DA, then a malformed DA, then another valid DA
    When the coverage is parsed
    Then both valid lines appear in the coverage
    And exactly 1 diagnostic is reported

  Scenario: DA line missing hit count produces a diagnostic
    Given LCOV data containing "DA:42"
    When the coverage is parsed
    Then a MalformedRecord diagnostic is reported

  Scenario: DA line with negative hit count produces a diagnostic
    Given LCOV data containing "DA:42,-1"
    When the coverage is parsed
    Then a MalformedRecord diagnostic is reported

  # --- Block delimiters ---

  Scenario: SF records delimit file blocks
    Given LCOV data with SF then DA records for file A
    And then SF then DA records for file B
    When the coverage is parsed
    Then file A and file B each have their own coverage entries

  Scenario: end_of_record markers are ignored
    Given LCOV data with end_of_record between blocks
    When the coverage is parsed
    Then the result is identical to parsing without end_of_record

  Scenario: Unterminated final block emits partial data
    Given LCOV data that ends after DA records without end_of_record
    When the coverage is parsed
    Then the final file's coverage is included in the result

  # --- Empty and edge cases ---

  Scenario: Empty input produces empty result
    Given empty LCOV data
    When the coverage is parsed
    Then the result contains 0 files
    And no diagnostics are reported

  Scenario: Empty SF path emits a diagnostic and skips the block
    Given LCOV data with "SF:" followed by DA records
    When the coverage is parsed
    Then an EmptySourceFile diagnostic is reported
    And no coverage entry is created for the empty path

  Scenario: Non-coverage LCOV records are ignored
    Given LCOV data containing FN, FNDA, BRDA, LF, and LH records
    When the coverage is parsed
    Then only SF and DA records affect the result
    And no diagnostics are reported for ignored record types

  # --- Property invariants (from CLAUDE.md) ---

  # These scenarios document the property test contracts.
  # Implementation uses proptest with custom strategies.

  Scenario: Any valid LCOV input produces a result without panicking
    Given any syntactically structured LCOV input
    When the coverage is parsed
    Then parsing completes without panicking
    And the result is a valid ParseOutput

  Scenario: All hit counts in the output are non-negative
    Given any LCOV input
    When the coverage is parsed
    Then every LineCoverage entry has hits >= 0

  Scenario: Coverage is file-scoped with no cross-file leakage
    Given LCOV data for files A and B with distinct DA records
    When the coverage is parsed
    Then file A's coverage contains none of file B's line numbers
    And file B's coverage contains none of file A's line numbers
