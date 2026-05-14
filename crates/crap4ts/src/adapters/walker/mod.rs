//! Oxc-based TypeScript complexity walker.
//!
//! Mirrors `crates/crap4rs/src/adapters/complexity/mod.rs` in shape and
//! responsibilities. The walker:
//!
//! 1. Parses the source via `oxc_parser::Parser` (with `SourceType`
//!    inferred from `file_path`'s extension via `SourceType::from_path`,
//!    falling back to `SourceType::ts()` for unrecognised extensions —
//!    W2.2 (#185) widens this dispatch to `.tsx` / `.jsx` / `.js` /
//!    `.mjs` / `.cjs` while keeping the same fallback shape).
//! 2. If `ret.errors` is non-empty, returns
//!    `Err(CrapError::SourceParse(format!("{file_path}: {}", err)))`.
//!    The orchestrator at `crap-core/src/core/mod.rs:286-310` catches
//!    this and increments `AnalysisDiagnostics.files_unparseable`.
//! 3. Otherwise walks the AST, emitting one `FunctionComplexity` per
//!    function declaration / function expression / arrow function /
//!    class method. Each function's contributors list captures the
//!    decision points scored inside its own body — nested functions
//!    accumulate independently.
//!
//! ## W1.2 scope (crap-rs#182)
//!
//! Six universal cyclomatic decision points map to existing
//! `ContributorKind` variants:
//!
//! | oxc AST node                                  | ContributorKind   |
//! |-----------------------------------------------|-------------------|
//! | `Statement::IfStatement`                      | `IfBranch`        |
//! | `Statement::ForStatement` / `ForOf` / `ForIn` | `ForLoop`         |
//! | `Statement::WhileStatement`                   | `WhileLoop`       |
//! | `Statement::DoWhileStatement`                 | `DoWhileLoop`     |
//! | `SwitchCase` (with `test: Some(_)`)           | `CaseBranch`      |
//! | `Expression::LogicalExpression(And\|Or)`      | `LogicalOperator` |
//!
//! TS-specific decision points (`?:` / `?.` / `??` / `catch` / JSX) are
//! intentionally deferred to W2.1 (#184) with ADR (a) — the walker
//! traverses past them without emitting contributors.
//!
//! ## Span semantics (Context7-verified against oxc 0.129)
//!
//! oxc `Span` is `[start, end)` over UTF-8 byte offsets (per
//! `oxc_span/README.md` — inclusive start, exclusive end). The domain
//! `SourceSpan.end_line` is 1-based **inclusive**, matching crap4rs's
//! syn walker behaviour. The conversion at the boundary is therefore:
//!
//! - `start_line = byte_to_line(source, span.start)`
//! - `end_line   = byte_to_line(source, span.end.saturating_sub(1))`
//!
//! i.e. `end_line` is the 1-based line of the LAST BYTE in the span.
//! The W1.2 prompt's "add +1" claim was a secondary-source error
//! (Context7-confirmed); see `observations.md` for the recorded pin.

use std::path::Path;

use crap_core::domain::types::{
    ComplexityContributor, ComplexityMetric, ContributorKind, CrapError, FunctionComplexity,
    FunctionIdentity, SourceSpan,
};
use crap_core::ports::ComplexityPort;
use oxc::allocator::Allocator;
use oxc::ast::ast::{
    ArrowFunctionExpression, BindingPattern, Class, ClassElement, Declaration, Expression,
    ForStatementInit, ForStatementLeft, Function, FunctionBody, IfStatement, LogicalExpression,
    MethodDefinition, ObjectExpression, ObjectPropertyKind, PropertyKey, Statement, SwitchCase,
    VariableDeclaration,
};
use oxc::parser::Parser;
use oxc::span::{SourceType, Span};
use oxc::syntax::operator::LogicalOperator;

/// Oxc-based complexity extractor implementing `ComplexityPort`.
///
/// crap4ts 2.0.0 supports cyclomatic counting only (one increment per
/// decision point). Cognitive counting is deferred to a follow-up
/// issue (per shaping Q2); requests for `ComplexityMetric::Cognitive`
/// return `CrapError::MetricNotSupported`, surfaced as an adapter-
/// specific user message at the CLI boundary. The default metric for
/// `crap4ts` is cyclomatic (locked decision #2) wired through
/// `AdapterMeta::default_metric`, so end-to-end runs without an
/// explicit `--metric` flag never trip the unsupported-metric path.
pub struct OxcWalker {
    _private: (),
}

