//! Complexity extraction adapter using syn 2.x AST walking.
//!
//! Supports both cognitive and cyclomatic complexity metrics.
//! Walks `ItemFn` and `ImplItemFn` nodes, counting decision points.

use crate::domain::types::{
    ComplexityMetric, CrapError, FunctionComplexity, FunctionIdentity, SourceSpan,
};
use crate::ports::ComplexityPort;
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
    current_impl_type: Option<String>,
    functions: Vec<FunctionComplexity>,
}

impl<'ast> Visit<'ast> for FunctionFinder {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        let name = node.sig.ident.to_string();
        let span = span_of(node);
        let complexity = count_complexity(&node.block, self.metric);

        self.functions.push(FunctionComplexity {
            identity: FunctionIdentity {
                file_path: self.file_path.clone(),
                qualified_name: name,
                span,
            },
            complexity,
            metric: self.metric,
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
        let method_name = node.sig.ident.to_string();
        let qualified = match &self.current_impl_type {
            Some(ty) => format!("{ty}::{method_name}"),
            None => method_name,
        };
        let span = span_of(node);
        let complexity = count_complexity(&node.block, self.metric);

        self.functions.push(FunctionComplexity {
            identity: FunctionIdentity {
                file_path: self.file_path.clone(),
                qualified_name: qualified,
                span,
            },
            complexity,
            metric: self.metric,
        });

        syn::visit::visit_impl_item_fn(self, node);
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        // Only record trait methods that have a default body
        if let Some(block) = &node.default {
            let method_name = node.sig.ident.to_string();
            let qualified = match &self.current_impl_type {
                Some(ty) => format!("{ty}::{method_name}"),
                None => method_name,
            };
            let span = span_of(node);
            let complexity = count_complexity(block, self.metric);

            self.functions.push(FunctionComplexity {
                identity: FunctionIdentity {
                    file_path: self.file_path.clone(),
                    qualified_name: qualified,
                    span,
                },
                complexity,
                metric: self.metric,
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

fn span_of(item: &impl syn::spanned::Spanned) -> SourceSpan {
    let sp = item.span();
    SourceSpan {
        start_line: sp.start().line,
        end_line: sp.end().line,
    }
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

fn count_complexity(block: &syn::Block, metric: ComplexityMetric) -> u32 {
    let raw = match metric {
        ComplexityMetric::Cognitive => count_cognitive_block(block, 0),
        ComplexityMetric::Cyclomatic => count_cyclomatic_block(block),
    };
    // Base complexity is always at least 1
    raw + 1
}

// ── Cognitive complexity ───────────────────────────────────────────────

fn count_cognitive_block(block: &syn::Block, nesting: u32) -> u32 {
    block
        .stmts
        .iter()
        .map(|stmt| count_cognitive_stmt(stmt, nesting))
        .sum()
}

fn count_cognitive_stmt(stmt: &syn::Stmt, nesting: u32) -> u32 {
    match stmt {
        syn::Stmt::Expr(expr, _) => count_cognitive_expr(expr, nesting),
        syn::Stmt::Local(local) => {
            let mut total = 0;
            if let Some(init) = &local.init {
                total += count_cognitive_expr(&init.expr, nesting);
                if let Some((_, diverge)) = &init.diverge {
                    // let...else is a branching construct: +1 structural + nesting
                    total += 1 + nesting;
                    total += count_cognitive_expr(diverge, nesting + 1);
                }
            }
            total
        }
        syn::Stmt::Item(_) | syn::Stmt::Macro(_) => 0,
    }
}

fn count_cognitive_expr(expr: &Expr, nesting: u32) -> u32 {
    match expr {
        Expr::If(expr_if) => count_cognitive_if(expr_if, nesting),
        Expr::Match(expr_match) => {
            let mut total = 1 + nesting; // +1 structural, +nesting
            total += count_cognitive_expr(&expr_match.expr, nesting);
            for arm in &expr_match.arms {
                if let Some(guard) = &arm.guard {
                    total += count_cognitive_expr(&guard.1, nesting + 1);
                }
                total += count_cognitive_expr(&arm.body, nesting + 1);
            }
            total
        }
        Expr::While(expr_while) => {
            let mut total = 1 + nesting;
            total += count_cognitive_expr(&expr_while.cond, nesting);
            total += count_cognitive_block(&expr_while.body, nesting + 1);
            total
        }
        Expr::ForLoop(expr_for) => {
            let mut total = 1 + nesting;
            total += count_cognitive_expr(&expr_for.expr, nesting);
            total += count_cognitive_block(&expr_for.body, nesting + 1);
            total
        }
        Expr::Loop(expr_loop) => {
            let mut total = 1 + nesting;
            total += count_cognitive_block(&expr_loop.body, nesting + 1);
            total
        }
        Expr::Binary(bin) => count_cognitive_binary_chain(bin),
        Expr::Try(expr_try) => 1 + count_cognitive_expr(&expr_try.expr, nesting),
        Expr::Break(_) => 1,
        Expr::Continue(_) => 1,
        Expr::Block(expr_block) => count_cognitive_block(&expr_block.block, nesting),
        Expr::Return(ret) => {
            if let Some(expr) = &ret.expr {
                count_cognitive_expr(expr, nesting)
            } else {
                0
            }
        }
        Expr::Closure(closure) => {
            // No structural increment, but increases nesting depth for nested structures
            count_cognitive_expr(&closure.body, nesting + 1)
        }
        Expr::Call(call) => {
            let mut total = count_cognitive_expr(&call.func, nesting);
            for arg in &call.args {
                total += count_cognitive_expr(arg, nesting);
            }
            total
        }
        Expr::MethodCall(mc) => {
            let mut total = count_cognitive_expr(&mc.receiver, nesting);
            for arg in &mc.args {
                total += count_cognitive_expr(arg, nesting);
            }
            total
        }
        Expr::Tuple(tuple) => {
            let mut total = 0;
            for elem in &tuple.elems {
                total += count_cognitive_expr(elem, nesting);
            }
            total
        }
        Expr::Reference(r) => count_cognitive_expr(&r.expr, nesting),
        Expr::Unary(u) => count_cognitive_expr(&u.expr, nesting),
        Expr::Paren(p) => count_cognitive_expr(&p.expr, nesting),
        _ => 0,
    }
}

fn count_cognitive_if(expr_if: &syn::ExprIf, nesting: u32) -> u32 {
    // if: +1 + nesting
    let mut total = 1 + nesting;

    // Count complexity in the condition (for && / || chains)
    total += count_cognitive_expr(&expr_if.cond, nesting);

    // Then branch
    total += count_cognitive_block(&expr_if.then_branch, nesting + 1);

    // Else branch
    if let Some((_, else_branch)) = &expr_if.else_branch {
        match else_branch.as_ref() {
            Expr::If(else_if) => {
                // else if: +1 continuation (no nesting increment)
                total += 1;
                total += count_cognitive_expr(&else_if.cond, nesting);
                total += count_cognitive_block(&else_if.then_branch, nesting + 1);
                if let Some((_, inner_else)) = &else_if.else_branch {
                    total += count_cognitive_else(inner_else, nesting);
                }
            }
            Expr::Block(block) => {
                // else: +0 (NOT a structural increment per Sonar spec)
                total += count_cognitive_block(&block.block, nesting + 1);
            }
            other => {
                total += count_cognitive_expr(other, nesting + 1);
            }
        }
    }

    total
}

fn count_cognitive_else(expr: &Expr, nesting: u32) -> u32 {
    match expr {
        Expr::If(else_if) => {
            let mut total = 1; // else if: +1 continuation
            total += count_cognitive_expr(&else_if.cond, nesting);
            total += count_cognitive_block(&else_if.then_branch, nesting + 1);
            if let Some((_, inner_else)) = &else_if.else_branch {
                total += count_cognitive_else(inner_else, nesting);
            }
            total
        }
        Expr::Block(block) => {
            // else: +0
            count_cognitive_block(&block.block, nesting + 1)
        }
        other => count_cognitive_expr(other, nesting + 1),
    }
}

/// Count cognitive complexity for a chain of binary operators.
/// Same-operator sequences count as +1 total; operator switches add +1 each.
fn count_cognitive_binary_chain(bin: &ExprBinary) -> u32 {
    let ops = flatten_binary_ops(bin);
    if ops.is_empty() {
        return 0;
    }

    let mut total = 0;
    let mut last_is_logical: Option<BoolOp> = None;

    for op in &ops {
        match op {
            BoolOp::And | BoolOp::Or => {
                if last_is_logical != Some(*op) {
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

/// Flatten a binary expression tree into an in-order sequence of operators.
fn flatten_binary_ops(bin: &ExprBinary) -> Vec<BoolOp> {
    let mut ops = Vec::new();
    flatten_binary_ops_inner(&bin.left, &mut ops);
    ops.push(classify_binop(&bin.op));
    flatten_binary_ops_inner(&bin.right, &mut ops);
    ops
}

fn flatten_binary_ops_inner(expr: &Expr, ops: &mut Vec<BoolOp>) {
    if let Expr::Binary(bin) = expr {
        flatten_binary_ops_inner(&bin.left, ops);
        ops.push(classify_binop(&bin.op));
        flatten_binary_ops_inner(&bin.right, ops);
    }
}

// ── Cyclomatic complexity ──────────────────────────────────────────────

fn count_cyclomatic_block(block: &syn::Block) -> u32 {
    block.stmts.iter().map(count_cyclomatic_stmt).sum()
}

fn count_cyclomatic_stmt(stmt: &syn::Stmt) -> u32 {
    match stmt {
        syn::Stmt::Expr(expr, _) => count_cyclomatic_expr(expr),
        syn::Stmt::Local(local) => {
            let mut total = 0;
            if let Some(init) = &local.init {
                total += count_cyclomatic_expr(&init.expr);
                if let Some((_, diverge)) = &init.diverge {
                    // let...else is a decision point: +1
                    total += 1;
                    total += count_cyclomatic_expr(diverge);
                }
            }
            total
        }
        syn::Stmt::Item(_) | syn::Stmt::Macro(_) => 0,
    }
}

fn count_cyclomatic_expr(expr: &Expr) -> u32 {
    match expr {
        Expr::If(expr_if) => {
            let mut total = 1; // +1 for if
            total += count_cyclomatic_expr(&expr_if.cond);
            total += count_cyclomatic_block(&expr_if.then_branch);
            if let Some((_, else_branch)) = &expr_if.else_branch {
                total += count_cyclomatic_expr(else_branch);
            }
            total
        }
        Expr::Match(expr_match) => {
            let mut total = expr_match.arms.len().saturating_sub(1) as u32;
            total += count_cyclomatic_expr(&expr_match.expr);
            for arm in &expr_match.arms {
                if let Some(guard) = &arm.guard {
                    total += count_cyclomatic_expr(&guard.1);
                }
                total += count_cyclomatic_expr(&arm.body);
            }
            total
        }
        Expr::While(expr_while) => {
            let mut total = 1;
            total += count_cyclomatic_expr(&expr_while.cond);
            total += count_cyclomatic_block(&expr_while.body);
            total
        }
        Expr::ForLoop(expr_for) => {
            let mut total = 1;
            total += count_cyclomatic_expr(&expr_for.expr);
            total += count_cyclomatic_block(&expr_for.body);
            total
        }
        Expr::Loop(expr_loop) => 1 + count_cyclomatic_block(&expr_loop.body),
        Expr::Binary(bin) => {
            let mut total = match bin.op {
                syn::BinOp::And(_) | syn::BinOp::Or(_) => 1,
                _ => 0,
            };
            total += count_cyclomatic_expr(&bin.left);
            total += count_cyclomatic_expr(&bin.right);
            total
        }
        Expr::Try(expr_try) => 1 + count_cyclomatic_expr(&expr_try.expr),
        Expr::Block(expr_block) => count_cyclomatic_block(&expr_block.block),
        Expr::Return(ret) => {
            if let Some(e) = &ret.expr {
                count_cyclomatic_expr(e)
            } else {
                0
            }
        }
        Expr::Closure(closure) => count_cyclomatic_expr(&closure.body),
        Expr::Call(call) => {
            let mut total = count_cyclomatic_expr(&call.func);
            for arg in &call.args {
                total += count_cyclomatic_expr(arg);
            }
            total
        }
        Expr::MethodCall(mc) => {
            let mut total = count_cyclomatic_expr(&mc.receiver);
            for arg in &mc.args {
                total += count_cyclomatic_expr(arg);
            }
            total
        }
        Expr::Tuple(tuple) => {
            let mut total = 0;
            for elem in &tuple.elems {
                total += count_cyclomatic_expr(elem);
            }
            total
        }
        Expr::Break(_) => 1,
        Expr::Continue(_) => 1,
        Expr::Reference(r) => count_cyclomatic_expr(&r.expr),
        Expr::Unary(u) => count_cyclomatic_expr(&u.expr),
        Expr::Paren(p) => count_cyclomatic_expr(&p.expr),
        _ => 0,
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
    }
}
