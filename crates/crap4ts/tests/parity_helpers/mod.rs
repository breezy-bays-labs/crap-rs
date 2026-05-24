//! Pure-Rust parse + diff for the crap4ts@1.x parity oracle (W3.2 #190).
//!
//! No node/pnpm. Consumes the committed `crap4ts-v1-reference.json`
//! (captured once during W3.1) and a live crap4ts@2 `--format json`
//! envelope, matches per function, and classifies every divergence.
//!
//! ## The oracle carries no contributor breakdown
//!
//! crap4ts@1.x's JSON emits a per-function cyclomatic *number* but no
//! per-contributor list. So the harness cannot diff a v1 contributor
//! list against v2's — there is none. Instead, divergence reports
//! surface v2's contributor breakdown (kind × count) so triaging a
//! score disagreement is one line, not a manual AST read (the CPO
//! sharpening). The README/plan "±1 line contributor drift" tolerance
//! presupposes oracle contributor data the v1.x format does not carry;
//! that is a documented surviving limitation, not a harness gap.
//!
//! ## Classification (per the W3.1 fixture README, post-#272)
//!
//! Every matched function falls into exactly one bucket. Score parity
//! is primary; the risk label is a *derived* attribute of the score
//! (it lives in `classify_risk`) and is not pinned across the v1.x
//! boundary — recalibrating risk tiers would otherwise force a new
//! exemption bucket for every future move. Risk-label correctness is
//! verified separately by unit tests of `classify_risk` itself.
//!
//! - **Match** — score within tolerance (same CC, |Δcrap| ≤ 0.5,
//!   coverage stable). Pass.
//! - **ThresholdDefaultChange** — score unchanged, only the pass/fail
//!   gate flips because the oracle used per-glob defaults and
//!   crap4ts@2 uses a different calibrated threshold. Pass (an
//!   intentional calibration break, not a regression).
//! - **Crap37Improvement** — v1.x reported 0% coverage where v2 reports
//!   real coverage on the same complexity. This is the crap4ts#37 v1.x
//!   span-overlap-ratio matcher bug; v2's strict line-range containment
//!   is structurally immune. Pass, logged as an improvement.
//! - **Crap252Improvement** — v1.x's per-function rollup conflated
//!   multi-statement source lines; v2's MIN aggregation reports the
//!   true per-line coverage. Pass.
//! - **ScoreRegression** — anything else: complexity differs, coverage
//!   differs without being a #37/#252 improvement, or |Δcrap| > 0.5
//!   with no benign explanation. Fail.

use std::collections::BTreeMap;

use serde::Deserialize;

/// `|Δcrap|` within this absolute band counts as "score unchanged"
/// (per the W3.1 README tolerance table).
pub const CRAP_EPS: f64 = 0.5;

/// Coverage delta within this band counts as "coverage unchanged". The
/// oracle rounds coverage to 2 decimals; crap4ts@2 emits full `f64`,
/// so an exact-equal test would spuriously fail on rounding alone.
pub const COV_EPS: f64 = 0.1;

/// Minimum fraction of matched functions that must exact-match
/// cyclomatic complexity (W3.1 README: "≥ 95%").
pub const MIN_EXACT_CC_RATE: f64 = 0.95;

// ── Oracle (crap4ts@1.x reference JSON) ──────────────────────────────

#[derive(Debug, Deserialize)]
struct OracleRoot {
    functions: Vec<OracleEntry>,
}

#[derive(Debug, Deserialize)]
struct OracleEntry {
    scored: OracleScored,
    // The oracle's per-function threshold is deliberately not read:
    // threshold-default-change is detected from the `exceeds` gate
    // verdict each side computed against its own threshold, not from
    // comparing raw threshold numbers. serde ignores the unread field.
    exceeds: bool,
}

#[derive(Debug, Deserialize)]
struct OracleScored {
    identity: OracleIdentity,
    #[serde(rename = "cyclomaticComplexity")]
    cyclomatic_complexity: u32,
    #[serde(rename = "coveragePercent")]
    coverage_percent: f64,
    crap: OracleCrap,
}

#[derive(Debug, Deserialize)]
struct OracleIdentity {
    #[serde(rename = "filePath")]
    file_path: String,
    #[serde(rename = "qualifiedName")]
    qualified_name: String,
    span: OracleSpan,
}

#[derive(Debug, Deserialize)]
struct OracleSpan {
    #[serde(rename = "startLine")]
    start_line: i64,
}

#[derive(Debug, Deserialize)]
struct OracleCrap {
    value: f64,
    #[serde(rename = "riskLevel")]
    risk_level: String,
}

