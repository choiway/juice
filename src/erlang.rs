pub fn module_attribute(name: &str) -> String {
    format!("-module({name}).")
}

pub fn behaviour_attribute(name: &str) -> String {
    format!("-behaviour({name}).")
}

pub fn export_attribute(funs: &[(&str, usize)]) -> String {
    let exports: Vec<String> = funs
        .iter()
        .map(|(name, arity)| format!("{name}/{arity}"))
        .collect();
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
    let inline_body = body.replace('\n', " ");
    format!("fun({params_str}) -> {inline_body} end")
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

pub fn gen_server_start_link_helper() -> String {
    "juice_gen_server_start_link(Module) ->\n    \
     gen_server:start_link(Module, [], [])."
        .to_string()
}

pub fn erlang_error(reason: &str) -> String {
    format!("erlang:error({{error, <<\"{reason}\">>}})")
}

pub fn erlang_error_expr(compiled_expr: &str) -> String {
    format!("erlang:error({{error, {compiled_expr}}})")
}

pub fn supervisor_module() -> String {
    "-module(juice_supervisor).\n\
     -behaviour(supervisor).\n\
     -export([start_link/2, init/1, find_child/2]).\n\
     \n\
     start_link(SupFlags, ChildSpecs) ->\n    \
         supervisor:start_link(?MODULE, {SupFlags, ChildSpecs}).\n\
     \n\
     init({SupFlags, ChildSpecs}) ->\n    \
         {ok, {SupFlags, ChildSpecs}}.\n\
     \n\
     find_child(SupPid, Id) ->\n    \
         find_child(SupPid, Id, 10).\n\
     \n\
     find_child(_SupPid, _Id, 0) -> undefined;\n\
     find_child(SupPid, Id, Retries) ->\n    \
         Children = supervisor:which_children(SupPid),\n    \
         case lists:keyfind(Id, 1, Children) of\n        \
             {Id, Pid, _, _} when is_pid(Pid) -> Pid;\n        \
             _ ->\n            \
                 timer:sleep(10),\n            \
                 find_child(SupPid, Id, Retries - 1)\n    \
         end.\n"
        .to_string()
}

pub fn gen_server_start_named_helper() -> String {
    "juice_gen_server_start_named(Module, Name) ->\n    \
     {ok, Pid} = gen_server:start_link({local, Name}, Module, [], []),\n    \
     Pid."
        .to_string()
}

pub fn gen_server_start_link_named_helper() -> String {
    "juice_gen_server_start_link_named(Module, Name) ->\n    \
     gen_server:start_link({local, Name}, Module, [], [])."
        .to_string()
}

pub fn shell_module() -> String {
    "-module(juice_shell).\n\
     -export([start/1, remote_eval/1]).\n\
     \n\
     start([UserModule]) ->\n    \
         process_flag(trap_exit, true),\n    \
         UserModule:main(),\n    \
         register(juice_shell, self()),\n    \
         Shell = self(),\n    \
         spawn_link(fun() -> stdin_reader(Shell) end),\n    \
         loop(erl_eval:new_bindings()).\n\
     \n\
     stdin_reader(Shell) ->\n    \
         case io:get_line(\"\") of\n        \
             eof -> Shell ! {stdin_eof};\n        \
             {error, _} -> Shell ! {stdin_eof};\n        \
             Line ->\n            \
                 Trimmed = string:strip(string:strip(Line, right, $\\n), right, $\\r),\n            \
                 Shell ! {stdin_line, Trimmed},\n            \
                 stdin_reader(Shell)\n    \
         end.\n\
     \n\
     loop(Bindings) ->\n    \
         receive\n        \
             {stdin_eof} ->\n            \
                 ok;\n        \
             {stdin_line, []} ->\n            \
                 io:format(\"\\0JUICE_RESULT\\0ok\\0JUICE_END\\0~n\"),\n            \
                 loop(Bindings);\n        \
             {stdin_line, Expr} ->\n            \
                 case catch eval(Expr, Bindings) of\n                \
                     {ok, Value, NewBindings} ->\n                    \
                         Fmt = lists:flatten(io_lib:format(\"~p\", [Value])),\n                    \
                         io:format(\"\\0JUICE_RESULT\\0~s\\0JUICE_END\\0~n\", [Fmt]),\n                    \
                         loop(NewBindings);\n                \
                     {error, Reason} ->\n                    \
                         Err = lists:flatten(io_lib:format(\"~p\", [Reason])),\n                    \
                         io:format(\"\\0JUICE_ERROR\\0~s\\0JUICE_END\\0~n\", [Err]),\n                    \
                         loop(Bindings);\n                \
                     Other ->\n                    \
                         Err = lists:flatten(io_lib:format(\"~p\", [Other])),\n                    \
                         io:format(\"\\0JUICE_ERROR\\0~s\\0JUICE_END\\0~n\", [Err]),\n                    \
                         loop(Bindings)\n            \
                 end;\n        \
             {eval, ExprStr, From} ->\n            \
                 case catch eval(ExprStr, Bindings) of\n                \
                     {ok, Value, NewBindings} ->\n                    \
                         From ! {eval_result, {ok, Value}},\n                    \
                         loop(NewBindings);\n                \
                     {error, Reason} ->\n                    \
                         From ! {eval_result, {error, Reason}},\n                    \
                         loop(Bindings);\n                \
                     Other ->\n                    \
                         From ! {eval_result, {error, Other}},\n                    \
                         loop(Bindings)\n            \
                 end\n    \
         end.\n\
     \n\
     remote_eval(ExprStr) ->\n    \
         juice_shell ! {eval, ExprStr, self()},\n    \
         receive\n        \
             {eval_result, {ok, Value}} -> Value;\n        \
             {eval_result, {error, Reason}} -> error(Reason)\n    \
         after 30000 ->\n        \
             error(timeout)\n    \
         end.\n\
     \n\
     eval(ExprStr, Bindings) ->\n    \
         {ok, Tokens, _} = erl_scan:string(ExprStr ++ \".\"),\n    \
         {ok, Exprs} = erl_parse:parse_exprs(Tokens),\n    \
         {value, Value, NewBindings} = erl_eval:exprs(Exprs, Bindings),\n    \
         {ok, Value, NewBindings}.\n"
    .to_string()
}

pub fn remote_shell_module() -> String {
    "-module(juice_remote_shell).\n\
     -export([start/1]).\n\
     \n\
     start([TargetNode]) ->\n    \
         case net_adm:ping(TargetNode) of\n        \
             pong ->\n            \
                 io:format(\"\\0JUICE_RESULT\\0connected\\0JUICE_END\\0~n\"),\n            \
                 loop(TargetNode, erl_eval:new_bindings());\n        \
             pang ->\n            \
                 io:format(\"\\0JUICE_ERROR\\0cannot connect to ~s\\0JUICE_END\\0~n\", [TargetNode]),\n            \
                 halt(1)\n    \
         end.\n\
     \n\
     loop(TargetNode, Bindings) ->\n    \
         case io:get_line(\"\") of\n        \
             eof -> ok;\n        \
             {error, _} -> ok;\n        \
             Line ->\n            \
                 Trimmed = string:strip(string:strip(Line, right, $\\n), right, $\\r),\n            \
                 case Trimmed of\n                \
                     [] ->\n                    \
                         io:format(\"\\0JUICE_RESULT\\0ok\\0JUICE_END\\0~n\"),\n                    \
                         loop(TargetNode, Bindings);\n                \
                     Expr ->\n                    \
                         case catch eval_remote(TargetNode, Expr, Bindings) of\n                        \
                             {ok, Value, NewBindings} ->\n                            \
                                 Fmt = lists:flatten(io_lib:format(\"~p\", [Value])),\n                            \
                                 io:format(\"\\0JUICE_RESULT\\0~s\\0JUICE_END\\0~n\", [Fmt]),\n                            \
                                 loop(TargetNode, NewBindings);\n                        \
                             {error, Reason} ->\n                            \
                                 Err = lists:flatten(io_lib:format(\"~p\", [Reason])),\n                            \
                                 io:format(\"\\0JUICE_ERROR\\0~s\\0JUICE_END\\0~n\", [Err]),\n                            \
                                 loop(TargetNode, Bindings);\n                        \
                             Other ->\n                            \
                                 Err = lists:flatten(io_lib:format(\"~p\", [Other])),\n                            \
                                 io:format(\"\\0JUICE_ERROR\\0~s\\0JUICE_END\\0~n\", [Err]),\n                            \
                                 loop(TargetNode, Bindings)\n                    \
                         end\n                \
                 end\n        \
         end.\n\
     \n\
     eval_remote(TargetNode, ExprStr, Bindings) ->\n    \
         {ok, Tokens, _} = erl_scan:string(ExprStr ++ \".\"),\n    \
         {ok, Exprs} = erl_parse:parse_exprs(Tokens),\n    \
         case rpc:call(TargetNode, erl_eval, exprs, [Exprs, Bindings]) of\n        \
             {value, Value, NewBindings} ->\n            \
                 {ok, Value, NewBindings};\n        \
             {badrpc, Reason} ->\n            \
                 {error, Reason}\n    \
         end.\n"
    .to_string()
}

fn escape_erlang_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
