use oxc_ast::{
    AstKind,
    ast::{
        Argument, AssignmentTarget, BindingIdentifier, CallExpression, Expression,
        FormalParameters, FunctionBody, IdentifierReference, MethodDefinitionKind, PropertyKind,
        Statement,
    },
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;

use crate::{AstNode, ast_util::iter_outer_expressions, context::LintContext, rule::Rule};

fn no_useless_forwarding_function_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Avoid a function that only forwards its parameters.")
        .with_help("Consider using the target function directly or adding behavior to the wrapper.")
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct NoUselessForwardingFunction;

// See <https://github.com/oxc-project/oxc/issues/6050> for documentation details.
declare_oxc_lint!(
    /// ### What it does
    ///
    /// Reports synchronous, ordinary functions whose only returned expression calls
    /// a directly named function with every simple parameter passed unchanged and in
    /// the same order. Rest parameters forwarded with spread syntax are also checked.
    ///
    /// ### Why is this bad?
    ///
    /// A forwarding function adds indirection and duplicates the target function's
    /// signature without adding behavior. The duplicate signature can drift when
    /// the target changes, and the wrapper makes the direct dependency harder to see.
    ///
    /// A wrapper can still be intentional when it provides a stable API name, limits
    /// extra arguments, normalizes omitted arguments, observes a reassigned target,
    /// or changes `this` behavior. This rule identifies syntactic forwarding; it
    /// cannot prove that two function objects are behaviorally interchangeable.
    /// It does not offer an automatic fix because replacing a wrapper can therefore
    /// change JavaScript behavior.
    ///
    /// This rule generalizes the idea behind TSLint's
    /// [`no-unnecessary-callback-wrapper`](https://palantir.github.io/tslint/rules/no-unnecessary-callback-wrapper/)
    /// and Clippy's
    /// [`redundant_closure`](https://rust-lang.github.io/rust-clippy/master/#redundant_closure)
    /// beyond callback positions.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```js
    /// const wrapper = (a, b) => callee(a, b);
    ///
    /// function forward(...args) {
    ///     return target(...args);
    /// }
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```js
    /// const direct = callee;
    ///
    /// const wrapper = (a, b) => {
    ///     logCall(a, b);
    ///     return callee(a, b);
    /// };
    /// ```
    NoUselessForwardingFunction,
    oxc,
    nursery,
    none,
    version = "next",
    short_description = "Disallows functions that only forward their parameters to another function.",
);

impl Rule for NoUselessForwardingFunction {
    fn run<'a>(&self, node: &AstNode<'a>, ctx: &LintContext<'a>) {
        let (params, call, function_id) =
            match node.kind() {
                AstKind::ArrowFunctionExpression(arrow) => {
                    if arrow.r#async {
                        return;
                    }

                    let Some(call) = arrow.get_expression().and_then(as_plain_call).or_else(|| {
                        if arrow.expression { None } else { returned_call(&arrow.body) }
                    }) else {
                        return;
                    };

                    (&arrow.params, call, None)
                }
                AstKind::Function(function) => {
                    if function.r#async || function.generator || function.this_param.is_some() {
                        return;
                    }

                    let Some(body) = &function.body else {
                        return;
                    };
                    let Some(call) = returned_call(body) else {
                        return;
                    };

                    (&function.params, call, function.id.as_ref())
                }
                _ => return,
            };

        let Some(callee) = direct_callee(call, ctx) else {
            return;
        };
        if !is_ordinary_function(node, ctx)
            || !forwards_all_parameters(params, call, ctx)
            || is_direct_recursive_call(callee, node, function_id, ctx)
        {
            return;
        }

        ctx.diagnostic(no_useless_forwarding_function_diagnostic(call.span));
    }
}

fn returned_call<'a>(body: &'a FunctionBody<'a>) -> Option<&'a CallExpression<'a>> {
    if !body.directives.is_empty() {
        return None;
    }

    let mut statements = body
        .statements
        .iter()
        .filter(|statement| !matches!(statement, Statement::EmptyStatement(_)));
    let Statement::ReturnStatement(return_statement) = statements.next()? else {
        return None;
    };
    if statements.next().is_some() {
        return None;
    }
    return_statement.argument.as_ref().and_then(as_plain_call)
}