impl OxcWalker {
    /// Construct a new walker. The walker is stateless — every call to
    /// `extract` is self-contained.
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for OxcWalker {
    fn default() -> Self {
        Self::new()
    }
}

impl ComplexityPort for OxcWalker {
    fn extract(
        &self,
        source: &str,
        file_path: &str,
        metric: ComplexityMetric,
    ) -> Result<Vec<FunctionComplexity>, CrapError> {
        // crap4ts 2.0.0 ships --metric cyclomatic only. Cognitive
        // complexity for TS is deferred to a follow-up issue (per
        // shaping Q2). Reject upfront so the binary's error renderer
        // surfaces the adapter-named hint at the CLI boundary
        // (`metric_unsupported.feature` scenario 1 contract).
        if metric == ComplexityMetric::Cognitive {
            return Err(CrapError::MetricNotSupported {
                metric: ComplexityMetric::Cognitive,
            });
        }

        let allocator = Allocator::default();
        let source_type =
            SourceType::from_path(Path::new(file_path)).unwrap_or_else(|_| SourceType::ts());

        let ret = Parser::new(&allocator, source, source_type).parse();
        if let Some(first) = ret.errors.first() {
            return Err(CrapError::SourceParse(format!("{file_path}: {first}")));
        }

        let mut finder = FunctionFinder {
            source,
            file_path,
            metric,
            functions: Vec::new(),
        };
        for stmt in &ret.program.body {
            finder.visit_top_level_statement(stmt);
        }
        Ok(finder.functions)
    }
}

// ── Function finder ─────────────────────────────────────────────────────

/// Walks top-level program statements (and class declarations) looking
/// for function-entry points. Each discovered function emits one
/// `FunctionComplexity` via `record_function`, which itself recurses
/// through the function body counting decision points and discovering
/// nested function entries.
struct FunctionFinder<'src> {
    source: &'src str,
    file_path: &'src str,
    metric: ComplexityMetric,
    functions: Vec<FunctionComplexity>,
}

impl<'src> FunctionFinder<'src> {
    /// Top-level dispatch over a `Statement` looking for function
    /// entries. Recognises:
    /// - `function foo() {}` declarations (including those re-exported
    ///   via `Statement::FunctionDeclaration` and the inherited
    ///   `ExportNamedDeclaration` / `ExportDefaultDeclaration` paths)
    /// - `const f = () => ...` / `const f = function() {}` declarators
    /// - `class Foo { method() {} }` method definitions
    fn visit_top_level_statement(&mut self, stmt: &Statement<'_>) {
        match stmt {
            Statement::FunctionDeclaration(func) => self.record_function(func, None),
            Statement::ClassDeclaration(class) => self.visit_class(class, None),
            Statement::VariableDeclaration(vd) => self.visit_variable_declaration(vd),
            Statement::ExportNamedDeclaration(decl) => {
                if let Some(inner) = &decl.declaration {
                    self.visit_top_level_declaration(inner);
                }
            }
            Statement::ExportDefaultDeclaration(decl) => {
                self.visit_export_default(decl);
            }
            // All other top-level statements may still contain nested
            // function-expression entries inside their expression trees
            // (e.g. an IIFE at module scope). For W1.2 the fixture
            // corpus never exercises this path; we still descend so
            // the walker is robust against real-world code.
            other => {
                let mut sink = Contributors::default();
                self.visit_statement(other, &mut sink, None);
            }
        }
    }

    /// Dispatch the body of `export default ...` — three branches:
    /// declaration-flavoured (function/class) recurse straight into the
    /// matching `record_*` helper; expression-flavoured route through
    /// the expression visitor with a throwaway sink (top-level
    /// expressions contribute to no parent accumulator).
    fn visit_export_default(&mut self, decl: &oxc::ast::ast::ExportDefaultDeclaration<'_>) {
        use oxc::ast::ast::ExportDefaultDeclarationKind as K;
        match &decl.declaration {
            K::FunctionDeclaration(func) => self.record_function(func, None),
            K::ClassDeclaration(class) => self.visit_class(class, None),
            expr_kind => {
                if let Some(expr) = export_default_as_expression(expr_kind) {
                    let mut sink = Contributors::default();
                    self.visit_expression(expr, &mut sink, None);
                }
            }
        }
    }

    fn visit_top_level_declaration(&mut self, decl: &Declaration<'_>) {
        match decl {
            Declaration::FunctionDeclaration(func) => self.record_function(func, None),
            Declaration::ClassDeclaration(class) => self.visit_class(class, None),
            Declaration::VariableDeclaration(vd) => self.visit_variable_declaration(vd),
            _ => {} // TS type / interface / enum declarations have no executable bodies.
        }
    }

