//! Cucumber-rs runner for `@wired` scenarios in
//! `tests/features/sarif_reporter.feature` (issue #70 / `--format sarif`).
//!
//! This harness pins the CLI-process contracts the running binary uniquely
//! captures: the SARIF v2.1.0 envelope, the driver version stamped from the
//! real binary, results derived from the unshapeable full analysis (so
//! `--top` / `--only-failing` / `--sort-by` never truncate / shrink /
//! reorder them), GitHub-compatible repo-relative locations, byte-stable
//! output, and the cross-format guarantee that `properties.diagnostic`
//! mirrors the `--format advice` wire shape.
//!
//! The reporter's pure mapping logic — RiskLevel → SARIF severity, the
//! column emit/omit branches, the `{file}:{qualified_name}` fingerprint
//! format string — is owned by `crap-core`'s `sarif.rs` unit + proptest
//! suite. The diagnostic *content* (extract_function candidates,
//! exactly-one-recommended) is owned by `domain::diagnostic`. So those
//! cases live there, not here (see `AGENTS.md` § BDD hygiene). Absorbs
//! `sarif_reporter_integration.rs` (which shelled the binary →
//! contributed no lib coverage; safe to fold).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use cucumber::{World, given, then, when, writer};

const BINARY: &str = env!("CARGO_BIN_EXE_crap4rs");

/// 6 functions: 3 trivial (covered, low CRAP), 3 branchy (uncovered, high
/// CRAP). The three branchy functions exceed threshold 8; the three trivial
/// ones pass.
const MIXED_SRC: &str = "\
pub fn passing_a() -> i32 { 1 }
pub fn passing_b() -> i32 { 2 }
pub fn passing_c() -> i32 { 3 }
pub fn failing_a(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
pub fn failing_b(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
pub fn failing_c(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
";

const MIXED_LCOV: &str = "\
SF:lib.rs
DA:1,1
DA:2,1
DA:3,1
DA:4,0
DA:5,0
DA:6,0
end_of_record
";

/// All-passing fixture (3 trivial fns, fully covered) for the empty-results
/// case.
const PASSING_SRC: &str = "\
pub fn passing_a() -> i32 { 1 }
pub fn passing_b() -> i32 { 2 }
pub fn passing_c() -> i32 { 3 }
";

const PASSING_LCOV: &str = "\
SF:lib.rs
DA:1,1
DA:2,1
DA:3,1
end_of_record
";

#[derive(Debug, Default, World)]
struct SarifWorld {
    project_dir: Option<PathBuf>,
    _tempdir: Option<tempfile::TempDir>,
    last_cmd: Option<String>,
    output: Option<Output>,
}

impl SarifWorld {
    fn require_dir(&self) -> &Path {
        self.project_dir
            .as_deref()
            .expect("scenario did not set up a project directory")
    }

    fn require_output(&self) -> &Output {
        self.output
            .as_ref()
            .expect("scenario did not run the binary")
    }

    fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.require_output().stdout).into_owned()
    }

    fn fail_context(&self) -> String {
        let o = self.require_output();
        format!(
            "exit: {:?}\nstdout:\n{}\nstderr:\n{}",
            o.status.code(),
            self.stdout(),
            String::from_utf8_lossy(&o.stderr)
        )
    }

    fn json(&self) -> serde_json::Value {
        let out = self.stdout();
        serde_json::from_str(&out)
            .unwrap_or_else(|e| panic!("stdout was not valid JSON: {e}\n{}", self.fail_context()))
    }

    fn results(&self) -> Vec<serde_json::Value> {
        let root = self.json();
        at(&root, "runs.0.results")
            .as_array()
            .unwrap_or_else(|| panic!("runs.0.results is not an array; envelope:\n{root:#}"))
            .clone()
    }
}

fn write_project(src: &str, lcov: &str) -> (PathBuf, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().to_path_buf();
    std::fs::create_dir_all(path.join("src")).expect("create src dir");
    std::fs::write(path.join("src/lib.rs"), src).expect("write lib.rs");
    std::fs::write(path.join("lcov.info"), lcov).expect("write lcov.info");
    (path, dir)
}

/// Navigate a dotted path; numeric segments index arrays, string segments
/// index objects. Returns None (rather than panicking) on a miss.
fn try_at<'a>(root: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut cur = root;
    for key in path.split('.') {
        cur = match key.parse::<usize>() {
            Ok(idx) => cur.get(idx).or_else(|| cur.get(key))?,
            Err(_) => cur.get(key)?,
        };
    }
    Some(cur)
}

