pub fn module_attribute(name: &str) -> String {
    format!("-module({name}).")
}

pub fn behaviour_attribute(name: &str) -> String {
    format!("-behaviour({name}).")
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

pub fn is_atom_string(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() || c == '_' => {
            chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        }
        _ => false,
    }
}

pub fn atom_literal(s: &str) -> String {
    s.to_string()
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

pub fn case_expression(condition: &str, true_body: &str, false_body: &str) -> String {
    format!(
        "case {condition} of\n        true ->\n            {true_body};\n        false ->\n            {false_body}\n    end"
    )
}

pub fn fun_expression(params: &[String], body: &str) -> String {
    let params_str = params.join(", ");
    format!("fun({params_str}) ->\n        {body}\n    end")
}

pub fn spawn_call(fun_expr: &str) -> String {
    format!("erlang:spawn({fun_expr})")
}

pub fn spawn_wait() -> String {
    "timer:sleep(100)".to_string()
}

pub fn self_call() -> String {
    "self()".to_string()
}

pub fn send_op(pid: &str, msg: &str) -> String {
    format!("{pid} ! {msg}")
}

pub fn tuple_literal(elements: &[String]) -> String {
    format!("{{{}}}", elements.join(", "))
}

pub fn foreach_seq(var: &str, from: &str, to: &str, body: &str) -> String {
    format!("lists:foreach(fun({var}) ->\n        {body}\n    end, lists:seq({from}, {to}))")
}

pub fn receive_expression(pattern: &str, body: &str) -> String {
    format!("receive\n            {pattern} ->\n                {body}\n        end")
}

pub fn to_string_helper() -> String {
    "juice_to_string(V) when is_integer(V) -> integer_to_list(V);\n\
     juice_to_string(V) when is_float(V) -> float_to_list(V, [{decimals, 10}, compact]);\n\
     juice_to_string(V) when is_atom(V) -> atom_to_list(V);\n\
     juice_to_string(V) when is_map(V) -> lists:flatten(io_lib:format(\"~p\", [V]));\n\
     juice_to_string(V) when is_list(V) -> V;\n\
     juice_to_string(V) -> lists:flatten(io_lib:format(\"~p\", [V]))."
        .to_string()
}

pub fn to_string_call(expr: &str) -> String {
    format!("juice_to_string({expr})")
}

pub fn js_var_to_erlang(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) => format!("{}{}", c.to_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

/// Check if a string is a valid unquoted Erlang atom.
/// Erlang rule: starts with lowercase letter, rest is alphanumeric, `_`, or `@`.
pub fn is_unquoted_atom(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {
            chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '@')
        }
        _ => false,
    }
}

/// Emit an atom suitable for use as a map key.
/// Bare if valid unquoted, single-quoted otherwise.
pub fn atom_key(name: &str) -> String {
    if is_unquoted_atom(name) {
        name.to_string()
    } else {
        format!("'{}'", name.replace('\\', "\\\\").replace('\'', "\\'"))
    }
}

pub fn map_literal(entries: &[(String, String)]) -> String {
    if entries.is_empty() {
        return "#{}".to_string();
    }
    let pairs: Vec<String> = entries
        .iter()
        .map(|(key, value)| format!("{} => {}", atom_key(key), value))
        .collect();
    format!("#{{{}}}", pairs.join(", "))
}

pub fn maps_get(key: &str, map: &str) -> String {
    format!("maps:get({}, {map})", atom_key(key))
}

pub fn init_function(body: &str) -> String {
    format!("init(_Args) ->\n    {{ok, {body}}}.")
}

pub fn handle_call_function(msg_param: &str, state_param: &str, body: &str) -> String {
    format!(
        "handle_call({msg_param}, _From, {state_param}) ->\n    \
         __Result = {body},\n    \
         case __Result of\n        \
         #{{reply := __Reply, state := __NewState}} ->\n            \
         {{reply, __Reply, __NewState}};\n        \
         _ ->\n            \
         {{reply, {{error, unhandled}}, {state_param}}}\n    \
         end."
    )
}

pub fn handle_cast_function(msg_param: &str, state_param: &str, body: &str) -> String {
    format!(
        "handle_cast({msg_param}, {state_param}) ->\n    \
         __Result = {body},\n    \
         case __Result of\n        \
         #{{state := __NewState}} ->\n            \
         {{noreply, __NewState}};\n        \
         _ ->\n            \
         {{noreply, {state_param}}}\n    \
         end."
    )
}

pub fn default_handle_cast() -> String {
    "handle_cast(_Msg, State) ->\n    {noreply, State}.".to_string()
}

pub fn default_handle_info() -> String {
    "handle_info(_Info, State) ->\n    {noreply, State}.".to_string()
}

pub fn gen_server_call(pid: &str, msg: &str) -> String {
    format!("gen_server:call({pid}, {msg})")
}

pub fn gen_server_cast(pid: &str, msg: &str) -> String {
    format!("gen_server:cast({pid}, {msg})")
}

pub fn gen_server_start_helper() -> String {
    "juice_gen_server_start(Module) ->\n    \
     {ok, Pid} = gen_server:start_link(Module, [], []),\n    \
     Pid."
    .to_string()
}

fn escape_erlang_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
