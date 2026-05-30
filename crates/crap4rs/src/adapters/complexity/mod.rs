//! Complexity extraction adapter using syn 2.x AST walking.
//!
//! Supports both cognitive and cyclomatic complexity metrics.
//! Walks `ItemFn` and `ImplItemFn` nodes, counting decision points.

use crate::domain::types::{
    ComplexityContributor, ComplexityMetric, ContributorKind, CrapError, FunctionComplexity,
    FunctionIdentity, SourceSpan,
};
use crate::ports::ComplexityPort;
use proc_macro2::Span;
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{Expr, ExprBinary, ItemFn, ItemImpl};

/// Syn-based complexity extractor implementing `ComplexityPort`.
#[derive(Default)]
pub struct SynComplexityAdapter;

impl SynComplexityAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl ComplexityPort for SynComplexityAdapter {
    fn extract(
        &self,
        source: &str,
        file_path: &str,
        metric: ComplexityMetric,
    ) -> Result<Vec<FunctionComplexity>, CrapError> {
        let file = syn::parse_file(source)
            .map_err(|e| CrapError::SourceParse(format!("{file_path}: {e}")))?;

        let mut finder = FunctionFinder {
            file_path: file_path.to_string(),
            metric,
            mod_path: Vec::new(),
            current_impl_type: None,
            functions: Vec::new(),
        };
        finder.visit_file(&file);

        Ok(finder.functions)
    }
}

// ── Function finder visitor ────────────────────────────────────────────

struct FunctionFinder {
    file_path: String,
    metric: ComplexityMetric,
    /// Stack of enclosing inline `mod` idents, outermost first. A
    /// function discovered while this stack is `["outer", "inner"]`
    /// is qualified `outer::inner::<name>`. The walker runs per file,
    /// so only *inline* module nesting is visible — the file's own
    /// position in the crate module tree is not reconstructed.
    mod_path: Vec<String>,
    current_impl_type: Option<String>,
    functions: Vec<FunctionComplexity>,
}

impl FunctionFinder {
    /// Build a function's qualified name from the current `mod` path,
    /// the optional enclosing impl/trait type, and the bare ident.
    ///
    /// The mod path is a pure prefix: a free fn becomes
    /// `outer::inner::f`; a `Type::method` inside a mod becomes
    /// `outer::Type::method`. `mod_path` and `current_impl_type` are
    /// orthogonal (Rust forbids `mod` inside `impl`), so they never
    /// interleave — the path is always `<mods>::<Type?>::<name>`.
    fn qualify(&self, name: &str) -> String {
        // Fast paths for the two overwhelmingly common shapes avoid the
        // Vec<&str> allocation that the general join needs:
        //   - top-level free fn  → bare name
        //   - top-level method   → `Type::name`
        // The nested-mod cases (rare) fall through to the Vec+join.
        match (self.mod_path.is_empty(), &self.current_impl_type) {
            (true, None) => name.to_string(),
            (true, Some(ty)) => format!("{ty}::{name}"),
            _ => {
                let mut segments: Vec<&str> = self.mod_path.iter().map(String::as_str).collect();
                if let Some(ty) = &self.current_impl_type {
                    segments.push(ty);
                }
                segments.push(name);
                segments.join("::")
            }
        }
    }
}

impl<'ast> Visit<'ast> for FunctionFinder {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        // Track inline module nesting so functions inside `mod a { mod b
        // { fn f } }` are qualified `a::b::f`. `#[cfg(test)]` and other
        // attributed modules are NOT special-cased — the walker is a
        // generic Rust analyzer and `tests::some_fn` is the genuinely
        // correct qualified name.
        //
        // Modules declared as `mod foo;` (file-backed, `content == None`)
        // have no inline body to walk, so pushing/popping the ident would
        // be wasted allocation — skip the stack mutation entirely and
        // just default-walk.
        if node.content.is_some() {
            self.mod_path.push(node.ident.to_string());
            syn::visit::visit_item_mod(self, node);
            self.mod_path.pop();
        } else {
            syn::visit::visit_item_mod(self, node);
        }
    }

    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        let name = self.qualify(&node.sig.ident.to_string());
        let span = span_of(node);
        let mut contributors = Vec::new();
        let complexity = count_complexity(&node.block, self.metric, &mut contributors);
        contributors.sort_by_key(|c| (c.line, c.column));

        self.functions.push(FunctionComplexity {
            identity: FunctionIdentity::new(self.file_path.clone(), name, span),
            complexity,
            metric: self.metric,
            contributors,
        });

        // Do NOT recurse into function bodies to find nested functions —
        // closures are counted as part of the parent, and nested `fn` items
        // are visited by the default walk.
        syn::visit::visit_item_fn(self, node);
    }

    fn visit_item_impl(&mut self, node: &'ast ItemImpl) {
        let prev = self.current_impl_type.take();
        self.current_impl_type = Some(extract_type_name(node));
        syn::visit::visit_item_impl(self, node);
        self.current_impl_type = prev;
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        let qualified = self.qualify(&node.sig.ident.to_string());
        let span = span_of(node);
        let mut contributors = Vec::new();
        let complexity = count_complexity(&node.block, self.metric, &mut contributors);
        contributors.sort_by_key(|c| (c.line, c.column));

        self.functions.push(FunctionComplexity {
            identity: FunctionIdentity::new(self.file_path.clone(), qualified, span),
            complexity,
            metric: self.metric,
            contributors,
        });

        syn::visit::visit_impl_item_fn(self, node);
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        // Only record trait methods that have a default body
        if let Some(block) = &node.default {
            let qualified = self.qualify(&node.sig.ident.to_string());
            let span = span_of(node);
            let mut contributors = Vec::new();
            let complexity = count_complexity(block, self.metric, &mut contributors);
            contributors.sort_by_key(|c| (c.line, c.column));

            self.functions.push(FunctionComplexity {
                identity: FunctionIdentity::new(self.file_path.clone(), qualified, span),
                complexity,
                metric: self.metric,
                contributors,
            });
        }

        syn::visit::visit_trait_item_fn(self, node);
    }

    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        let prev = self.current_impl_type.take();
        self.current_impl_type = Some(node.ident.to_string());
        syn::visit::visit_item_trait(self, node);
        self.current_impl_type = prev;
    }
}

// ── Helpers ────────────────────────────────────────────────────────────

fn add_contributor(
    contributors: &mut Vec<ComplexityContributor>,
    kind: ContributorKind,
    span: Span,
    end_line: usize,
    nesting_depth: u32,
    increment: u32,
) {
    // syn `Span::start().column` is 0-based; `ComplexityContributor.column`
    // is 1-based inclusive (aligned with `SourceSpan::start_column` and
    // SARIF). +1 here is the only conversion site.
    contributors.push(ComplexityContributor::new(
        kind,
        span.start().line,
        Some(span.start().column as u32 + 1),
        increment,
        end_line,
        nesting_depth,
    ));
}

/// 1-based inclusive end line of a construct's full span. Used by the
/// walker to record `end_line` for compound contributors (`if`, `match`,
/// loop variants) so domain helpers can ask "does this contributor cover
/// line N" when building the nesting hierarchy.
fn end_line_of(item: &impl Spanned) -> usize {
    item.span().end().line
}

fn span_of(item: &impl syn::spanned::Spanned) -> SourceSpan {
    // `proc_macro2::LineColumn::line` is 1-indexed already; `column` is
    // 0-indexed (UTF-8 chars). `Span::start()` is the first character of
    // the span (inclusive); `Span::end()` is one past the last character
    // (exclusive). `SourceSpan` stores 1-based inclusive columns, in
    // parallel with the inclusive `end_line` convention:
    //
    //   start_column = start_col_0based + 1
    //   end_column   = end_col_0based       (because 0-based exclusive
    //                                        end equals 1-based inclusive
    //                                        end of the same character)
    //
    // Reporters that want SARIF-style exclusive endColumn add 1 at emit
    // time; everyone else gets the intuitive inclusive bound for free.
    let sp = item.span();
    SourceSpan::new(
        sp.start().line,
        sp.end().line,
        sp.start().column + 1,
        sp.end().column,
    )
}

fn extract_type_name(item_impl: &ItemImpl) -> String {
    // Extract the last path segment ident (handles `impl<T> Foo<T>`).
    if let syn::Type::Path(type_path) = item_impl.self_ty.as_ref()
        && let Some(seg) = type_path.path.segments.last()
    {
        return seg.ident.to_string();
    }
    // Fallback for unusual types
    "<unknown>".to_string()
}

// ── Complexity counting ────────────────────────────────────────────────

fn count_complexity(
    block: &syn::Block,
    metric: ComplexityMetric,
    contributors: &mut Vec<ComplexityContributor>,
) -> u32 {
    let raw = match metric {
        ComplexityMetric::Cognitive => count_cognitive_block(block, 0, contributors),
        ComplexityMetric::Cyclomatic => count_cyclomatic_block(block, contributors),
    };
    // Base complexity is always at least 1
    raw + 1
}