fn at<'a>(root: &'a serde_json::Value, path: &str) -> &'a serde_json::Value {
    try_at(root, path).unwrap_or_else(|| panic!("JSON path {path:?} missing; envelope:\n{root:#}"))
}

fn parse_args(cmd: &str) -> Vec<String> {
    cmd.split_whitespace().skip(1).map(str::to_string).collect()
}

fn run(dir: &Path, args: &[String]) -> Output {
    Command::new(BINARY)
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to invoke crap4rs binary at {BINARY:?}: {e}"))
}

fn result_uri(r: &serde_json::Value) -> &str {
    r["locations"][0]["physicalLocation"]["artifactLocation"]["uri"]
        .as_str()
        .unwrap_or_else(|| panic!("result missing locations[0]…artifactLocation.uri: {r}"))
}

// ── Given steps ──────────────────────────────────────────────────────

#[given("a project with several functions whose CRAP scores cross the threshold")]
fn given_mixed(world: &mut SarifWorld) {
    let (path, dir) = write_project(MIXED_SRC, MIXED_LCOV);
    world.project_dir = Some(path);
    world._tempdir = Some(dir);
}

#[given("every function is below the threshold")]
fn given_all_passing(world: &mut SarifWorld) {
    let (path, dir) = write_project(PASSING_SRC, PASSING_LCOV);
    world.project_dir = Some(path);
    world._tempdir = Some(dir);
}

// ── When step ────────────────────────────────────────────────────────

#[when(regex = r#"^the operator runs `([^`]+)`$"#)]
fn when_run(world: &mut SarifWorld, cmd: String) {
    let args = parse_args(&cmd);
    world.output = Some(run(world.require_dir(), &args));
    world.last_cmd = Some(cmd);
}

// ── Then steps: envelope shape ───────────────────────────────────────

#[then("stdout is parseable JSON")]
fn then_parseable(world: &mut SarifWorld) {
    let _ = world.json();
}

#[then(regex = r#"^the document at "([^"]+)" is "([^"]*)"$"#)]
fn then_doc_is(world: &mut SarifWorld, path: String, expected: String) {
    let root = world.json();
    let actual = at(&root, &path);
    assert_eq!(
        actual.as_str(),
        Some(expected.as_str()),
        "JSON path {path:?}: expected {expected:?}, got {actual}"
    );
}

#[then(regex = r#"^the document at "([^"]+)" matches the binary version$"#)]
fn then_doc_version(world: &mut SarifWorld, path: String) {
    let root = world.json();
    let actual = at(&root, &path);
    assert_eq!(
        actual.as_str(),
        Some(env!("CARGO_PKG_VERSION")),
        "JSON path {path:?}: expected binary version {:?}, got {actual}",
        env!("CARGO_PKG_VERSION")
    );
}

#[then(regex = r#"^the document at "([^"]+)" has (\d+) entr(?:y|ies)$"#)]
fn then_doc_len(world: &mut SarifWorld, path: String, n: usize) {
    let root = world.json();
    let arr = at(&root, &path)
        .as_array()
        .unwrap_or_else(|| panic!("JSON path {path:?} is not an array; envelope:\n{root:#}"));
    assert_eq!(
        arr.len(),
        n,
        "JSON path {path:?}: expected {n} entries, got {}",
        arr.len()
    );
}

#[then("every result carries the mandatory SARIF result fields")]
fn then_mandatory_fields(world: &mut SarifWorld) {
    let results = world.results();
    assert!(!results.is_empty(), "fixture must produce results");
    for r in &results {
        assert!(r["ruleId"].is_string(), "missing ruleId: {r}");
        let level = r["level"]
            .as_str()
            .unwrap_or_else(|| panic!("missing level: {r}"));
        assert!(
            matches!(level, "error" | "warning" | "note"),
            "level {level:?} is not a valid SARIF severity: {r}"
        );
        assert!(
            r["message"]["text"].is_string(),
            "missing message.text: {r}"
        );
        let locs = r["locations"]
            .as_array()
            .unwrap_or_else(|| panic!("missing locations array: {r}"));
        assert_eq!(locs.len(), 1, "expected exactly one location: {r}");
        assert!(
            r["locations"][0]["physicalLocation"].is_object(),
            "missing physicalLocation: {r}"
        );
        let fp = r["partialFingerprints"]["functionIdentity"]
            .as_str()
            .unwrap_or_else(|| panic!("missing partialFingerprints.functionIdentity: {r}"));
        // Cross-field consistency the unit test can't prove: the fingerprint
        // is built from the SAME repo-relative uri the location carries.
        let prefix = format!("{}:", result_uri(r));
        assert!(
            fp.starts_with(&prefix),
            "fingerprint {fp:?} should start with {prefix:?}"
        );
    }
}

