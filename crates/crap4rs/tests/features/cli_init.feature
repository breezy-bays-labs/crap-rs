Feature: crap4rs init subcommand

  `crap4rs init` generates a starter `crap4rs.toml` in the current
  directory. Interactive by default (one prompt: threshold preset),
  `--non-interactive` for CI, `--force` to overwrite an existing
  config. crap4ts inherits the same behavior via `AdapterMeta`
  (`config_file_name`, `extensions`) and writes `crap4ts.toml`.

  The generated config is self-documenting (inline comments explain
  each option) and round-trips through `load_file_config` without
  errors. Auto-detect rules pick `src/` then `crates/` then fall
  back to `src` with a hint comment.

  # ── Non-interactive (CI path) ──────────────────────────────────────

  @wired
  Scenario: --non-interactive writes a default config in an empty directory
    Given an empty project directory
    When the operator runs `crap4rs init --non-interactive`
    Then a file named "crap4rs.toml" exists in the project directory
    And the config file contains 'preset = "default"'
    And the config file contains 'src = "src"'
    And the exit code is 0

  @wired
  Scenario: --non-interactive auto-detects single-crate src layout
    Given a project directory with a "src" subdirectory
    When the operator runs `crap4rs init --non-interactive`
    Then the config file contains 'src = "src"'
    And the config file does not contain "adjust if your sources live elsewhere"
    And the exit code is 0

  @wired
  Scenario: --non-interactive auto-detects workspace crates layout
    Given a project directory with a "crates" subdirectory but no "src" subdirectory
    When the operator runs `crap4rs init --non-interactive`
    Then the config file contains 'src = "crates"'
    And the config file does not contain "adjust if your sources live elsewhere"
    And the exit code is 0

  @wired
  Scenario: --non-interactive falls back to src and includes a hint comment when no layout matches
    Given an empty project directory
    When the operator runs `crap4rs init --non-interactive`
    Then the config file contains 'src = "src"'
    And the config file contains "adjust if your sources live elsewhere"

  @wired
  Scenario: generated config includes commented-out common exclude patterns
    Given an empty project directory
    When the operator runs `crap4rs init --non-interactive`
    Then the config file contains '# exclude = ['
    And the config file contains "tests/**"
    And the config file contains "benches/**"
    And the config file contains "examples/**"

  @wired
  Scenario: generated config includes header comments explaining each option
    Given an empty project directory
    When the operator runs `crap4rs init --non-interactive`
    Then the config file contains "# crap4rs.toml"
    And the config file contains "Threshold preset"
    And the config file contains "strict (8)"
    And the config file contains "default (15)"
    And the config file contains "lenient (25)"

  @wired
  Scenario: generated config round-trips through the loader
    Given an empty project directory
    When the operator runs `crap4rs init --non-interactive`
    Then the generated config file loads without error
    And the loaded config has preset "default"
    And the loaded config has src "src"

  # ── Collision handling ────────────────────────────────────────────

  @wired
  Scenario: refuses to overwrite an existing config without --force
    Given a project directory with an existing "crap4rs.toml" containing 'preset = "lenient"'
    When the operator runs `crap4rs init --non-interactive`
    Then the exit code is 2
    And stderr contains "crap4rs.toml already exists"
    And stderr contains "--force"
    And the config file still contains 'preset = "lenient"'

  @wired
  Scenario: --force overwrites an existing config
    Given a project directory with an existing "crap4rs.toml" containing 'preset = "lenient"'
    When the operator runs `crap4rs init --non-interactive --force`
    Then the exit code is 0
    And the config file contains 'preset = "default"'
    And the config file does not contain 'preset = "lenient"'

  # Cross-adapter parity for `crap4ts init` is exercised by the
  # plain integration test at `crates/crap4ts/tests/cli_init_integration.rs`
  # — the harness here cannot resolve `CARGO_BIN_EXE_crap4ts` because
  # that env var is only set for binaries in the same package.

  # ── Help text ─────────────────────────────────────────────────────

  @wired
  Scenario: init subcommand appears in --help output
    When the operator runs `crap4rs --help`
    Then stdout contains "init"
    And stdout contains "starter"

  @wired
  Scenario: init --help describes the flags
    When the operator runs `crap4rs init --help`
    Then stdout contains "--non-interactive"
    And stdout contains "--force"

  # ── Interactive path (stdin-driven prompt) ─────────────────────────
  #
  # The prompt reads a single line from stdin and maps the first char
  # to `ThresholdPreset` (s|S → Strict, l|L → Lenient, else → Default).
  # The harness pipes input via `Command::stdin(Stdio::piped())`; no
  # pty crate needed since the handler does no `isatty()` branching.
  # CI users pass `--non-interactive` to skip the prompt.

  @wired
  Scenario: interactive prompt accepts "s" for strict
    Given an empty project directory
    When the operator runs `crap4rs init` with stdin "s\n"
    Then the config file contains 'preset = "strict"'

  @wired
  Scenario: interactive prompt accepts "l" for lenient
    Given an empty project directory
    When the operator runs `crap4rs init` with stdin "l\n"
    Then the config file contains 'preset = "lenient"'

  @wired
  Scenario: interactive prompt defaults to "default" on empty input
    Given an empty project directory
    When the operator runs `crap4rs init` with stdin "\n"
    Then the config file contains 'preset = "default"'
