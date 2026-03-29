use oxc_ast::ast::*;

use crate::erlang;

pub fn compile(module_name: &str, program: &Program) -> String {
    let mut body_lines: Vec<String> = Vec::new();

    for stmt in &program.body {
        if let Some(line) = compile_statement(stmt) {
            body_lines.push(line);
        }
    }

    let body = if body_lines.is_empty() {
        "ok".to_string()
    } else {
        body_lines.join(",\n    ")
    };

    let mut output = String::new();
    output.push_str(&erlang::module_attribute(module_name));
    output.push('\n');
    output.push_str(&erlang::export_attribute(&[("main", 0)]));
    output.push('\n');
    output.push('\n');
    output.push_str(&erlang::function_def("main", &body));
    output.push('\n');

    output
}

pub fn compile_expr(expr: &Expression) -> Option<String> {
    compile_expression(expr)
}

pub fn compile_stmt(stmt: &Statement) -> Option<String> {
    compile_statement(stmt)
}

fn compile_statement(stmt: &Statement) -> Option<String> {
    match stmt {
        Statement::ExpressionStatement(expr_stmt) => compile_expression(&expr_stmt.expression),
        Statement::VariableDeclaration(decl) => compile_var_declaration(decl),
        _ => None,
    }
}

fn compile_var_declaration(decl: &VariableDeclaration) -> Option<String> {
    let mut bindings: Vec<String> = Vec::new();

    for declarator in &decl.declarations {
        if let Some(binding) = compile_declarator(declarator) {
            bindings.push(binding);
        }
    }

    if bindings.is_empty() {
        None
    } else {
        Some(bindings.join(",\n    "))
    }
}

fn compile_declarator(decl: &VariableDeclarator) -> Option<String> {
    let name = match &decl.id {
        BindingPattern::BindingIdentifier(ident) => &ident.name,
        _ => return None,
    };

    let init = decl.init.as_ref()?;
    let value = compile_expression(init)?;

    let erl_name = erlang::js_var_to_erlang(name);
    Some(format!("{erl_name} = {value}"))
}

fn compile_expression(expr: &Expression) -> Option<String> {
    match expr {
        Expression::CallExpression(call) => compile_call(call),
        Expression::StringLiteral(s) => Some(erlang::string_literal(&s.value)),
        Expression::NumericLiteral(n) => Some(erlang::number_literal(n.value)),
        Expression::Identifier(ident) => Some(erlang::js_var_to_erlang(&ident.name)),
        Expression::BinaryExpression(bin) => compile_binary_expression(bin),
        Expression::ParenthesizedExpression(paren) => compile_expression(&paren.expression),
        _ => None,
    }
}

fn compile_binary_expression(bin: &BinaryExpression) -> Option<String> {
    let left = compile_expression(&bin.left)?;
    let erl_op = erlang::binary_op(bin.operator.as_str())?;
    let right = compile_expression(&bin.right)?;
    Some(erlang::binary_expression(&left, erl_op, &right))
}

fn compile_call(call: &CallExpression) -> Option<String> {
    if is_console_log(call) {
        let arg = call.arguments.first()?;
        match arg {
            Argument::StringLiteral(s) => Some(erlang::io_format(&s.value)),
            _ => {
                let expr = compile_argument(arg)?;
                Some(erlang::io_format_expr(&expr))
            }
        }
    } else {
        None
    }
}

fn compile_argument(arg: &Argument) -> Option<String> {
    match arg {
        Argument::StringLiteral(s) => Some(erlang::string_literal(&s.value)),
        _ => {
            let expr = arg.as_expression()?;
            compile_expression(expr)
        }
    }
}

fn is_console_log(call: &CallExpression) -> bool {
    if let Expression::StaticMemberExpression(member) = &call.callee {
        if let Expression::Identifier(obj) = &member.object {
            return obj.name == "console" && member.property.name == "log";
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    fn compile_js(source: &str) -> String {
        let allocator = Allocator::default();
        let source_type = SourceType::from_path("test.ts").unwrap();
        let parsed = Parser::new(&allocator, source, source_type).parse();
        assert!(parsed.errors.is_empty(), "Parse errors: {:?}", parsed.errors);
        compile("test", &parsed.program)
    }

    fn main_body(source: &str) -> String {
        let erl = compile_js(source);
        // Extract the body between "main() ->\n    " and "."
        let start = erl.find("main() ->\n    ").unwrap() + "main() ->\n    ".len();
        let end = erl.len() - 2; // trim trailing ".\n"
        erl[start..end].to_string()
    }

    #[test]
    fn number_literal() {
        assert_eq!(main_body("console.log(42)"), "io:format(\"~p~n\", [42])");
    }

    #[test]
    fn float_literal() {
        assert_eq!(main_body("console.log(3.14)"), "io:format(\"~p~n\", [3.14])");
    }

    #[test]
    fn number_variable() {
        assert_eq!(
            main_body("const x = 10\nconsole.log(x)"),
            "X = 10,\n    io:format(\"~p~n\", [X])"
        );
    }

    #[test]
    fn binary_addition() {
        assert_eq!(
            main_body("console.log(1 + 2)"),
            "io:format(\"~p~n\", [(1 + 2)])"
        );
    }

    #[test]
    fn binary_comparison() {
        assert_eq!(
            main_body("console.log(1 === 2)"),
            "io:format(\"~p~n\", [(1 =:= 2)])"
        );
    }

    #[test]
    fn less_equal_maps_to_erlang() {
        assert_eq!(
            main_body("console.log(1 <= 2)"),
            "io:format(\"~p~n\", [(1 =< 2)])"
        );
    }

    #[test]
    fn nested_expression() {
        assert_eq!(
            main_body("console.log(1 + 2 * 3)"),
            "io:format(\"~p~n\", [(1 + (2 * 3))])"
        );
    }

    #[test]
    fn string_literal_unchanged() {
        assert_eq!(
            main_body("console.log(\"hello\")"),
            "io:format(\"hello~n\")"
        );
    }
}