    fn visit_variable_declaration(&mut self, vd: &VariableDeclaration<'_>) {
        for declarator in &vd.declarations {
            let Some(init) = &declarator.init else {
                continue;
            };
            let name = binding_name(&declarator.id);
            self.record_initializer(init, name);
        }
    }

    /// Route a top-level variable initializer to the matching
    /// function-entry recorder. Non-function initializers walk into the
    /// expression tree purely for nested-function discovery; they
    /// contribute no decision points to any parent accumulator.
    fn record_initializer(&mut self, init: &Expression<'_>, name: Option<String>) {
        match init {
            Expression::ArrowFunctionExpression(arrow) => self.record_arrow(arrow, name),
            Expression::FunctionExpression(func) => self.record_function(func, name),
            Expression::ClassExpression(class) => self.visit_class(class, name),
            other => {
                let mut sink = Contributors::default();
                self.visit_expression(other, &mut sink, None);
            }
        }
    }

    fn visit_class(&mut self, class: &Class<'_>, name_hint: Option<String>) {
        let class_name = class
            .id
            .as_ref()
            .map(|bi| bi.name.as_str().to_string())
            .or(name_hint)
            .unwrap_or_else(|| "<anonymous class>".to_string());
        for element in &class.body.body {
            if let ClassElement::MethodDefinition(method) = element {
                self.record_method(method, &class_name);
            }
        }
    }

    fn record_method(&mut self, method: &MethodDefinition<'_>, class_name: &str) {
        let method_name = property_key_name(&method.key).unwrap_or_else(|| "<computed>".into());
        let qualified = format!("{class_name}.{method_name}");
        // `method.value` is the Function (FunctionExpression) holding
        // the body.
        self.record_function(&method.value, Some(qualified));
    }

    fn record_function(&mut self, func: &Function<'_>, name_hint: Option<String>) {
        let name = name_hint
            .or_else(|| func.id.as_ref().map(|id| id.name.as_str().to_string()))
            .unwrap_or_else(|| "<anonymous>".to_string());

        let mut contributors = Contributors::default();
        if let Some(body) = func.body.as_ref() {
            self.visit_function_body(body, &mut contributors);
        }

        self.push_function(name, func.span, contributors);
    }

    fn record_arrow(&mut self, arrow: &ArrowFunctionExpression<'_>, name_hint: Option<String>) {
        let name = name_hint.unwrap_or_else(|| "<arrow>".to_string());

        let mut contributors = Contributors::default();
        // Arrow bodies are wrapped in `FunctionBody` regardless of
        // expression-vs-block form — the `body.statements` is a single
        // `ReturnStatement` for expression bodies. `arrow.expression`
        // signals which form was written; the body shape is uniform.
        self.visit_function_body(&arrow.body, &mut contributors);

        self.push_function(name, arrow.span, contributors);
    }

    fn push_function(&mut self, name: String, span: Span, contributors: Contributors) {
        let mut emitted = contributors.list;
        emitted.sort_by_key(|c| (c.line, c.column.unwrap_or(0)));
        let complexity = 1 + contributors.count;
        let span = source_span(self.source, span);
        self.functions.push(FunctionComplexity {
            identity: FunctionIdentity::new(self.file_path.to_string(), name, span),
            complexity,
            metric: self.metric,
            contributors: emitted,
        });
    }

    /// Walk a function body, accumulating contributors into `out` and
    /// discovering nested function entries (which get their own
    /// accumulator via `record_function` / `record_arrow`).
    fn visit_function_body(&mut self, body: &FunctionBody<'_>, out: &mut Contributors) {
        for stmt in &body.statements {
            self.visit_statement(stmt, out, Some(0));
        }
    }