// ── crap4ts@2 (`--format json` envelope) ─────────────────────────────

#[derive(Debug, Deserialize)]
struct V2Root {
    result: V2Result,
}

#[derive(Debug, Deserialize)]
struct V2Result {
    functions: Vec<V2Entry>,
}

#[derive(Debug, Deserialize)]
struct V2Entry {
    scored: V2Scored,
    // crap4ts@2's per-function threshold is likewise unread — same
    // rationale as `OracleEntry`: the `exceeds` verdict is the signal.
    exceeds: bool,
}

#[derive(Debug, Deserialize)]
struct V2Scored {
    identity: V2Identity,
    complexity: u32,
    coverage_percent: f64,
    crap: V2Crap,
    #[serde(default)]
    contributors: Vec<V2Contributor>,
}

#[derive(Debug, Deserialize)]
struct V2Identity {
    file_path: String,
    qualified_name: String,
    span: V2Span,
}

#[derive(Debug, Deserialize)]
struct V2Span {
    start_line: i64,
}

#[derive(Debug, Deserialize)]
struct V2Crap {
    value: f64,
    risk_level: String,
}

#[derive(Debug, Deserialize, Clone)]
struct V2Contributor {
    kind: String,
}

// ── Normalized records ───────────────────────────────────────────────

/// A function as seen by either side, normalized for cross-version
/// matching. `file` is forward-slash, `src/`-prefix-stripped.
#[derive(Debug, Clone)]
pub struct FnRecord {
    pub file: String,
    pub name: String,
    pub start_line: i64,
    pub cc: u32,
    pub coverage: f64,
    pub crap: f64,
    pub risk: String,
    /// Did this side's threshold gate mark the function as exceeding?
    pub exceeds: bool,
    /// v2 only — empty for the oracle (v1.x emits no contributor list).
    pub contributors: Vec<String>,
}

/// Strip a single leading `src/` so oracle paths
/// (`src/adapters/x.ts`) align with crap4ts@2 `--src`-relative paths
/// (`adapters/x.ts`). Backslashes are folded to `/` first so a
/// Windows-emitted path matches the forward-slash oracle regardless of
/// the platform the matrix runs on.
fn norm_path(p: &str) -> String {
    let p = p.replace('\\', "/");
    p.strip_prefix("src/").unwrap_or(&p).to_string()
}

/// Parse the committed oracle JSON. Panics with context on malformed
/// input — the oracle is a committed fixture, so a parse failure is a
/// fixture defect, not a runtime condition to recover from.
pub fn parse_oracle(json: &str) -> Vec<FnRecord> {
    let root: OracleRoot =
        serde_json::from_str(json).expect("crap4ts-v1-reference.json is valid oracle JSON");
    root.functions
        .into_iter()
        .map(|e| FnRecord {
            file: norm_path(&e.scored.identity.file_path),
            name: e.scored.identity.qualified_name,
            start_line: e.scored.identity.span.start_line,
            cc: e.scored.cyclomatic_complexity,
            coverage: e.scored.coverage_percent,
            crap: e.scored.crap.value,
            risk: e.scored.crap.risk_level,
            exceeds: e.exceeds,
            contributors: Vec::new(),
        })
        .collect()
}

/// Parse a crap4ts@2 `--format json` envelope (`.result.functions`).
pub fn parse_v2(json: &str) -> Vec<FnRecord> {
    let root: V2Root =
        serde_json::from_str(json).expect("crap4ts --format json emits a valid envelope");
    root.result
        .functions
        .into_iter()
        .map(|e| FnRecord {
            file: norm_path(&e.scored.identity.file_path),
            name: e.scored.identity.qualified_name,
            start_line: e.scored.identity.span.start_line,
            cc: e.scored.complexity,
            coverage: e.scored.coverage_percent,
            crap: e.scored.crap.value,
            risk: e.scored.crap.risk_level,
            exceeds: e.exceeds,
            contributors: e.scored.contributors.into_iter().map(|c| c.kind).collect(),
        })
        .collect()
}

// ── Classification ───────────────────────────────────────────────────

