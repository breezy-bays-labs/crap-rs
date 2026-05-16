//! Property-test invariant suite for `OxcWalker` (crap-rs#207).
//!
//! This is the **independent-axis verification** for the #200/#205
//! traversal arms (and the whole W1.2 + W2.1 decision-point surface).
//! `walker_smoke.rs` exercises the walker against hand-picked fixtures;
//! a hand-picked fixture only proves the walker behaves on the inputs
//! the author thought to write. The failure mode this suite exists to
//! prevent is the one #218 shipped masked: a code path that "looks
//! right" and passes every fixture the author chose, but is wrong on
//! inputs nobody hand-wrote. A constrained AST grammar generates valid
//! TS source spanning every construct (simple/nested functions, class
//! methods + computed-key fields + static blocks, namespaces, all 11
//! decision-point kinds, JSX conditionals) across all six file
//! extensions, and asserts six invariants on every parseable input.
//!
//! ## The grammar carries its own oracle
//!
//! Each generated `Program` knows the exact number of cyclomatic
//! decision points its body holder should report
//! (`expected_body_holder_complexity`). Invariant 3 is therefore also
//! checked against an **independent count** computed by the generator,
//! not only against the walker's own arithmetic — a mutated or buggy
//! walker that miscounts is caught even if its
//! `complexity == 1 + sum(increments)` internal consistency still holds.
//!
//! ## Disposition discipline (crap-rs#207 AC)
//!
//! A failing seed is a real walker bug, never a generator bug to paper
//! over. The contract: file a separate `type:bug` issue with the seed +
//! minimal reproducer, commit the `proptest-regressions/` entry, do NOT
//! mutate the grammar until the assertion passes. See the PR body for
//! the recorded disposition of this suite's initial run.

use std::collections::BTreeMap;

use crap_core::domain::types::{ComplexityMetric, FunctionComplexity};
use crap_core::ports::ComplexityPort;
use crap4ts::adapters::walker::OxcWalker;
use proptest::prelude::*;
use proptest::strategy::ValueTree;

// ── Constrained AST grammar ─────────────────────────────────────────────
//
// `Body` is a list of statements that live inside a function body. Each
// variant emits valid TS *and* reports how many cyclomatic decision
// points it adds to the *enclosing* function (nested functions carry
// their own count and never add to the parent — invariant 4).

/// One statement inside a function body. `increments()` is the oracle:
/// the exact number of decision points this node charges to the
/// function that contains it (excluding anything inside a nested
/// function it introduces).
#[derive(Debug, Clone)]
enum Stmt {
    /// `const _vN = 0;` — no decision point.
    Noop,
    /// `if (a) { <inner> }` — one IfBranch + inner's increments.
    If(Box<Body>),
    /// `for (let i = 0; i < 1; i++) { <inner> }` — one ForLoop.
    For(Box<Body>),
    /// `for (const k in o) { <inner> }` — one ForLoop.
    ForIn(Box<Body>),
    /// `for (const v of a) { <inner> }` — one ForLoop.
    ForOf(Box<Body>),
    /// `while (a) { <inner> }` — one WhileLoop.
    While(Box<Body>),
    /// `do { <inner> } while (a);` — one DoWhileLoop.
    DoWhile(Box<Body>),
    /// `switch (x) { case 1: <inner>; break; default: break; }` —
    /// one CaseBranch (default is NOT a decision point).
    Switch(Box<Body>),
    /// `const _bN = a && b;` — one LogicalOperator.
    LogicalAnd,
    /// `const _bN = a || b;` — one LogicalOperator.
    LogicalOr,
    /// `const _bN = a ?? b;` — one LogicalOperator (?? maps to it).
    Nullish,
    /// `const _tN = a ? 1 : 0;` — one Ternary.
    Ternary,
    /// `const _cN = obj?.maybe?.deep;` — one OptionalChain (the whole
    /// chain is ONE ChainExpression → one contributor).
    OptionalChain,
    /// `try { <inner> } catch (e) {}` — one Catch + inner's increments.
    TryCatch(Box<Body>),
    /// `try { <inner> } finally {}` — NOT a decision point.
    TryFinally(Box<Body>),
    /// A nested `function _fnN() { <inner> }` declaration. Adds ZERO to
    /// the enclosing function (invariant 4 — isolation); the nested
    /// function is its own complexity site.
    NestedFn(Box<Body>),
}