// ── Then steps: gate keystone (display flags don't reshape SARIF) ─────

#[then(regex = r#"^the SARIF output is byte-identical to the same command without `([^`]+)`$"#)]
fn then_byte_identical_without(world: &mut SarifWorld, flag: String) {
    let cmd = world.last_cmd.clone().expect("no command was run");
    let stripped = cmd.replace(&format!(" {flag}"), "").replace(&flag, "");
    assert_ne!(stripped, cmd, "flag {flag:?} not found in command {cmd:?}");
    let baseline = run(world.require_dir(), &parse_args(&stripped));
    let baseline_out = String::from_utf8_lossy(&baseline.stdout);
    assert_eq!(
        world.stdout(),
        baseline_out,
        "{flag:?} must not change SARIF output\nwith flag:\n{}\nwithout flag:\n{}",
        world.stdout(),
        baseline_out
    );
}

// ── Then steps: exit code + streams ──────────────────────────────────

#[then(regex = r"^the exit code is (\d+)$")]
fn then_exit_code(world: &mut SarifWorld, expected: i32) {
    let actual = world
        .require_output()
        .status
        .code()
        .expect("process exited via signal");
    assert_eq!(
        actual,
        expected,
        "exit code mismatch — expected {expected}, got {actual}\n{}",
        world.fail_context()
    );
}

#[then("stdout is non-empty")]
fn then_stdout_nonempty(world: &mut SarifWorld) {
    assert!(
        !world.stdout().trim().is_empty(),
        "stdout was empty\n{}",
        world.fail_context()
    );
}

#[then("stderr is empty")]
fn then_stderr_empty(world: &mut SarifWorld) {
    let stderr = String::from_utf8_lossy(&world.require_output().stderr).into_owned();
    assert!(
        stderr.is_empty(),
        "stderr was not empty under --format sarif:\n{stderr}"
    );
}

// ── Then steps: GitHub-compatible locations ──────────────────────────

#[then(
    regex = r#"^every result's artifact URI is repo-relative with no "file://" or leading "/"$"#
)]
fn then_uri_repo_relative(world: &mut SarifWorld) {
    let results = world.results();
    assert!(!results.is_empty(), "fixture must produce results");
    for r in &results {
        let uri = result_uri(r);
        assert!(
            !uri.starts_with("file://"),
            "uri must not be a file:// URL: {uri}"
        );
        assert!(!uri.starts_with('/'), "uri must be repo-relative: {uri}");
        assert!(uri.ends_with(".rs"), "expected a .rs path, got {uri}");
    }
}

#[then("every region's startLine and endLine are 1-based with endLine at least startLine")]
fn then_region_lines(world: &mut SarifWorld) {
    let results = world.results();
    assert!(!results.is_empty(), "fixture must produce results");
    for r in &results {
        let region = &r["locations"][0]["physicalLocation"]["region"];
        let start = region["startLine"]
            .as_u64()
            .unwrap_or_else(|| panic!("region missing startLine: {region}"));
        let end = region["endLine"]
            .as_u64()
            .unwrap_or_else(|| panic!("region missing endLine: {region}"));
        assert!(start >= 1, "startLine must be 1-based, got {start}");
        assert!(
            end >= start,
            "endLine ({end}) must be >= startLine ({start})"
        );
    }
}

#[then(
    "every region carries startColumn and endColumn, both at least 1, with endColumn greater than startColumn"
)]
fn then_region_columns(world: &mut SarifWorld) {
    let results = world.results();
    assert!(!results.is_empty(), "fixture must produce results");
    for r in &results {
        let region = &r["locations"][0]["physicalLocation"]["region"];
        let start = region["startColumn"]
            .as_u64()
            .unwrap_or_else(|| panic!("region missing startColumn: {region}"));
        let end = region["endColumn"]
            .as_u64()
            .unwrap_or_else(|| panic!("region missing endColumn: {region}"));
        assert!(start >= 1, "startColumn must be 1-based, got {start}");
        assert!(end >= 1, "endColumn must be 1-based, got {end}");
        // MIXED_SRC failing fns are unindented single-line spans: the
        // closing `}` is well past column 1, so the wire-level exclusive
        // endColumn must strictly exceed startColumn.
        assert!(
            end > start,
            "endColumn ({end}) must exceed startColumn ({start}) on a single-line span"
        );
    }
}

// ── Then steps: diagnostic enrichment ────────────────────────────────