    /// Walk a statement, charging decision points to `out` and
    /// recursing into substructure. When `nesting` is `Some(n)`, we are
    /// inside a function body at depth `n`; when `None`, we are at the
    /// top level of the module (no parent function to charge).
    fn visit_statement(
        &mut self,
        stmt: &Statement<'_>,
        out: &mut Contributors,
        nesting: Option<u32>,
    ) {
        match stmt {
            // Function-entry statements (start a new accumulator).
            Statement::FunctionDeclaration(func) => self.record_function(func, None),
            Statement::ClassDeclaration(class) => self.visit_class(class, None),
            Statement::VariableDeclaration(vd) => {
                self.visit_variable_declaration_in_body(vd, out, nesting);
            }

            // Decision-point statements.
            Statement::IfStatement(if_stmt) => self.visit_if(if_stmt, out, nesting),
            Statement::ForStatement(for_stmt) => self.visit_for(for_stmt, out, nesting),
            Statement::ForInStatement(for_in) => self.visit_for_in(for_in, out, nesting),
            Statement::ForOfStatement(for_of) => self.visit_for_of(for_of, out, nesting),
            Statement::WhileStatement(w) => self.visit_while(w, out, nesting),
            Statement::DoWhileStatement(dw) => self.visit_do_while(dw, out, nesting),
            Statement::SwitchStatement(sw) => self.visit_switch(sw, out, nesting),

            // Recursing statements (no scoring at this node, recurse into children).
            Statement::BlockStatement(b) => self.visit_block(&b.body, out, nesting),
            Statement::ExpressionStatement(es) => {
                self.visit_expression(&es.expression, out, nesting);
            }
            Statement::ReturnStatement(r) => {
                if let Some(arg) = &r.argument {
                    self.visit_expression(arg, out, nesting);
                }
            }
            Statement::TryStatement(t) => self.visit_try(t, out, nesting),
            Statement::LabeledStatement(l) => self.visit_statement(&l.body, out, nesting),
            Statement::ThrowStatement(th) => self.visit_expression(&th.argument, out, nesting),

            // Module-level statements (valid only at top-level program scope).
            Statement::ExportNamedDeclaration(decl) => {
                if let Some(inner) = &decl.declaration {
                    self.visit_top_level_declaration(inner);
                }
            }

            // Everything else is structural-only at this scope: leaf
            // statements (break/continue/debugger/empty/with), bare
            // module statements (re-exports / imports / TS-flavoured
            // assignment-exports), and TypeScript type-only
            // declarations all contribute no executable decision
            // points to the enclosing function body.
            _ => {}
        }
    }

    fn visit_variable_declaration_in_body(
        &mut self,
        vd: &VariableDeclaration<'_>,
        out: &mut Contributors,
        nesting: Option<u32>,
    ) {
        for declarator in &vd.declarations {
            let name = binding_name(&declarator.id);
            let Some(init) = &declarator.init else {
                continue;
            };
            match init {
                Expression::ArrowFunctionExpression(arrow) => self.record_arrow(arrow, name),
                Expression::FunctionExpression(func) => self.record_function(func, name),
                Expression::ClassExpression(class) => self.visit_class(class, name),
                other => self.visit_expression(other, out, nesting),
            }
        }
    }