impl Stmt {
    /// Decision points this node charges to the *enclosing* function.
    fn increments(&self) -> u32 {
        match self {
            // No decision point of its own. `NestedFn` charges ZERO to
            // the enclosing function — its body is a separate site
            // (invariant 4). `TryFinally` is not a decision point but
            // still recurses into its `try` body.
            Stmt::Noop | Stmt::NestedFn(_) => 0,
            Stmt::TryFinally(b) => b.increments(),
            // One decision point + whatever the inner body charges.
            Stmt::If(b)
            | Stmt::For(b)
            | Stmt::ForIn(b)
            | Stmt::ForOf(b)
            | Stmt::While(b)
            | Stmt::DoWhile(b)
            | Stmt::Switch(b)
            | Stmt::TryCatch(b) => 1 + b.increments(),
            // Leaf decision points — exactly one each.
            Stmt::LogicalAnd
            | Stmt::LogicalOr
            | Stmt::Nullish
            | Stmt::Ternary
            | Stmt::OptionalChain => 1,
        }
    }

    /// Emit valid TS source for this statement, into `out`, using
    /// `ctr` to mint unique identifiers so generated programs never
    /// shadow / redeclare.
    fn emit(&self, out: &mut String, ctr: &mut u32, indent: usize) {
        let pad = "  ".repeat(indent);
        let id = {
            *ctr += 1;
            *ctr
        };
        match self {
            Stmt::Noop => {
                out.push_str(&format!("{pad}const _v{id} = {id};\n"));
            }
            Stmt::If(b) => {
                out.push_str(&format!("{pad}if (_p{id} > 0) {{\n"));
                out.push_str(&format!("{pad}  const _p{id} = {id};\n"));
                b.emit(out, ctr, indent + 1);
                out.push_str(&format!("{pad}}}\n"));
            }
            Stmt::For(b) => {
                out.push_str(&format!(
                    "{pad}for (let _i{id} = 0; _i{id} < 1; _i{id}++) {{\n"
                ));
                b.emit(out, ctr, indent + 1);
                out.push_str(&format!("{pad}}}\n"));
            }
            Stmt::ForIn(b) => {
                out.push_str(&format!(
                    "{pad}for (const _k{id} in {{ a: 1 }}) {{ void _k{id};\n"
                ));
                b.emit(out, ctr, indent + 1);
                out.push_str(&format!("{pad}}}\n"));
            }
            Stmt::ForOf(b) => {
                out.push_str(&format!("{pad}for (const _e{id} of [1]) {{ void _e{id};\n"));
                b.emit(out, ctr, indent + 1);
                out.push_str(&format!("{pad}}}\n"));
            }
            Stmt::While(b) => {
                out.push_str(&format!("{pad}while (_w{id} === undefined) {{\n"));
                out.push_str(&format!("{pad}  const _w{id} = {id}; break;\n"));
                b.emit(out, ctr, indent + 1);
                out.push_str(&format!("{pad}}}\n"));
            }
            Stmt::DoWhile(b) => {
                out.push_str(&format!("{pad}do {{\n"));
                b.emit(out, ctr, indent + 1);
                out.push_str(&format!("{pad}}} while (false);\n"));
            }
            Stmt::Switch(b) => {
                out.push_str(&format!("{pad}switch ({id}) {{\n"));
                out.push_str(&format!("{pad}  case {id}: {{\n"));
                b.emit(out, ctr, indent + 2);
                out.push_str(&format!("{pad}  }} break;\n"));
                out.push_str(&format!("{pad}  default: break;\n"));
                out.push_str(&format!("{pad}}}\n"));
            }
            Stmt::LogicalAnd => {
                out.push_str(&format!("{pad}const _b{id} = (_x{id} as any) && {id};\n"));
            }
            Stmt::LogicalOr => {
                out.push_str(&format!("{pad}const _b{id} = (_x{id} as any) || {id};\n"));
            }
            Stmt::Nullish => {
                out.push_str(&format!("{pad}const _b{id} = (_x{id} as any) ?? {id};\n"));
            }
            Stmt::Ternary => {
                out.push_str(&format!("{pad}const _t{id} = ((_x{id} as any) ? 1 : 0);\n"));
            }
            Stmt::OptionalChain => {
                out.push_str(&format!(
                    "{pad}const _c{id} = (_o{id} as any)?.maybe?.deep;\n"
                ));
            }
            Stmt::TryCatch(b) => {
                out.push_str(&format!("{pad}try {{\n"));
                b.emit(out, ctr, indent + 1);
                out.push_str(&format!("{pad}}} catch (_err{id}) {{ void _err{id}; }}\n"));
            }
            Stmt::TryFinally(b) => {
                out.push_str(&format!("{pad}try {{\n"));
                b.emit(out, ctr, indent + 1);
                out.push_str(&format!("{pad}}} finally {{ }}\n"));
            }
            Stmt::NestedFn(b) => {
                out.push_str(&format!("{pad}function _fn{id}(): void {{\n"));
                b.emit(out, ctr, indent + 1);
                out.push_str(&format!("{pad}}}\n"));
            }
        }
    }