// ── Cognitive complexity ───────────────────────────────────────────────

fn count_cognitive_block(
    block: &syn::Block,
    nesting: u32,
    contributors: &mut Vec<ComplexityContributor>,
) -> u32 {
    block
        .stmts
        .iter()
        .map(|stmt| count_cognitive_stmt(stmt, nesting, contributors))
        .sum()
}

fn count_cognitive_stmt(
    stmt: &syn::Stmt,
    nesting: u32,
    contributors: &mut Vec<ComplexityContributor>,
) -> u32 {
    match stmt {
        syn::Stmt::Expr(expr, _) => count_cognitive_expr(expr, nesting, contributors),
        syn::Stmt::Local(local) => {
            let mut total = 0;
            if let Some(init) = &local.init {
                total += count_cognitive_expr(&init.expr, nesting, contributors);
                if let Some((else_token, diverge)) = &init.diverge {
                    // let...else is a branching construct: +1 structural + nesting
                    add_contributor(
                        contributors,
                        ContributorKind::LetElse,
                        else_token.span,
                        end_line_of(diverge.as_ref()),
                        nesting,
                        1 + nesting,
                    );
                    total += 1 + nesting;
                    total += count_cognitive_expr(diverge, nesting + 1, contributors);
                }
            }
            total
        }
        syn::Stmt::Item(_) | syn::Stmt::Macro(_) => 0,
    }
}

fn count_cognitive_expr(
    expr: &Expr,
    nesting: u32,
    contributors: &mut Vec<ComplexityContributor>,
) -> u32 {
    match expr {
        Expr::If(expr_if) => count_cognitive_if(expr_if, nesting, contributors),
        Expr::Match(expr_match) => count_cognitive_match(expr_match, nesting, contributors),
        Expr::While(expr_while) => count_cognitive_while(expr_while, nesting, contributors),
        Expr::ForLoop(expr_for) => count_cognitive_for_loop(expr_for, nesting, contributors),
        Expr::Loop(expr_loop) => count_cognitive_loop(expr_loop, nesting, contributors),
        Expr::Binary(bin) => count_cognitive_binary(bin, nesting, contributors),
        Expr::Try(expr_try) => count_cognitive_try(expr_try, nesting, contributors),
        Expr::Break(expr_break) => count_cognitive_break(expr_break, nesting, contributors),
        Expr::Continue(expr_continue) => {
            count_cognitive_continue(expr_continue, nesting, contributors)
        }
        Expr::Block(expr_block) => count_cognitive_block(&expr_block.block, nesting, contributors),
        Expr::Return(ret) => count_cognitive_return(ret, nesting, contributors),
        Expr::Closure(closure) => count_cognitive_closure(closure, nesting, contributors),
        Expr::Call(call) => count_cognitive_call(call, nesting, contributors),
        Expr::MethodCall(mc) => count_cognitive_method_call(mc, nesting, contributors),
        Expr::Tuple(tuple) => count_cognitive_tuple(tuple, nesting, contributors),
        Expr::Reference(r) => count_cognitive_expr(&r.expr, nesting, contributors),
        Expr::Unary(u) => count_cognitive_expr(&u.expr, nesting, contributors),
        Expr::Paren(p) => count_cognitive_expr(&p.expr, nesting, contributors),
        _ => 0,
    }
}

fn count_cognitive_match(
    expr_match: &syn::ExprMatch,
    nesting: u32,
    contributors: &mut Vec<ComplexityContributor>,
) -> u32 {
    add_contributor(
        contributors,
        ContributorKind::Match,
        expr_match.match_token.span,
        end_line_of(expr_match),
        nesting,
        1 + nesting,
    );
    let mut total = 1 + nesting; // +1 structural, +nesting
    total += count_cognitive_expr(&expr_match.expr, nesting, contributors);
    for arm in &expr_match.arms {
        if let Some(guard) = &arm.guard {
            total += count_cognitive_expr(&guard.1, nesting + 1, contributors);
        }
        total += count_cognitive_expr(&arm.body, nesting + 1, contributors);
    }
    total
}

fn count_cognitive_while(
    expr_while: &syn::ExprWhile,
    nesting: u32,
    contributors: &mut Vec<ComplexityContributor>,
) -> u32 {
    add_contributor(
        contributors,
        ContributorKind::WhileLoop,
        expr_while.while_token.span,
        end_line_of(expr_while),
        nesting,
        1 + nesting,
    );
    let mut total = 1 + nesting;
    total += count_cognitive_expr(&expr_while.cond, nesting, contributors);
    total += count_cognitive_block(&expr_while.body, nesting + 1, contributors);
    total
}

fn count_cognitive_for_loop(
    expr_for: &syn::ExprForLoop,
    nesting: u32,
    contributors: &mut Vec<ComplexityContributor>,
) -> u32 {
    add_contributor(
        contributors,
        ContributorKind::ForLoop,
        expr_for.for_token.span,
        end_line_of(expr_for),
        nesting,
        1 + nesting,
    );
    let mut total = 1 + nesting;
    total += count_cognitive_expr(&expr_for.expr, nesting, contributors);
    total += count_cognitive_block(&expr_for.body, nesting + 1, contributors);
    total
}

fn count_cognitive_loop(
    expr_loop: &syn::ExprLoop,
    nesting: u32,
    contributors: &mut Vec<ComplexityContributor>,
) -> u32 {
    add_contributor(
        contributors,
        ContributorKind::Loop,
        expr_loop.loop_token.span,
        end_line_of(expr_loop),
        nesting,
        1 + nesting,
    );
    let mut total = 1 + nesting;
    total += count_cognitive_block(&expr_loop.body, nesting + 1, contributors);
    total
}

// `count_cognitive_binary_chain` walks all `&&` / `||` operators in the
// tree; `count_cognitive_binary_operands` walks the non-binary leaves so
// constructs like `?`, `if`, etc. inside binary expressions are also
// counted.
fn count_cognitive_binary(
    bin: &ExprBinary,
    nesting: u32,
    contributors: &mut Vec<ComplexityContributor>,
) -> u32 {
    count_cognitive_binary_chain(bin, nesting, contributors)
        + count_cognitive_binary_operands(&bin.left, nesting, contributors)
        + count_cognitive_binary_operands(&bin.right, nesting, contributors)
}

fn count_cognitive_try(
    expr_try: &syn::ExprTry,
    nesting: u32,
    contributors: &mut Vec<ComplexityContributor>,
) -> u32 {
    add_contributor(
        contributors,
        ContributorKind::Try,
        expr_try.question_token.span,
        expr_try.question_token.span.end().line,
        nesting,
        1,
    );
    1 + count_cognitive_expr(&expr_try.expr, nesting, contributors)
}

fn count_cognitive_break(
    expr_break: &syn::ExprBreak,
    nesting: u32,
    contributors: &mut Vec<ComplexityContributor>,
) -> u32 {
    add_contributor(
        contributors,
        ContributorKind::Break,
        expr_break.break_token.span,
        expr_break.break_token.span.end().line,
        nesting,
        1,
    );
    1
}

fn count_cognitive_continue(
    expr_continue: &syn::ExprContinue,
    nesting: u32,
    contributors: &mut Vec<ComplexityContributor>,
) -> u32 {
    add_contributor(
        contributors,
        ContributorKind::Continue,
        expr_continue.continue_token.span,
        expr_continue.continue_token.span.end().line,
        nesting,
        1,
    );
    1
}

fn count_cognitive_return(
    ret: &syn::ExprReturn,
    nesting: u32,
    contributors: &mut Vec<ComplexityContributor>,
) -> u32 {
    match &ret.expr {
        Some(expr) => count_cognitive_expr(expr, nesting, contributors),
        None => 0,
    }
}

// Closures don't add a structural increment but bump nesting so any
// branching inside the closure body is charged for being deeper.
fn count_cognitive_closure(
    closure: &syn::ExprClosure,
    nesting: u32,
    contributors: &mut Vec<ComplexityContributor>,
) -> u32 {
    count_cognitive_expr(&closure.body, nesting + 1, contributors)
}

fn count_cognitive_call(
    call: &syn::ExprCall,
    nesting: u32,
    contributors: &mut Vec<ComplexityContributor>,
) -> u32 {
    let mut total = count_cognitive_expr(&call.func, nesting, contributors);
    for arg in &call.args {
        total += count_cognitive_expr(arg, nesting, contributors);
    }
    total
}

fn count_cognitive_method_call(
    mc: &syn::ExprMethodCall,
    nesting: u32,
    contributors: &mut Vec<ComplexityContributor>,
) -> u32 {
    let mut total = count_cognitive_expr(&mc.receiver, nesting, contributors);
    for arg in &mc.args {
        total += count_cognitive_expr(arg, nesting, contributors);
    }
    total
}

