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
    format!("io:format(\"~s~n\", [{expr}])")
}

pub fn string_literal(s: &str) -> String {
    format!("\"{}\"", escape_erlang_string(s))
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