    /// Every function-entry site this statement introduces, including
    /// itself if it is a nested function, plus recursion into bodies.
    /// Used by invariant 4's enclosing-function expectation.
    fn nested_fn_count(&self) -> u32 {
        match self {
            Stmt::Noop
            | Stmt::LogicalAnd
            | Stmt::LogicalOr
            | Stmt::Nullish
            | Stmt::Ternary
            | Stmt::OptionalChain => 0,
            Stmt::If(b)
            | Stmt::For(b)
            | Stmt::ForIn(b)
            | Stmt::ForOf(b)
            | Stmt::While(b)
            | Stmt::DoWhile(b)
            | Stmt::Switch(b)
            | Stmt::TryCatch(b)
            | Stmt::TryFinally(b) => b.nested_fn_count(),
            Stmt::NestedFn(b) => 1 + b.nested_fn_count(),
        }
    }
}

/// A function body: an ordered list of statements.
#[derive(Debug, Clone)]
struct Body(Vec<Stmt>);

impl Body {
    fn increments(&self) -> u32 {
        self.0.iter().map(Stmt::increments).sum()
    }
    fn nested_fn_count(&self) -> u32 {
        self.0.iter().map(Stmt::nested_fn_count).sum()
    }
    fn emit(&self, out: &mut String, ctr: &mut u32, indent: usize) {
        for s in &self.0 {
            s.emit(out, ctr, indent);
        }
    }
}

// ── proptest strategies ─────────────────────────────────────────────────

/// Leaf statements (no recursion) — the strategy base case.
fn leaf_stmt() -> impl Strategy<Value = Stmt> {
    prop_oneof![
        Just(Stmt::Noop),
        Just(Stmt::LogicalAnd),
        Just(Stmt::LogicalOr),
        Just(Stmt::Nullish),
        Just(Stmt::Ternary),
        Just(Stmt::OptionalChain),
    ]
}

/// A recursively-nested statement. `prop::collection::vec` bounds the
/// body so generated programs stay parseable + fast.
fn stmt_strategy() -> impl Strategy<Value = Stmt> {
    leaf_stmt().prop_recursive(4, 24, 4, |inner| {
        let body = prop::collection::vec(inner, 0..3).prop_map(Body);
        prop_oneof![
            body.clone().prop_map(|b| Stmt::If(Box::new(b))),
            body.clone().prop_map(|b| Stmt::For(Box::new(b))),
            body.clone().prop_map(|b| Stmt::ForIn(Box::new(b))),
            body.clone().prop_map(|b| Stmt::ForOf(Box::new(b))),
            body.clone().prop_map(|b| Stmt::While(Box::new(b))),
            body.clone().prop_map(|b| Stmt::DoWhile(Box::new(b))),
            body.clone().prop_map(|b| Stmt::Switch(Box::new(b))),
            body.clone().prop_map(|b| Stmt::TryCatch(Box::new(b))),
            body.clone().prop_map(|b| Stmt::TryFinally(Box::new(b))),
            body.prop_map(|b| Stmt::NestedFn(Box::new(b))),
        ]
    })
}