    fn visit_block(
        &mut self,
        body: &[Statement<'_>],
        out: &mut Contributors,
        nesting: Option<u32>,
    ) {
        for s in body {
            self.visit_statement(s, out, nesting);
        }
    }

    fn visit_for(
        &mut self,
        for_stmt: &oxc::ast::ast::ForStatement<'_>,
        out: &mut Contributors,
        nesting: Option<u32>,
    ) {
        self.charge_for(for_stmt.span, out, nesting);
        if let Some(init) = &for_stmt.init {
            self.visit_for_init(init, out, nesting);
        }
        if let Some(test) = &for_stmt.test {
            self.visit_expression(test, out, nesting);
        }
        if let Some(update) = &for_stmt.update {
            self.visit_expression(update, out, nesting);
        }
        self.visit_statement(&for_stmt.body, out, nesting.map(|n| n + 1));
    }

    fn visit_for_in(
        &mut self,
        for_in: &oxc::ast::ast::ForInStatement<'_>,
        out: &mut Contributors,
        nesting: Option<u32>,
    ) {
        self.charge_for(for_in.span, out, nesting);
        self.visit_for_left(&for_in.left, out, nesting);
        self.visit_expression(&for_in.right, out, nesting);
        self.visit_statement(&for_in.body, out, nesting.map(|n| n + 1));
    }

    fn visit_for_of(
        &mut self,
        for_of: &oxc::ast::ast::ForOfStatement<'_>,
        out: &mut Contributors,
        nesting: Option<u32>,
    ) {
        self.charge_for(for_of.span, out, nesting);
        self.visit_for_left(&for_of.left, out, nesting);
        self.visit_expression(&for_of.right, out, nesting);
        self.visit_statement(&for_of.body, out, nesting.map(|n| n + 1));
    }

    fn visit_while(
        &mut self,
        w: &oxc::ast::ast::WhileStatement<'_>,
        out: &mut Contributors,
        nesting: Option<u32>,
    ) {
        if let Some(n) = nesting {
            out.push(contributor(
                ContributorKind::WhileLoop,
                self.source,
                w.span,
                n,
            ));
        }
        self.visit_expression(&w.test, out, nesting);
        self.visit_statement(&w.body, out, nesting.map(|n| n + 1));
    }

    fn visit_do_while(
        &mut self,
        dw: &oxc::ast::ast::DoWhileStatement<'_>,
        out: &mut Contributors,
        nesting: Option<u32>,
    ) {
        if let Some(n) = nesting {
            out.push(contributor(
                ContributorKind::DoWhileLoop,
                self.source,
                dw.span,
                n,
            ));
        }
        self.visit_statement(&dw.body, out, nesting.map(|n| n + 1));
        self.visit_expression(&dw.test, out, nesting);
    }

    fn visit_switch(
        &mut self,
        sw: &oxc::ast::ast::SwitchStatement<'_>,
        out: &mut Contributors,
        nesting: Option<u32>,
    ) {
        self.visit_expression(&sw.discriminant, out, nesting);
        for case in &sw.cases {
            self.visit_switch_case(case, out, nesting);
        }
    }

    /// W1.2 walks the try block, handler body, and finalizer for
    /// nested-function discovery + any universal decision points
    /// inside them. It does NOT score `catch` itself — that's W2.1's
    /// job (ADR (a) on cyclomatic decision-point mappings for TS).
    fn visit_try(
        &mut self,
        t: &oxc::ast::ast::TryStatement<'_>,
        out: &mut Contributors,
        nesting: Option<u32>,
    ) {
        self.visit_block(&t.block.body, out, nesting);
        if let Some(handler) = &t.handler {
            self.visit_block(&handler.body.body, out, nesting);
        }
        if let Some(finalizer) = &t.finalizer {
            self.visit_block(&finalizer.body, out, nesting);
        }
    }

    fn visit_if(
        &mut self,
        if_stmt: &IfStatement<'_>,
        out: &mut Contributors,
        nesting: Option<u32>,
    ) {
        if let Some(n) = nesting {
            out.push(contributor(
                ContributorKind::IfBranch,
                self.source,
                if_stmt.span,
                n,
            ));
        }
        self.visit_expression(&if_stmt.test, out, nesting);
        self.visit_statement(&if_stmt.consequent, out, nesting.map(|n| n + 1));
        if let Some(alt) = &if_stmt.alternate {
            // `else if (...)` is encoded as `alternate: IfStatement`;
            // we descend without bumping nesting so the chain reads as
            // a flat ladder (matches crap4rs's else-if continuation
            // handling in `count_cognitive_else`).
            match alt {
                Statement::IfStatement(_) => self.visit_statement(alt, out, nesting),
                _ => self.visit_statement(alt, out, nesting.map(|n| n + 1)),
            }
        }
    }

    fn visit_switch_case(
        &mut self,
        case: &SwitchCase<'_>,
        out: &mut Contributors,
        nesting: Option<u32>,
    ) {
        // `default:` has `test: None` — NOT a decision point, NOT
        // counted (per `cyclomatic_walker.feature` outline row
        // semantics: "case 1: ..." adds 1, fallthrough does not).
        if let Some(test) = &case.test {
            if let Some(n) = nesting {
                out.push(contributor(
                    ContributorKind::CaseBranch,
                    self.source,
                    case.span,
                    n,
                ));
            }
            self.visit_expression(test, out, nesting);
        }
        for s in &case.consequent {
            self.visit_statement(s, out, nesting.map(|n| n + 1));
        }
    }

    fn visit_for_init(
        &mut self,
        init: &ForStatementInit<'_>,
        out: &mut Contributors,
        nesting: Option<u32>,
    ) {
        match init {
            ForStatementInit::VariableDeclaration(vd) => {
                for d in &vd.declarations {
                    if let Some(init_expr) = &d.init {
                        self.visit_expression(init_expr, out, nesting);
                    }
                }
            }
            other => {
                if let Some(expr) = for_init_as_expression(other) {
                    self.visit_expression(expr, out, nesting);
                }
            }
        }
    }

    fn visit_for_left(
        &mut self,
        _left: &ForStatementLeft<'_>,
        _out: &mut Contributors,
        _nesting: Option<u32>,
    ) {
        // The LHS of for-in / for-of is a BindingPattern or
        // AssignmentTarget — no decision points are encoded there for
        // W1.2's universal set.
    }

    /// Push a `ForLoop` contributor + recurse via the caller — separated
    /// from the inline statement match purely so all three for-flavours
    /// share one push site.
    fn charge_for(&mut self, span: Span, out: &mut Contributors, nesting: Option<u32>) {
        if let Some(n) = nesting {
            out.push(contributor(ContributorKind::ForLoop, self.source, span, n));
        }
    }

    /// Walk an expression, charging decision points + descending into
    /// nested function expressions and arrow functions (which become
    /// their own complexity sites). The arms here are intentionally
    /// thin — each non-trivial recurser is its own helper so this
    /// dispatch table stays a simple match.
    fn visit_expression(
        &mut self,
        expr: &Expression<'_>,
        out: &mut Contributors,
        nesting: Option<u32>,
    ) {
        match expr {
            // Decision-point expressions.
            Expression::LogicalExpression(le) => self.visit_logical(le, out, nesting),

            // Function-entry expressions (start a new accumulator).
            Expression::ArrowFunctionExpression(arrow) => self.record_arrow(arrow, None),
            Expression::FunctionExpression(func) => self.record_function(func, None),
            Expression::ClassExpression(class) => self.visit_class(class, None),

            // TS-specific expressions intentionally NOT counted in W1.2
            // (deferred to W2.1, ADR (a)) — we still walk inner
            // expressions for nested-function discovery and for any
            // UNIVERSAL decision points inside them.
            Expression::ConditionalExpression(ce) => self.visit_conditional(ce, out, nesting),
            Expression::ChainExpression(ce) => {
                // ChainExpression wraps a (member or call) expression
                // chain that contains optional links. W1.2 traverses
                // into the inner expression without scoring `?.` — the
                // entire `?.` chain is W2.1 / ADR (a)'s scope. We
                // recurse manually into ChainElement variants because
                // ChainElement only inherits MemberExpression (not the
                // full Expression set) so `as_expression()` isn't
                // generated for it.
                self.visit_chain_element(&ce.expression, out, nesting);
            }

            // Recursing expressions (universal — no scoring at this
            // node, but children may contribute).
            Expression::CallExpression(call) => {
                self.visit_expression(&call.callee, out, nesting);
                self.visit_arguments(&call.arguments, out, nesting);
            }
            Expression::NewExpression(new_expr) => {
                self.visit_expression(&new_expr.callee, out, nesting);
                self.visit_arguments(&new_expr.arguments, out, nesting);
            }
            Expression::ParenthesizedExpression(p) => {
                self.visit_expression(&p.expression, out, nesting);
            }
            Expression::SequenceExpression(seq) => self.visit_each(&seq.expressions, out, nesting),
            Expression::UnaryExpression(u) => self.visit_expression(&u.argument, out, nesting),
            Expression::BinaryExpression(b) => {
                self.visit_expression(&b.left, out, nesting);
                self.visit_expression(&b.right, out, nesting);
            }
            Expression::AssignmentExpression(a) => {
                self.visit_expression(&a.right, out, nesting);
            }
            Expression::ArrayExpression(arr) => {
                self.visit_array_elements(&arr.elements, out, nesting);
            }
            Expression::ObjectExpression(obj) => self.visit_object_expression(obj, out, nesting),
            Expression::TemplateLiteral(tl) => self.visit_each(&tl.expressions, out, nesting),
            Expression::TaggedTemplateExpression(tt) => {
                self.visit_expression(&tt.tag, out, nesting);
                self.visit_each(&tt.quasi.expressions, out, nesting);
            }
            Expression::AwaitExpression(a) => self.visit_expression(&a.argument, out, nesting),
            Expression::YieldExpression(y) => {
                if let Some(arg) = &y.argument {
                    self.visit_expression(arg, out, nesting);
                }
            }
            Expression::ImportExpression(ie) => self.visit_expression(&ie.source, out, nesting),

            // TS type-coercion wrappers — recurse into payload.
            Expression::TSAsExpression(ts) => self.visit_expression(&ts.expression, out, nesting),
            Expression::TSSatisfiesExpression(ts) => {
                self.visit_expression(&ts.expression, out, nesting)
            }
            Expression::TSTypeAssertion(ts) => self.visit_expression(&ts.expression, out, nesting),
            Expression::TSNonNullExpression(ts) => {
                self.visit_expression(&ts.expression, out, nesting)
            }
            Expression::TSInstantiationExpression(ts) => {
                self.visit_expression(&ts.expression, out, nesting)
            }

            // Member-expression family (inherited variants on
            // `Expression` from `MemberExpression`). Treat as opaque
            // receivers for W1.2; no decision-point scoring.
            Expression::ComputedMemberExpression(m) => {
                self.visit_expression(&m.object, out, nesting);
                self.visit_expression(&m.expression, out, nesting);
            }
            Expression::StaticMemberExpression(m) => {
                self.visit_expression(&m.object, out, nesting);
            }
            Expression::PrivateFieldExpression(m) => {
                self.visit_expression(&m.object, out, nesting);
            }

            // Leaves + JSX (out of scope for W1.2) — no scoring, no
            // recursion. JSX bodies get full coverage in W2.1.
            _ => {}
        }
    }

    /// Visit a `ConditionalExpression` — `?:` is NOT scored in W1.2
    /// (W2.1 / ADR (a)) but the test/consequent/alternate may contain
    /// universal decision points and nested functions.
    fn visit_conditional(
        &mut self,
        ce: &oxc::ast::ast::ConditionalExpression<'_>,
        out: &mut Contributors,
        nesting: Option<u32>,
    ) {
        self.visit_expression(&ce.test, out, nesting);
        self.visit_expression(&ce.consequent, out, nesting);
        self.visit_expression(&ce.alternate, out, nesting);
    }

    fn visit_arguments(
        &mut self,
        args: &[oxc::ast::ast::Argument<'_>],
        out: &mut Contributors,
        nesting: Option<u32>,
    ) {
        for arg in args {
            if let Some(e) = argument_as_expression(arg) {
                self.visit_expression(e, out, nesting);
            }
        }
    }

    fn visit_array_elements(
        &mut self,
        elements: &[oxc::ast::ast::ArrayExpressionElement<'_>],
        out: &mut Contributors,
        nesting: Option<u32>,
    ) {
        for element in elements {
            if let Some(e) = array_element_as_expression(element) {
                self.visit_expression(e, out, nesting);
            }
        }
    }

    fn visit_each(
        &mut self,
        exprs: &[Expression<'_>],
        out: &mut Contributors,
        nesting: Option<u32>,
    ) {
        for e in exprs {
            self.visit_expression(e, out, nesting);
        }
    }

    fn visit_logical(
        &mut self,
        le: &LogicalExpression<'_>,
        out: &mut Contributors,
        nesting: Option<u32>,
    ) {
        // W1.2 counts `&&` and `||` but NOT `??` (Coalesce) — that
        // lands in W2.1 with ADR (a).
        match le.operator {
            LogicalOperator::And | LogicalOperator::Or => {
                if let Some(n) = nesting {
                    out.push(contributor(
                        ContributorKind::LogicalOperator,
                        self.source,
                        le.span,
                        n,
                    ));
                }
            }
            LogicalOperator::Coalesce => {}
        }
        self.visit_expression(&le.left, out, nesting);
        self.visit_expression(&le.right, out, nesting);
    }

    /// Walk a `ChainElement` — the inner of a `ChainExpression`. The
    /// enum inherits from `MemberExpression`, so it has 5 variants in
    /// oxc 0.129: `CallExpression`, `TSNonNullExpression`, and three
    /// inherited member-expression variants. Each is descended for
    /// nested-function / universal-decision-point discovery.
    fn visit_chain_element(
        &mut self,
        ce: &oxc::ast::ast::ChainElement<'_>,
        out: &mut Contributors,
        nesting: Option<u32>,
    ) {
        use oxc::ast::ast::ChainElement;
        match ce {
            ChainElement::CallExpression(call) => {
                self.visit_expression(&call.callee, out, nesting);
                for arg in &call.arguments {
                    if let Some(e) = argument_as_expression(arg) {
                        self.visit_expression(e, out, nesting);
                    }
                }
            }
            ChainElement::TSNonNullExpression(ts) => {
                self.visit_expression(&ts.expression, out, nesting);
            }
            ChainElement::ComputedMemberExpression(m) => {
                self.visit_expression(&m.object, out, nesting);
                self.visit_expression(&m.expression, out, nesting);
            }
            ChainElement::StaticMemberExpression(m) => {
                self.visit_expression(&m.object, out, nesting);
            }
            ChainElement::PrivateFieldExpression(m) => {
                self.visit_expression(&m.object, out, nesting);
            }
        }
    }

    fn visit_object_expression(
        &mut self,
        obj: &ObjectExpression<'_>,
        out: &mut Contributors,
        nesting: Option<u32>,
    ) {
        for prop in &obj.properties {
            match prop {
                ObjectPropertyKind::ObjectProperty(p) => {
                    self.visit_expression(&p.value, out, nesting);
                }
                ObjectPropertyKind::SpreadProperty(s) => {
                    self.visit_expression(&s.argument, out, nesting);
                }
            }
        }
    }
}

// ── Contributor accumulator ─────────────────────────────────────────────

/// Per-function contributor accumulator. `count` is the running
/// decision-point increment sum (used to compute `complexity = 1 +
/// count`); `list` is the materialised contributor entries that ship
/// in `FunctionComplexity.contributors`.
#[derive(Default)]
struct Contributors {
    list: Vec<ComplexityContributor>,
    count: u32,
}

impl Contributors {
    fn push(&mut self, c: ComplexityContributor) {
        self.count += c.increment;
        self.list.push(c);
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn contributor(
    kind: ContributorKind,
    source: &str,
    span: Span,
    nesting: u32,
) -> ComplexityContributor {
    let (line, column) = byte_to_line_col(source, span.start);
    let end_line = byte_to_line(source, span.end.saturating_sub(1));
    ComplexityContributor::new(kind, line, Some(column), 1, end_line, nesting)
}

fn source_span(source: &str, span: Span) -> SourceSpan {
    let (start_line, start_col) = byte_to_line_col(source, span.start);
    let (end_line, end_col) = byte_to_line_col(source, span.end.saturating_sub(1));
    SourceSpan::new(start_line, end_line, start_col as usize, end_col as usize)
}

/// Convert a UTF-8 byte offset to a 1-based line number. Clamps
/// offsets past EOF to the source length so synthetic spans (e.g.
/// `span.end.saturating_sub(1)` on a zero-length span at offset 0)
/// never panic.
fn byte_to_line(source: &str, byte: u32) -> usize {
    let limit = (byte as usize).min(source.len());
    source.as_bytes()[..limit]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

/// Convert a UTF-8 byte offset to a (1-based line, 1-based column)
/// pair. Columns count UTF-8 *code units* (bytes), aligning with how
/// oxc spans are emitted (the same convention oxc uses for diagnostic
/// label positions). `byte_to_line` is reused for the line count so
/// the two helpers stay self-consistent on EOF clamp behaviour.
fn byte_to_line_col(source: &str, byte: u32) -> (usize, u32) {
    let bytes = source.as_bytes();
    let limit = (byte as usize).min(bytes.len());
    let line = byte_to_line(source, byte);
    let line_start = bytes[..limit]
        .iter()
        .rposition(|&b| b == b'\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let column = (limit - line_start) as u32 + 1;
    (line, column)
}

/// Extract the simple binding name from a declarator's pattern.
/// Object / array / assignment patterns return `None` — destructuring
/// is uncommon for top-level function-binding declarators, and the
/// W1.2 walker prefers `<arrow>` / `<anonymous>` sentinels over
/// inventing a synthetic name for `const {a, b} = ...` shapes.
fn binding_name(pattern: &BindingPattern<'_>) -> Option<String> {
    match pattern {
        BindingPattern::BindingIdentifier(bi) => Some(bi.name.as_str().to_string()),
        _ => None,
    }
}

/// Extract a string name from a property key for class-method
/// qualification. `[computed]` keys fall back to `None` so callers
/// substitute a sentinel name.
fn property_key_name(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.as_str().to_string()),
        PropertyKey::PrivateIdentifier(id) => Some(format!("#{}", id.name.as_str())),
        PropertyKey::StringLiteral(s) => Some(s.value.to_string()),
        PropertyKey::NumericLiteral(n) => Some(n.value.to_string()),
        _ => None,
    }
}

/// Treat an `ExportDefaultDeclarationKind` as `Expression` when
/// possible. Declaration variants (function/class/TS interface) are
/// routed through bespoke arms in the caller and return `None` here.
fn export_default_as_expression<'b>(
    kind: &'b oxc::ast::ast::ExportDefaultDeclarationKind<'b>,
) -> Option<&'b Expression<'b>> {
    use oxc::ast::ast::ExportDefaultDeclarationKind as K;
    match kind {
        K::FunctionDeclaration(_) | K::ClassDeclaration(_) | K::TSInterfaceDeclaration(_) => None,
        // The remaining variants `@inherit Expression`; `as_expression`
        // is generated by `inherit_variants!` to perform the typed
        // downcast back to `&Expression`.
        other => other.as_expression(),
    }
}

/// Treat `Argument` as `Expression` when possible (Argument inherits
/// from Expression plus a `SpreadElement` variant).
fn argument_as_expression<'b>(arg: &'b oxc::ast::ast::Argument<'b>) -> Option<&'b Expression<'b>> {
    arg.as_expression()
}

/// Treat `ArrayExpressionElement` as `Expression` when possible.
fn array_element_as_expression<'b>(
    el: &'b oxc::ast::ast::ArrayExpressionElement<'b>,
) -> Option<&'b Expression<'b>> {
    el.as_expression()
}

/// Treat `ForStatementInit` as `Expression` when possible (the
/// non-`VariableDeclaration` variants inherit from Expression).
fn for_init_as_expression<'b>(init: &'b ForStatementInit<'b>) -> Option<&'b Expression<'b>> {
    init.as_expression()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oxc_walker_constructible() {
        let _w = OxcWalker::new();
        let _w2 = OxcWalker::default();
    }
}