/// Which bucket a matched (oracle, v2) pair falls into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Score within tolerance (same CC, |Δcrap| ≤ 0.5, coverage stable).
    /// Risk label is derived from score and not pinned here.
    Match,
    /// Score unchanged; only the pass/fail gate flips (oracle 8/12 vs
    /// crap4ts@2's calibrated default). Intentional calibration break
    /// — passes.
    ThresholdDefaultChange,
    /// v1.x reported 0% coverage on a function v2 covers (crap4ts#37
    /// v1.x matcher bug). Passes, logged as an improvement.
    Crap37Improvement,
    /// crap-rs#252: v1.x's per-function rollup OVERcounted coverage on
    /// any function whose body shared a source line with its `const`/
    /// `let`/`var` declaration (single-line arrows, function expressions,
    /// inline `array.map(arrow)` patterns, mixed bodies with declared +
    /// expression on the same line). v2's `line_coverage_for` MIN
    /// aggregation deflates per-line hits to the worst-statement value,
    /// catching the uncovered body even when the declaration ran at
    /// module load. Direction is consistent across the v1 corpus:
    /// `same_cc && v2.coverage ≤ v1.coverage && v2.crap ≥ v1.crap`.
    /// Risk class typically moves UP by one step (low → acceptable,
    /// acceptable → moderate) — the natural consequence of more honest
    /// coverage. Passes, logged as an improvement.
    Crap252Improvement,
    /// A genuine adapter divergence. Fails the parity gate.
    ScoreRegression,
}

impl Class {
    /// Does this classification pass the parity gate?
    pub fn is_pass(self) -> bool {
        !matches!(self, Class::ScoreRegression)
    }
}

/// One classified divergence, carrying both sides for the report.
#[derive(Debug, Clone)]
pub struct Divergence {
    pub file: String,
    pub name: String,
    pub class: Class,
    pub v1_cc: u32,
    pub v2_cc: u32,
    pub v1_cov: f64,
    pub v2_cov: f64,
    pub v1_crap: f64,
    pub v2_crap: f64,
    pub v1_risk: String,
    pub v2_risk: String,
    /// v2's contributor breakdown (`2× if-branch + 1× ternary`) — the
    /// actionable triage line. Empty when v2 reported none.
    pub v2_contributors: String,
}

/// Render `["if-branch","if-branch","ternary"]` as
/// `2× if-branch + 1× ternary` (kinds in first-seen order).
fn summarize_contributors(kinds: &[String]) -> String {
    if kinds.is_empty() {
        return "(none)".to_string();
    }
    let mut order: Vec<String> = Vec::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for k in kinds {
        if !counts.contains_key(k) {
            order.push(k.clone());
        }
        *counts.entry(k.clone()).or_insert(0) += 1;
    }
    order
        .iter()
        .map(|k| format!("{}× {}", counts[k], k))
        .collect::<Vec<_>>()
        .join(" + ")
}

fn classify(v1: &FnRecord, v2: &FnRecord) -> Class {
    let same_cc = v1.cc == v2.cc;
    let cov_unchanged = (v1.coverage - v2.coverage).abs() <= COV_EPS;
    let crap_unchanged = (v1.crap - v2.crap).abs() <= CRAP_EPS;

    // crap4ts#37: v1.x's 80%-overlap matcher reported literal 0%
    // coverage where real test data existed; v2's line-range
    // containment recovers it. Same complexity + v1 0% + v2 > 0%.
    if same_cc && v1.coverage == 0.0 && v2.coverage > 0.0 {
        return Class::Crap37Improvement;
    }

    // crap-rs#252: v1.x's per-function rollup conflated multi-statement
    // lines. Every Istanbul statement at the same source line was
    // emitted as a separate covered-line record, so:
    //   - For single-line arrows where the body shared its `const`
    //     declaration's line, an uncovered body was masked by the
    //     module-load hit on the declaration (the cube case →
    //     coverage moves DOWN under MIN).
    //   - For functions whose span carried lines with multiple
    //     uncovered duplicate statements, the duplicates inflated the
    //     denominator without contributing to covered count
    //     (coverage moves UP under MIN as the phantom uncovered
    //     statements collapse).
    // v2's `line_coverage_for` MIN aggregation deflates each line to
    // its worst-statement hit count, so both directions land on the
    // more accurate per-line answer.
    //
    // The signature is structural: same CC (no walker change), a
    // coverage drift > `COV_EPS` (the change is real, not noise), and
    // CRAP movement in the direction consistent with coverage's
    // direction (CRAP is monotone-decreasing in coverage at fixed CC,
    // so a coverage rise must not push CRAP up by more than the
    // tolerance band, and a coverage drop must not push CRAP down by
    // more than the band either). CC mismatches stay in
    // `ScoreRegression` so walker drifts can't slip through here.
    if same_cc && (v2.coverage - v1.coverage).abs() > COV_EPS {
        let crap_consistent_with_coverage_direction = if v2.coverage > v1.coverage {
            // Coverage rose → CRAP should stay flat or fall (never rise
            // beyond noise). Bounded by `CRAP_EPS`.
            v2.crap <= v1.crap + CRAP_EPS
        } else {
            // Coverage fell → CRAP should stay flat or rise (never fall
            // beyond noise). Bounded by `CRAP_EPS`.
            v2.crap >= v1.crap - CRAP_EPS
        };
        if crap_consistent_with_coverage_direction {
            return Class::Crap252Improvement;
        }
    }

    if same_cc && cov_unchanged && crap_unchanged {
        // Score is unchanged. The risk label is a *derived* attribute
        // of the score — it lives in `classify_risk` and shifts when
        // tier boundaries are recalibrated (e.g. #272). Pinning risk
        // labels across the v1.x boundary would force a new exemption
        // bucket for every future calibration; instead the parity
        // contract treats score parity as primary and risk parity as
        // downstream.
        //
        // If the gate verdict flipped while the score held, that is
        // the threshold default moving (oracle 8/12 → crap4ts@2 15);
        // a real regression would have moved the score too.
        if v1.exceeds != v2.exceeds {
            return Class::ThresholdDefaultChange;
        }
        // Score stable → clean parity, regardless of derived risk
        // label. Risk-label correctness is verified separately, by
        // unit tests of `classify_risk` (see crap-core).
        return Class::Match;
    }

    Class::ScoreRegression
}