#[then(
    "every result's properties.diagnostic carries the coverage_gaps, complexity_drivers, suggested_actions, and root_cause fields"
)]
fn then_diagnostic_fields(world: &mut SarifWorld) {
    let results = world.results();
    assert!(!results.is_empty(), "fixture must produce results");
    for r in &results {
        let diag = r["properties"]["diagnostic"]
            .as_object()
            .unwrap_or_else(|| panic!("missing properties.diagnostic on result: {r}"));
        for field in [
            "coverage_gaps",
            "complexity_drivers",
            "suggested_actions",
            "root_cause",
        ] {
            assert!(
                diag.contains_key(field),
                "properties.diagnostic missing {field:?}: {diag:?}"
            );
        }
    }
}

#[then(
    regex = r#"^each result's properties.diagnostic equals the same function's diagnostic under `([^`]+)`$"#
)]
fn then_diagnostic_mirrors_advice(world: &mut SarifWorld, advice_cmd: String) {
    let sarif = world.json();
    let advice = {
        let out = run(world.require_dir(), &parse_args(&advice_cmd));
        // Fail fast with actionable context if the comparison run itself
        // fails — otherwise a non-zero advice exit surfaces only as an
        // opaque JSON parse error below. The command carries `--no-fail`,
        // so exit 0 is expected.
        assert!(
            out.status.success(),
            "advice comparison run failed (status {:?})\nstdout:\n{}\nstderr:\n{}",
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let s = String::from_utf8_lossy(&out.stdout).into_owned();
        serde_json::from_str::<serde_json::Value>(&s)
            .unwrap_or_else(|e| panic!("advice stdout was not valid JSON: {e}\n{s}"))
    };

    // SARIF: fingerprint ({file}:{qn}) → diagnostic.
    let mut sarif_diags: HashMap<String, serde_json::Value> = HashMap::new();
    for r in sarif["runs"][0]["results"].as_array().unwrap() {
        let key = r["partialFingerprints"]["functionIdentity"]
            .as_str()
            .unwrap()
            .to_string();
        sarif_diags.insert(key, r["properties"]["diagnostic"].clone());
    }

    // Advice: exceeding view.shown rows, keyed {file}:{qn} → diagnostic.
    let mut advice_diags: HashMap<String, serde_json::Value> = HashMap::new();
    for v in advice["view"]["shown"].as_array().unwrap() {
        if !v["exceeds"].as_bool().unwrap_or(false) {
            continue;
        }
        let file = v["scored"]["identity"]["file_path"].as_str().unwrap();
        let qn = v["scored"]["identity"]["qualified_name"].as_str().unwrap();
        advice_diags.insert(format!("{file}:{qn}"), v["diagnostic"].clone());
    }

    assert!(!sarif_diags.is_empty(), "SARIF produced no diagnostics");
    assert!(!advice_diags.is_empty(), "advice produced no diagnostics");
    // A mirror is a bijection, not a subset: both surfaces derive from the
    // same set of exceeding functions, so neither may carry a diagnostic the
    // other lacks. Assert key-set equality before comparing values.
    let sarif_keys: std::collections::BTreeSet<_> = sarif_diags.keys().cloned().collect();
    let advice_keys: std::collections::BTreeSet<_> = advice_diags.keys().cloned().collect();
    assert_eq!(
        sarif_keys, advice_keys,
        "SARIF and --format advice diagnostic key sets differ"
    );
    for (key, advice_diag) in &advice_diags {
        let sarif_diag = sarif_diags
            .get(key)
            .unwrap_or_else(|| panic!("SARIF missing result for {key}"));
        assert_eq!(
            sarif_diag, advice_diag,
            "SARIF properties.diagnostic must match --format advice for {key}"
        );
    }
}

// ── Then steps: determinism ──────────────────────────────────────────

#[then("running the same command again produces byte-identical SARIF")]
fn then_deterministic(world: &mut SarifWorld) {
    let cmd = world.last_cmd.clone().expect("no command was run");
    let again = run(world.require_dir(), &parse_args(&cmd));
    assert_eq!(
        world.stdout(),
        String::from_utf8_lossy(&again.stdout),
        "SARIF must be byte-deterministic across runs"
    );
}

#[then("every result's properties.diagnostic is present")]
fn then_diagnostic_present(world: &mut SarifWorld) {
    let results = world.results();
    assert!(!results.is_empty(), "fixture must produce results");
    for r in &results {
        assert!(
            r["properties"]["diagnostic"].is_object(),
            "result missing properties.diagnostic: {r}"
        );
    }
}

// ── Runner ──────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    SarifWorld::cucumber()
        .with_writer(writer::Libtest::or_basic())
        .filter_run_and_exit("tests/features/sarif_reporter.feature", |_, _, scenario| {
            scenario.tags.iter().any(|t| t == "wired")
        })
        .await;
}