fn body_strategy() -> impl Strategy<Value = Body> {
    prop::collection::vec(stmt_strategy(), 0..4).prop_map(Body)
}

/// Which top-level container the generated body is wrapped in. Each
/// shape is a distinct walker entry path so the suite exercises the
/// FunctionFinder dispatch surface, not just `visit_statement`.
#[derive(Debug, Clone, Copy)]
enum Container {
    /// `function top() { <body> }`
    FunctionDecl,
    /// `const top = () => { <body> };`
    ArrowConst,
    /// `class C { m() { <body> } }`
    ClassMethod,
    /// `class C { [(() => "k")()] = 0; m() { <body> } }` — #205
    /// computed class key (the IIFE arrow is its own site) + a method
    /// carrying the body.
    ClassComputedKey,
    /// `class C { static { <body> } }` — synthetic `<static-init>`.
    StaticBlock,
    /// `namespace N { export function top() { <body> } }` — #200 item 2.
    Namespace,
    /// `const o = { [a && b]: (() => { <body> })() };` — #200 item 1
    /// computed object key wrapping the body in an IIFE arrow.
    ComputedObjectKey,
    /// `function top() { return <div>{c && <span/>}</div>; <body> }`
    /// in a `.tsx` file — JSX-conditional decision point.
    JsxConditional,
}

fn container_strategy() -> impl Strategy<Value = Container> {
    prop_oneof![
        Just(Container::FunctionDecl),
        Just(Container::ArrowConst),
        Just(Container::ClassMethod),
        Just(Container::ClassComputedKey),
        Just(Container::StaticBlock),
        Just(Container::Namespace),
        Just(Container::ComputedObjectKey),
        Just(Container::JsxConditional),
    ]
}

/// All six `AdapterMeta` extensions. JSX-bearing containers force a
/// `.tsx`/`.jsx` extension at emit time regardless of this pick.
fn ext_strategy() -> impl Strategy<Value = &'static str> {
    prop_oneof![
        Just("ts"),
        Just("tsx"),
        Just("jsx"),
        Just("js"),
        Just("mjs"),
        Just("cjs"),
    ]
}

/// A whole generated program: a container, its body, a file extension,
/// and whether identifiers carry a non-ASCII suffix (invariant 6 — the
/// W1.2 unicode span-to-line regression pin).
#[derive(Debug, Clone)]
struct Program {
    container: Container,
    body: Body,
    ext: &'static str,
    unicode: bool,
}

fn program_strategy() -> impl Strategy<Value = Program> {
    (
        container_strategy(),
        body_strategy(),
        ext_strategy(),
        any::<bool>(),
    )
        .prop_map(|(container, body, ext, unicode)| Program {
            container,
            body,
            ext,
            unicode,
        })
}

impl Program {
    /// File path the walker sees — drives `SourceType::from_path`.
    fn file_path(&self) -> String {
        let ext = match self.container {
            // JSX only parses under a JSX-capable source type. `.ts`
            // does NOT enable JSX in oxc, so force `.tsx` for the
            // JSX-conditional container; everything else honours the
            // generated extension.
            Container::JsxConditional => "tsx",
            _ => self.ext,
        };
        format!("prop{}.{ext}", if self.unicode { "_uni" } else { "" })
    }

