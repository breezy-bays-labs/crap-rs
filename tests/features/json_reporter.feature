Feature: JSON reporter

  The JSON reporter formats CRAP analysis results as structured JSON
  with a versioned envelope for CI pipelines and tooling consumption.

  # ── Envelope Structure ─────────────────────────────────────────────

  Scenario: JSON output contains a versioned envelope
    Given an analysis result
    When the JSON is formatted
    Then the output contains "schema_version" with value 2
    And the output contains "tool_version"
    And the output contains "language" with value "rust"
    And the output contains "timestamp" as an ISO 8601 string
    And the output contains "metric"
    And the output contains "threshold"

  Scenario: Analysis result is nested under "result" key
    Given an analysis result
    When the JSON is formatted
    Then the "result" object contains "functions"
    And the "result" object contains "summary"
    And the "result" object contains "passed"

  Scenario: Schema version is an integer
    Given an analysis result
    When the JSON is formatted
    Then "schema_version" is the integer 2

  # ── Result Content ─────────────────────────────────────────────────

  Scenario: Function entries contain all scored fields
    Given an analysis with one function "compute_crap" in "src/domain/crap.rs" with complexity 5, coverage 80.0%, and CRAP score 5.16
    When the JSON is formatted
    Then the functions array has one entry
    And the entry contains "identity" with "qualified_name" equal to "compute_crap"
    And the entry contains "identity" with "file_path" equal to "src/domain/crap.rs"
    And the entry contains "complexity" equal to 5
    And the entry contains "coverage_percent" equal to 80.0
    And the entry contains "crap" with "value" equal to 5.16
    And the entry contains "crap" with "risk_level" equal to "acceptable"
    And the entry contains "exceeds" equal to false

  Scenario: Summary contains aggregate statistics
    Given an analysis with 10 functions, 2 exceeding threshold
    When the JSON is formatted
    Then "result.summary.total_functions" equals 10
    And "result.summary.exceeding_threshold" equals 2
    And "result.summary.average_crap" is a number
    And "result.summary.median_crap" is a number

  Scenario: Summary contains risk distribution
    Given an analysis with distribution low=5 acceptable=3 moderate=1 high=1
    When the JSON is formatted
    Then "result.summary.distribution.low" equals 5
    And "result.summary.distribution.acceptable" equals 3
    And "result.summary.distribution.moderate" equals 1
    And "result.summary.distribution.high" equals 1

  Scenario: Passed reflects threshold compliance
    Given an analysis where all functions are within threshold
    When the JSON is formatted
    Then "result.passed" is true

  Scenario: Failed reflects threshold violations
    Given an analysis where 1 function exceeds the threshold
    When the JSON is formatted
    Then "result.passed" is false

  # ── Empty Results ──────────────────────────────────────────────────

  Scenario: Empty analysis produces valid JSON
    Given an analysis with no functions
    When the JSON is formatted
    Then the output is valid JSON
    And "result.functions" is an empty array
    And "result.summary.total_functions" equals 0
    And "result.passed" is true

  # ── Envelope Metadata ──────────────────────────────────────────────

  Scenario: Metric field reflects the configured complexity metric
    Given the analysis used cognitive complexity
    When the JSON is formatted with metric "cognitive"
    Then "metric" equals "cognitive"

  Scenario: Threshold field reflects the configured threshold
    Given the analysis used threshold 8.0
    When the JSON is formatted with threshold 8.0
    Then "threshold" equals 8.0

  Scenario: Timestamp is a valid ISO 8601 datetime
    Given an analysis result
    When the JSON is formatted
    Then "timestamp" is a valid ISO 8601 datetime
