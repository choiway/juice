use oxc_ast::ast::*;

use crate::erlang;

pub fn compile(module_name: &str, program: &Program) -> String {
    let mut body_lines: Vec<String> = Vec::new();

    for stmt in &program.body {
        if let Some(line) = compile_statement(stmt) {
            body_lines.push(line);
        }
    }

    let uses_spawn = body_lines.iter().any(|line| line.contains("erlang:spawn("));

    let body = if body_lines.is_empty() {
        "ok".to_string()
    } else {
        let joined = body_lines.join(",\n    ");
        if uses_spawn {
            format!("{joined},\n    {}", erlang::spawn_wait())
        } else {
            joined
        }
    };

    let mut output = String::new();
    output.push_str(&erlang::module_attribute(module_name));
    output.push('\n');
    output.push_str(&erlang::export_attribute(&[("main", 0)]));
    output.push('\n');
    output.push('\n');
    output.push_str(&erlang::function_def("main", &body));
    output.push('\n');
    output.push('\n');
    output.push_str(&erlang::to_string_helper());
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
        Statement::IfStatement(if_stmt) => compile_if_statement(if_stmt),
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
        Expression::TemplateLiteral(template) => compile_template_literal(template),
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

fn compile_receive_body(body: &FunctionBody) -> Option<String> {
    let lines: Vec<String> = body.statements.iter().filter_map(|s| compile_statement(s)).collect();
    if lines.is_empty() {
        Some("ok".to_string())
    } else {
        Some(lines.join(",\n                "))
    }
}

fn compile_function_body(body: &FunctionBody) -> Option<String> {
    let lines: Vec<String> = body.statements.iter().filter_map(|s| compile_statement(s)).collect();
    if lines.is_empty() {
        Some("ok".to_string())
    } else {
        Some(lines.join(",\n        "))
    }
}

fn compile_if_statement(if_stmt: &IfStatement) -> Option<String> {
    let condition = compile_expression(&if_stmt.test)?;
    let consequent = compile_block_statement(&if_stmt.consequent)?;
    let alternate = match &if_stmt.alternate {
        Some(stmt) => compile_block_statement(stmt)?,
        None => "ok".to_string(),
    };
    Some(erlang::case_expression(&condition, &consequent, &alternate))
}

fn compile_block_statement(stmt: &Statement) -> Option<String> {
    match stmt {
        Statement::BlockStatement(block) => {
            let lines: Vec<String> = block.body.iter().filter_map(|s| compile_statement(s)).collect();
            if lines.is_empty() {
                Some("ok".to_string())
            } else {
                Some(lines.join(",\n            "))
            }
        }
        Statement::IfStatement(if_stmt) => compile_if_statement(if_stmt),
        _ => compile_statement(stmt),
    }
}

fn compile_template_literal(template: &TemplateLiteral) -> Option<String> {
    if template.expressions.is_empty() {
        let text = &template.quasis[0].value.raw;
        return Some(erlang::string_literal(text));
    }

    let mut parts: Vec<String> = Vec::new();
    for (i, quasi) in template.quasis.iter().enumerate() {
        let text = &quasi.value.raw;
        if !text.is_empty() {
            parts.push(erlang::string_literal(text));
        }
        if i < template.expressions.len() {
            let expr = compile_expression(&template.expressions[i])?;
            parts.push(erlang::to_string_call(&expr));
        }
    }

    Some(format!("lists:flatten([{}])", parts.join(", ")))
}

fn is_string_concat(expr: &Expression) -> bool {
    match expr {
        Expression::StringLiteral(_) => true,
        Expression::TemplateLiteral(_) => true,
        Expression::BinaryExpression(bin) if bin.operator.as_str() == "+" => {
            is_string_concat(&bin.left) || is_string_concat(&bin.right)
        }
        _ => false,
    }
}

fn compile_string_operand(expr: &Expression) -> Option<String> {
    let compiled = compile_expression(expr)?;
    match expr {
        Expression::StringLiteral(_) => Some(compiled),
        Expression::TemplateLiteral(_) => Some(compiled),
        Expression::BinaryExpression(bin) if bin.operator.as_str() == "+" => Some(compiled),
        _ => Some(erlang::to_string_call(&compiled)),
    }
}

fn compile_binary_expression(bin: &BinaryExpression) -> Option<String> {
    if bin.operator.as_str() == "+"
        && (is_string_concat(&bin.left) || is_string_concat(&bin.right))
    {
        let left = compile_string_operand(&bin.left)?;
        let right = compile_string_operand(&bin.right)?;
        return Some(format!("({left} ++ {right})"));
    }

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
    } else if is_spawn(call) {
        compile_spawn(call)
    } else if is_receive(call) {
        compile_receive(call)
    } else if is_send(call) {
        compile_send(call)
    } else if is_self(call) {
        compile_self(call)
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

fn is_spawn(call: &CallExpression) -> bool {
    if let Expression::Identifier(ident) = &call.callee {
        return ident.name == "spawn";
    }
    false
}

fn is_self(call: &CallExpression) -> bool {
    if let Expression::Identifier(ident) = &call.callee {
        return ident.name == "self";
    }
    false
}

fn compile_self(_call: &CallExpression) -> Option<String> {
    Some(erlang::self_call())
}

fn is_send(call: &CallExpression) -> bool {
    if let Expression::Identifier(ident) = &call.callee {
        return ident.name == "send";
    }
    false
}

fn compile_send(call: &CallExpression) -> Option<String> {
    if call.arguments.len() != 2 {
        return None;
    }
    let pid_expr = compile_argument(&call.arguments[0])?;
    let msg_expr = compile_argument(&call.arguments[1])?;
    Some(erlang::send_op(&pid_expr, &msg_expr))
}

fn is_receive(call: &CallExpression) -> bool {
    if let Expression::Identifier(ident) = &call.callee {
        return ident.name == "receive";
    }
    false
}

fn compile_receive(call: &CallExpression) -> Option<String> {
    let arg = call.arguments.first()?;
    let expr = arg.as_expression()?;

    if let Expression::ArrowFunctionExpression(arrow) = expr {
        let pattern = if let Some(param) = arrow.params.items.first() {
            match &param.pattern {
                BindingPattern::BindingIdentifier(ident) => erlang::js_var_to_erlang(&ident.name),
                _ => "_Msg".to_string(),
            }
        } else {
            "_Msg".to_string()
        };

        let body = compile_receive_body(&arrow.body)?;
        Some(erlang::receive_expression(&pattern, &body))
    } else {
        None
    }
}

fn compile_spawn(call: &CallExpression) -> Option<String> {
    let arg = call.arguments.first()?;
    let expr = arg.as_expression()?;
    let compiled_fn = compile_expression(expr)?;
    Some(erlang::spawn_call(&compiled_fn))
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
        let prefix = "main() ->\n    ";
        let start = erl.find(prefix).unwrap() + prefix.len();
        // Find the first ".\n" after main body (end of function)
        let end = erl[start..].find(".\n").unwrap() + start;
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

    // --- String concatenation ---

    #[test]
    fn string_concat_literal_plus_var() {
        assert_eq!(
            main_body("const name = \"beam\"\nconsole.log(\"hello \" + name)"),
            "Name = \"beam\",\n    io:format(\"~p~n\", [(\"hello \" ++ juice_to_string(Name))])"
        );
    }

    #[test]
    fn string_concat_var_plus_literal() {
        assert_eq!(
            main_body("const name = \"hello\"\nconsole.log(name + \" world\")"),
            "Name = \"hello\",\n    io:format(\"~p~n\", [(juice_to_string(Name) ++ \" world\")])"
        );
    }

    #[test]
    fn string_concat_two_literals() {
        assert_eq!(
            main_body("console.log(\"hello \" + \"world\")"),
            "io:format(\"~p~n\", [(\"hello \" ++ \"world\")])"
        );
    }

    // --- If/else ---

    #[test]
    fn if_statement_basic() {
        assert_eq!(
            main_body("const x = 1\nif (x === 1) { console.log(x) }"),
            "X = 1,\n    case (X =:= 1) of\n        true ->\n            io:format(\"~p~n\", [X]);\n        false ->\n            ok\n    end"
        );
    }

    #[test]
    fn if_else_statement() {
        assert_eq!(
            main_body("const x = 1\nif (x === 1) { console.log(\"yes\") } else { console.log(\"no\") }"),
            "X = 1,\n    case (X =:= 1) of\n        true ->\n            io:format(\"yes~n\");\n        false ->\n            io:format(\"no~n\")\n    end"
        );
    }

    #[test]
    fn if_else_if_chain() {
        assert_eq!(
            main_body("const x = 1\nif (x === 1) { console.log(\"one\") } else if (x === 2) { console.log(\"two\") }"),
            "X = 1,\n    case (X =:= 1) of\n        true ->\n            io:format(\"one~n\");\n        false ->\n            case (X =:= 2) of\n        true ->\n            io:format(\"two~n\");\n        false ->\n            ok\n    end\n    end"
        );
    }

    #[test]
    fn if_multi_statement_body() {
        assert_eq!(
            main_body("const x = 1\nif (x > 0) { const y = x + 1\nconsole.log(y) }"),
            "X = 1,\n    case (X > 0) of\n        true ->\n            Y = (X + 1),\n            io:format(\"~p~n\", [Y]);\n        false ->\n            ok\n    end"
        );
    }

    // --- REPL: string concat and if/else ---

    #[test]
    fn repl_string_concat() {
        assert_eq!(
            repl_compile("console.log(\"hello \" + \"world\")"),
            vec!["io:format(\"~p~n\", [(\"hello \" ++ \"world\")])"]
        );
    }

    #[test]
    fn repl_if_else() {
        assert_eq!(
            repl_compile("const x = 1\nif (x === 1) { console.log(\"yes\") } else { console.log(\"no\") }"),
            vec![
                "X = 1",
                "case (X =:= 1) of\n        true ->\n            io:format(\"yes~n\");\n        false ->\n            io:format(\"no~n\")\n    end"
            ]
        );
    }

    // --- Chained string concatenation ---

    #[test]
    fn string_concat_chained() {
        assert_eq!(
            main_body("const x = \"w\"\nconsole.log(\"a\" + x + \"b\")"),
            "X = \"w\",\n    io:format(\"~p~n\", [((\"a\" ++ juice_to_string(X)) ++ \"b\")])"
        );
    }

    #[test]
    fn string_concat_phase2_pattern() {
        assert_eq!(
            main_body("const i = 1\nconst msg = \"hi\"\nconsole.log(\"process \" + i + \" got: \" + msg)"),
            "I = 1,\n    Msg = \"hi\",\n    io:format(\"~p~n\", [(((\"process \" ++ juice_to_string(I)) ++ \" got: \") ++ juice_to_string(Msg))])"
        );
    }

    // --- Template literals ---

    #[test]
    fn template_literal_no_interpolation() {
        assert_eq!(
            main_body("console.log(`hello`)"),
            "io:format(\"~p~n\", [\"hello\"])"
        );
    }

    #[test]
    fn template_literal_one_expr() {
        assert_eq!(
            main_body("const name = \"beam\"\nconsole.log(`hello ${name}`)"),
            "Name = \"beam\",\n    io:format(\"~p~n\", [lists:flatten([\"hello \", juice_to_string(Name)])])"
        );
    }

    #[test]
    fn template_literal_multiple_expr() {
        assert_eq!(
            main_body("const a = 1\nconst b = 2\nconsole.log(`${a} + ${b}`)"),
            "A = 1,\n    B = 2,\n    io:format(\"~p~n\", [lists:flatten([juice_to_string(A), \" + \", juice_to_string(B)])])"
        );
    }

    // --- Spawn ---

    #[test]
    fn spawn_basic() {
        assert_eq!(
            main_body("spawn(() => { console.log(\"hello\") })"),
            "erlang:spawn(fun() ->\n        io:format(\"hello~n\")\n    end),\n    timer:sleep(100)"
        );
    }

    #[test]
    fn spawn_assigned_to_variable() {
        assert_eq!(
            main_body("const pid = spawn(() => { console.log(\"hello\") })"),
            "Pid = erlang:spawn(fun() ->\n        io:format(\"hello~n\")\n    end),\n    timer:sleep(100)"
        );
    }

    #[test]
    fn spawn_multi_statement_body() {
        assert_eq!(
            main_body("spawn(() => { const x = 42\nconsole.log(x) })"),
            "erlang:spawn(fun() ->\n        X = 42,\n        io:format(\"~p~n\", [X])\n    end),\n    timer:sleep(100)"
        );
    }

    #[test]
    fn spawn_followed_by_other_code() {
        assert_eq!(
            main_body("const pid = spawn(() => { console.log(\"bg\") })\nconsole.log(\"main\")"),
            "Pid = erlang:spawn(fun() ->\n        io:format(\"bg~n\")\n    end),\n    io:format(\"main~n\"),\n    timer:sleep(100)"
        );
    }

    #[test]
    fn send_with_expression_message() {
        assert_eq!(
            main_body("const pid = spawn(() => { console.log(\"hi\") })\nsend(pid, 1 + 2)"),
            "Pid = erlang:spawn(fun() ->\n        io:format(\"hi~n\")\n    end),\n    Pid ! (1 + 2),\n    timer:sleep(100)"
        );
    }

    #[test]
    fn send_with_self() {
        assert_eq!(
            main_body("const pid = spawn(() => { console.log(\"hi\") })\nsend(pid, self())"),
            "Pid = erlang:spawn(fun() ->\n        io:format(\"hi~n\")\n    end),\n    Pid ! self(),\n    timer:sleep(100)"
        );
    }

    #[test]
    fn send_no_spawn_no_sleep() {
        assert_eq!(
            main_body("send(pid, \"hello\")"),
            "Pid ! \"hello\""
        );
    }

    // --- Self ---

    #[test]
    fn self_basic() {
        assert_eq!(
            main_body("console.log(self())"),
            "io:format(\"~p~n\", [self()])"
        );
    }

    // --- Receive ---

    #[test]
    fn receive_basic() {
        assert_eq!(
            main_body("spawn(() => { receive((msg) => { console.log(msg) }) })"),
            "erlang:spawn(fun() ->\n        receive\n            Msg ->\n                io:format(\"~p~n\", [Msg])\n        end\n    end),\n    timer:sleep(100)"
        );
    }

    #[test]
    fn receive_multi_statement_body() {
        assert_eq!(
            main_body("spawn(() => { receive((msg) => { const x = msg\nconsole.log(x) }) })"),
            "erlang:spawn(fun() ->\n        receive\n            Msg ->\n                X = Msg,\n                io:format(\"~p~n\", [X])\n        end\n    end),\n    timer:sleep(100)"
        );
    }

    #[test]
    fn receive_string_concat() {
        assert_eq!(
            main_body("spawn(() => { receive((msg) => { console.log(\"got: \" + msg) }) })"),
            "erlang:spawn(fun() ->\n        receive\n            Msg ->\n                io:format(\"~p~n\", [(\"got: \" ++ juice_to_string(Msg))])\n        end\n    end),\n    timer:sleep(100)"
        );
    }

    #[test]
    fn receive_standalone() {
        assert_eq!(
            main_body("receive((msg) => { console.log(msg) })"),
            "receive\n            Msg ->\n                io:format(\"~p~n\", [Msg])\n        end"
        );
    }

    #[test]
    fn receive_milestone_demo() {
        assert_eq!(
            main_body(
                "const pid = spawn(() => { receive((msg) => { console.log(\"got: \" + msg) }) })\nsend(pid, \"hello from another process!\")"
            ),
            "Pid = erlang:spawn(fun() ->\n        receive\n            Msg ->\n                io:format(\"~p~n\", [(\"got: \" ++ juice_to_string(Msg))])\n        end\n    end),\n    Pid ! \"hello from another process!\",\n    timer:sleep(100)"
        );
    }

    #[test]
    fn receive_no_param() {
        assert_eq!(
            main_body("receive(() => { console.log(\"got something\") })"),
            "receive\n            _Msg ->\n                io:format(\"got something~n\")\n        end"
        );
    }

    // --- Send ---

    #[test]
    fn send_basic() {
        assert_eq!(
            main_body("const pid = spawn(() => { console.log(\"hi\") })\nsend(pid, \"hello\")"),
            "Pid = erlang:spawn(fun() ->\n        io:format(\"hi~n\")\n    end),\n    Pid ! \"hello\",\n    timer:sleep(100)"
        );
    }

    #[test]
    fn self_assigned_to_variable() {
        assert_eq!(
            main_body("const me = self()\nconsole.log(me)"),
            "Me = self(),\n    io:format(\"~p~n\", [Me])"
        );
    }

    #[test]
    fn no_spawn_no_sleep() {
        assert_eq!(
            main_body("console.log(\"hello\")"),
            "io:format(\"hello~n\")"
        );
    }

    #[test]
    fn spawn_full_module() {
        let erl = compile_js("const pid = spawn(() => { console.log(\"hi\") })");
        assert!(erl.contains("-module(test)."));
        assert!(erl.contains("erlang:spawn("));
        assert!(erl.contains("timer:sleep(100)"));
    }

    #[test]
    fn template_literal_in_arrow() {
        assert_eq!(
            main_body("const f = (x) => `val: ${x}`"),
            "F = fun(X) ->\n        lists:flatten([\"val: \", juice_to_string(X)])\n    end"
        );
    }
}
