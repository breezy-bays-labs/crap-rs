Feature: crap4rs init subcommand

  `crap4rs init` writes a starter `crap.toml` to the current directory.
  As of the init rewrite, the output is a single deterministic document:
  the exhaustive annotated config reference (every supported option
  present, each annotated with an explanatory comment). The same content
  is emitted regardless of CLI flags, piped stdin, or the project's
  directory layout — there is no auto-detect and no interactive prompt.
  `--non-interactive` is retained for back-compat but no longer changes
  the output. `--force` overwrites an existing config; without it, init
  refuses to clobber. crap4ts inherits the same behavior via `AdapterMeta`
  and writes the same canonical `crap.toml`.

  The generated config is self-documenting (inline comments explain each
  option) and round-trips through `load_file_config` without errors. It
  sets `threshold` live and leaves `preset` as a commented alternative,
  so a freshly generated config has no top-level `preset` key.

  # ── Deterministic annotated dump ───────────────────────────────────

  @wired
  Scenario: init emits the fixed annotated dump, ignoring project layout
    Given a project directory with a "crates" subdirectory
    When the operator runs `crap4rs init --non-interactive`
    Then a file named "crap.toml" exists in the project directory
    And the config file contains "exhaustive annotated config reference"
    And the config file contains 'src = ["crates/core/src", "crates/cli/src"]'
    And the config file contains 'threshold = 15.0'
    And the exit code is 0

  @wired
  Scenario: stdin is ignored — there is no interactive prompt to answer
    Given an empty project directory
    When the operator runs `crap4rs init` with stdin "s\n"
    Then the config file contains 'threshold = 15.0'
    And the config file does not contain 'preset = "strict"'
    And the config file does not contain 'preset = "lenient"'
    And the exit code is 0

  @wired
  Scenario: generated config lists common exclude patterns live (not commented out)
    Given an empty project directory
    When the operator runs `crap4rs init --non-interactive`
    Then the config file contains 'exclude = ["tests/**"'
    And the config file contains "benches/**"
    And the config file contains "examples/**"
    And the config file does not contain "# exclude = ["

  @wired
  Scenario: generated config documents every section with header comments
    Given an empty project directory
    When the operator runs `crap4rs init --non-interactive`
    Then the config file contains "# crap.toml"
    And the config file contains "exhaustive annotated config reference"
    And the config file contains "[language.rust]"
    And the config file contains "[language.typescript]"
    And the config file contains "[output]"
    And the config file contains "title ="

  @wired
  Scenario: generated config round-trips through the loader with threshold live and no preset
    Given an empty project directory
    When the operator runs `crap4rs init --non-interactive`
    Then the generated config file loads without error
    And the config file contains 'threshold = 15.0'
    And the loaded config has no top-level preset

  # ── Collision handling ────────────────────────────────────────────

  @wired
  Scenario: refuses to overwrite an existing config without --force
    Given a project directory with an existing "crap.toml" containing 'preset = "lenient"'
    When the operator runs `crap4rs init --non-interactive`
    Then the exit code is 2
    And stderr contains "crap.toml already exists"
    And stderr contains "--force"
    And the config file still contains 'preset = "lenient"'

  @wired
  Scenario: --force overwrites an existing config
    Given a project directory with an existing "crap.toml" containing 'preset = "lenient"'
    When the operator runs `crap4rs init --non-interactive --force`
    Then the exit code is 0
    And the config file contains 'threshold = 15.0'
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