fn as_plain_call<'a>(expression: &'a Expression<'a>) -> Option<&'a CallExpression<'a>> {
    let Expression::CallExpression(call) = expression.get_inner_expression() else {
        return None;
    };
    (!call.optional && !call.callee.is_super()).then_some(call)
}

fn direct_callee<'a>(
    call: &'a CallExpression<'a>,
    ctx: &LintContext<'a>,
) -> Option<&'a IdentifierReference<'a>> {
    let callee = call.callee.get_inner_expression().get_identifier_reference()?;
    if callee.name == "eval" && ctx.is_reference_to_global_variable(callee) {
        return None;
    }
    Some(callee)
}

fn is_ordinary_function(node: &AstNode<'_>, ctx: &LintContext<'_>) -> bool {
    let Some(parent) = iter_outer_expressions(ctx.nodes(), node.id()).next() else {
        return true;
    };
    match parent {
        AstKind::MethodDefinition(method) => {
            method.kind == MethodDefinitionKind::Method && method.decorators.is_empty()
        }
        AstKind::ObjectProperty(property) => property.kind == PropertyKind::Init,
        AstKind::PropertyDefinition(property) => property.decorators.is_empty(),
        AstKind::AccessorProperty(property) => property.decorators.is_empty(),
        _ => true,
    }
}

fn forwards_all_parameters(
    params: &FormalParameters<'_>,
    call: &CallExpression<'_>,
    ctx: &LintContext<'_>,
) -> bool {
    let expected_argument_count = params.items.len() + usize::from(params.rest.is_some());
    if call.arguments.len() != expected_argument_count {
        return false;
    }

    for (param, argument) in params.items.iter().zip(&call.arguments) {
        if param.initializer.is_some() || !param.decorators.is_empty() {
            return false;
        }

        let Some(binding) = param.pattern.get_binding_identifier() else {
            return false;
        };
        let Some(argument_identifier) = argument
            .as_expression()
            .map(Expression::get_inner_expression)
            .and_then(Expression::get_identifier_reference)
        else {
            return false;
        };

        if !is_only_reference_to_binding(binding, argument_identifier, ctx) {
            return false;
        }
    }

    let Some(rest) = &params.rest else {
        return true;
    };
    if !rest.decorators.is_empty() {
        return false;
    }
    let Some(binding) = rest.rest.argument.get_binding_identifier() else {
        return false;
    };
    let Some(Argument::SpreadElement(spread)) = call.arguments.last() else {
        return false;
    };
    let Some(argument_identifier) =
        spread.argument.get_inner_expression().get_identifier_reference()
    else {
        return false;
    };

    is_only_reference_to_binding(binding, argument_identifier, ctx)
}

fn is_only_reference_to_binding(
    binding: &BindingIdentifier<'_>,
    reference: &oxc_ast::ast::IdentifierReference<'_>,
    ctx: &LintContext<'_>,
) -> bool {
    ctx.scoping().get_reference(reference.reference_id()).symbol_id() == Some(binding.symbol_id())
        && ctx.symbol_references(binding.symbol_id()).nth(1).is_none()
}