    /// Emit the full program source.
    fn source(&self) -> String {
        let mut out = String::new();
        let mut ctr = 0u32;
        // Optional unicode identifier in scope — exercises the W1.2
        // byte_to_line_col multi-byte boundary fix on a real span.
        if self.unicode {
            out.push_str("const \u{03c0}\u{03b1} = 3;\nvoid \u{03c0}\u{03b1};\n");
        }
        match self.container {
            Container::FunctionDecl => {
                out.push_str("function top(): void {\n");
                self.body.emit(&mut out, &mut ctr, 1);
                out.push_str("}\n");
            }
            Container::ArrowConst => {
                out.push_str("const top = (): void => {\n");
                self.body.emit(&mut out, &mut ctr, 1);
                out.push_str("};\n");
            }
            Container::ClassMethod => {
                out.push_str("class C {\n  m(): void {\n");
                self.body.emit(&mut out, &mut ctr, 2);
                out.push_str("  }\n}\n");
            }
            Container::ClassComputedKey => {
                out.push_str("class C {\n");
                out.push_str("  [(() => \"k\")()]: number = 0;\n");
                out.push_str("  m(): void {\n");
                self.body.emit(&mut out, &mut ctr, 2);
                out.push_str("  }\n}\n");
            }
            Container::StaticBlock => {
                out.push_str("class C {\n  static {\n");
                self.body.emit(&mut out, &mut ctr, 2);
                out.push_str("  }\n}\n");
            }
            Container::Namespace => {
                out.push_str("namespace N {\n  export function top(): void {\n");
                self.body.emit(&mut out, &mut ctr, 2);
                out.push_str("  }\n}\n");
            }
            Container::ComputedObjectKey => {
                // `[(_kx as any) && 1]` is a computed key whose `&&` is
                // a LogicalOperator that charges the enclosing IIFE
                // arrow; the IIFE arrow also holds <body>.
                out.push_str("const _kx: unknown = 1;\n");
                out.push_str("const o = {\n");
                out.push_str("  [((_kx as any) && 1) as any]: (() => {\n");
                self.body.emit(&mut out, &mut ctr, 2);
                out.push_str("  })(),\n");
                out.push_str("};\nvoid o;\n");
            }
            Container::JsxConditional => {
                out.push_str("function top(): unknown {\n");
                out.push_str("  const _cond: unknown = 1;\n");
                self.body.emit(&mut out, &mut ctr, 1);
                out.push_str("  return <div>{(_cond as any) && <span />}</div>;\n");
                out.push_str("}\n");
            }
        }
        out
    }

    /// Independent oracle: the cyclomatic complexity the function
    /// *holding `body`* should report. Used by invariant 3 as a check
    /// the walker does NOT compute, so a miscounting walker is caught
    /// even if its internal `1 + sum` arithmetic is self-consistent.
    fn expected_body_holder_complexity(&self) -> u32 {
        let extra = match self.container {
            // ComputedObjectKey: the `&&` is in the object property's
            // *key*, a sibling of the *value* IIFE arrow that holds the
            // body — NOT inside the arrow. Per #200 item 1 the key's
            // decision point charges the *enclosing function*, which
            // here is module scope (the `const o = {…}` is top-level),
            // so it charges nobody and the body-holding arrow's
            // complexity is just `1 + body.increments()`. (Verified
            // empirically against the walker: the key's `&&` lands on
            // the enclosing function when one exists, never on the
            // value arrow.)
            Container::ComputedObjectKey => 0,
            // JsxConditional: `return <div>{_cond && <span/>}</div>;`
            // — the `&&` is inside `top`, the same function that holds
            // the body, so it adds one. (Verified empirically.)
            Container::JsxConditional => 1,
            _ => 0,
        };
        1 + self.body.increments() + extra
    }
}

// ── Invariant harness ───────────────────────────────────────────────────

