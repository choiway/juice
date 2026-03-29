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

fn escape_erlang_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
