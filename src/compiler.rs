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
        Expression::Identifier(ident) => Some(erlang::js_var_to_erlang(&ident.name)),
        _ => None,
    }
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