// ── Report ───────────────────────────────────────────────────────────

/// The full parity outcome. `gate_passes()` is the single source of
/// truth for the test assertion; `render()` is the human-facing diff.
///
/// `Clone` lets a cucumber harness cache one real-corpus run in a
/// `OnceLock` and hand each scenario its own owned copy.
#[derive(Debug, Clone)]
pub struct ParityReport {
    pub matched: usize,
    pub exact_cc: usize,
    /// Oracle functions with no crap4ts@2 match — a discovery failure
    /// (crap4ts@2 must discover a superset of the oracle).
    pub v1_only: Vec<String>,
    /// crap4ts@2 functions absent from the oracle. Informational:
    /// v2's walker is more thorough; a superset is expected, not a
    /// regression.
    pub v2_only_count: usize,
    pub divergences: Vec<Divergence>,
}

impl ParityReport {
    pub fn exact_cc_rate(&self) -> f64 {
        if self.matched == 0 {
            return 0.0;
        }
        self.exact_cc as f64 / self.matched as f64
    }

    /// Divergences that fail the gate (genuine regressions only).
    pub fn regressions(&self) -> Vec<&Divergence> {
        self.divergences
            .iter()
            .filter(|d| !d.class.is_pass())
            .collect()
    }

    fn count(&self, c: Class) -> usize {
        self.divergences.iter().filter(|d| d.class == c).count()
    }

    /// The hierarchical tolerance gate (W3.1 README + CQO ADVISORY-4):
    /// every oracle function discovered, ≥95% exact CC, zero genuine
    /// score-regressions. Risk labels are derived from the score (via
    /// `classify_risk`) and verified by that function's own unit tests
    /// — they are not part of the cross-version parity contract, so a
    /// tier recalibration that moves labels without moving scores does
    /// not trip this gate.
    pub fn gate_passes(&self) -> bool {
        self.v1_only.is_empty()
            && self.exact_cc_rate() >= MIN_EXACT_CC_RATE
            && self.regressions().is_empty()
    }