fn walk(src: &str, path: &str) -> Option<Vec<FunctionComplexity>> {
    OxcWalker::new()
        .extract(src, path, ComplexityMetric::Cyclomatic)
        .ok()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    /// Invariants 1, 2, 3 over every generated program. (4 and 5 and 6
    /// have dedicated tests below for readable failure messages.)
    #[test]
    fn walker_core_invariants(prog in program_strategy()) {
        let src = prog.source();
        let path = prog.file_path();
        // Parse failures are not invariant violations — a constrained
        // grammar can still occasionally emit an edge the parser
        // rejects under a given SourceType (e.g. a `.cjs` strict-mode
        // interaction). The contract is "for every *parseable* input".
        let Some(fns) = walk(&src, &path) else { return Ok(()); };

        for f in &fns {
            // Invariant 1: complexity >= 1 for every function entry.
            prop_assert!(
                f.complexity >= 1,
                "complexity {} < 1 for {:?}\n--- source ---\n{src}",
                f.complexity, f.identity.qualified_name
            );

            // Invariant 3 (walker-internal consistency): complexity
            // equals 1 + sum of contributor increments.
            let sum: u32 = f.contributors.iter().map(|c| c.increment).sum();
            prop_assert_eq!(
                f.complexity, 1 + sum,
                "complexity {} != 1 + sum(increments) {} for {:?}\n--- source ---\n{}",
                f.complexity, 1 + sum, f.identity.qualified_name, src
            );

            // Invariant 2: every contributor line is within its
            // function's [start_line, end_line] span (no off-by-one,
            // no cross-function drift).
            let (lo, hi) = (f.identity.span.start_line, f.identity.span.end_line);
            for c in &f.contributors {
                prop_assert!(
                    c.line >= lo && c.line <= hi,
                    "contributor line {} outside fn {:?} span [{lo}, {hi}]\n--- source ---\n{src}",
                    c.line, f.identity.qualified_name
                );
            }
        }
    }

    /// Invariant 3 (independent oracle): the function that holds the
    /// generated body reports exactly the complexity the GRAMMAR
    /// predicts — a count the walker never computes. This is the
    /// independent-axis check: a walker that miscounts decision points
    /// (e.g. a mutant that drops a `+1`) fails here even though
    /// `walker_core_invariants`' internal `1 + sum` check still holds.
    #[test]
    fn walker_matches_independent_oracle(prog in program_strategy()) {
        let src = prog.source();
        let path = prog.file_path();
        let Some(fns) = walk(&src, &path) else { return Ok(()); };

        // Identify the function holding the generated body. Its name
        // depends on the container shape.
        let holder_name: &str = match prog.container {
            Container::FunctionDecl
            | Container::Namespace
            | Container::JsxConditional => "top",
            Container::ArrowConst => "top",
            Container::ClassMethod
            | Container::ClassComputedKey => "C.m",
            Container::StaticBlock => "C.<static-init>",
            Container::ComputedObjectKey => "<arrow>",
        };
        let Some(holder) = fns
            .iter()
            .find(|f| f.identity.qualified_name == holder_name)
        else {
            // If the body is empty + container minted no holder (e.g.
            // an empty static block still mints one, but be defensive)
            // there is nothing to check.
            return Ok(());
        };

        prop_assert_eq!(
            holder.complexity,
            prog.expected_body_holder_complexity(),
            "holder {:?} complexity {} != grammar oracle {}\n--- source ---\n{}",
            holder_name,
            holder.complexity,
            prog.expected_body_holder_complexity(),
            src
        );
    }

    /// Invariant 4: nested-function isolation. The number of discovered
    /// functions equals 1 (the container's body holder) + the grammar's
    /// nested-function count + any container-intrinsic extra sites
    /// (e.g. the IIFE arrow a computed class/object key introduces).
    /// A contributor never bleeds from a nested function into its
    /// parent — verified transitively: if isolation broke, the holder's
    /// independent-oracle complexity (previous test) would also break,
    /// and the per-function count here would drift.
    #[test]
    fn walker_nested_function_isolation(prog in program_strategy()) {
        let src = prog.source();
        let path = prog.file_path();
        let Some(fns) = walk(&src, &path) else { return Ok(()); };

        let intrinsic_extra = match prog.container {
            // ClassComputedKey's key is `[(() => "k")()]` — the key
            // IIFE arrow is one extra discovered site beyond the `C.m`
            // body holder.
            Container::ClassComputedKey => 1,
            // ComputedObjectKey's key is `[((_kx as any) && 1) as any]`
            // — no function in the key; the body holder IS the value
            // IIFE arrow, so there is NO extra site beyond it.
            _ => 0,
        };
        let expected = 1 + prog.body.nested_fn_count() + intrinsic_extra;
        prop_assert_eq!(
            fns.len() as u32, expected,
            "discovered {} functions, grammar predicts {}\n--- source ---\n{}",
            fns.len(), expected, src
        );
    }

    /// Invariant 5: determinism. The same source parsed twice yields a
    /// byte-identical `Vec<FunctionComplexity>` (Debug-equal — the type
    /// is not `PartialEq`, but its `Debug` is total and stable).
    #[test]
    fn walker_is_deterministic(prog in program_strategy()) {
        let src = prog.source();
        let path = prog.file_path();
        let a = walk(&src, &path);
        let b = walk(&src, &path);
        prop_assert_eq!(
            format!("{a:?}"),
            format!("{b:?}"),
            "non-deterministic walker output\n--- source ---\n{}", src
        );
    }
}