fn count_cognitive_tuple(
    tuple: &syn::ExprTuple,
    nesting: u32,
    contributors: &mut Vec<ComplexityContributor>,
) -> u32 {
    let mut total = 0;
    for elem in &tuple.elems {
        total += count_cognitive_expr(elem, nesting, contributors);
    }
    total
}

fn count_cognitive_if(
    expr_if: &syn::ExprIf,
    nesting: u32,
    contributors: &mut Vec<ComplexityContributor>,
) -> u32 {
    // if: +1 + nesting
    add_contributor(
        contributors,
        ContributorKind::IfBranch,
        expr_if.if_token.span,
        end_line_of(expr_if),
        nesting,
        1 + nesting,
    );
    let mut total = 1 + nesting;

    // Count complexity in the condition (for && / || chains)
    total += count_cognitive_expr(&expr_if.cond, nesting, contributors);

    // Then branch
    total += count_cognitive_block(&expr_if.then_branch, nesting + 1, contributors);

    // Else branch
    if let Some((_, else_branch)) = &expr_if.else_branch {
        match else_branch.as_ref() {
            Expr::If(else_if) => {
                // else if: +1 continuation (no nesting increment)
                add_contributor(
                    contributors,
                    ContributorKind::IfBranch,
                    else_if.if_token.span,
                    end_line_of(else_if),
                    nesting,
                    1,
                );
                total += 1;
                total += count_cognitive_expr(&else_if.cond, nesting, contributors);
                total += count_cognitive_block(&else_if.then_branch, nesting + 1, contributors);
                if let Some((_, inner_else)) = &else_if.else_branch {
                    total += count_cognitive_else(inner_else, nesting, contributors);
                }
            }
            Expr::Block(block) => {
                // else: +0 (NOT a structural increment per Sonar spec)
                total += count_cognitive_block(&block.block, nesting + 1, contributors);
            }
            other => {
                total += count_cognitive_expr(other, nesting + 1, contributors);
            }
        }
    }

    total
}

fn count_cognitive_else(
    expr: &Expr,
    nesting: u32,
    contributors: &mut Vec<ComplexityContributor>,
) -> u32 {
    match expr {
        Expr::If(else_if) => {
            // else if: +1 continuation
            add_contributor(
                contributors,
                ContributorKind::IfBranch,
                else_if.if_token.span,
                end_line_of(else_if),
                nesting,
                1,
            );
            let mut total = 1;
            total += count_cognitive_expr(&else_if.cond, nesting, contributors);
            total += count_cognitive_block(&else_if.then_branch, nesting + 1, contributors);
            if let Some((_, inner_else)) = &else_if.else_branch {
                total += count_cognitive_else(inner_else, nesting, contributors);
            }
            total
        }
        Expr::Block(block) => {
            // else: +0
            count_cognitive_block(&block.block, nesting + 1, contributors)
        }
        other => count_cognitive_expr(other, nesting + 1, contributors),
    }
}

/// Count cognitive complexity for a chain of binary operators.
/// Same-operator sequences count as +1 total; operator switches add +1 each.
fn count_cognitive_binary_chain(
    bin: &ExprBinary,
    nesting: u32,
    contributors: &mut Vec<ComplexityContributor>,
) -> u32 {
    let ops = flatten_binary_ops(bin);
    if ops.is_empty() {
        return 0;
    }

    let mut total = 0;
    let mut last_is_logical: Option<BoolOp> = None;

    for (op, span) in &ops {
        match op {
            BoolOp::And | BoolOp::Or => {
                if last_is_logical != Some(*op) {
                    add_contributor(
                        contributors,
                        ContributorKind::LogicalOperator,
                        *span,
                        span.end().line,
                        nesting,
                        1,
                    );
                    total += 1; // New sequence or operator switch
                }
                last_is_logical = Some(*op);
            }
            BoolOp::Other => {
                last_is_logical = None;
            }
        }
    }

    total
}

