use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_parser::Parser;
use oxc_span::SourceType;

use crate::erlang;

struct GenServerDef<'a> {
    init_body: &'a FunctionBody<'a>,
    handle_call: Option<(&'a FormalParameters<'a>, &'a FunctionBody<'a>)>,
    handle_cast: Option<(&'a FormalParameters<'a>, &'a FunctionBody<'a>)>,
}

pub struct CompileResult {
    pub source: String,
    pub needs_supervisor: bool,
}

pub fn compile(module_name: &str, program: &Program) -> CompileResult {
    // Pass 1: detect GenServer definitions and supervisor usage
    let mut genserver: Option<GenServerDef> = None;
    let mut genserver_stmt_index: Option<usize> = None;
    let mut needs_supervisor = false;

    for (i, stmt) in program.body.iter().enumerate() {
        if let Statement::VariableDeclaration(decl) = stmt {
            if genserver.is_none() {
                if let Some(gs) = detect_genserver(decl) {
                    genserver = Some(gs);
                    genserver_stmt_index = Some(i);
                }
            }
            // Check if initializer contains Supervisor.start
            if detect_supervisor_usage(stmt) {
                needs_supervisor = true;
            }
        } else if detect_supervisor_usage(stmt) {
            needs_supervisor = true;
        }
    }

    // Pass 2: compile remaining statements into main/0
    let mut body_lines: Vec<String> = Vec::new();

    for (i, stmt) in program.body.iter().enumerate() {
        if Some(i) == genserver_stmt_index {
            continue;
        }
        if let Some(line) = compile_statement_in_main(stmt, needs_supervisor) {
            body_lines.push(line);
        }
    }

    let uses_spawn = body_lines.iter().any(|line| line.contains("erlang:spawn("));
    let uses_named_genserver = body_lines
        .iter()
        .any(|line| line.contains("start_named(") || line.contains("start_link_named"));

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

    if genserver.is_some() {
        output.push_str(&erlang::behaviour_attribute("gen_server"));
        output.push('\n');
        let mut exports: Vec<(&str, usize)> = vec![
            ("main", 0),
            ("init", 1),
            ("handle_call", 3),
            ("handle_cast", 2),
            ("handle_info", 2),
        ];
        if needs_supervisor {
            exports.push(("juice_gen_server_start_link", 1));
        }
        if uses_named_genserver {
            exports.push(("juice_gen_server_start_link_named", 2));
        }
        output.push_str(&erlang::export_attribute(&exports));
    } else {
        output.push_str(&erlang::export_attribute(&[("main", 0)]));
    }
    output.push('\n');
    output.push('\n');

    if let Some(ref gs) = genserver {
        output.push_str(&compile_init_callback(gs));
        output.push('\n');
        output.push('\n');
        output.push_str(&compile_handle_call_callback(gs));
        output.push('\n');
        output.push('\n');
        output.push_str(&compile_handle_cast_callback(gs));
        output.push('\n');
        output.push('\n');
        output.push_str(&erlang::default_handle_info());
        output.push('\n');
        output.push('\n');
    }

    output.push_str(&erlang::function_def("main", &body));
    output.push('\n');

    if body.contains("juice_to_string(") {
        output.push('\n');
        output.push_str(&erlang::to_string_helper());
        output.push('\n');
    }

    if genserver.is_some() {
        if body.contains("juice_gen_server_start(") {
            output.push('\n');
            output.push_str(&erlang::gen_server_start_helper());
            output.push('\n');
        }
        if needs_supervisor {
            output.push('\n');
            output.push_str(&erlang::gen_server_start_link_helper());
            output.push('\n');
        }
        if body.contains("juice_gen_server_start_named(") {
            output.push('\n');
            output.push_str(&erlang::gen_server_start_named_helper());
            output.push('\n');
        }
        if uses_named_genserver && needs_supervisor {
            output.push('\n');
            output.push_str(&erlang::gen_server_start_link_named_helper());
            output.push('\n');
        }
    }

    CompileResult {
        source: output,
        needs_supervisor,
    }
}

pub fn compile_stmt(stmt: &Statement) -> Option<String> {
    compile_statement(stmt)
}

/// Pre-scan: does this statement contain a Supervisor.start call?
fn detect_supervisor_usage(stmt: &Statement) -> bool {
    match stmt {
        Statement::VariableDeclaration(decl) => decl.declarations.iter().any(|d| {
            d.init.as_ref().is_some_and(|expr| {
                if let Expression::CallExpression(call) = expr {
                    is_supervisor_start(call)
                } else {
                    false
                }
            })
        }),
        Statement::ExpressionStatement(expr_stmt) => {
            if let Expression::CallExpression(call) = &expr_stmt.expression {
                is_supervisor_start(call)
            } else {
                false
            }
        }
        _ => false,
    }
}

fn detect_genserver<'a>(decl: &'a VariableDeclaration<'a>) -> Option<GenServerDef<'a>> {
    let declarator = decl.declarations.first()?;
    let init_expr = declarator.init.as_ref()?;
    let obj = match init_expr {
        Expression::ObjectExpression(obj) => obj,
        _ => return None,
    };

    let mut init_body: Option<&'a FunctionBody<'a>> = None;
    let mut handle_call: Option<(&'a FormalParameters<'a>, &'a FunctionBody<'a>)> = None;
    let mut handle_cast: Option<(&'a FormalParameters<'a>, &'a FunctionBody<'a>)> = None;

    for prop in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(p) = prop {
            let key = match &p.key {
                PropertyKey::StaticIdentifier(ident) => ident.name.as_str(),
                _ => continue,
            };
            if let Expression::ArrowFunctionExpression(arrow) = &p.value {
                match key {
                    "init" => init_body = Some(&arrow.body),
                    "handleCall" => handle_call = Some((&arrow.params, &arrow.body)),
                    "handleCast" => handle_cast = Some((&arrow.params, &arrow.body)),
                    _ => {}
                }
            }
        }
    }

    let init_body = init_body?;
    Some(GenServerDef {
        init_body,
        handle_call,
        handle_cast,
    })
}

fn compile_init_callback(gs: &GenServerDef) -> String {
    let body = compile_function_body(gs.init_body).unwrap_or_else(|| "ok".to_string());
    erlang::init_function(&body)
}

fn compile_handle_call_callback(gs: &GenServerDef) -> String {
    match gs.handle_call {
        Some((params, body)) => {
            let msg_param = extract_param(params, 0, "Msg");
            let state_param = extract_param(params, 1, "State");
            let body_str = compile_function_body(body).unwrap_or_else(|| "ok".to_string());
            erlang::handle_call_function(&msg_param, &state_param, &body_str)
        }
        None => "handle_call(_Msg, _From, State) ->\n    {reply, ok, State}.".to_string(),
    }
}

fn compile_handle_cast_callback(gs: &GenServerDef) -> String {
    match gs.handle_cast {
        Some((params, body)) => {
            let msg_param = extract_param(params, 0, "Msg");
            let state_param = extract_param(params, 1, "State");
            let body_str = compile_function_body(body).unwrap_or_else(|| "ok".to_string());
            erlang::handle_cast_function(&msg_param, &state_param, &body_str)
        }
        None => erlang::default_handle_cast(),
    }
}

