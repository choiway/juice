use oxc_ast::ast::*;

use crate::erlang;

pub fn compile(module_name: &str, program: &Program) -> String {
    let mut body_lines: Vec<String> = Vec::new();

    for stmt in &program.body {
        if let Statement::ExpressionStatement(expr_stmt) = stmt {
            if let Some(line) = compile_expression(&expr_stmt.expression) {
                body_lines.push(line);
            }
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

fn compile_expression(expr: &Expression) -> Option<String> {
    match expr {
        Expression::CallExpression(call) => compile_call(call),
        _ => None,
    }
}

fn compile_call(call: &CallExpression) -> Option<String> {
    if is_console_log(call) {
        let arg = call.arguments.first()?;
        match arg {
            Argument::StringLiteral(s) => Some(erlang::io_format(&s.value)),
            _ => None,
        }
    } else {
        None
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