/// Walk the non-binary leaf sub-expressions of a binary expression tree and count their
/// cognitive contributions. Binary sub-expressions are skipped (their logical operators
/// are already counted by `count_cognitive_binary_chain`); only non-binary leaves (e.g.,
/// `?`, nested `if`, closures) reach `count_cognitive_expr`.
fn count_cognitive_binary_operands(
    expr: &Expr,
    nesting: u32,
    contributors: &mut Vec<ComplexityContributor>,
) -> u32 {
    match expr {
        Expr::Binary(bin) => {
            count_cognitive_binary_operands(&bin.left, nesting, contributors)
                + count_cognitive_binary_operands(&bin.right, nesting, contributors)
        }
        other => count_cognitive_expr(other, nesting, contributors),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoolOp {
    And,
    Or,
    Other,
}

fn classify_binop(op: &syn::BinOp) -> BoolOp {
    match op {
        syn::BinOp::And(_) => BoolOp::And,
        syn::BinOp::Or(_) => BoolOp::Or,
        _ => BoolOp::Other,
    }
}

fn binop_span(op: &syn::BinOp) -> proc_macro2::Span {
    match op {
        syn::BinOp::And(t) => t.spans[0],
        syn::BinOp::Or(t) => t.spans[0],
        // Non-logical operators (e.g. +, ==) appear when flattening nested
        // binary expressions; their spans are never accessed (BoolOp::Other
        // only resets the sequence counter in the caller).
        _ => proc_macro2::Span::call_site(),
    }
}

/// Flatten a binary expression tree into an in-order sequence of `(BoolOp, Span)` pairs.
/// Span points to the operator token that would fire the increment.
fn flatten_binary_ops(bin: &ExprBinary) -> Vec<(BoolOp, proc_macro2::Span)> {
    let mut ops = Vec::new();
    flatten_binary_ops_inner(&bin.left, &mut ops);
    ops.push((classify_binop(&bin.op), binop_span(&bin.op)));
    flatten_binary_ops_inner(&bin.right, &mut ops);
    ops
}

fn flatten_binary_ops_inner(expr: &Expr, ops: &mut Vec<(BoolOp, proc_macro2::Span)>) {
    if let Expr::Binary(bin) = expr {
        flatten_binary_ops_inner(&bin.left, ops);
        ops.push((classify_binop(&bin.op), binop_span(&bin.op)));
        flatten_binary_ops_inner(&bin.right, ops);
    }
}

// ── Cyclomatic complexity ──────────────────────────────────────────────

fn count_cyclomatic_block(
    block: &syn::Block,
    contributors: &mut Vec<ComplexityContributor>,
) -> u32 {
    CyclomaticCounter::count_block(block, contributors)
}

struct CyclomaticCounter<'a> {
    contributors: &'a mut Vec<ComplexityContributor>,
    total: u32,
    nesting: u32,
}

impl<'a> CyclomaticCounter<'a> {
    fn count_block(block: &syn::Block, contributors: &'a mut Vec<ComplexityContributor>) -> u32 {
        let mut counter = Self {
            contributors,
            total: 0,
            nesting: 0,
        };
        counter.visit_block(block);
        counter.total
    }

    fn bump(&mut self, kind: ContributorKind, span: Span, end_line: usize, increment: u32) {
        add_contributor(
            self.contributors,
            kind,
            span,
            end_line,
            self.nesting,
            increment,
        );
        self.total += increment;
    }

    fn with_nesting<F: FnOnce(&mut Self)>(&mut self, f: F) {
        self.nesting += 1;
        f(self);
        self.nesting -= 1;
    }
}

impl<'ast> Visit<'ast> for CyclomaticCounter<'_> {
    fn visit_stmt(&mut self, stmt: &'ast syn::Stmt) {
        match stmt {
            syn::Stmt::Expr(expr, _) => self.visit_expr(expr),
            syn::Stmt::Local(local) => self.visit_local(local),
            syn::Stmt::Item(_) | syn::Stmt::Macro(_) => {}
        }
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        let Some(init) = &local.init else {
            return;
        };

        self.visit_expr(&init.expr);
        if let Some((else_token, diverge)) = &init.diverge {
            self.bump(
                ContributorKind::LetElse,
                else_token.span,
                end_line_of(diverge.as_ref()),
                1,
            );
            self.with_nesting(|s| s.visit_expr(diverge));
        }
    }

    fn visit_expr_if(&mut self, expr_if: &'ast syn::ExprIf) {
        self.bump(
            ContributorKind::IfBranch,
            expr_if.if_token.span,
            end_line_of(expr_if),
            1,
        );
        self.visit_expr(&expr_if.cond);
        self.with_nesting(|s| s.visit_block(&expr_if.then_branch));
        if let Some((_, else_branch)) = &expr_if.else_branch {
            match else_branch.as_ref() {
                // `else if` continues the chain at the same nesting level (matches
                // cognitive's `count_cognitive_else` which does not bump nesting
                // for the else-if construct itself, only for its body).
                Expr::If(_) => self.visit_expr(else_branch),
                // `else { ... }` opens a new block at +1 nesting.
                _ => self.with_nesting(|s| s.visit_expr(else_branch)),
            }
        }
    }

    fn visit_expr_match(&mut self, expr_match: &'ast syn::ExprMatch) {
        for arm in expr_match.arms.iter().skip(1) {
            self.bump(
                ContributorKind::MatchArm,
                arm.pat.span(),
                end_line_of(arm),
                1,
            );
        }

        self.visit_expr(&expr_match.expr);
        for arm in &expr_match.arms {
            self.with_nesting(|s| s.visit_arm(arm));
        }
    }

    fn visit_arm(&mut self, arm: &'ast syn::Arm) {
        if let Some((_, guard)) = &arm.guard {
            self.visit_expr(guard);
        }
        self.visit_expr(&arm.body);
    }

    fn visit_expr_while(&mut self, expr_while: &'ast syn::ExprWhile) {
        self.bump(
            ContributorKind::WhileLoop,
            expr_while.while_token.span,
            end_line_of(expr_while),
            1,
        );
        self.visit_expr(&expr_while.cond);
        self.with_nesting(|s| s.visit_block(&expr_while.body));
    }

    fn visit_expr_for_loop(&mut self, expr_for: &'ast syn::ExprForLoop) {
        self.bump(
            ContributorKind::ForLoop,
            expr_for.for_token.span,
            end_line_of(expr_for),
            1,
        );
        self.visit_expr(&expr_for.expr);
        self.with_nesting(|s| s.visit_block(&expr_for.body));
    }

    fn visit_expr_loop(&mut self, expr_loop: &'ast syn::ExprLoop) {
        self.bump(
            ContributorKind::Loop,
            expr_loop.loop_token.span,
            end_line_of(expr_loop),
            1,
        );
        self.with_nesting(|s| s.visit_block(&expr_loop.body));
    }

    fn visit_expr_binary(&mut self, expr_binary: &'ast syn::ExprBinary) {
        if matches!(expr_binary.op, syn::BinOp::And(_) | syn::BinOp::Or(_)) {
            let span = binop_span(&expr_binary.op);
            self.bump(ContributorKind::LogicalOperator, span, span.end().line, 1);
        }
        self.visit_expr(&expr_binary.left);
        self.visit_expr(&expr_binary.right);
    }

    fn visit_expr_try(&mut self, expr_try: &'ast syn::ExprTry) {
        self.bump(
            ContributorKind::Try,
            expr_try.question_token.span,
            expr_try.question_token.span.end().line,
            1,
        );
        self.visit_expr(&expr_try.expr);
    }

    fn visit_expr_break(&mut self, expr_break: &'ast syn::ExprBreak) {
        self.bump(
            ContributorKind::Break,
            expr_break.break_token.span,
            expr_break.break_token.span.end().line,
            1,
        );
        if let Some(expr) = &expr_break.expr {
            self.visit_expr(expr);
        }
    }

    fn visit_expr_continue(&mut self, expr_continue: &'ast syn::ExprContinue) {
        self.bump(
            ContributorKind::Continue,
            expr_continue.continue_token.span,
            expr_continue.continue_token.span.end().line,
            1,
        );
    }

    fn visit_expr_closure(&mut self, expr_closure: &'ast syn::ExprClosure) {
        // Closures count as deeper nesting for any constructs in their body
        // (mirrors `count_cognitive_expr`'s `Expr::Closure` branch which
        // recurses with `nesting + 1`).
        self.with_nesting(|s| s.visit_expr(&expr_closure.body));
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod test_helpers {
    use super::*;

    pub fn adapter() -> SynComplexityAdapter {
        SynComplexityAdapter::new()
    }

    pub fn load_fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to read fixture {path}: {e}"))
    }

    pub fn extract_fixture(name: &str, metric: ComplexityMetric) -> Vec<FunctionComplexity> {
        let source = load_fixture(name);
        adapter()
            .extract(&source, &format!("tests/fixtures/{name}"), metric)
            .unwrap()
    }

    pub fn find_fn<'a>(fns: &'a [FunctionComplexity], name: &str) -> &'a FunctionComplexity {
        fns.iter()
            .find(|f| f.identity.qualified_name == name)
            .unwrap_or_else(|| {
                panic!(
                    "Function '{name}' not found in: {:?}",
                    fns.iter()
                        .map(|f| &f.identity.qualified_name)
                        .collect::<Vec<_>>()
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::test_helpers::*;
    use super::*;
    use pretty_assertions::assert_eq;

    // ── Baseline ───────────────────────────────────────────────────────

    #[test]
    fn baseline_complexity_is_one_cognitive() {
        let fns = extract_fixture("simple_functions.rs", ComplexityMetric::Cognitive);
        let f = find_fn(&fns, "empty_body");
        assert_eq!(f.complexity, 1);
    }

    #[test]
    fn baseline_complexity_is_one_cyclomatic() {
        let fns = extract_fixture("simple_functions.rs", ComplexityMetric::Cyclomatic);
        let f = find_fn(&fns, "empty_body");
        assert_eq!(f.complexity, 1);
    }

    // ── Identity & Span ────────────────────────────────────────────────

    #[test]
    fn top_level_fn_identity() {
        let fns = extract_fixture("simple_functions.rs", ComplexityMetric::Cognitive);
        let f = find_fn(&fns, "empty_body");
        assert_eq!(f.identity.file_path, "tests/fixtures/simple_functions.rs");
        assert!(f.identity.span.start_line > 0);
        assert!(f.identity.span.end_line >= f.identity.span.start_line);
    }

    #[test]
    fn impl_method_qualified_name() {
        let fns = extract_fixture("impl_methods.rs", ComplexityMetric::Cognitive);
        let _new = find_fn(&fns, "Calculator::new");
        let _add = find_fn(&fns, "Calculator::add");
        let _div = find_fn(&fns, "Calculator::divide");
    }

    // ── Nested-mod qualified names (crap-rs#283) ───────────────────────

    #[test]
    fn top_level_fn_in_nested_mods_fixture_is_unqualified() {
        // A free function at file root carries no mod prefix — its
        // qualified name is just its ident.
        let fns = extract_fixture("nested_mods.rs", ComplexityMetric::Cognitive);
        let _top = find_fn(&fns, "top_level");
    }

    #[test]
    fn one_level_mod_qualified_name() {
        let fns = extract_fixture("nested_mods.rs", ComplexityMetric::Cognitive);
        let _f = find_fn(&fns, "outer::in_outer");
    }

    #[test]
    fn multi_level_mod_qualified_name() {
        let fns = extract_fixture("nested_mods.rs", ComplexityMetric::Cognitive);
        let _f = find_fn(&fns, "outer::inner::deep");
    }

    #[test]
    fn impl_method_inside_mod_prepends_mod_path() {
        // The Discovery answer: `Type::method` inside `mod outer` is
        // `outer::Type::method` — the mod path is a pure prefix on top
        // of the existing impl-type qualification.
        let fns = extract_fixture("nested_mods.rs", ComplexityMetric::Cognitive);
        let _m = find_fn(&fns, "outer::Widget::render");
    }

    #[test]
    fn nested_fn_inside_mod_is_mod_scoped_not_fn_scoped() {
        // The walker threads `mod` nesting only, never `fn` nesting. A
        // `fn helper` declared inside `outer::with_nested` is emitted as
        // `outer::helper`, NOT `outer::with_nested::helper`.
        let fns = extract_fixture("nested_mods.rs", ComplexityMetric::Cognitive);
        let _outer = find_fn(&fns, "outer::with_nested");
        let _helper = find_fn(&fns, "outer::helper");
        assert!(
            fns.iter()
                .all(|f| f.identity.qualified_name != "outer::with_nested::helper"),
            "nested fn must not inherit the enclosing fn's name as a path segment"
        );
    }

    #[test]
    fn mod_qualification_does_not_disturb_complexity() {
        // Threading the mod path is a display-only change — complexity
        // for a fn inside a mod is computed exactly as if it were at
        // file root.
        let fns = extract_fixture("nested_mods.rs", ComplexityMetric::Cognitive);
        let render = find_fn(&fns, "outer::Widget::render");
        assert_eq!(render.complexity, 1, "render has no branching");
        let deep = find_fn(&fns, "outer::inner::deep");
        assert_eq!(deep.complexity, 1, "deep is an empty body");
    }

    #[test]
    fn span_accuracy() {
        let source = load_fixture("simple_functions.rs");
        let fns = adapter()
            .extract(&source, "test.rs", ComplexityMetric::Cognitive)
            .unwrap();
        let f = find_fn(&fns, "empty_body");
        // Verify the span covers the function — start may include doc comments
        let lines: Vec<&str> = source.lines().collect();
        let span_lines: Vec<&str> =
            lines[f.identity.span.start_line - 1..f.identity.span.end_line].to_vec();
        let span_text = span_lines.join("\n");
        assert!(
            span_text.contains("fn empty_body"),
            "Span should contain fn signature, got: {span_text}"
        );
        assert!(
            f.identity.span.end_line > f.identity.span.start_line
                || span_text.contains('}')
                || span_text.contains("{}")
        );
    }

    #[test]
    fn span_carries_one_based_columns() {
        // proc_macro2::LineColumn columns are 0-based; SourceSpan must carry
        // 1-based columns so SARIF region.startColumn / endColumn render
        // correctly under GitHub Code Scanning. `pub fn empty_body() {}` is
        // unindented in simple_functions.rs, so column 0 (0-based) maps to
        // column 1 (1-based) on the wire — never 0.
        let fns = extract_fixture("simple_functions.rs", ComplexityMetric::Cognitive);
        let f = find_fn(&fns, "empty_body");
        assert!(
            f.identity.span.start_column >= 1,
            "start_column should be 1-based and >=1, got {}",
            f.identity.span.start_column
        );
        assert!(
            f.identity.span.end_column >= 1,
            "end_column should be 1-based and >=1, got {}",
            f.identity.span.end_column
        );
    }

    #[test]
    fn columns_are_exact_one_based_offsets_unindented() {
        // Deterministic source: `fn foo() {}` on line 1 starts with `f`
        // at column 0 (0-based) and the closing `}` is at column 10
        // (0-based, last character). `SourceSpan` stores 1-based
        // *inclusive* columns, mirroring `end_line`:
        //
        //   start_column = 1   (`f` at 0-based 0  → 1-based 1 inclusive)
        //   end_column   = 11  (`}` at 0-based 10 → 1-based 11 inclusive)
        //
        // Pinning EXACT values is what kills the cargo-mutants survivors:
        //   `start_col + 1` → `start_col - 1`: 0-1 wraps, ≠ 1
        //   `start_col + 1` → `start_col * 1`: 0*1 = 0, ≠ 1
        //   `end_col`        → any arithmetic mutant: ≠ 11
        let source = "fn foo() {}\n";
        let fns = adapter()
            .extract(source, "probe.rs", ComplexityMetric::Cognitive)
            .unwrap();
        let f = &fns[0];
        assert_eq!(f.identity.qualified_name, "foo");
        assert_eq!(
            f.identity.span.start_column, 1,
            "`f` at column 0 (0-based) → 1 (1-based inclusive)"
        );
        assert_eq!(
            f.identity.span.end_column, 11,
            "`}}` at column 10 (0-based) → 11 (1-based inclusive)"
        );
    }

    #[test]
    fn columns_are_exact_one_based_offsets_indented() {
        // Indented variant pins both columns to nonzero asymmetric values,
        // so a `* 1` mutation on `start_column` can't accidentally
        // produce the right answer either.
        //   "    pub fn bar() {}"
        //    0123456789012345678
        //        ^ p at col 4 → start_column = 5
        //                      ^ } at col 18 → end_column = 19
        let source = "    pub fn bar() {}\n";
        let fns = adapter()
            .extract(source, "probe.rs", ComplexityMetric::Cognitive)
            .unwrap();
        let f = &fns[0];
        assert_eq!(f.identity.qualified_name, "bar");
        assert_eq!(
            f.identity.span.start_column, 5,
            "`p` at column 4 (0-based) → 5 (1-based inclusive)"
        );
        assert_eq!(
            f.identity.span.end_column, 19,
            "`}}` at column 18 (0-based) → 19 (1-based inclusive)"
        );
    }

    #[test]
    fn multiple_fns_returns_all() {
        let fns = extract_fixture("simple_functions.rs", ComplexityMetric::Cognitive);
        assert_eq!(fns.len(), 4); // empty_body, single_if, nested_if, early_return
    }

    #[test]
    fn async_fn_detected() {
        let fns = extract_fixture("control_flow.rs", ComplexityMetric::Cognitive);
        let f = find_fn(&fns, "async_fetch");
        assert!(f.identity.span.start_line > 0);
        assert!(f.complexity >= 1);
    }

    // ── Cognitive complexity ───────────────────────────────────────────

    #[test]
    fn if_else_cognitive() {
        let fns = extract_fixture("simple_functions.rs", ComplexityMetric::Cognitive);
        let f = find_fn(&fns, "single_if");
        // if(+1+0nesting) + base(1) = 2
        assert_eq!(f.complexity, 2);
    }

    #[test]
    fn nested_if_cognitive_penalty() {
        let fns = extract_fixture("simple_functions.rs", ComplexityMetric::Cognitive);
        let f = find_fn(&fns, "nested_if");
        // base(1) + outer_if(+1+0) + inner_if(+1+1nesting) = 4
        assert_eq!(f.complexity, 4);
    }

    #[test]
    fn else_is_not_counted_cognitive() {
        // single_if has if/else — else should be +0
        let fns = extract_fixture("simple_functions.rs", ComplexityMetric::Cognitive);
        let f = find_fn(&fns, "single_if");
        // if(+1), else(+0), base(1) = 2 (NOT 3)
        assert_eq!(f.complexity, 2);
    }

    #[test]
    fn match_flat_cognitive() {
        let fns = extract_fixture("flat_match.rs", ComplexityMetric::Cognitive);
        let f = find_fn(&fns, "http_status_text");
        // base(1) + match(+1+0nesting) = 2
        assert_eq!(f.complexity, 2);
    }

    // ── Cyclomatic complexity ──────────────────────────────────────────

    #[test]
    fn if_else_cyclomatic() {
        let fns = extract_fixture("simple_functions.rs", ComplexityMetric::Cyclomatic);
        let f = find_fn(&fns, "single_if");
        // base(1) + if(+1) = 2
        assert_eq!(f.complexity, 2);
    }

    #[test]
    fn nested_if_cyclomatic() {
        let fns = extract_fixture("simple_functions.rs", ComplexityMetric::Cyclomatic);
        let f = find_fn(&fns, "nested_if");
        // base(1) + outer_if(+1) + inner_if(+1) = 3
        assert_eq!(f.complexity, 3);
    }

    #[test]
    fn match_arms_cyclomatic() {
        let fns = extract_fixture("flat_match.rs", ComplexityMetric::Cyclomatic);
        let f = find_fn(&fns, "http_status_text");
        // base(1) + (11 arms - 1) = 11
        assert_eq!(f.complexity, 11);
    }

    #[test]
    fn cognitive_gte_cyclomatic_for_nested() {
        // Nested if: cognitive should be >= cyclomatic due to nesting penalty
        let cog = extract_fixture("simple_functions.rs", ComplexityMetric::Cognitive);
        let cyc = extract_fixture("simple_functions.rs", ComplexityMetric::Cyclomatic);
        let cog_val = find_fn(&cog, "nested_if").complexity;
        let cyc_val = find_fn(&cyc, "nested_if").complexity;
        assert!(
            cog_val >= cyc_val,
            "Cognitive ({cog_val}) should be >= cyclomatic ({cyc_val}) for nested code"
        );
    }

    // ── Boolean operators ──────────────────────────────────────────────

    #[test]
    fn bool_same_sequence_cognitive() {
        let fns = extract_fixture("bool_operators.rs", ComplexityMetric::Cognitive);
        let f = find_fn(&fns, "same_sequence");
        // base(1) + &&-sequence(+1) = 2
        assert_eq!(f.complexity, 2);
    }

    #[test]
    fn bool_same_sequence_cyclomatic() {
        let fns = extract_fixture("bool_operators.rs", ComplexityMetric::Cyclomatic);
        let f = find_fn(&fns, "same_sequence");
        // base(1) + &&(+1) + &&(+1) = 3
        assert_eq!(f.complexity, 3);
    }

    #[test]
    fn bool_operator_switch_cognitive() {
        let fns = extract_fixture("bool_operators.rs", ComplexityMetric::Cognitive);
        let f = find_fn(&fns, "operator_switch");
        // base(1) + &&-sequence(+1) + ||-switch(+1) = 3
        assert_eq!(f.complexity, 3);
    }

    #[test]
    fn bool_operator_switch_cyclomatic() {
        let fns = extract_fixture("bool_operators.rs", ComplexityMetric::Cyclomatic);
        let f = find_fn(&fns, "operator_switch");
        // base(1) + &&(+1) + ||(+1) = 3
        assert_eq!(f.complexity, 3);
    }

    #[test]
    fn bool_in_condition_cognitive() {
        let fns = extract_fixture("bool_operators.rs", ComplexityMetric::Cognitive);
        let f = find_fn(&fns, "bool_in_condition");
        // base(1) + if(+1+0) + &&(+1) + ||(+1) = 4
        assert_eq!(f.complexity, 4);
    }

    #[test]
    fn long_or_chain_cognitive() {
        let fns = extract_fixture("bool_operators.rs", ComplexityMetric::Cognitive);
        let f = find_fn(&fns, "long_or_chain");
        // base(1) + ||-sequence(+1) = 2
        assert_eq!(f.complexity, 2);
    }

    #[test]
    fn alternating_operators_cognitive() {
        let fns = extract_fixture("bool_operators.rs", ComplexityMetric::Cognitive);
        let f = find_fn(&fns, "alternating_operators");
        // base(1) + &&(+1) + ||(+1) + &&(+1) = 4
        assert_eq!(f.complexity, 4);
    }

    // ── Closure ────────────────────────────────────────────────────────

    #[test]
    fn closure_in_parent_cognitive() {
        let fns = extract_fixture("control_flow.rs", ComplexityMetric::Cognitive);
        let f = find_fn(&fns, "with_closure");
        // base(1) + closure-body &&(+1) = 2 (closure itself is +0)
        assert!(
            f.complexity >= 2,
            "Closure branches should count toward parent"
        );
    }

    // ── Try operator ───────────────────────────────────────────────────

    #[test]
    fn try_operator_cognitive() {
        let fns = extract_fixture("control_flow.rs", ComplexityMetric::Cognitive);
        let f = find_fn(&fns, "with_try_operator");
        // base(1) + ?(+1) = 2
        assert_eq!(f.complexity, 2);
    }

    #[test]
    fn try_operator_cyclomatic() {
        let fns = extract_fixture("control_flow.rs", ComplexityMetric::Cyclomatic);
        let f = find_fn(&fns, "with_try_operator");
        // base(1) + ?(+1) = 2
        assert_eq!(f.complexity, 2);
    }

    // ── Break / Continue ───────────────────────────────────────────────

    #[test]
    fn loop_with_break_cognitive() {
        let fns = extract_fixture("control_flow.rs", ComplexityMetric::Cognitive);
        let f = find_fn(&fns, "loop_with_break");
        // base(1) + loop(+1+0) + if(+1+1nesting) + break(+1) = 5
        assert_eq!(f.complexity, 5);
    }

    #[test]
    fn loop_with_break_cyclomatic() {
        let fns = extract_fixture("control_flow.rs", ComplexityMetric::Cyclomatic);
        let f = find_fn(&fns, "loop_with_break");
        // base(1) + loop(+1) + if(+1) + break(+1) = 4
        assert_eq!(f.complexity, 4);
    }

    // ── let...else ─────────────────────────────────────────────────────

    #[test]
    fn let_else_cognitive() {
        let fns = extract_fixture("control_flow.rs", ComplexityMetric::Cognitive);
        let f = find_fn(&fns, "let_else_early_exit");
        // base(1) + let-else(+1+0nesting) = 2
        assert_eq!(f.complexity, 2);
    }

    #[test]
    fn let_else_cyclomatic() {
        let fns = extract_fixture("control_flow.rs", ComplexityMetric::Cyclomatic);
        let f = find_fn(&fns, "let_else_early_exit");
        // base(1) + let-else(+1) = 2
        assert_eq!(f.complexity, 2);
    }

    // ── Chained ? and nested expressions ──────────────────────────────

    #[test]
    fn chained_try_cognitive() {
        let fns = extract_fixture("control_flow.rs", ComplexityMetric::Cognitive);
        let f = find_fn(&fns, "chained_try");
        // base(1) + ?(+1) + ?(+1) = 3
        assert_eq!(f.complexity, 3);
    }

    #[test]
    fn chained_try_cyclomatic() {
        let fns = extract_fixture("control_flow.rs", ComplexityMetric::Cyclomatic);
        let f = find_fn(&fns, "chained_try");
        // base(1) + ?(+1) + ?(+1) = 3
        assert_eq!(f.complexity, 3);
    }

    #[test]
    fn for_iterator_try_cognitive() {
        let fns = extract_fixture("control_flow.rs", ComplexityMetric::Cognitive);
        let f = find_fn(&fns, "for_with_try_iterator");
        // base(1) + for(+1+0) + ?(+1) in iterator + if(+1+1nesting) = 5
        assert_eq!(f.complexity, 5);
    }

    #[test]
    fn for_iterator_try_cyclomatic() {
        let fns = extract_fixture("control_flow.rs", ComplexityMetric::Cyclomatic);
        let f = find_fn(&fns, "for_with_try_iterator");
        // base(1) + for(+1) + ?(+1) in iterator + if(+1) = 4
        assert_eq!(f.complexity, 4);
    }

    #[test]
    fn match_scrutinee_try_cognitive() {
        let fns = extract_fixture("control_flow.rs", ComplexityMetric::Cognitive);
        let f = find_fn(&fns, "match_with_try_scrutinee");
        // base(1) + match(+1+0) + ?(+1) in scrutinee = 3
        assert_eq!(f.complexity, 3);
    }

    // ── Trait default methods ─────────────────────────────────────────

    #[test]
    fn trait_default_method_found() {
        let fns = extract_fixture("control_flow.rs", ComplexityMetric::Cognitive);
        let f = find_fn(&fns, "Describable::describe");
        // base(1) + if(+1+0) = 2
        assert_eq!(f.complexity, 2);
    }

    #[test]
    fn trait_method_without_default_not_found() {
        let fns = extract_fixture("control_flow.rs", ComplexityMetric::Cognitive);
        // `name` has no default body — should NOT appear
        assert!(
            fns.iter()
                .all(|f| f.identity.qualified_name != "Describable::name"),
            "Trait methods without default bodies should not be recorded"
        );
    }

    /// Regression guard: trait default body + concrete impl that
    /// overrides the same-named method must yield two FunctionComplexity
    /// entries with disjoint spans. The walker uses separate visitors
    /// (`visit_trait_item_fn` for the default, `visit_impl_item_fn` for the
    /// override), each calling `count_complexity` on its own block — so
    /// contributors and `ProposedSplit`s cannot leak across the
    /// trait/impl boundary by construction.
    #[test]
    fn trait_default_and_concrete_override_have_disjoint_spans() {
        let fns = extract_fixture("trait_default_override.rs", ComplexityMetric::Cognitive);

        let default = find_fn(&fns, "Greeter::greet");
        let override_ = find_fn(&fns, "Casual::greet");

        assert_eq!(default.complexity, 2, "trait default body");
        // base(1) + if(+1+0) + else-if(+1+0) = 3 (else-if does not add nesting)
        assert_eq!(override_.complexity, 3, "concrete impl override body");

        let default_span = &default.identity.span;
        let override_span = &override_.identity.span;
        assert!(
            default_span.end_line < override_span.start_line,
            "default body must end strictly before concrete override begins \
             (default {default_span:?}, override {override_span:?})"
        );

        // Contributors must stay inside their own function's span.
        for c in &default.contributors {
            assert!(
                c.line >= default_span.start_line && c.line <= default_span.end_line,
                "default contributor at line {} escaped span {default_span:?}",
                c.line
            );
        }
        for c in &override_.contributors {
            assert!(
                c.line >= override_span.start_line && c.line <= override_span.end_line,
                "override contributor at line {} escaped span {override_span:?}",
                c.line
            );
        }
    }

    // ── Impl methods complexity ────────────────────────────────────────

    #[test]
    fn impl_method_no_branch() {
        let fns = extract_fixture("impl_methods.rs", ComplexityMetric::Cognitive);
        let f = find_fn(&fns, "Calculator::add");
        assert_eq!(f.complexity, 1);
    }

    #[test]
    fn impl_method_with_branch() {
        let fns = extract_fixture("impl_methods.rs", ComplexityMetric::Cognitive);
        let f = find_fn(&fns, "Calculator::divide");
        // base(1) + if(+1+0) = 2
        assert_eq!(f.complexity, 2);
    }

    // ── Error handling ─────────────────────────────────────────────────

    #[test]
    fn invalid_source_returns_error() {
        let result = adapter().extract("fn {{{{ broken", "test.rs", ComplexityMetric::Cognitive);
        assert!(result.is_err());
        match result.unwrap_err() {
            CrapError::SourceParse(msg) => assert!(msg.contains("test.rs")),
            other => panic!("Expected SourceParse, got: {other:?}"),
        }
    }
}

/// ── Contributor golden tests ──────────────────────────────────────────────
#[cfg(test)]
mod contributor_tests {
    use super::test_helpers::*;
    use super::*;
    use crate::domain::types::ContributorKind;

    fn contributors_for(
        fn_name: &str,
        metric: ComplexityMetric,
    ) -> Vec<crate::domain::types::ComplexityContributor> {
        let fns = extract_fixture("contributors_fixture.rs", metric);
        let f = find_fn(&fns, fn_name);
        f.contributors.clone()
    }

    #[test]
    fn base_fn_empty_contributors() {
        for metric in [ComplexityMetric::Cognitive, ComplexityMetric::Cyclomatic] {
            let fns = extract_fixture("contributors_fixture.rs", metric);
            let f = find_fn(&fns, "empty_fn");
            assert!(
                f.contributors.is_empty(),
                "empty_fn should have no contributors for {metric}"
            );
            assert_eq!(f.complexity, 1);
        }
    }

    #[test]
    fn if_branch_contributor_cognitive() {
        let cs = contributors_for("single_if_fn", ComplexityMetric::Cognitive);
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].kind, ContributorKind::IfBranch);
        assert_eq!(cs[0].increment, 1);
        assert!(cs[0].line > 0);
    }

    #[test]
    fn nested_if_nesting_increment() {
        let cs = contributors_for("nested_if_fn", ComplexityMetric::Cognitive);
        assert_eq!(cs.len(), 2);
        // Sorted by line: outer if first, inner if second
        assert_eq!(cs[0].kind, ContributorKind::IfBranch);
        assert_eq!(cs[0].increment, 1); // outer: nesting=0
        assert_eq!(cs[1].kind, ContributorKind::IfBranch);
        assert_eq!(cs[1].increment, 2); // inner: nesting=1
        assert!(cs[0].line < cs[1].line);
    }

    #[test]
    fn match_contributor_cognitive() {
        let cs = contributors_for("match_fn", ComplexityMetric::Cognitive);
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].kind, ContributorKind::Match);
        assert_eq!(cs[0].increment, 1); // 1 + nesting(0)
        assert!(cs[0].line > 0);
    }

    #[test]
    fn match_arm_contributor_cyclomatic() {
        let fns = extract_fixture("contributors_fixture.rs", ComplexityMetric::Cyclomatic);
        let f = find_fn(&fns, "match_fn");
        // 4 arms → 3 MatchArm contributors (N-1)
        let arms: Vec<_> = f
            .contributors
            .iter()
            .filter(|c| c.kind == ContributorKind::MatchArm)
            .collect();
        assert_eq!(arms.len(), 3);
        for c in &arms {
            assert_eq!(c.increment, 1);
        }
    }

    #[test]
    fn try_contributor_cognitive() {
        let cs = contributors_for("try_fn", ComplexityMetric::Cognitive);
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].kind, ContributorKind::Try);
        assert_eq!(cs[0].increment, 1);
    }

    #[test]
    fn try_contributor_cyclomatic() {
        let cs = contributors_for("try_fn", ComplexityMetric::Cyclomatic);
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].kind, ContributorKind::Try);
        assert_eq!(cs[0].increment, 1);
    }

    #[test]
    fn let_else_contributor_cognitive() {
        let cs = contributors_for("let_else_fn", ComplexityMetric::Cognitive);
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].kind, ContributorKind::LetElse);
        assert_eq!(cs[0].increment, 1); // 1 + nesting(0)
    }

    #[test]
    fn let_else_contributor_cyclomatic() {
        let cs = contributors_for("let_else_fn", ComplexityMetric::Cyclomatic);
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].kind, ContributorKind::LetElse);
        assert_eq!(cs[0].increment, 1);
    }

    #[test]
    fn loop_contributor_cognitive() {
        let cs = contributors_for("loop_fn", ComplexityMetric::Cognitive);
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].kind, ContributorKind::Loop);
        assert_eq!(cs[0].increment, 1); // 1 + nesting(0)
    }

    #[test]
    fn for_loop_contributor_cognitive() {
        let cs = contributors_for("for_loop_fn", ComplexityMetric::Cognitive);
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].kind, ContributorKind::ForLoop);
        assert_eq!(cs[0].increment, 1);
    }

    #[test]
    fn for_loop_contributor_cyclomatic() {
        let cs = contributors_for("for_loop_fn", ComplexityMetric::Cyclomatic);
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].kind, ContributorKind::ForLoop);
        assert_eq!(cs[0].increment, 1);
    }

    #[test]
    fn while_loop_contributor_cognitive() {
        let cs = contributors_for("while_loop_fn", ComplexityMetric::Cognitive);
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].kind, ContributorKind::WhileLoop);
        assert_eq!(cs[0].increment, 1);
    }

    #[test]
    fn while_loop_contributor_cyclomatic() {
        let cs = contributors_for("while_loop_fn", ComplexityMetric::Cyclomatic);
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].kind, ContributorKind::WhileLoop);
        assert_eq!(cs[0].increment, 1);
    }

    #[test]
    fn logical_operator_same_chain_cognitive() {
        // a && b && c → 1 LogicalOperator contributor (same-operator chain)
        let cs = contributors_for("logical_same_chain_fn", ComplexityMetric::Cognitive);
        let logical: Vec<_> = cs
            .iter()
            .filter(|c| c.kind == ContributorKind::LogicalOperator)
            .collect();
        assert_eq!(
            logical.len(),
            1,
            "Same && chain should produce 1 contributor"
        );
        assert_eq!(logical[0].increment, 1);
    }

    #[test]
    fn logical_operator_switch_cognitive() {
        // a && b || c → 2 LogicalOperator contributors (operator switch)
        let cs = contributors_for("logical_op_switch_fn", ComplexityMetric::Cognitive);
        let logical: Vec<_> = cs
            .iter()
            .filter(|c| c.kind == ContributorKind::LogicalOperator)
            .collect();
        assert_eq!(
            logical.len(),
            2,
            "Operator switch should produce 2 contributors"
        );
    }

    #[test]
    fn break_contributor() {
        let fns = extract_fixture("contributors_fixture.rs", ComplexityMetric::Cognitive);
        let f = find_fn(&fns, "loop_with_break_fn");
        let breaks: Vec<_> = f
            .contributors
            .iter()
            .filter(|c| c.kind == ContributorKind::Break)
            .collect();
        assert_eq!(breaks.len(), 1);
        assert_eq!(breaks[0].increment, 1);
    }

    #[test]
    fn continue_contributor() {
        let fns = extract_fixture("contributors_fixture.rs", ComplexityMetric::Cognitive);
        let f = find_fn(&fns, "for_with_continue_fn");
        let conts: Vec<_> = f
            .contributors
            .iter()
            .filter(|c| c.kind == ContributorKind::Continue)
            .collect();
        assert_eq!(conts.len(), 1);
        assert_eq!(conts[0].increment, 1);
    }

    #[test]
    fn unsafe_block_produces_no_unsafe_contributor() {
        // ContributorKind::Unsafe is intentionally not emitted — unsafe blocks
        // are not yet counted as complexity contributors.
        let source = r#"
            fn fn_with_unsafe(ptr: *const i32) -> i32 {
                unsafe { *ptr }
            }
        "#;
        let fns = adapter()
            .extract(source, "unsafe_test.rs", ComplexityMetric::Cognitive)
            .unwrap();
        let f = fns
            .iter()
            .find(|f| f.identity.qualified_name == "fn_with_unsafe")
            .expect("fn_with_unsafe not found");
        let unsafe_contributors: Vec<_> = f
            .contributors
            .iter()
            .filter(|c| c.kind == ContributorKind::Unsafe)
            .collect();
        assert!(
            unsafe_contributors.is_empty(),
            "Unsafe blocks should not produce Unsafe contributors (not yet counted): {:?}",
            unsafe_contributors
        );
    }

    #[test]
    fn closure_no_contributor() {
        let fns = extract_fixture("contributors_fixture.rs", ComplexityMetric::Cognitive);
        let f = find_fn(&fns, "with_closure_fn");
        // with_closure_fn has no if/loop inside the closure body — no contributors expected
        assert!(
            f.contributors.is_empty(),
            "Closure with no branches should have no contributors, got: {:?}",
            f.contributors
        );
    }

    // ── end_line + nesting_depth ───────────────────────────────────────

    #[test]
    fn nested_if_records_construct_end_line_and_nesting_depth_cognitive() {
        // contributors_fixture.rs::nested_if_fn spans lines 16..26 (inclusive).
        // Outer `if x > 0` opens at line 17, closes at line 25. Inner
        // `if y > 0` opens at line 18, closes at line 22. The walker
        // records `end_line` from the full construct span so
        // `extract_split_candidates` can ask "does this contributor cover
        // line N" when reconstructing the nesting hierarchy.
        let cs = contributors_for("nested_if_fn", ComplexityMetric::Cognitive);
        assert_eq!(cs.len(), 2, "two if-branches expected");
        let outer = &cs[0]; // sorted ascending by line
        let inner = &cs[1];

        assert_eq!(outer.nesting_depth, 0, "outer `if` is top-level");
        assert_eq!(inner.nesting_depth, 1, "inner `if` is nested under outer");
        assert!(
            outer.end_line > outer.line,
            "outer `if` is multi-line: line={} end_line={}",
            outer.line,
            outer.end_line
        );
        assert!(
            outer.end_line >= inner.end_line,
            "outer's range [{}, {}] must enclose inner's [{}, {}]",
            outer.line,
            outer.end_line,
            inner.line,
            inner.end_line,
        );
    }

    #[test]
    fn try_records_token_span_for_atomic_contributor() {
        // `?` is a single-token contributor — `end_line == line`.
        let cs = contributors_for("try_fn", ComplexityMetric::Cognitive);
        assert_eq!(cs.len(), 1);
        let try_c = &cs[0];
        assert_eq!(try_c.kind, ContributorKind::Try);
        assert_eq!(
            try_c.end_line, try_c.line,
            "atomic constructs have end_line == line"
        );
        assert_eq!(try_c.nesting_depth, 0);
    }

    #[test]
    fn for_with_continue_threads_nesting_depth_cognitive() {
        // for(nesting=0) { if(nesting=1) { continue(nesting=2); } }
        let cs = contributors_for("for_with_continue_fn", ComplexityMetric::Cognitive);
        let for_c = cs
            .iter()
            .find(|c| c.kind == ContributorKind::ForLoop)
            .expect("ForLoop contributor missing");
        let if_c = cs
            .iter()
            .find(|c| c.kind == ContributorKind::IfBranch)
            .expect("IfBranch contributor missing");
        let cont_c = cs
            .iter()
            .find(|c| c.kind == ContributorKind::Continue)
            .expect("Continue contributor missing");

        assert_eq!(for_c.nesting_depth, 0);
        assert_eq!(if_c.nesting_depth, 1);
        assert_eq!(cont_c.nesting_depth, 2);
    }

    #[test]
    fn nested_if_records_nesting_depth_cyclomatic() {
        // Cyclomatic must thread nesting_depth identically to cognitive so
        // domain helpers can run on either metric's contributor list.
        let cs = contributors_for("nested_if_fn", ComplexityMetric::Cyclomatic);
        let mut ifs: Vec<_> = cs
            .iter()
            .filter(|c| c.kind == ContributorKind::IfBranch)
            .collect();
        ifs.sort_by_key(|c| c.line);
        assert_eq!(ifs.len(), 2);
        assert_eq!(ifs[0].nesting_depth, 0);
        assert_eq!(ifs[1].nesting_depth, 1);
        assert!(
            ifs[0].end_line > ifs[0].line,
            "cyclomatic if also records full construct span"
        );
    }

    #[test]
    fn for_with_continue_records_nesting_depth_cyclomatic() {
        let cs = contributors_for("for_with_continue_fn", ComplexityMetric::Cyclomatic);
        let for_c = cs
            .iter()
            .find(|c| c.kind == ContributorKind::ForLoop)
            .unwrap();
        let if_c = cs
            .iter()
            .find(|c| c.kind == ContributorKind::IfBranch)
            .unwrap();
        let cont_c = cs
            .iter()
            .find(|c| c.kind == ContributorKind::Continue)
            .unwrap();
        assert_eq!(for_c.nesting_depth, 0);
        assert_eq!(if_c.nesting_depth, 1);
        assert_eq!(cont_c.nesting_depth, 2);
    }

    #[test]
    fn match_records_end_line_covering_arms_cognitive() {
        // contributors_fixture.rs::match_fn spans lines 28..35 — the Match
        // contributor's end_line should reach the closing `}` of the
        // match expression, not just the `match` keyword.
        let cs = contributors_for("match_fn", ComplexityMetric::Cognitive);
        let match_c = cs
            .iter()
            .find(|c| c.kind == ContributorKind::Match)
            .expect("Match contributor missing");
        assert_eq!(match_c.nesting_depth, 0);
        assert!(
            match_c.end_line > match_c.line,
            "match end_line must cover arms: line={} end_line={}",
            match_c.line,
            match_c.end_line
        );
    }

    #[test]
    fn contributors_sorted_by_line() {
        let fns = extract_fixture("contributors_fixture.rs", ComplexityMetric::Cognitive);
        let f = find_fn(&fns, "sorted_by_line_fn");
        // if(line X) then for(line Y > X) → sorted ascending
        assert!(
            f.contributors.len() >= 2,
            "sorted_by_line_fn should have at least 2 contributors"
        );
        for i in 1..f.contributors.len() {
            let prev = &f.contributors[i - 1];
            let curr = &f.contributors[i];
            assert!(
                prev.line < curr.line || (prev.line == curr.line && prev.column <= curr.column),
                "Contributors not sorted: {:?} should come before {:?}",
                prev,
                curr
            );
        }
    }
}

