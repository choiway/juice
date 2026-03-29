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

pub fn compile_stmt_repl(stmt: &Statement) -> Option<String> {
    match stmt {
        Statement::ExpressionStatement(expr_stmt) => {
            if let Expression::CallExpression(call) = &expr_stmt.expression {
                if is_console_log(call) {
                    return compile_expression(&expr_stmt.expression);
                }
            }
            let compiled = compile_expression(&expr_stmt.expression)?;
            Some(erlang::io_format_expr(&compiled))
        }
        _ => compile_statement(stmt),
    }
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
        Expression::ArrowFunctionExpression(arrow) => compile_arrow_function(arrow),
        Expression::ParenthesizedExpression(paren) => compile_expression(&paren.expression),
        _ => None,
    }
}

fn compile_arrow_function(arrow: &ArrowFunctionExpression) -> Option<String> {
    let params: Vec<String> = arrow.params.items.iter().filter_map(|param| {
        match &param.pattern {
            BindingPattern::BindingIdentifier(ident) => Some(erlang::js_var_to_erlang(&ident.name)),
            _ => None,
        }
    }).collect();

    let body = compile_function_body(&arrow.body)?;
    Some(erlang::fun_expression(&params, &body))
}

fn compile_function_body(body: &FunctionBody) -> Option<String> {
    let lines: Vec<String> = body.statements.iter().filter_map(|s| compile_statement(s)).collect();
    if lines.is_empty() {
        Some("ok".to_string())
    } else {
        Some(lines.join(",\n        "))
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
    } else if let Expression::Identifier(ident) = &call.callee {
        let func_name = erlang::js_var_to_erlang(&ident.name);
        let args: Vec<String> = call.arguments.iter().filter_map(|a| compile_argument(a)).collect();
        Some(format!("{}({})", func_name, args.join(", ")))
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

    fn repl_compile(source: &str) -> Vec<String> {
        let allocator = Allocator::default();
        let source_type = SourceType::from_path("repl.ts").unwrap();
        let parsed = Parser::new(&allocator, source, source_type).parse();
        assert!(parsed.errors.is_empty(), "Parse errors: {:?}", parsed.errors);
        parsed
            .program
            .body
            .iter()
            .filter_map(|stmt| compile_stmt_repl(stmt))
            .collect()
    }

    #[test]
    fn repl_bare_expression_prints() {
        assert_eq!(repl_compile("1 + 1"), vec!["io:format(\"~p~n\", [(1 + 1)])"]);
    }

    #[test]
    fn repl_bare_identifier_prints() {
        assert_eq!(repl_compile("const x = 10\nx"), vec!["X = 10", "io:format(\"~p~n\", [X])"]);
    }

    #[test]
    fn repl_console_log_not_double_wrapped() {
        assert_eq!(repl_compile("console.log(42)"), vec!["io:format(\"~p~n\", [42])"]);
    }

    #[test]
    fn repl_console_log_string_not_double_wrapped() {
        assert_eq!(repl_compile("console.log(\"hi\")"), vec!["io:format(\"hi~n\")"]);
    }

    #[test]
    fn repl_var_declaration_no_wrapping() {
        assert_eq!(repl_compile("const x = 10"), vec!["X = 10"]);
    }

    #[test]
    fn arrow_function_no_params() {
        assert_eq!(
            main_body("const f = () => { console.log(42) }"),
            "F = fun() ->\n        io:format(\"~p~n\", [42])\n    end"
        );
    }

    #[test]
    fn arrow_function_one_param() {
        assert_eq!(
            main_body("const greet = (name) => { console.log(name) }"),
            "Greet = fun(Name) ->\n        io:format(\"~p~n\", [Name])\n    end"
        );
    }

    #[test]
    fn arrow_function_multiple_params() {
        assert_eq!(
            main_body("const add = (a, b) => { console.log(a + b) }"),
            "Add = fun(A, B) ->\n        io:format(\"~p~n\", [(A + B)])\n    end"
        );
    }

    #[test]
    fn arrow_function_expression_body() {
        assert_eq!(
            main_body("const inc = (x) => x + 1"),
            "Inc = fun(X) ->\n        (X + 1)\n    end"
        );
    }

    #[test]
    fn arrow_function_multi_statement_body() {
        assert_eq!(
            main_body("const f = () => { const x = 1\nconsole.log(x) }"),
            "F = fun() ->\n        X = 1,\n        io:format(\"~p~n\", [X])\n    end"
        );
    }

    #[test]
    fn arrow_function_empty_body() {
        assert_eq!(
            main_body("const f = () => {}"),
            "F = fun() ->\n        ok\n    end"
        );
    }

    #[test]
    fn repl_arrow_function_var() {
        assert_eq!(
            repl_compile("const greet = (name) => { console.log(name) }"),
            vec!["Greet = fun(Name) ->\n        io:format(\"~p~n\", [Name])\n    end"]
        );
    }

    #[test]
    fn function_call_no_args() {
        assert_eq!(
            main_body("const f = () => { console.log(42) }\nf()"),
            "F = fun() ->\n        io:format(\"~p~n\", [42])\n    end,\n    F()"
        );
    }

    #[test]
    fn function_call_one_arg() {
        assert_eq!(
            main_body("const greet = (name) => { console.log(name) }\ngreet(\"hello\")"),
            "Greet = fun(Name) ->\n        io:format(\"~p~n\", [Name])\n    end,\n    Greet(\"hello\")"
        );
    }

    #[test]
    fn function_call_multiple_args() {
        assert_eq!(
            main_body("const add = (a, b) => { console.log(a + b) }\nadd(1, 2)"),
            "Add = fun(A, B) ->\n        io:format(\"~p~n\", [(A + B)])\n    end,\n    Add(1, 2)"
        );
    }

    #[test]
    fn function_call_with_expression_arg() {
        assert_eq!(
            main_body("const f = (x) => { console.log(x) }\nf(1 + 2)"),
            "F = fun(X) ->\n        io:format(\"~p~n\", [X])\n    end,\n    F((1 + 2))"
        );
    }

    #[test]
    fn function_call_with_variable_arg() {
        assert_eq!(
            main_body("const x = 10\nconst f = (n) => { console.log(n) }\nf(x)"),
            "X = 10,\n    F = fun(N) ->\n        io:format(\"~p~n\", [N])\n    end,\n    F(X)"
        );
    }

    #[test]
    fn console_log_nested_function_call() {
        assert_eq!(
            main_body("const inc = (x) => x + 1\nconsole.log(inc(5))"),
            "Inc = fun(X) ->\n        (X + 1)\n    end,\n    io:format(\"~p~n\", [Inc(5)])"
        );
    }

    #[test]
    fn repl_function_call_auto_prints() {
        assert_eq!(
            repl_compile("const inc = (x) => x + 1\ninc(5)"),
            vec![
                "Inc = fun(X) ->\n        (X + 1)\n    end",
                "io:format(\"~p~n\", [Inc(5)])"
            ]
        );
    }
}