fn extract_param(params: &FormalParameters, index: usize, default: &str) -> String {
    params
        .items
        .get(index)
        .and_then(|param| match &param.pattern {
            BindingPattern::BindingIdentifier(ident) => Some(erlang::js_var_to_erlang(&ident.name)),
            _ => None,
        })
        .unwrap_or_else(|| format!("_{default}"))
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

pub fn compile_stmt_persistent_repl(stmt: &Statement) -> Option<String> {
    // Like compile_stmt_repl but bare expressions are NOT wrapped in io:format —
    // the eval server handles display via the protocol.
    compile_statement(stmt)
}

/// Compile a statement in main/0 context, with optional catch wrapping for
/// bare GenServer.call when supervision is active.
fn compile_statement_in_main(stmt: &Statement, uses_supervisor: bool) -> Option<String> {
    if uses_supervisor {
        if let Statement::ExpressionStatement(expr_stmt) = stmt {
            if let Expression::CallExpression(call) = &expr_stmt.expression {
                if is_genserver_call(call) {
                    let compiled = compile_genserver_call(call)?;
                    return Some(format!("(catch {compiled})"));
                }
            }
        }
    }
    compile_statement(stmt)
}

fn compile_statement(stmt: &Statement) -> Option<String> {
    match stmt {
        Statement::ExpressionStatement(expr_stmt) => compile_expression(&expr_stmt.expression),
        Statement::VariableDeclaration(decl) => compile_var_declaration(decl),
        Statement::IfStatement(if_stmt) => compile_if_statement(if_stmt),
        Statement::ForStatement(for_stmt) => compile_for_statement(for_stmt),
        Statement::ReturnStatement(ret) => compile_return_statement(ret),
        Statement::ThrowStatement(throw_stmt) => compile_throw_statement(throw_stmt),
        Statement::FunctionDeclaration(func) => compile_function_declaration(func),
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

fn compile_return_statement(ret: &ReturnStatement) -> Option<String> {
    match &ret.argument {
        Some(expr) => compile_expression(expr),
        None => Some("ok".to_string()),
    }
}

fn compile_throw_statement(throw_stmt: &ThrowStatement) -> Option<String> {
    // Match: throw new Error("message")
    if let Expression::NewExpression(new_expr) = &throw_stmt.argument {
        if let Expression::Identifier(ident) = &new_expr.callee {
            if ident.name == "Error" {
                if let Some(arg) = new_expr.arguments.first() {
                    if let Argument::StringLiteral(s) = arg {
                        return Some(erlang::erlang_error(&s.value));
                    }
                    // Dynamic expression: throw new Error(expr)
                    let compiled = compile_argument(arg)?;
                    return Some(erlang::erlang_error_expr(&compiled));
                }
                // throw new Error() with no message
                return Some(erlang::erlang_error("error"));
            }
        }
    }
    // Fallback: throw <expr>
    let compiled = compile_expression(&throw_stmt.argument)?;
    Some(erlang::erlang_error_expr(&compiled))
}

fn compile_declarator(decl: &VariableDeclarator) -> Option<String> {
    let name = match &decl.id {
        BindingPattern::BindingIdentifier(ident) => &ident.name,
        _ => return None,
    };

    let init = decl.init.as_ref()?;

    // Supervisor.start() result is typically unused — prefix with _ to suppress Erlang warning
    if let Expression::CallExpression(call) = init {
        if is_supervisor_start(call) {
            let value = compile_supervisor_start(call)?;
            let erl_name = erlang::js_var_to_erlang(name);
            return Some(format!("_{erl_name} = {value}"));
        }
    }

    let value = compile_expression(init)?;

    let erl_name = erlang::js_var_to_erlang(name);
    Some(format!("{erl_name} = {value}"))
}

fn compile_expression(expr: &Expression) -> Option<String> {
    match expr {
        Expression::CallExpression(call) => compile_call(call),
        Expression::StringLiteral(s) => {
            if erlang::is_atom_string(&s.value) {
                Some(erlang::atom_literal(&s.value))
            } else {
                Some(erlang::string_literal(&s.value))
            }
        }
        Expression::NumericLiteral(n) => Some(erlang::number_literal(n.value)),
        Expression::Identifier(ident) => Some(erlang::js_var_to_erlang(&ident.name)),
        Expression::BinaryExpression(bin) => compile_binary_expression(bin),
        Expression::ArrowFunctionExpression(arrow) => compile_arrow_function(arrow),
        Expression::TemplateLiteral(template) => compile_template_literal(template),
        Expression::ArrayExpression(array) => compile_array(array),
        Expression::ObjectExpression(obj) => compile_object(obj),
        Expression::StaticMemberExpression(member) => compile_member_access(member),
        Expression::ParenthesizedExpression(paren) => compile_expression(&paren.expression),
        _ => None,
    }
}

fn compile_array(array: &ArrayExpression) -> Option<String> {
    let elements: Vec<String> = array
        .elements
        .iter()
        .filter_map(|elem| {
            let expr = elem.as_expression()?;
            compile_expression(expr)
        })
        .collect();
    Some(erlang::tuple_literal(&elements))
}

fn compile_object(obj: &ObjectExpression) -> Option<String> {
    let mut entries: Vec<(String, String)> = Vec::new();

    for prop in &obj.properties {
        match prop {
            ObjectPropertyKind::ObjectProperty(p) => {
                let key = match &p.key {
                    PropertyKey::StaticIdentifier(ident) => ident.name.to_string(),
                    _ => return None,
                };
                let value = compile_expression(&p.value)?;
                entries.push((key, value));
            }
            _ => return None,
        }
    }

    Some(erlang::map_literal(&entries))
}

fn compile_member_access(member: &StaticMemberExpression) -> Option<String> {
    let object = compile_expression(&member.object)?;
    let property = member.property.name.to_string();
    Some(erlang::maps_get(&property, &object))
}

fn compile_function_declaration(func: &Function) -> Option<String> {
    let name = erlang::js_var_to_erlang(&func.id.as_ref()?.name);
    let params: Vec<String> = func
        .params
        .items
        .iter()
        .filter_map(|param| match &param.pattern {
            BindingPattern::BindingIdentifier(ident) => Some(erlang::js_var_to_erlang(&ident.name)),
            _ => None,
        })
        .collect();
    let body = compile_function_body(func.body.as_ref()?)?;
    Some(format!("{name} = {}", erlang::fun_expression(&params, &body)))
}

fn compile_arrow_function(arrow: &ArrowFunctionExpression) -> Option<String> {
    let params: Vec<String> = arrow
        .params
        .items
        .iter()
        .filter_map(|param| match &param.pattern {
            BindingPattern::BindingIdentifier(ident) => Some(erlang::js_var_to_erlang(&ident.name)),
            _ => None,
        })
        .collect();

    let body = compile_function_body(&arrow.body)?;
    Some(erlang::fun_expression(&params, &body))
}

fn compile_receive_body(body: &FunctionBody) -> Option<String> {
    let lines: Vec<String> = body
        .statements
        .iter()
        .filter_map(|s| compile_statement(s))
        .collect();
    if lines.is_empty() {
        Some("ok".to_string())
    } else {
        Some(lines.join(",\n                "))
    }
}

fn compile_function_body(body: &FunctionBody) -> Option<String> {
    let lines: Vec<String> = body
        .statements
        .iter()
        .filter_map(|s| compile_statement(s))
        .collect();
    if lines.is_empty() {
        Some("ok".to_string())
    } else {
        Some(lines.join(",\n        "))
    }
}

fn compile_for_statement(for_stmt: &ForStatement) -> Option<String> {
    // 1. Extract init variable name and start value
    let (var_name, init_value) = match &for_stmt.init {
        Some(ForStatementInit::VariableDeclaration(decl)) => {
            let declarator = decl.declarations.first()?;
            let name = match &declarator.id {
                BindingPattern::BindingIdentifier(ident) => &ident.name,
                _ => return None,
            };
            let value = compile_expression(declarator.init.as_ref()?)?;
            (erlang::js_var_to_erlang(name), value)
        }
        _ => return None,
    };

    // 2. Extract limit from test expression (i < N or i <= N)
    let test = for_stmt.test.as_ref()?;
    let upper_bound = match test {
        Expression::BinaryExpression(bin) => {
            let op = bin.operator.as_str();
            if op != "<" && op != "<=" {
                return None;
            }
            let limit = compile_expression(&bin.right)?;
            if op == "<" {
                if let Ok(n) = limit.parse::<i64>() {
                    format!("{}", n - 1)
                } else {
                    format!("({limit} - 1)")
                }
            } else {
                limit
            }
        }
        _ => return None,
    };

    // 3. Verify update is i++
    match &for_stmt.update {
        Some(Expression::UpdateExpression(update)) => {
            if update.operator != UpdateOperator::Increment {
                return None;
            }
        }
        _ => return None,
    };

    // 4. Compile body
    let body = compile_for_body(&for_stmt.body)?;

    Some(erlang::foreach_seq(
        &var_name,
        &init_value,
        &upper_bound,
        &body,
    ))
}

fn compile_for_body(stmt: &Statement) -> Option<String> {
    match stmt {
        Statement::BlockStatement(block) => {
            let lines: Vec<String> = block
                .body
                .iter()
                .filter_map(|s| compile_statement(s))
                .collect();
            if lines.is_empty() {
                Some("ok".to_string())
            } else {
                Some(lines.join(",\n        "))
            }
        }
        _ => compile_statement(stmt),
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
            let lines: Vec<String> = block
                .body
                .iter()
                .filter_map(|s| compile_statement(s))
                .collect();
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
    match expr {
        Expression::StringLiteral(s) => Some(erlang::string_literal(&s.value)),
        _ => {
            let compiled = compile_expression(expr)?;
            match expr {
                Expression::TemplateLiteral(_) => Some(compiled),
                Expression::BinaryExpression(bin) if bin.operator.as_str() == "+" => Some(compiled),
                _ => Some(erlang::to_string_call(&compiled)),
            }
        }
    }
}

fn compile_binary_expression(bin: &BinaryExpression) -> Option<String> {
    if bin.operator.as_str() == "+" && (is_string_concat(&bin.left) || is_string_concat(&bin.right))
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
    } else if is_genserver_start(call) {
        compile_genserver_start(call)
    } else if is_genserver_call(call) {
        compile_genserver_call(call)
    } else if is_genserver_cast(call) {
        compile_genserver_cast(call)
    } else if is_supervisor_start(call) {
        compile_supervisor_start(call)
    } else if is_supervisor_find_child(call) {
        compile_supervisor_find_child(call)
    } else if is_supervisor_which_children(call) {
        compile_supervisor_which_children(call)
    } else if is_process_exit(call) {
        compile_process_exit(call)
    } else if is_process_register(call) {
        compile_process_register(call)
    } else if is_process_whereis(call) {
        compile_process_whereis(call)
    } else if is_node_connect(call) {
        compile_node_connect(call)
    } else if is_node_self(call) {
        compile_node_self(call)
    } else if is_node_list(call) {
        compile_node_list(call)
    } else if is_node_eval(call) {
        compile_node_eval(call)
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
        let args: Vec<String> = call
            .arguments
            .iter()
            .filter_map(|a| compile_argument(a))
            .collect();
        Some(format!("{}({})", func_name, args.join(", ")))
    } else {
        None
    }
}

fn compile_argument(arg: &Argument) -> Option<String> {
    match arg {
        Argument::StringLiteral(s) => {
            if erlang::is_atom_string(&s.value) {
                Some(erlang::atom_literal(&s.value))
            } else {
                Some(erlang::string_literal(&s.value))
            }
        }
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

fn is_genserver_start(call: &CallExpression) -> bool {
    if let Expression::StaticMemberExpression(member) = &call.callee {
        if let Expression::Identifier(obj) = &member.object {
            return obj.name == "GenServer" && member.property.name == "start";
        }
    }
    false
}

fn compile_genserver_start(call: &CallExpression) -> Option<String> {
    // Check for optional second argument: { name: "counter" }
    if call.arguments.len() >= 2 {
        if let Some(name) = extract_genserver_name(&call.arguments[1]) {
            return Some(format!("juice_gen_server_start_named(?MODULE, {name})"));
        }
    }
    Some("juice_gen_server_start(?MODULE)".to_string())
}

fn extract_genserver_name(arg: &Argument) -> Option<String> {
    let expr = arg.as_expression()?;
    if let Expression::ObjectExpression(obj) = expr {
        for prop in &obj.properties {
            if let ObjectPropertyKind::ObjectProperty(p) = prop {
                if let PropertyKey::StaticIdentifier(ident) = &p.key {
                    if ident.name == "name" {
                        if let Expression::StringLiteral(s) = &p.value {
                            return Some(erlang::atom_literal(&s.value));
                        }
                    }
                }
            }
        }
    }
    None
}

fn is_genserver_call(call: &CallExpression) -> bool {
    if let Expression::StaticMemberExpression(member) = &call.callee {
        if let Expression::Identifier(obj) = &member.object {
            return obj.name == "GenServer" && member.property.name == "call";
        }
    }
    false
}

fn compile_genserver_call(call: &CallExpression) -> Option<String> {
    if call.arguments.len() != 2 {
        return None;
    }
    let pid = compile_argument(&call.arguments[0])?;
    let msg = compile_argument(&call.arguments[1])?;
    Some(erlang::gen_server_call(&pid, &msg))
}

fn is_genserver_cast(call: &CallExpression) -> bool {
    if let Expression::StaticMemberExpression(member) = &call.callee {
        if let Expression::Identifier(obj) = &member.object {
            return obj.name == "GenServer" && member.property.name == "cast";
        }
    }
    false
}

fn compile_genserver_cast(call: &CallExpression) -> Option<String> {
    if call.arguments.len() != 2 {
        return None;
    }
    let pid = compile_argument(&call.arguments[0])?;
    let msg = compile_argument(&call.arguments[1])?;
    Some(erlang::gen_server_cast(&pid, &msg))
}

fn is_supervisor_start(call: &CallExpression) -> bool {
    if let Expression::StaticMemberExpression(member) = &call.callee {
        if let Expression::Identifier(obj) = &member.object {
            return obj.name == "Supervisor" && member.property.name == "start";
        }
    }
    false
}

fn compile_supervisor_start(call: &CallExpression) -> Option<String> {
    let arg = call.arguments.first()?;
    let expr = arg.as_expression()?;
    let obj = match expr {
        Expression::ObjectExpression(obj) => obj,
        _ => return None,
    };

    let mut strategy = "one_for_one".to_string();
    let mut children_expr = None;

    for prop in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(p) = prop {
            let key = match &p.key {
                PropertyKey::StaticIdentifier(ident) => ident.name.as_str(),
                _ => continue,
            };
            match key {
                "strategy" => {
                    if let Expression::StringLiteral(s) = &p.value {
                        strategy = s.value.to_string();
                    }
                }
                "children" => {
                    children_expr = Some(&p.value);
                }
                _ => {}
            }
        }
    }

    let children_erl = compile_child_specs(children_expr?)?;
    let sup_flags = format!("#{{strategy => {strategy}, intensity => 3, period => 5}}");

    Some(format!(
        "element(2, juice_supervisor:start_link({sup_flags}, [{children_erl}]))"
    ))
}

fn compile_child_specs(expr: &Expression) -> Option<String> {
    let array = match expr {
        Expression::ArrayExpression(arr) => arr,
        _ => return None,
    };

    let specs: Vec<String> = array
        .elements
        .iter()
        .filter_map(|elem| {
            let expr = elem.as_expression()?;
            compile_child_spec(expr)
        })
        .collect();

    Some(specs.join(", "))
}

fn compile_child_spec(expr: &Expression) -> Option<String> {
    let obj = match expr {
        Expression::ObjectExpression(obj) => obj,
        _ => return None,
    };

    let mut id = None;
    let mut genserver_name = None;

    for prop in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(p) = prop {
            let key = match &p.key {
                PropertyKey::StaticIdentifier(ident) => ident.name.as_str(),
                _ => continue,
            };
            match key {
                "id" => {
                    if let Expression::StringLiteral(s) = &p.value {
                        id = Some(s.value.to_string());
                    }
                }
                "start" => {
                    // Extract name from: () => GenServer.start(Counter, { name: "x" })
                    genserver_name = extract_child_start_name(&p.value);
                }
                _ => {}
            }
        }
    }

    let id_str = id?;
    let id_erl = if erlang::is_atom_string(&id_str) {
        erlang::atom_literal(&id_str)
    } else {
        erlang::string_literal(&id_str)
    };

    let start_mfa = if let Some(name) = genserver_name {
        format!("{{?MODULE, juice_gen_server_start_link_named, [?MODULE, {name}]}}")
    } else {
        "{?MODULE, juice_gen_server_start_link, [?MODULE]}".to_string()
    };

    Some(format!(
        "#{{id => {id_erl}, start => {start_mfa}, restart => permanent, type => worker}}"
    ))
}

/// Extract the GenServer name from a child spec's start arrow function.
/// Matches: () => GenServer.start(Counter, { name: "x" })
fn extract_child_start_name(expr: &Expression) -> Option<String> {
    if let Expression::ArrowFunctionExpression(arrow) = expr {
        // Look at the body — it should be an expression body or single-statement body
        // containing a GenServer.start call with a name option
        for stmt in &arrow.body.statements {
            if let Statement::ExpressionStatement(expr_stmt) = stmt {
                if let Expression::CallExpression(call) = &expr_stmt.expression {
                    if is_genserver_start(call) && call.arguments.len() >= 2 {
                        return extract_genserver_name(&call.arguments[1]);
                    }
                }
            }
            if let Statement::ReturnStatement(ret) = stmt {
                if let Some(Expression::CallExpression(call)) = &ret.argument {
                    if is_genserver_start(call) && call.arguments.len() >= 2 {
                        return extract_genserver_name(&call.arguments[1]);
                    }
                }
            }
        }
    }
    None
}

fn is_supervisor_find_child(call: &CallExpression) -> bool {
    if let Expression::StaticMemberExpression(member) = &call.callee {
        if let Expression::Identifier(obj) = &member.object {
            return obj.name == "Supervisor" && member.property.name == "findChild";
        }
    }
    false
}

fn compile_supervisor_find_child(call: &CallExpression) -> Option<String> {
    if call.arguments.len() != 2 {
        return None;
    }
    let sup = compile_argument(&call.arguments[0])?;
    let id = compile_argument(&call.arguments[1])?;
    Some(format!("juice_supervisor:find_child({sup}, {id})"))
}

fn is_supervisor_which_children(call: &CallExpression) -> bool {
    if let Expression::StaticMemberExpression(member) = &call.callee {
        if let Expression::Identifier(obj) = &member.object {
            return obj.name == "Supervisor" && member.property.name == "whichChildren";
        }
    }
    false
}

fn compile_supervisor_which_children(call: &CallExpression) -> Option<String> {
    if call.arguments.len() != 1 {
        return None;
    }
    let sup = compile_argument(&call.arguments[0])?;
    Some(format!("supervisor:which_children({sup})"))
}

fn is_process_exit(call: &CallExpression) -> bool {
    if let Expression::StaticMemberExpression(member) = &call.callee {
        if let Expression::Identifier(obj) = &member.object {
            return obj.name == "Process" && member.property.name == "exit";
        }
    }
    false
}

fn compile_process_exit(call: &CallExpression) -> Option<String> {
    if call.arguments.len() != 2 {
        return None;
    }
    let pid = compile_argument(&call.arguments[0])?;
    let reason = compile_argument(&call.arguments[1])?;
    Some(format!("erlang:exit({pid}, {reason})"))
}

fn is_process_register(call: &CallExpression) -> bool {
    if let Expression::StaticMemberExpression(member) = &call.callee {
        if let Expression::Identifier(obj) = &member.object {
            return obj.name == "Process" && member.property.name == "register";
        }
    }
    false
}

fn compile_process_register(call: &CallExpression) -> Option<String> {
    if call.arguments.len() != 2 {
        return None;
    }
    let name = compile_argument(&call.arguments[0])?;
    let pid = compile_argument(&call.arguments[1])?;
    Some(format!("erlang:register({name}, {pid})"))
}

fn is_process_whereis(call: &CallExpression) -> bool {
    if let Expression::StaticMemberExpression(member) = &call.callee {
        if let Expression::Identifier(obj) = &member.object {
            return obj.name == "Process" && member.property.name == "whereis";
        }
    }
    false
}

fn compile_process_whereis(call: &CallExpression) -> Option<String> {
    if call.arguments.len() != 1 {
        return None;
    }
    let name = compile_argument(&call.arguments[0])?;
    Some(format!("erlang:whereis({name})"))
}

fn is_node_connect(call: &CallExpression) -> bool {
    if let Expression::StaticMemberExpression(member) = &call.callee {
        if let Expression::Identifier(obj) = &member.object {
            return obj.name == "Node" && member.property.name == "connect";
        }
    }
    false
}

fn compile_node_connect(call: &CallExpression) -> Option<String> {
    if call.arguments.len() != 1 {
        return None;
    }
    // Node names always need to be quoted atoms (contain @)
    if let Some(Argument::StringLiteral(s)) = call.arguments.first() {
        return Some(format!("net_adm:ping('{}')", s.value));
    }
    let node = compile_argument(&call.arguments[0])?;
    Some(format!("net_adm:ping({node})"))
}

fn is_node_self(call: &CallExpression) -> bool {
    if let Expression::StaticMemberExpression(member) = &call.callee {
        if let Expression::Identifier(obj) = &member.object {
            return obj.name == "Node" && member.property.name == "self";
        }
    }
    false
}

fn compile_node_self(_call: &CallExpression) -> Option<String> {
    Some("node()".to_string())
}

fn is_node_list(call: &CallExpression) -> bool {
    if let Expression::StaticMemberExpression(member) = &call.callee {
        if let Expression::Identifier(obj) = &member.object {
            return obj.name == "Node" && member.property.name == "list";
        }
    }
    false
}

fn compile_node_list(_call: &CallExpression) -> Option<String> {
    Some("nodes()".to_string())
}

fn is_node_eval(call: &CallExpression) -> bool {
    if let Expression::StaticMemberExpression(member) = &call.callee {
        if let Expression::Identifier(obj) = &member.object {
            return obj.name == "Node" && member.property.name == "eval";
        }
    }
    false
}

fn compile_node_eval(call: &CallExpression) -> Option<String> {
    if call.arguments.len() != 2 {
        return None;
    }
    // First arg: node name string, second arg: JS expression string
    let node = if let Some(Argument::StringLiteral(s)) = call.arguments.first() {
        format!("'{}'", s.value)
    } else {
        return None;
    };
    let js_expr = if let Some(Argument::StringLiteral(s)) = call.arguments.get(1) {
        s.value.to_string()
    } else {
        return None;
    };
    // Parse the JS expression and compile it to Erlang
    let allocator = Allocator::default();
    let source_type = SourceType::from_path("eval.ts").unwrap();
    let parsed = Parser::new(&allocator, &js_expr, source_type).parse();
    if !parsed.errors.is_empty() {
        return None;
    }
    let mut exprs: Vec<String> = Vec::new();
    for stmt in &parsed.program.body {
        if let Some(erl) = compile_stmt_persistent_repl(stmt) {
            exprs.push(erl);
        }
    }
    if exprs.is_empty() {
        return None;
    }
    let erl_expr = exprs.join(", ");
    // Escape any quotes in the compiled Erlang for embedding in a string
    let escaped = erl_expr.replace('\\', "\\\\").replace('"', "\\\"");
    Some(format!(
        "rpc:call({node}, juice_shell, remote_eval, [\"{escaped}\"])"
    ))
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
        assert!(
            parsed.errors.is_empty(),
            "Parse errors: {:?}",
            parsed.errors
        );
        compile("test", &parsed.program).source
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
        assert_eq!(
            main_body("console.log(3.14)"),
            "io:format(\"~p~n\", [3.14])"
        );
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
        assert!(
            parsed.errors.is_empty(),
            "Parse errors: {:?}",
            parsed.errors
        );
        parsed
            .program
            .body
            .iter()
            .filter_map(|stmt| compile_stmt_repl(stmt))
            .collect()
    }

    #[test]
    fn repl_bare_expression_prints() {
        assert_eq!(
            repl_compile("1 + 1"),
            vec!["io:format(\"~p~n\", [(1 + 1)])"]
        );
    }

    #[test]
    fn repl_bare_identifier_prints() {
        assert_eq!(
            repl_compile("const x = 10\nx"),
            vec!["X = 10", "io:format(\"~p~n\", [X])"]
        );
    }

    #[test]
    fn repl_console_log_not_double_wrapped() {
        assert_eq!(
            repl_compile("console.log(42)"),
            vec!["io:format(\"~p~n\", [42])"]
        );
    }

    #[test]
    fn repl_console_log_string_not_double_wrapped() {
        assert_eq!(
            repl_compile("console.log(\"hi\")"),
            vec!["io:format(\"hi~n\")"]
        );
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
            "Greet = fun(Name) ->\n        io:format(\"~p~n\", [Name])\n    end,\n    Greet(hello)"
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
            "Name = beam,\n    io:format(\"~p~n\", [(\"hello \" ++ juice_to_string(Name))])"
        );
    }

    #[test]
    fn string_concat_var_plus_literal() {
        assert_eq!(
            main_body("const name = \"hello\"\nconsole.log(name + \" world\")"),
            "Name = hello,\n    io:format(\"~p~n\", [(juice_to_string(Name) ++ \" world\")])"
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
            "X = w,\n    io:format(\"~p~n\", [((\"a\" ++ juice_to_string(X)) ++ \"b\")])"
        );
    }

    #[test]
    fn string_concat_phase2_pattern() {
        assert_eq!(
            main_body("const i = 1\nconst msg = \"hi\"\nconsole.log(\"process \" + i + \" got: \" + msg)"),
            "I = 1,\n    Msg = hi,\n    io:format(\"~p~n\", [(((\"process \" ++ juice_to_string(I)) ++ \" got: \") ++ juice_to_string(Msg))])"
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
            "Name = beam,\n    io:format(\"~p~n\", [lists:flatten([\"hello \", juice_to_string(Name)])])"
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
        assert_eq!(main_body("send(pid, \"hello\")"), "Pid ! hello");
    }

    // --- Self ---

    #[test]
    fn self_basic() {
        assert_eq!(
            main_body("console.log(self())"),
            "io:format(\"~p~n\", [self()])"
        );
    }

    // --- Tuples ---

    #[test]
    fn tuple_basic() {
        assert_eq!(main_body("const t = [1, 2, 3]"), "T = {1, 2, 3}");
    }

    #[test]
    fn tuple_mixed_types() {
        assert_eq!(
            main_body("const t = [1, \"hello\", 3]"),
            "T = {1, hello, 3}"
        );
    }

    #[test]
    fn tuple_in_send() {
        assert_eq!(
            main_body("send(pid, [self(), \"hello\"])"),
            "Pid ! {self(), hello}"
        );
    }

    #[test]
    fn tuple_with_variables() {
        assert_eq!(
            main_body("const x = 1\nconst t = [x, 2]"),
            "X = 1,\n    T = {X, 2}"
        );
    }

    #[test]
    fn tuple_nested() {
        assert_eq!(main_body("const t = [1, [2, 3]]"), "T = {1, {2, 3}}");
    }

    #[test]
    fn tuple_empty() {
        assert_eq!(main_body("const t = []"), "T = {}");
    }

    // --- Atoms ---

    #[test]
    fn atom_in_send() {
        assert_eq!(main_body("send(pid, \"hello\")"), "Pid ! hello");
    }

    #[test]
    fn non_atom_string_stays_string() {
        assert_eq!(
            main_body("send(pid, \"hello world\")"),
            "Pid ! \"hello world\""
        );
    }

    #[test]
    fn atom_in_comparison() {
        assert_eq!(
            main_body("const x = \"hello\"\nif (x === \"hello\") { console.log(\"yes\") }"),
            "X = hello,\n    case (X =:= hello) of\n        true ->\n            io:format(\"yes~n\");\n        false ->\n            ok\n    end"
        );
    }

    #[test]
    fn atom_string_concat_preserved() {
        assert_eq!(
            main_body("const name = \"beam\"\nconsole.log(\"hello \" + name)"),
            "Name = beam,\n    io:format(\"~p~n\", [(\"hello \" ++ juice_to_string(Name))])"
        );
    }

    #[test]
    fn atom_in_function_call() {
        assert_eq!(
            main_body("const greet = (name) => { console.log(name) }\ngreet(\"hello\")"),
            "Greet = fun(Name) ->\n        io:format(\"~p~n\", [Name])\n    end,\n    Greet(hello)"
        );
    }

    #[test]
    fn atom_in_variable() {
        assert_eq!(
            main_body("const x = \"hello\"\nconsole.log(x)"),
            "X = hello,\n    io:format(\"~p~n\", [X])"
        );
    }

    // --- For loop ---

    #[test]
    fn for_loop_basic() {
        assert_eq!(
            main_body("for (let i = 0; i < 3; i++) { console.log(i) }"),
            "lists:foreach(fun(I) ->\n        io:format(\"~p~n\", [I])\n    end, lists:seq(0, 2))"
        );
    }

    #[test]
    fn for_loop_multi_statement() {
        assert_eq!(
            main_body("for (let i = 0; i < 3; i++) { const x = i + 1\nconsole.log(x) }"),
            "lists:foreach(fun(I) ->\n        X = (I + 1),\n        io:format(\"~p~n\", [X])\n    end, lists:seq(0, 2))"
        );
    }

    #[test]
    fn for_loop_variable_limit() {
        assert_eq!(
            main_body("const n = 10\nfor (let j = 0; j < n; j++) { console.log(j) }"),
            "N = 10,\n    lists:foreach(fun(J) ->\n        io:format(\"~p~n\", [J])\n    end, lists:seq(0, (N - 1)))"
        );
    }

    #[test]
    fn for_loop_with_spawn() {
        assert_eq!(
            main_body("for (let i = 0; i < 3; i++) { spawn(() => { console.log(i) }) }"),
            "lists:foreach(fun(I) ->\n        erlang:spawn(fun() ->\n        io:format(\"~p~n\", [I])\n    end)\n    end, lists:seq(0, 2)),\n    timer:sleep(100)"
        );
    }

    #[test]
    fn for_loop_lte() {
        assert_eq!(
            main_body("for (let i = 0; i <= 2; i++) { console.log(i) }"),
            "lists:foreach(fun(I) ->\n        io:format(\"~p~n\", [I])\n    end, lists:seq(0, 2))"
        );
    }

    #[test]
    fn for_loop_spawn_receive() {
        assert_eq!(
            main_body("for (let i = 0; i < 3; i++) { spawn(() => { receive((msg) => { console.log(\"process \" + i + \" got: \" + msg) }) }) }"),
            "lists:foreach(fun(I) ->\n        erlang:spawn(fun() ->\n        receive\n            Msg ->\n                io:format(\"~p~n\", [(((\"process \" ++ juice_to_string(I)) ++ \" got: \") ++ juice_to_string(Msg))])\n        end\n    end)\n    end, lists:seq(0, 2)),\n    timer:sleep(100)"
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
            "Pid = erlang:spawn(fun() ->\n        io:format(\"hi~n\")\n    end),\n    Pid ! hello,\n    timer:sleep(100)"
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

    // === Object literals ===

    #[test]
    fn object_literal_basic() {
        assert_eq!(
            main_body("const state = { count: 0 }"),
            "State = #{count => 0}"
        );
    }

    #[test]
    fn object_literal_multi_property() {
        assert_eq!(
            main_body("const obj = { a: 1, b: 2 }"),
            "Obj = #{a => 1, b => 2}"
        );
    }

    #[test]
    fn object_literal_string_values() {
        assert_eq!(
            main_body("const obj = { name: \"hello\", status: \"ok\" }"),
            "Obj = #{name => hello, status => ok}"
        );
    }

    #[test]
    fn object_literal_variable_values() {
        assert_eq!(
            main_body("const x = 42\nconst obj = { count: x }"),
            "X = 42,\n    Obj = #{count => X}"
        );
    }

    #[test]
    fn object_literal_empty() {
        assert_eq!(main_body("const obj = {}"), "Obj = #{}");
    }

    #[test]
    fn object_literal_nested() {
        assert_eq!(
            main_body("const obj = { inner: { x: 1 } }"),
            "Obj = #{inner => #{x => 1}}"
        );
    }

    // === Property access ===

    #[test]
    fn property_access_basic() {
        assert_eq!(
            main_body("const state = { count: 0 }\nconsole.log(state.count)"),
            "State = #{count => 0},\n    io:format(\"~p~n\", [maps:get(count, State)])"
        );
    }

    #[test]
    fn property_access_in_expression() {
        assert_eq!(
            main_body("const state = { count: 0 }\nconsole.log(state.count + 1)"),
            "State = #{count => 0},\n    io:format(\"~p~n\", [(maps:get(count, State) + 1)])"
        );
    }

    #[test]
    fn property_access_assign() {
        assert_eq!(
            main_body("const state = { count: 0 }\nconst c = state.count"),
            "State = #{count => 0},\n    C = maps:get(count, State)"
        );
    }

    #[test]
    fn property_access_nested() {
        assert_eq!(
            main_body("const state = { inner: { x: 1 } }\nconsole.log(state.inner.x)"),
            "State = #{inner => #{x => 1}},\n    io:format(\"~p~n\", [maps:get(x, maps:get(inner, State))])"
        );
    }

    #[test]
    fn object_in_arrow_return() {
        assert_eq!(
            main_body("const init = () => ({ count: 0 })"),
            "Init = fun() ->\n        #{count => 0}\n    end"
        );
    }

    #[test]
    fn object_genserver_style_return() {
        assert_eq!(
            main_body("const state = { count: 1 }\nconst result = { reply: state.count, state: state }"),
            "State = #{count => 1},\n    Result = #{reply => maps:get(count, State), state => State}"
        );
    }

    // === Atom key quoting edge cases ===

    #[test]
    fn object_camel_case_key_bare() {
        // camelCase starts lowercase → valid bare atom
        assert_eq!(
            main_body("const m = { handleCall: 1, handleCast: 2 }"),
            "M = #{handleCall => 1, handleCast => 2}"
        );
    }

    #[test]
    fn property_access_camel_case_key() {
        assert_eq!(
            main_body("const m = { handleCall: 1 }\nconsole.log(m.handleCall)"),
            "M = #{handleCall => 1},\n    io:format(\"~p~n\", [maps:get(handleCall, M)])"
        );
    }

    #[test]
    fn object_uppercase_key_quoted() {
        // PascalCase starts uppercase → must be single-quoted to avoid variable
        assert_eq!(main_body("const m = { MyKey: 1 }"), "M = #{'MyKey' => 1}");
    }

    #[test]
    fn property_access_uppercase_key_quoted() {
        assert_eq!(
            main_body("const m = { MyKey: 1 }\nconst v = m.MyKey"),
            "M = #{'MyKey' => 1},\n    V = maps:get('MyKey', M)"
        );
    }

    #[test]
    fn object_dollar_key_quoted() {
        assert_eq!(main_body("const m = { $ref: 1 }"), "M = #{'$ref' => 1}");
    }

    #[test]
    fn object_underscore_key_quoted() {
        // _foo in Erlang is a variable, not an atom — must quote
        assert_eq!(
            main_body("const m = { _private: 1 }"),
            "M = #{'_private' => 1}"
        );
    }

    // === to_string with maps ===

    #[test]
    fn string_concat_with_object() {
        assert_eq!(
            main_body("const s = { count: 0 }\nconsole.log(\"state: \" + s)"),
            "S = #{count => 0},\n    io:format(\"~p~n\", [(\"state: \" ++ juice_to_string(S))])"
        );
    }

    #[test]
    fn to_string_helper_has_map_clause() {
        let erl = compile_js("console.log(\"hello \" + name)");
        assert!(erl.contains("is_map(V)"));
    }

    #[test]
    fn to_string_helper_omitted_when_unused() {
        let erl = compile_js("console.log(\"hi\")");
        assert!(!erl.contains("juice_to_string"));
    }

    // === ReturnStatement ===

    #[test]
    fn return_expression() {
        assert_eq!(
            main_body("const f = (x) => { return x + 1 }"),
            "F = fun(X) ->\n        (X + 1)\n    end"
        );
    }

    #[test]
    fn return_object() {
        assert_eq!(
            main_body("const f = () => { return { count: 0 } }"),
            "F = fun() ->\n        #{count => 0}\n    end"
        );
    }

    #[test]
    fn return_bare() {
        assert_eq!(
            main_body("const f = () => { return }"),
            "F = fun() ->\n        ok\n    end"
        );
    }

    // === GenServer.call / GenServer.cast ===

    #[test]
    fn genserver_call_compiles() {
        // Without supervision, bare GenServer.call is NOT caught
        assert_eq!(
            main_body("GenServer.call(pid, \"increment\")"),
            "gen_server:call(Pid, increment)"
        );
    }

    #[test]
    fn genserver_cast_compiles() {
        assert_eq!(
            main_body("GenServer.cast(pid, \"update\")"),
            "gen_server:cast(Pid, update)"
        );
    }

    #[test]
    fn genserver_call_in_console_log() {
        assert_eq!(
            main_body("console.log(GenServer.call(pid, \"get\"))"),
            "io:format(\"~p~n\", [gen_server:call(Pid, get)])"
        );
    }

    // === GenServer.start ===

    #[test]
    fn genserver_start_compiles() {
        assert_eq!(
            main_body("const pid = GenServer.start(Counter)"),
            "Pid = juice_gen_server_start(?MODULE)"
        );
    }

    // === GenServer module structure ===

    fn genserver_source() -> &'static str {
        "const Counter = {\n  init: () => ({ count: 0 }),\n  handleCall: (msg, state) => {\n    if (msg === \"increment\") {\n      const next = { count: state.count + 1 }\n      return { reply: next.count, state: next }\n    } else if (msg === \"get\") {\n      return { reply: state.count, state: state }\n    }\n  }\n}\nconst pid = GenServer.start(Counter)\nconsole.log(GenServer.call(pid, \"increment\"))"
    }

    #[test]
    fn genserver_module_has_behaviour() {
        let erl = compile_js(genserver_source());
        assert!(
            erl.contains("-behaviour(gen_server)."),
            "missing behaviour: {erl}"
        );
    }

    #[test]
    fn genserver_module_exports_callbacks() {
        let erl = compile_js(genserver_source());
        assert!(erl.contains("init/1"), "missing init export: {erl}");
        assert!(
            erl.contains("handle_call/3"),
            "missing handle_call export: {erl}"
        );
        assert!(
            erl.contains("handle_cast/2"),
            "missing handle_cast export: {erl}"
        );
        assert!(
            erl.contains("handle_info/2"),
            "missing handle_info export: {erl}"
        );
    }

    #[test]
    fn genserver_init_callback() {
        let erl = compile_js(genserver_source());
        assert!(
            erl.contains("init(_Args) ->\n    {ok, #{count => 0}}."),
            "missing init callback: {erl}"
        );
    }

    #[test]
    fn genserver_definition_skipped_in_main() {
        let erl = compile_js(genserver_source());
        // main/0 should NOT contain the Counter map definition
        let main_start = erl.find("main() ->").expect("no main");
        let main_section = &erl[main_start..];
        assert!(
            !main_section.contains("Counter ="),
            "Counter definition should be skipped in main: {main_section}"
        );
    }

    // === handle_call callback ===

    #[test]
    fn genserver_handle_call_callback() {
        let erl = compile_js(genserver_source());
        assert!(
            erl.contains("handle_call(Msg, _From, State) ->"),
            "missing handle_call signature: {erl}"
        );
        assert!(
            erl.contains("#{reply := __Reply, state := __NewState}"),
            "missing map pattern match: {erl}"
        );
        assert!(
            erl.contains("{reply, __Reply, __NewState}"),
            "missing reply tuple: {erl}"
        );
        assert!(
            erl.contains("{reply, {error, unhandled}, State}"),
            "missing fallback: {erl}"
        );
    }

    #[test]
    fn genserver_handle_call_body_compiles() {
        let erl = compile_js(genserver_source());
        // The if/else-if chain should produce nested case expressions
        assert!(
            erl.contains("Msg =:= increment"),
            "missing increment case: {erl}"
        );
        assert!(erl.contains("Msg =:= get"), "missing get case: {erl}");
    }

    // === handle_cast callback ===

    #[test]
    fn genserver_default_handle_cast() {
        // genserver_source() has no handleCast → should get default
        let erl = compile_js(genserver_source());
        assert!(
            erl.contains("handle_cast(_Msg, State) ->\n    {noreply, State}."),
            "missing default handle_cast: {erl}"
        );
    }

    #[test]
    fn genserver_custom_handle_cast() {
        let src = "const Counter = {\n  init: () => ({ count: 0 }),\n  handleCast: (msg, state) => {\n    if (msg === \"reset\") {\n      return { state: { count: 0 } }\n    }\n  }\n}\nconsole.log(\"hi\")";
        let erl = compile_js(src);
        assert!(
            erl.contains("handle_cast(Msg, State) ->"),
            "missing handle_cast signature: {erl}"
        );
        assert!(
            erl.contains("#{state := __NewState}"),
            "missing map pattern match: {erl}"
        );
        assert!(
            erl.contains("{noreply, __NewState}"),
            "missing noreply tuple: {erl}"
        );
        assert!(erl.contains("{noreply, State}"), "missing fallback: {erl}");
    }

    // === handle_info default ===

    #[test]
    fn genserver_default_handle_info() {
        let erl = compile_js(genserver_source());
        assert!(
            erl.contains("handle_info(_Info, State) ->\n    {noreply, State}."),
            "missing default handle_info: {erl}"
        );
    }

    // === GenServer start helper + integration ===

    #[test]
    fn genserver_start_helper_present() {
        let erl = compile_js(genserver_source());
        assert!(
            erl.contains("juice_gen_server_start(Module) ->"),
            "missing start helper: {erl}"
        );
        assert!(
            erl.contains("gen_server:start_link(Module, [], [])"),
            "missing start_link: {erl}"
        );
    }

    #[test]
    fn genserver_milestone_demo() {
        let src = "const Counter = {\n  init: () => ({ count: 0 }),\n  handleCall: (msg, state) => {\n    if (msg === \"increment\") {\n      const next = { count: state.count + 1 }\n      return { reply: next.count, state: next }\n    } else if (msg === \"get\") {\n      return { reply: state.count, state: state }\n    }\n  }\n}\nconst pid = GenServer.start(Counter)\nconsole.log(GenServer.call(pid, \"increment\"))\nconsole.log(GenServer.call(pid, \"increment\"))\nconsole.log(GenServer.call(pid, \"get\"))";
        let erl = compile_js(src);
        // Module structure
        assert!(erl.contains("-behaviour(gen_server)."), "missing behaviour");
        assert!(erl.contains("init/1"), "missing init export");
        assert!(erl.contains("handle_call/3"), "missing handle_call export");
        // Callbacks
        assert!(erl.contains("init(_Args) ->"), "missing init");
        assert!(
            erl.contains("handle_call(Msg, _From, State) ->"),
            "missing handle_call"
        );
        assert!(
            erl.contains("handle_cast(_Msg, State) ->"),
            "missing default handle_cast"
        );
        // main/0
        assert!(
            erl.contains("juice_gen_server_start(?MODULE)"),
            "missing start in main"
        );
        assert!(
            erl.contains("gen_server:call(Pid, increment)"),
            "missing call in main"
        );
        assert!(
            erl.contains("gen_server:call(Pid, get)"),
            "missing get call in main"
        );
        // Helper
        assert!(
            erl.contains("juice_gen_server_start(Module) ->"),
            "missing start helper"
        );
    }

    #[test]
    fn non_genserver_module_no_behaviour() {
        let erl = compile_js("console.log(\"hello\")");
        assert!(
            !erl.contains("-behaviour"),
            "non-genserver should have no behaviour"
        );
        assert!(
            !erl.contains("init/1"),
            "non-genserver should have no init export"
        );
        assert!(
            !erl.contains("juice_gen_server_start"),
            "non-genserver should have no start helper"
        );
    }

    // === Phase 4: Supervision ===

    #[test]
    fn throw_new_error() {
        assert_eq!(
            main_body("throw new Error(\"crash!\")"),
            "erlang:error({error, <<\"crash!\">>})"
        );
    }

    #[test]
    fn throw_new_error_no_message() {
        assert_eq!(
            main_body("throw new Error()"),
            "erlang:error({error, <<\"error\">>})"
        );
    }

    #[test]
    fn supervisor_start_basic() {
        let source = r#"Supervisor.start({
            strategy: "one_for_one",
            children: [
                { id: "counter", start: () => GenServer.start(Counter) }
            ]
        })"#;
        let body = main_body(source);
        assert!(
            body.contains("element(2, juice_supervisor:start_link("),
            "should unwrap {{ok, Pid}}"
        );
        assert!(
            body.contains("strategy => one_for_one"),
            "should have strategy"
        );
        assert!(body.contains("intensity => 3"), "should have intensity 3");
        assert!(body.contains("id => counter"), "should have child id");
        assert!(
            body.contains("start => {?MODULE, juice_gen_server_start_link, [?MODULE]}"),
            "should have MFA"
        );
    }

    #[test]
    fn supervisor_find_child() {
        assert_eq!(
            main_body("Supervisor.findChild(sup, \"counter\")"),
            "juice_supervisor:find_child(Sup, counter)"
        );
    }

    #[test]
    fn supervisor_which_children() {
        assert_eq!(
            main_body("Supervisor.whichChildren(sup)"),
            "supervisor:which_children(Sup)"
        );
    }

    #[test]
    fn process_exit() {
        assert_eq!(
            main_body("Process.exit(pid, \"kill\")"),
            "erlang:exit(Pid, kill)"
        );
    }

    #[test]
    fn genserver_call_bare_statement_caught() {
        // With supervision present, bare GenServer.call is caught
        let source = r#"
            const sup = Supervisor.start({
                strategy: "one_for_one",
                children: [{ id: "w", start: () => GenServer.start(W) }]
            })
            GenServer.call(pid, "boom")
        "#;
        let body = main_body(source);
        assert!(
            body.contains("(catch gen_server:call(Pid, boom))"),
            "should wrap in catch: {body}"
        );
    }

    #[test]
    fn genserver_call_in_assignment_not_caught() {
        let body = main_body("const result = GenServer.call(pid, \"get\")");
        assert_eq!(body, "Result = gen_server:call(Pid, get)");
    }

    #[test]
    fn genserver_call_in_console_log_not_caught() {
        let body = main_body("console.log(GenServer.call(pid, \"get\"))");
        assert_eq!(body, "io:format(\"~p~n\", [gen_server:call(Pid, get)])");
    }

    #[test]
    fn supervisor_module_exports_start_link() {
        let source = r#"
            const Counter = {
                init: () => ({ count: 0 }),
                handleCall: (msg, state) => {
                    return { reply: state.count, state: state }
                }
            }
            const sup = Supervisor.start({
                strategy: "one_for_one",
                children: [
                    { id: "counter", start: () => GenServer.start(Counter) }
                ]
            })
        "#;
        let erl = compile_js(source);
        assert!(
            erl.contains("juice_gen_server_start_link/1"),
            "should export start_link"
        );
        assert!(
            erl.contains("juice_gen_server_start_link(Module) ->"),
            "should have start_link helper"
        );
    }

    #[test]
    fn supervisor_strategy_one_for_all() {
        let source = r#"Supervisor.start({
            strategy: "one_for_all",
            children: [
                { id: "worker", start: () => GenServer.start(Worker) }
            ]
        })"#;
        let body = main_body(source);
        assert!(
            body.contains("strategy => one_for_all"),
            "should have one_for_all strategy"
        );
    }

    #[test]
    fn supervisor_needs_supervisor_flag() {
        let allocator = Allocator::default();
        let source_type = SourceType::from_path("test.ts").unwrap();
        let source = r#"Supervisor.start({
            strategy: "one_for_one",
            children: [{ id: "w", start: () => GenServer.start(W) }]
        })"#;
        let parsed = Parser::new(&allocator, source, source_type).parse();
        let result = compile("test", &parsed.program);
        assert!(result.needs_supervisor, "should set needs_supervisor flag");
    }

    #[test]
    fn no_supervisor_no_flag() {
        let allocator = Allocator::default();
        let source_type = SourceType::from_path("test.ts").unwrap();
        let source = "console.log(\"hello\")";
        let parsed = Parser::new(&allocator, source, source_type).parse();
        let result = compile("test", &parsed.program);
        assert!(
            !result.needs_supervisor,
            "should not set needs_supervisor flag"
        );
    }

    // === Named GenServer ===

    #[test]
    fn genserver_start_named() {
        assert_eq!(
            main_body("GenServer.start(Counter, { name: \"counter\" })"),
            "juice_gen_server_start_named(?MODULE, counter)"
        );
    }

    #[test]
    fn genserver_start_unnamed_unchanged() {
        assert_eq!(
            main_body("GenServer.start(Counter)"),
            "juice_gen_server_start(?MODULE)"
        );
    }

    // === Process builtins ===

    #[test]
    fn process_register() {
        assert_eq!(
            main_body("Process.register(\"counter\", pid)"),
            "erlang:register(counter, Pid)"
        );
    }

    #[test]
    fn process_whereis() {
        assert_eq!(
            main_body("Process.whereis(\"counter\")"),
            "erlang:whereis(counter)"
        );
    }

    // === Node builtins ===

    #[test]
    fn node_self() {
        assert_eq!(
            main_body("console.log(Node.self())"),
            "io:format(\"~p~n\", [node()])"
        );
    }

    #[test]
    fn node_connect() {
        assert_eq!(
            main_body("Node.connect(\"node1@localhost\")"),
            "net_adm:ping('node1@localhost')"
        );
    }

    #[test]
    fn node_list() {
        assert_eq!(
            main_body("console.log(Node.list())"),
            "io:format(\"~p~n\", [nodes()])"
        );
    }

    // === Named GenServer in supervision ===

    #[test]
    fn named_child_spec_uses_named_start_link() {
        let source = r#"Supervisor.start({
            strategy: "one_for_one",
            children: [
                { id: "counter", start: () => GenServer.start(Counter, { name: "counter" }) }
            ]
        })"#;
        let body = main_body(source);
        assert!(
            body.contains("start_link_named, [?MODULE, counter]"),
            "should use named start_link: {body}"
        );
    }
}