fn is_direct_recursive_call(
    callee: &IdentifierReference<'_>,
    node: &AstNode<'_>,
    function_id: Option<&BindingIdentifier<'_>>,
    ctx: &LintContext<'_>,
) -> bool {
    let callee_symbol_id = ctx.scoping().get_reference(callee.reference_id()).symbol_id();
    if function_id.is_some_and(|id| callee_symbol_id == Some(id.symbol_id())) {
        return true;
    }

    let Some(callee_symbol_id) = callee_symbol_id else {
        return false;
    };
    let declaration_id = ctx.scoping().symbol_declaration(callee_symbol_id);
    for ancestor in ctx.nodes().ancestors(node.id()) {
        if ancestor.id() == declaration_id {
            return true;
        }

        if let AstKind::AssignmentExpression(assignment) = ancestor.kind()
            && let AssignmentTarget::AssignmentTargetIdentifier(id) = &assignment.left
            && ctx.scoping().get_reference(id.reference_id()).symbol_id() == Some(callee_symbol_id)
        {
            return true;
        }

        if matches!(ancestor.kind(), AstKind::ArrowFunctionExpression(_) | AstKind::Function(_)) {
            break;
        }
    }

    false
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        "const wrapper = callee;",
        "const wrapper = (a, b) => callee(b, a);",
        "const wrapper = (a, b) => callee(a);",
        "const wrapper = (a, b) => callee(a, b, 1);",
        "const wrapper = (a = 1) => callee(a);",
        "const wrapper = ({ a }) => callee(a);",
        "const wrapper = (a) => { log(a); return callee(a); };",
        "const wrapper = (a) => { callee(a); };",
        "const wrapper = async (a) => callee(a);",
        "async function wrapper(a) { return callee(a); }",
        "function* wrapper(a) { return callee(a); }",
        "function wrapper(a) { return wrapper(a); }",
        "const wrapper = (a) => wrapper(a);",
        "let wrapper; wrapper = (a) => wrapper(a);",
        "const wrapper = memo((a) => wrapper(a));",
        "let assigned; assigned = memo((a) => assigned(a));",
        "const wrapper = function inner(a) { return inner(a); };",
        "const wrapper = (fn, a) => fn(fn, a);",
        "const wrapper = (a) => callee?.(a);",
        "const wrapper = (a) => object.callee(a);",
        "const wrapper = (a) => object[method](a);",
        "function wrapper(a, ...args) { return object.callee(a, ...args); }",
        "const wrapper = (a) => getCallee()(a);",
        "const wrapper = (a) => (condition ? first : second)(a);",
        "const wrapper = (a) => (0, callee)(a);",
        "const wrapper = (a) => this.callee(a);",
        "function wrapper(a) { return this.callee(a); }",
        "function wrapper(source) { return eval(source); }",
        "const wrapper = (source) => (eval)(source);",
        "function wrapper(a) { 'use strict'; return callee(a); }",
        "class Derived extends Base { constructor(...args) { return super(...args); } }",
        "class Example { constructor(a) { return callee(a); } }",
        "class Example { get wrapper() { return callee(); } }",
        "class Example { set wrapper(a) { return callee(a); } }",
        "class Derived extends Base { wrapper(a) { return super.callee(a); } }",
        "const object = { get wrapper() { return callee(); } };",
        "const object = { set wrapper(a) { return callee(a); } };",
    ];

    let fail = vec![
        "const wrapper = (a, b) => callee(a, b);",
        "const wrapper = (a, b) => (callee(a, b));",
        "const wrapper = (a, b) => { return callee(a, b); };",
        "function wrapper(a, b) { return callee(a, b); }",
        "const wrapper = function (a, b) { return callee(a, b); };",
        "const object = { wrapper(a, b) { return callee(a, b); } };",
        "class Example { wrapper(a, b) { return callee(a, b); } }",
        "const wrapper = () => callee();",
        "const wrapper = (...args) => callee(...args);",
        "function wrapper(a, ...args) { ; return callee(a, ...args); ; }",
        "const eval = callee; const wrapper = (a) => eval(a);",
        "const outer = () => { const wrapper = (a) => outer(a); return wrapper; };",
    ];

    Tester::new(NoUselessForwardingFunction::NAME, NoUselessForwardingFunction::PLUGIN, pass, fail)
        .test_and_snapshot();
}

#[test]
fn test_typescript() {
    use crate::tester::Tester;

    let pass = vec![
        "function wrapper(this: Context, a: number) { return callee(a); }",
        "const wrapper = (a: number = 1) => callee(a);",
        "class Example { @decorator wrapper(a: number) { return callee(a); } }",
        "class Example { @decorator wrapper = (a: number) => callee(a); }",
        "class Example { @decorator wrapper = ((a: number) => callee(a)) as Fn; }",
        "class Example { @decorator accessor wrapper = ((a: number) => callee(a)) as Fn; }",
        "class Example { wrapper(@decorator a: number) { return callee(a); } }",
        "const wrapper = ((a: number) => wrapper(a)) as Fn;",
        "let wrapper: Fn; wrapper = ((a: number) => wrapper(a)) satisfies Fn;",
    ];
    let fail = vec![
        "const wrapper = (a: number, b?: string) => callee(a, b);",
        "function wrapper<T>(a: T): T { return callee(a); }",
        "const wrapper = ((a: number) => callee!(a)) satisfies Fn;",
    ];

    Tester::new(NoUselessForwardingFunction::NAME, NoUselessForwardingFunction::PLUGIN, pass, fail)
        .change_rule_path_extension("ts")
        .with_snapshot_suffix("typescript")
        .test_and_snapshot();
}