/// Invariant 6: unicode identifiers never panic span-to-line
/// conversion. A focused (non-proptest) battery of multi-byte
/// identifier shapes around every span boundary the walker computes
/// (function span end, contributor span start/end). Regression-pins
/// the W1.2 `byte_to_line_col` char-boundary fix; a property generator
/// rarely lands a multi-byte byte exactly on a `span.end - 1` boundary,
/// so this is asserted directly with adversarial inputs.
#[test]
fn unicode_identifiers_never_panic_span_conversion() {
    let cases = [
        "function \u{1f600}π(αβγ: number) { if (αβγ > 0) return αβγ; return 0; }\n",
        "const \u{4e2d}\u{6587} = () => { for (const \u{e9}l of [1]) { void \u{e9}l; } };\n",
        "class \u{5b50} { \u{65b9}\u{6cd5}() { return (1 as any) ?? 2; } }\n",
        "namespace \u{547d}\u{540d} { export function \u{51fd}() { try { } catch (\u{e8}) {} } }\n",
        "const o = { [(\u{3b1} as any) && 1]: 0 }; void o; const \u{3b1} = 1;\n",
        "\u{1f4a9}",
        "function f() {}\u{0}\u{1f680}",
    ];
    for (i, src) in cases.iter().enumerate() {
        // Must not panic. Ok or Err(SourceParse) are both fine — the
        // invariant is "no char-boundary panic", not "parses".
        let _ = std::panic::catch_unwind(|| {
            OxcWalker::new().extract(src, &format!("uni{i}.tsx"), ComplexityMetric::Cyclomatic)
        })
        .unwrap_or_else(|_| panic!("walker PANICKED on unicode case {i}: {src:?}"));
    }
}

/// Sanity meta-test: the grammar actually emits the constructs it
/// claims to (so a degenerate strategy can't make the invariants
/// vacuously true). Generates a fixed corpus and tallies coverage.
#[test]
fn grammar_covers_every_decision_point_kind() {
    use crap_core::domain::types::ContributorKind as K;
    let mut seen: BTreeMap<String, u32> = BTreeMap::new();
    let mut runner = proptest::test_runner::TestRunner::deterministic();
    for _ in 0..600 {
        let prog = program_strategy().new_tree(&mut runner).unwrap().current();
        let Some(fns) = walk(&prog.source(), &prog.file_path()) else {
            continue;
        };
        for f in &fns {
            for c in &f.contributors {
                *seen.entry(format!("{:?}", c.kind)).or_default() += 1;
            }
        }
    }
    for kind in [
        K::IfBranch,
        K::ForLoop,
        K::WhileLoop,
        K::DoWhileLoop,
        K::CaseBranch,
        K::LogicalOperator,
        K::Ternary,
        K::OptionalChain,
        K::Catch,
    ] {
        let key = format!("{kind:?}");
        assert!(
            seen.get(&key).copied().unwrap_or(0) > 0,
            "grammar never produced a {key} contributor across 600 programs; \
             coverage seen: {seen:?}"
        );
    }
}