/// ── Contributor property tests ─────────────────────────────────────────────
#[cfg(test)]
mod contributor_proptests {
    use super::test_helpers::*;
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn contributor_increments_sum_to_complexity_minus_one(
            fixture in prop_oneof![
                Just("simple_functions.rs"),
                Just("impl_methods.rs"),
                Just("control_flow.rs"),
                Just("flat_match.rs"),
                Just("bool_operators.rs"),
                Just("contributors_fixture.rs"),
            ],
            metric in prop_oneof![
                Just(ComplexityMetric::Cognitive),
                Just(ComplexityMetric::Cyclomatic),
            ],
        ) {
            let fns = extract_fixture(fixture, metric);
            for f in &fns {
                let sum: u32 = f.contributors.iter().map(|c| c.increment).sum();
                prop_assert_eq!(
                    sum,
                    f.complexity - 1,
                    "sum({}) != complexity({}) - 1 for {} in {}/{:?}",
                    sum, f.complexity, f.identity.qualified_name, fixture, metric
                );
            }
        }

        #[test]
        fn every_contributor_has_positive_increment(
            fixture in prop_oneof![
                Just("simple_functions.rs"),
                Just("impl_methods.rs"),
                Just("control_flow.rs"),
                Just("flat_match.rs"),
                Just("bool_operators.rs"),
                Just("contributors_fixture.rs"),
            ],
            metric in prop_oneof![
                Just(ComplexityMetric::Cognitive),
                Just(ComplexityMetric::Cyclomatic),
            ],
        ) {
            let fns = extract_fixture(fixture, metric);
            for f in &fns {
                for c in &f.contributors {
                    prop_assert!(
                        c.increment >= 1,
                        "Contributor {:?} in {} has zero increment",
                        c.kind, f.identity.qualified_name
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod proptests {
    use super::test_helpers::*;
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn complexity_gte_one(
            body in prop::collection::vec(
                prop_oneof![
                    Just("let x = 1;".to_string()),
                    Just("if true { 1 } else { 2 };".to_string()),
                    Just("for i in 0..10 {}".to_string()),
                    Just("while false {}".to_string()),
                ],
                0..5
            ),
            metric in prop_oneof![
                Just(ComplexityMetric::Cognitive),
                Just(ComplexityMetric::Cyclomatic),
            ],
        ) {
            let source = format!("fn test_fn() {{ {} }}", body.join("\n"));
            let result = adapter().extract(&source, "prop_test.rs", metric);
            if let Ok(fns) = result {
                for f in &fns {
                    prop_assert!(f.complexity >= 1, "Complexity was {} for fn {}", f.complexity, f.identity.qualified_name);
                }
            }
        }

        #[test]
        fn no_panic_on_fixture_files(
            fixture in prop_oneof![
                Just("simple_functions.rs"),
                Just("impl_methods.rs"),
                Just("control_flow.rs"),
                Just("flat_match.rs"),
                Just("bool_operators.rs"),
            ],
            metric in prop_oneof![
                Just(ComplexityMetric::Cognitive),
                Just(ComplexityMetric::Cyclomatic),
            ],
        ) {
            let source = load_fixture(fixture);
            let result = adapter().extract(&source, fixture, metric);
            prop_assert!(result.is_ok(), "Failed on {fixture}: {:?}", result.err());
        }

        #[test]
        fn same_fn_count_both_metrics(
            fixture in prop_oneof![
                Just("simple_functions.rs"),
                Just("impl_methods.rs"),
                Just("control_flow.rs"),
                Just("flat_match.rs"),
                Just("bool_operators.rs"),
            ],
        ) {
            let source = load_fixture(fixture);
            let cog = adapter().extract(&source, fixture, ComplexityMetric::Cognitive).unwrap();
            let cyc = adapter().extract(&source, fixture, ComplexityMetric::Cyclomatic).unwrap();
            prop_assert_eq!(
                cog.len(), cyc.len(),
                "Metric should not change function count for {}", fixture
            );
        }

        // crap-rs#283 AC: qualified_name length monotonically reflects
        // nesting depth. Synthesize a source wrapping a single free fn
        // in `depth` levels of nested mods (`m0::m1::...::f`) and assert
        // the number of `::`-separated segments is exactly `depth + 1`
        // — strictly increasing in `depth`.
        #[test]
        fn qualified_name_segment_count_tracks_mod_depth(
            depth in 0u32..6,
            metric in prop_oneof![
                Just(ComplexityMetric::Cognitive),
                Just(ComplexityMetric::Cyclomatic),
            ],
        ) {
            // Build `mod m0 { mod m1 { ... fn f() {} ... } }`.
            let mut source = String::from("fn f() {}");
            for level in (0..depth).rev() {
                source = format!("mod m{level} {{ {source} }}");
            }

            let fns = adapter().extract(&source, "synth.rs", metric).unwrap();
            prop_assert_eq!(fns.len(), 1, "exactly one fn expected: {}", source);

            let name = &fns[0].identity.qualified_name;
            let segments = name.split("::").count();
            prop_assert_eq!(
                segments as u32,
                depth + 1,
                "fn at mod depth {} should have {} segments, got {:?}",
                depth, depth + 1, name
            );

            // The bare ident is always the final segment.
            prop_assert!(
                name.ends_with('f'),
                "qualified name {:?} must end with the fn ident", name
            );
        }
    }
}