    /// Structured, copy-pasteable diff. Lists every non-`Match`
    /// divergence with both sides + v2's contributor breakdown, then a
    /// follow-up recommendation for any genuine regression (feature
    /// scenario 5: a divergence not explained by a benign bucket
    /// recommends a tracked follow-up under epic #173).
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "parity: {} oracle functions, {} matched ({} exact CC = {:.1}%), \
             {} v1-only, {} v2-only\n",
            self.matched + self.v1_only.len(),
            self.matched,
            self.exact_cc,
            self.exact_cc_rate() * 100.0,
            self.v1_only.len(),
            self.v2_only_count,
        ));
        s.push_str(&format!(
            "  buckets: {} match · {} threshold-default-change · \
             {} crap4ts#37-improvement · {} crap-rs#252-improvement · \
             {} score-regression\n",
            self.count(Class::Match),
            self.count(Class::ThresholdDefaultChange),
            self.count(Class::Crap37Improvement),
            self.count(Class::Crap252Improvement),
            self.count(Class::ScoreRegression),
        ));

        if !self.v1_only.is_empty() {
            s.push_str("\n  DISCOVERY FAILURE — oracle functions crap4ts@2 did not find:\n");
            for k in &self.v1_only {
                s.push_str(&format!("    - {k}\n"));
            }
        }

        let interesting: Vec<&Divergence> = self
            .divergences
            .iter()
            .filter(|d| d.class != Class::Match)
            .collect();
        if !interesting.is_empty() {
            s.push_str("\n  per-function divergences:\n");
            for d in interesting {
                let tag = match d.class {
                    Class::Match => "match",
                    Class::ThresholdDefaultChange => "threshold-default-change",
                    Class::Crap37Improvement => "crap4ts#37-improvement",
                    Class::Crap252Improvement => "crap-rs#252-improvement",
                    Class::ScoreRegression => "SCORE-REGRESSION",
                };
                s.push_str(&format!(
                    "    [{tag}] {}::{}\n      v1: cc={} cov={:.2}% crap={:.2} risk={}\n      \
                     v2: cc={} cov={:.2}% crap={:.2} risk={}  contributors: {}\n",
                    d.file,
                    d.name,
                    d.v1_cc,
                    d.v1_cov,
                    d.v1_crap,
                    d.v1_risk,
                    d.v2_cc,
                    d.v2_cov,
                    d.v2_crap,
                    d.v2_risk,
                    d.v2_contributors,
                ));
            }
        }

        let regs = self.regressions();
        if !regs.is_empty() {
            s.push_str(
                "\n  ACTION: the following are unexplained regressions — file a \
                 follow-up under epic #173 with the function name + v2 \
                 contributor breakdown:\n",
            );
            for d in regs {
                s.push_str(&format!(
                    "    - {}::{} (v1 crap {:.2} → v2 {:.2}; v2 contributors: {})\n",
                    d.file, d.name, d.v1_crap, d.v2_crap, d.v2_contributors,
                ));
            }
        }
        s
    }
}

/// Match oracle → v2 by `(file, qualified_name)`, choosing the nearest
/// v2 candidate by start line and requiring ≤ 1 line drift (half-open
/// span boundary tolerance). Classify every matched pair; collect
/// unmatched oracle functions as discovery failures.
pub fn diff(oracle: &[FnRecord], v2: &[FnRecord]) -> ParityReport {
    // (file, name) → candidate indices into `v2`, so duplicate
    // anonymous functions in one file (`<arrow>` ×N) are disambiguated
    // by start line.
    let mut v2_index: BTreeMap<(&str, &str), Vec<usize>> = BTreeMap::new();
    for (i, f) in v2.iter().enumerate() {
        v2_index
            .entry((f.file.as_str(), f.name.as_str()))
            .or_default()
            .push(i);
    }

    let mut report = ParityReport {
        matched: 0,
        exact_cc: 0,
        v1_only: Vec::new(),
        v2_only_count: 0,
        divergences: Vec::new(),
    };

    // Each v2 record is consumed by at most one oracle entry. Without
    // this, a future regression where v2 drops a function but a sibling
    // (same file+name, start line within ±1) absorbs two oracle entries
    // would be silently masked — both oracle entries "match" the same
    // surviving v2 record and the dropped one never surfaces as v1_only.
    let mut consumed: std::collections::HashSet<usize> = std::collections::HashSet::new();

    for o in oracle {
        let key = (o.file.as_str(), o.name.as_str());
        let cand = v2_index.get(&key).and_then(|idxs| {
            idxs.iter()
                .copied()
                .filter(|&i| !consumed.contains(&i))
                .filter(|&i| (v2[i].start_line - o.start_line).abs() <= 1)
                .min_by_key(|&i| (v2[i].start_line - o.start_line).abs())
        });
        match cand {
            None => report.v1_only.push(format!("{}::{}", o.file, o.name)),
            Some(mi) => {
                consumed.insert(mi);
                let m = &v2[mi];
                report.matched += 1;
                if o.cc == m.cc {
                    report.exact_cc += 1;
                }
                let class = classify(o, m);
                report.divergences.push(Divergence {
                    file: o.file.clone(),
                    name: o.name.clone(),
                    class,
                    v1_cc: o.cc,
                    v2_cc: m.cc,
                    v1_cov: o.coverage,
                    v2_cov: m.coverage,
                    v1_crap: o.crap,
                    v2_crap: m.crap,
                    v1_risk: o.risk.clone(),
                    v2_risk: m.risk.clone(),
                    v2_contributors: summarize_contributors(&m.contributors),
                });
            }
        }
    }

    // v2 records never consumed by a 1-to-1 oracle match. crap4ts@2's
    // walker is a deliberate superset of v1.x, so this is the genuine
    // extra-discovery delta — not a `(file, name)`-existence heuristic,
    // which undercounts whenever v2 adds a function sharing a name with
    // an oracle entry (common for anonymous `<arrow>` / nested fns).
    report.v2_only_count = v2.len() - report.matched;

    report
}
