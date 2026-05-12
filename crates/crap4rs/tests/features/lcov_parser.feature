Feature: LCOV coverage parsing

  The LCOV parser extracts per-file, per-line hit counts from
  cargo-llvm-cov output. It produces structured coverage data
  alongside diagnostics for any issues encountered during parsing.

  # --- Valid parsing ---

  @unwired
  Scenario: Single source file with line coverage
    # tracked: crap-rs#169 — lcov-parser cucumber harness not yet built (unit tests cover the path)
    Given LCOV data for one source file with 3 covered lines
    When the coverage is parsed
    Then the result contains 1 file
    And that file has 3 line coverage entries
    And no diagnostics are reported

  @unwired
  Scenario: Multiple source files in one LCOV report
    # tracked: crap-rs#169 — lcov-parser cucumber harness not yet built (unit tests cover the path)
    Given LCOV data for 3 source files
    When the coverage is parsed
    Then the result contains 3 files
    And each file has its own line coverage entries

  @unwired
  Scenario: Line hit counts are preserved
    # tracked: crap-rs#169 — lcov-parser cucumber harness not yet built (unit tests cover the path)
    Given a DA record "DA:42,7"
    When the coverage is parsed
    Then line 42 has 7 hits

  @unwired
  Scenario: Zero-hit lines are included
    # tracked: crap-rs#169 — lcov-parser cucumber harness not yet built (unit tests cover the path)
    Given a DA record "DA:10,0"
    When the coverage is parsed
    Then line 10 has 0 hits

  # --- Path normalization ---

  @unwired
  Scenario: Absolute paths are stripped to project-relative
    # tracked: crap-rs#169 — lcov-parser cucumber harness not yet built (unit tests cover the path)
    Given a root path of "/Users/dev/project"
    And an SF record "SF:/Users/dev/project/src/main.rs"
    When the coverage is parsed
    Then the file key is "src/main.rs"

  @unwired
  Scenario: Non-matching paths pass through unchanged
    # tracked: crap-rs#169 — lcov-parser cucumber harness not yet built (unit tests cover the path)
    Given a root path of "/Users/dev/project"
    And an SF record "SF:/other/path/lib.rs"
    When the coverage is parsed
    Then the file key is "/other/path/lib.rs"

  @unwired
  Scenario: Path separators are normalized to forward slashes
    # tracked: crap-rs#169 — lcov-parser cucumber harness not yet built (unit tests cover the path)
    Given a root path with platform-specific separators
    And an SF record with a matching path
    When the coverage is parsed
    Then the file key uses forward slashes only

  # --- Duplicate DA lines ---

  @unwired
  Scenario: Duplicate DA lines for the same line number are summed
    # tracked: crap-rs#169 — lcov-parser cucumber harness not yet built (unit tests cover the path)
    Given LCOV data with two DA records for line 42
      | record   |
      | DA:42,3  |
      | DA:42,1  |
    When the coverage is parsed
    Then line 42 has 4 hits

  @unwired
  Scenario: Multiple duplicates across many lines are all summed
    # tracked: crap-rs#169 — lcov-parser cucumber harness not yet built (unit tests cover the path)
    Given LCOV data with duplicate DA records for lines 10, 20, and 30
    When the coverage is parsed
    Then each line's hits equal the sum of all its DA entries

  # --- Malformed DA handling ---

  @unwired
  Scenario: Malformed DA line produces a diagnostic
    # tracked: crap-rs#169 — lcov-parser cucumber harness not yet built (unit tests cover the path)
    Given LCOV data containing "DA:not_a_number"
    When the coverage is parsed
    Then a MalformedRecord diagnostic is reported
    And the diagnostic includes the line number in the input
    And the diagnostic includes the raw content "DA:not_a_number"

  @unwired
  Scenario: Malformed DA line does not prevent parsing other lines
    # tracked: crap-rs#169 — lcov-parser cucumber harness not yet built (unit tests cover the path)
    Given LCOV data with a valid DA, then a malformed DA, then another valid DA
    When the coverage is parsed
    Then both valid lines appear in the coverage
    And exactly 1 diagnostic is reported

  @unwired
  Scenario: DA line missing hit count produces a diagnostic
    # tracked: crap-rs#169 — lcov-parser cucumber harness not yet built (unit tests cover the path)
    Given LCOV data containing "DA:42"
    When the coverage is parsed
    Then a MalformedRecord diagnostic is reported

  @unwired
  Scenario: DA line with negative hit count produces a diagnostic
    # tracked: crap-rs#169 — lcov-parser cucumber harness not yet built (unit tests cover the path)
    Given LCOV data containing "DA:42,-1"
    When the coverage is parsed
    Then a MalformedRecord diagnostic is reported

  # --- Block delimiters ---

  @unwired
  Scenario: SF records delimit file blocks
    # tracked: crap-rs#169 — lcov-parser cucumber harness not yet built (unit tests cover the path)
    Given LCOV data with SF then DA records for file A
    And then SF then DA records for file B
    When the coverage is parsed
    Then file A and file B each have their own coverage entries

  @unwired
  Scenario: end_of_record markers are ignored
    # tracked: crap-rs#169 — lcov-parser cucumber harness not yet built (unit tests cover the path)
    Given LCOV data with end_of_record between blocks
    When the coverage is parsed
    Then the result is identical to parsing without end_of_record

  @unwired
  Scenario: Unterminated final block emits partial data
    # tracked: crap-rs#169 — lcov-parser cucumber harness not yet built (unit tests cover the path)
    Given LCOV data that ends after DA records without end_of_record
    When the coverage is parsed
    Then the final file's coverage is included in the result

  # --- Empty and edge cases ---

  @unwired
  Scenario: Empty input produces empty result
    # tracked: crap-rs#169 — lcov-parser cucumber harness not yet built (unit tests cover the path)
    Given empty LCOV data
    When the coverage is parsed
    Then the result contains 0 files
    And no diagnostics are reported

  @unwired
  Scenario: Empty SF path emits a diagnostic and skips the block
    # tracked: crap-rs#169 — lcov-parser cucumber harness not yet built (unit tests cover the path)
    Given LCOV data with "SF:" followed by DA records
    When the coverage is parsed
    Then an EmptySourceFile diagnostic is reported
    And no coverage entry is created for the empty path

  @unwired
  Scenario: Non-coverage LCOV records are ignored
    # tracked: crap-rs#169 — lcov-parser cucumber harness not yet built (unit tests cover the path)
    Given LCOV data containing FN, FNDA, LF, and LH records
    When the coverage is parsed
    Then only SF, DA, and BRDA records affect the result
    And no diagnostics are reported for ignored record types

  @unwired
  Scenario: BRDA records are parsed into the branches map
    # tracked: crap-rs#169 — lcov-parser cucumber harness not yet built (unit tests cover the path)
    Given LCOV data containing well-formed BRDA records under an SF block
    When the coverage is parsed
    Then `ParseOutput.branches` reflects each BRDA entry
    And no diagnostics are reported for well-formed BRDA lines

  @unwired
  Scenario: Malformed BRDA records emit a MalformedRecord diagnostic
    # tracked: crap-rs#169 — lcov-parser cucumber harness not yet built (unit tests cover the path)
    Given LCOV data containing an unparseable BRDA record under an SF block
    When the coverage is parsed
    Then a MalformedRecord diagnostic is reported for the offending line
    And the SF block continues to parse without halting

  # --- Property invariants (from CLAUDE.md) ---

  # These scenarios document the property test contracts.
  # Implementation uses proptest with custom strategies.

  @unwired
  Scenario: Any valid LCOV input produces a result without panicking
    # tracked: crap-rs#169 — lcov-parser cucumber harness not yet built (unit tests cover the path)
    Given any syntactically structured LCOV input
    When the coverage is parsed
    Then parsing completes without panicking
    And the result is a valid ParseOutput

  @unwired
  Scenario: All hit counts in the output are non-negative
    # tracked: crap-rs#169 — lcov-parser cucumber harness not yet built (unit tests cover the path)
    Given any LCOV input
    When the coverage is parsed
    Then every LineCoverage entry has hits >= 0

  @unwired
  Scenario: Coverage is file-scoped with no cross-file leakage
    # tracked: crap-rs#169 — lcov-parser cucumber harness not yet built (unit tests cover the path)
    Given LCOV data for files A and B with distinct DA records
    When the coverage is parsed
    Then file A's coverage contains none of file B's line numbers
    And file B's coverage contains none of file A's line numbers
