pub fn module_attribute(name: &str) -> String {
    format!("-module({name}).")
}

pub fn export_attribute(funs: &[(&str, usize)]) -> String {
    let exports: Vec<String> = funs.iter().map(|(name, arity)| format!("{name}/{arity}")).collect();
    format!("-export([{}]).", exports.join(", "))
}

pub fn function_def(name: &str, body: &str) -> String {
    format!("{name}() ->\n    {body}.")
}

pub fn io_format(text: &str) -> String {
    format!("io:format(\"{}~n\")", escape_erlang_string(text))
}

pub fn io_format_expr(expr: &str) -> String {
    format!("io:format(\"~p~n\", [{expr}])")
}

pub fn string_literal(s: &str) -> String {
    format!("\"{}\"", escape_erlang_string(s))
}

pub fn number_literal(value: f64) -> String {
    if value.fract() == 0.0 && value.is_finite() {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

pub fn binary_op(op: &str) -> Option<&'static str> {
    match op {
        "+" => Some("+"),
        "-" => Some("-"),
        "*" => Some("*"),
        "/" => Some("/"),
        "%" => Some("rem"),
        "===" => Some("=:="),
        "!==" => Some("=/="),
        "<" => Some("<"),
        ">" => Some(">"),
        "<=" => Some("=<"),
        ">=" => Some(">="),
        _ => None,
    }
}

pub fn binary_expression(left: &str, op: &str, right: &str) -> String {
    format!("({left} {op} {right})")
}

pub fn fun_expression(params: &[String], body: &str) -> String {
    let params_str = params.join(", ");
    format!("fun({params_str}) ->\n        {body}\n    end")
}

pub fn js_var_to_erlang(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) => format!("{}{}", c.to_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

fn escape_erlang_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
