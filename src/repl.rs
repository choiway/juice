use std::borrow::Cow;
use std::io::{self, Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use rustyline::completion::Completer;
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::highlight::CmdKind;
use rustyline::history::DefaultHistory;
use rustyline::{Editor, Helper};

use crate::compiler;

struct JsHelper;

impl Helper for JsHelper {}

impl Completer for JsHelper {
    type Candidate = String;
}

impl Hinter for JsHelper {
    type Hint = String;
}

impl Validator for JsHelper {}

impl Highlighter for JsHelper {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        Cow::Owned(highlight_js(line))
    }

    fn highlight_char(&self, _line: &str, _pos: usize, _kind: CmdKind) -> bool {
        true
    }
}

fn highlight_js(line: &str) -> String {
    const KEYWORD: &str = "\x1b[35m";
    const LITERAL: &str = "\x1b[33m";
    const STRING: &str = "\x1b[32m";
    const NUMBER: &str = "\x1b[33m";
    const COMMENT: &str = "\x1b[90m";
    const RESET: &str = "\x1b[0m";

    let mut out = String::with_capacity(line.len() * 2);
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let c = chars[i];

        // Line comment
        if c == '/' && i + 1 < len && chars[i + 1] == '/' {
            out.push_str(COMMENT);
            while i < len {
                out.push(chars[i]);
                i += 1;
            }
            out.push_str(RESET);
            continue;
        }

        // Block comment
        if c == '/' && i + 1 < len && chars[i + 1] == '*' {
            out.push_str(COMMENT);
            out.push('/');
            out.push('*');
            i += 2;
            while i < len {
                if chars[i] == '*' && i + 1 < len && chars[i + 1] == '/' {
                    out.push('*');
                    out.push('/');
                    i += 2;
                    break;
                }
                out.push(chars[i]);
                i += 1;
            }
            out.push_str(RESET);
            continue;
        }

        // String literals
        if c == '"' || c == '\'' || c == '`' {
            let quote = c;
            out.push_str(STRING);
            out.push(c);
            i += 1;
            while i < len {
                let sc = chars[i];
                if sc == '\\' && i + 1 < len {
                    out.push(sc);
                    out.push(chars[i + 1]);
                    i += 2;
                    continue;
                }
                out.push(sc);
                i += 1;
                if sc == quote {
                    break;
                }
            }
            out.push_str(RESET);
            continue;
        }

        // Numbers
        if c.is_ascii_digit() || (c == '.' && i + 1 < len && chars[i + 1].is_ascii_digit()) {
            out.push_str(NUMBER);
            if c == '0' && i + 1 < len && matches!(chars[i + 1], 'x' | 'X' | 'b' | 'B' | 'o' | 'O')
            {
                out.push(c);
                out.push(chars[i + 1]);
                i += 2;
                while i < len && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    out.push(chars[i]);
                    i += 1;
                }
            } else {
                while i < len && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == '_') {
                    out.push(chars[i]);
                    i += 1;
                }
                if i < len && (chars[i] == 'e' || chars[i] == 'E') {
                    out.push(chars[i]);
                    i += 1;
                    if i < len && (chars[i] == '+' || chars[i] == '-') {
                        out.push(chars[i]);
                        i += 1;
                    }
                    while i < len && chars[i].is_ascii_digit() {
                        out.push(chars[i]);
                        i += 1;
                    }
                }
            }
            out.push_str(RESET);
            continue;
        }

        // Identifiers and keywords
        if c.is_ascii_alphabetic() || c == '_' || c == '$' {
            let start = i;
            while i < len && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == '$')
            {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            match word.as_str() {
                "let" | "const" | "var" | "function" | "if" | "else" | "for" | "while"
                | "return" | "new" | "class" | "extends" | "import" | "export" | "async"
                | "await" | "throw" | "try" | "catch" | "finally" | "switch" | "case"
                | "break" | "continue" | "typeof" | "instanceof" | "in" | "of" | "default"
                | "from" | "yield" | "delete" | "void" | "with" | "do" | "debugger"
                | "static" | "this" | "super" => {
                    out.push_str(KEYWORD);
                    out.push_str(&word);
                    out.push_str(RESET);
                }
                "true" | "false" | "null" | "undefined" | "NaN" | "Infinity" => {
                    out.push_str(LITERAL);
                    out.push_str(&word);
                    out.push_str(RESET);
                }
                _ => out.push_str(&word),
            }
            continue;
        }

        out.push(c);
        i += 1;
    }

    out
}


fn history_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".juice_history")
}

fn friendly_erl_error(raw: &str) -> String {
    // {unbound_var, 'X'} — undefined variable
    if let Some(start) = raw.find("{unbound_var,") {
        let rest = &raw[start + "{unbound_var,".len()..];
        if let Some(end) = rest.find('}') {
            let var = rest[..end].trim().trim_matches('\'');
            return format!("undefined variable '{var}'");
        }
    }
    // {unbound, 'Foo'} — undefined function (called but never assigned)
    if let Some(start) = raw.find("{unbound,") {
        let rest = &raw[start + "{unbound,".len()..];
        if let Some(end) = rest.find('}') {
            let name = rest[..end].trim().trim_matches('\'');
            return format!("undefined function '{name}'");
        }
    }
    // {{badmatch,...}} — pattern match failure
    if raw.contains("{badmatch,") {
        return "no match for right-hand side value".to_string();
    }
    // {badarith,...} — arithmetic error
    if raw.contains("{badarith,") {
        return "arithmetic error".to_string();
    }
    // {badarg,...}
    if raw.contains("{badarg,") {
        return "bad argument".to_string();
    }
    // {function_clause,...}
    if raw.contains("{function_clause,") {
        return "no matching function clause".to_string();
    }
    // {error, <<"reason">>} — user throw/error
    if let Some(start) = raw.find("{error,<<\"") {
        let rest = &raw[start + "{error,<<\"".len()..];
        if let Some(end) = rest.find("\">>") {
            return rest[..end].to_string();
        }
    }
    // {noproc,...} — process not found (e.g. GenServer.call to dead process)
    if raw.contains("{noproc,") {
        return "process not found".to_string();
    }
    // Fallback: first non-empty line, stripped of noise
    raw.lines()
        .find(|l| !l.is_empty() && !l.contains("Crash dump") && !l.contains("Runtime terminating"))
        .unwrap_or("runtime error")
        .to_string()
}

pub fn run() {
    println!("box v0.1.0 - juice interactive shell");
    println!("Type a JS expression. Ctrl+D to exit.");
    println!();

    let mut cmd = Command::new("erl");
    cmd.arg("-noshell")
        .arg("-pa")
        .arg(".")
        .arg("-s")
        .arg("juice_shell")
        .arg("start")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());

    let mut child = cmd.spawn().unwrap_or_else(|e| {
        eprintln!("Error starting Erlang VM: {e}");
        std::process::exit(1);
    });

    let child_stdin = child.stdin.take().expect("Failed to open stdin");
    let child_stdout = child.stdout.take().expect("Failed to open stdout");

    let (result_tx, result_rx) = mpsc::channel::<ShellMessage>();
    start_reader_thread(child_stdout, result_tx);

    thread::sleep(std::time::Duration::from_millis(300));

    // Set up JuiceToString helper in the eval server's bindings
    let mut erl_stdin = child_stdin;
    {
        use io::Write as _;
        let to_string_def = "JuiceToString = fun(V) when is_integer(V) -> integer_to_list(V); (V) when is_float(V) -> float_to_list(V, [{decimals, 10}, compact]); (V) when is_atom(V) -> atom_to_list(V); (V) when is_list(V) -> V; (V) -> lists:flatten(io_lib:format(\"~p\", [V])) end";
        if writeln!(erl_stdin, "{to_string_def}").is_err() {
            eprintln!("VM process exited");
            std::process::exit(1);
        }
        let _ = erl_stdin.flush();
        let _ = result_rx.recv();
    }

    let mut rl = Editor::<JsHelper, DefaultHistory>::new().expect("Failed to initialize editor");
    rl.set_helper(Some(JsHelper));
    let hist = history_path();
    let _ = rl.load_history(&hist);

    let mut buffer = String::new();

    loop {
        let prompt = if buffer.is_empty() { "box> " } else { "...> " };
        let line: String = match rl.readline(prompt) {
            Ok(l) => l,
            Err(ReadlineError::Interrupted) => {
                if buffer.is_empty() {
                    break;
                }
                buffer.clear();
                continue;
            }
            Err(ReadlineError::Eof) => break,
            Err(e) => {
                eprintln!("Input error: {e}");
                break;
            }
        };

        let trimmed = line.trim();
        if trimmed.is_empty() && buffer.is_empty() {
            continue;
        }

        if buffer.is_empty() {
            buffer = trimmed.to_string();
        } else {
            buffer.push('\n');
            buffer.push_str(trimmed);
        }

        let allocator = Allocator::default();
        let source_type = SourceType::from_path("repl.ts").unwrap();
        let parser_return = Parser::new(&allocator, &buffer, source_type).parse();

        if !parser_return.errors.is_empty() {
            if buffer.matches('{').count() > buffer.matches('}').count()
                || buffer.matches('(').count() > buffer.matches(')').count()
            {
                continue;
            }
            eprintln!("Parse error: {}", parser_return.errors[0]);
            buffer.clear();
            continue;
        }

        let input_source = buffer.clone();

        let mut exprs: Vec<String> = Vec::new();
        for stmt in &parser_return.program.body {
            if let Some(erl) = compiler::compile_stmt_persistent_repl(stmt) {
                exprs.push(erl);
            } else {
                eprintln!("Unsupported statement");
            }
        }
        buffer.clear();

        if exprs.is_empty() {
            continue;
        }

        let eval_str = exprs.join(", ")
            .replace("juice_to_string(", "JuiceToString(")
            .replace('\n', " ");
        use io::Write as _;
        if writeln!(erl_stdin, "{eval_str}").is_err() {
            eprintln!("VM process exited");
            break;
        }
        if erl_stdin.flush().is_err() {
            eprintln!("VM process exited");
            break;
        }

        match result_rx.recv() {
            Ok(ShellMessage::Result(value)) => {
                println!("\x1b[36m{value}\x1b[0m");
            }
            Ok(ShellMessage::Error(err)) => {
                eprintln!("\x1b[31mError: {}\x1b[0m", friendly_erl_error(&err));
            }
            Err(_) => {
                eprintln!("VM process exited");
                break;
            }
        }

        let _ = rl.add_history_entry(&input_source);
    }

    let _ = rl.save_history(&hist);
    let _ = child.wait();
    println!("Goodbye!");
}

enum ShellMessage {
    Result(String),
    Error(String),
}

#[derive(Debug, PartialEq, Eq)]
enum ParsedShellChunk {
    PassThrough(String),
    Result(String),
    Error(String),
}

const RESULT_PREFIX: &str = "\0JUICE_RESULT\0";
const ERROR_PREFIX: &str = "\0JUICE_ERROR\0";
const END_MARKER: &str = "\0JUICE_END\0";

fn next_shell_frame(buffer: &str) -> Option<(usize, bool)> {
    let result_pos = buffer.find(RESULT_PREFIX);
    let error_pos = buffer.find(ERROR_PREFIX);

    match (result_pos, error_pos) {
        (Some(r), Some(e)) => Some(if r <= e { (r, true) } else { (e, false) }),
        (Some(r), None) => Some((r, true)),
        (None, Some(e)) => Some((e, false)),
        (None, None) => None,
    }
}

fn drain_shell_output(buffer: &mut String, flush_partial: bool) -> Vec<ParsedShellChunk> {
    let mut chunks = Vec::new();

    loop {
        match next_shell_frame(buffer) {
            Some((start, is_result)) => {
                if start > 0 {
                    chunks.push(ParsedShellChunk::PassThrough(buffer[..start].to_string()));
                    buffer.drain(..start);
                    continue;
                }

                let prefix = if is_result {
                    RESULT_PREFIX
                } else {
                    ERROR_PREFIX
                };
                let payload_start = prefix.len();
                let Some(end_rel) = buffer[payload_start..].find(END_MARKER) else {
                    break;
                };

                let payload_end = payload_start + end_rel;
                let payload = buffer[payload_start..payload_end].to_string();
                if is_result {
                    chunks.push(ParsedShellChunk::Result(payload));
                } else {
                    chunks.push(ParsedShellChunk::Error(payload));
                }

                let mut drain_len = payload_end + END_MARKER.len();
                if buffer[drain_len..].starts_with("\r\n") {
                    drain_len += 2;
                } else if buffer[drain_len..].starts_with('\n')
                    || buffer[drain_len..].starts_with('\r')
                {
                    drain_len += 1;
                }
                buffer.drain(..drain_len);
            }
            None => {
                if flush_partial {
                    if !buffer.is_empty() {
                        chunks.push(ParsedShellChunk::PassThrough(std::mem::take(buffer)));
                    }
                } else if let Some(last_newline) = buffer.rfind('\n') {
                    let split = last_newline + 1;
                    chunks.push(ParsedShellChunk::PassThrough(buffer[..split].to_string()));
                    buffer.drain(..split);
                    continue;
                }
                break;
            }
        }
    }

    chunks
}

fn emit_shell_chunks(chunks: Vec<ParsedShellChunk>, tx: &mpsc::Sender<ShellMessage>) {
    for chunk in chunks {
        match chunk {
            ParsedShellChunk::PassThrough(text) => {
                print!("{text}");
                let _ = io::stdout().flush();
            }
            ParsedShellChunk::Result(value) => {
                let _ = tx.send(ShellMessage::Result(value));
            }
            ParsedShellChunk::Error(value) => {
                let _ = tx.send(ShellMessage::Error(value));
            }
        }
    }
}

/// Parse stdout from the Erlang eval server, extracting delimited results.
/// Non-delimited output (io:format from user code) is printed directly.
fn start_reader_thread(mut stdout: std::process::ChildStdout, tx: mpsc::Sender<ShellMessage>) {
    thread::spawn(move || {
        let mut raw = [0_u8; 4096];
        let mut buffer = String::new();

        loop {
            match stdout.read(&mut raw) {
                Ok(0) => {
                    emit_shell_chunks(drain_shell_output(&mut buffer, true), &tx);
                    break;
                }
                Ok(n) => {
                    buffer.push_str(&String::from_utf8_lossy(&raw[..n]));
                    emit_shell_chunks(drain_shell_output(&mut buffer, false), &tx);
                }
                Err(_) => {
                    emit_shell_chunks(drain_shell_output(&mut buffer, true), &tx);
                    break;
                }
            }
        }
    });
}

/// Run a persistent REPL connected to a long-running Erlang VM.
/// The VM starts the user's supervision tree, then accepts eval requests.
pub fn run_persistent(module_name: &str, node_name: Option<&str>, beam_dir: &str) {
    let prompt = match node_name {
        Some(name) => format!("juice@{name}> "),
        None => "juice> ".to_string(),
    };

    let mut cmd = Command::new("erl");
    cmd.arg("-noshell")
        .arg("-pa")
        .arg(beam_dir)
        .arg("-kernel")
        .arg("logger")
        .arg("[{handler, default, logger_std_h, #{config => #{type => standard_error}}}]")
        .arg("-s")
        .arg("juice_shell")
        .arg("start")
        .arg(module_name)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());

    if let Some(name) = node_name {
        cmd.arg("-sname").arg(name);
        cmd.arg("-setcookie").arg("juice");
    }

    let mut child = cmd.spawn().unwrap_or_else(|e| {
        eprintln!("Error starting Erlang VM: {e}");
        std::process::exit(1);
    });

    let child_stdin = child.stdin.take().expect("Failed to open stdin");
    let child_stdout = child.stdout.take().expect("Failed to open stdout");

    // Result channel from reader thread
    let (result_tx, result_rx) = mpsc::channel::<ShellMessage>();
    start_reader_thread(child_stdout, result_tx);

    // Wait for main/0 to complete (starts supervision tree synchronously)
    // Give a brief moment for the shell to enter its eval loop
    thread::sleep(std::time::Duration::from_millis(300));

    let mut rl = Editor::<JsHelper, DefaultHistory>::new().expect("Failed to initialize editor");
    rl.set_helper(Some(JsHelper));
    let hist = history_path();
    let _ = rl.load_history(&hist);

    let mut erl_stdin = child_stdin;
    let mut buffer = String::new();

    loop {
        let p = if buffer.is_empty() { prompt.as_str() } else { "...> " };
        let line: String = match rl.readline(p) {
            Ok(l) => l,
            Err(ReadlineError::Interrupted) => {
                if buffer.is_empty() {
                    break;
                }
                buffer.clear();
                continue;
            }
            Err(ReadlineError::Eof) => break,
            Err(e) => {
                eprintln!("Input error: {e}");
                break;
            }
        };

        let trimmed = line.trim();
        if trimmed.is_empty() && buffer.is_empty() {
            continue;
        }

        if buffer.is_empty() {
            buffer = trimmed.to_string();
        } else {
            buffer.push('\n');
            buffer.push_str(trimmed);
        }

        // Parse JS
        let allocator = Allocator::default();
        let source_type = SourceType::from_path("repl.ts").unwrap();
        let parser_return = Parser::new(&allocator, &buffer, source_type).parse();

        if !parser_return.errors.is_empty() {
            if buffer.matches('{').count() > buffer.matches('}').count()
                || buffer.matches('(').count() > buffer.matches(')').count()
            {
                continue;
            }
            eprintln!("Parse error: {}", parser_return.errors[0]);
            buffer.clear();
            continue;
        }

        let input_source = buffer.clone();

        // Compile JS → Erlang expressions
        let mut exprs: Vec<String> = Vec::new();
        for stmt in &parser_return.program.body {
            if let Some(erl) = compiler::compile_stmt_persistent_repl(stmt) {
                exprs.push(erl);
            } else {
                eprintln!("Unsupported statement");
            }
        }
        buffer.clear();

        if exprs.is_empty() {
            continue;
        }

        // Send compiled Erlang to the eval server (comma-separated, one line)
        let eval_str = exprs.join(", ");
        use io::Write as _;
        if writeln!(erl_stdin, "{eval_str}").is_err() {
            eprintln!("VM process exited");
            break;
        }
        if erl_stdin.flush().is_err() {
            eprintln!("VM process exited");
            break;
        }

        // Wait for result
        match result_rx.recv() {
            Ok(ShellMessage::Result(value)) => {
                println!("\x1b[36m{value}\x1b[0m");
            }
            Ok(ShellMessage::Error(err)) => {
                eprintln!("\x1b[31mError: {}\x1b[0m", friendly_erl_error(&err));
            }
            Err(_) => {
                eprintln!("VM process exited");
                break;
            }
        }

        let _ = rl.add_history_entry(&input_source);
    }

    let _ = rl.save_history(&hist);
    let _ = child.wait();
    println!("Goodbye!");
}

#[cfg(test)]
mod tests {
    use super::{drain_shell_output, ParsedShellChunk, END_MARKER, ERROR_PREFIX, RESULT_PREFIX};

    #[test]
    fn parses_multiline_error_frames() {
        let mut buffer = format!("{ERROR_PREFIX}{{'EXIT',\n  crash}}{END_MARKER}\n");

        let chunks = drain_shell_output(&mut buffer, false);
        assert_eq!(
            chunks,
            vec![ParsedShellChunk::Error("{'EXIT',\n  crash}".to_string())]
        );
        assert!(buffer.is_empty());
    }

    #[test]
    fn preserves_passthrough_between_frames() {
        let mut buffer = format!("hello\n{RESULT_PREFIX}ok{END_MARKER}\nworld\n");

        let chunks = drain_shell_output(&mut buffer, false);
        assert_eq!(
            chunks,
            vec![
                ParsedShellChunk::PassThrough("hello\n".to_string()),
                ParsedShellChunk::Result("ok".to_string()),
                ParsedShellChunk::PassThrough("world\n".to_string()),
            ]
        );
        assert!(buffer.is_empty());
    }

    #[test]
    fn waits_for_complete_frames_across_reads() {
        let mut buffer = format!("{ERROR_PREFIX}partial");
        assert!(drain_shell_output(&mut buffer, false).is_empty());

        buffer.push_str(&format!("\nframe{END_MARKER}\n"));
        let chunks = drain_shell_output(&mut buffer, false);
        assert_eq!(
            chunks,
            vec![ParsedShellChunk::Error("partial\nframe".to_string())]
        );
        assert!(buffer.is_empty());
    }
}

/// Run a REPL connected to a remote Erlang node.
pub fn run_connect(target_node: &str) {
    let prompt = format!(
        "juice@{}> ",
        target_node.split('@').next().unwrap_or(target_node)
    );

    // Generate a unique client node name
    let client_name = format!("juice_client_{}", std::process::id());

    let mut cmd = Command::new("erl");
    cmd.arg("-noshell")
        .arg("-hidden")
        .arg("-pa")
        .arg(".")
        .arg("-kernel")
        .arg("logger")
        .arg("[{handler, default, logger_std_h, #{config => #{type => standard_error}}}]")
        .arg("-sname")
        .arg(&client_name)
        .arg("-setcookie")
        .arg("juice")
        .arg("-s")
        .arg("juice_remote_shell")
        .arg("start")
        .arg(target_node)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());

    let mut child = cmd.spawn().unwrap_or_else(|e| {
        eprintln!("Error starting Erlang VM: {e}");
        std::process::exit(1);
    });

    let child_stdin = child.stdin.take().expect("Failed to open stdin");
    let child_stdout = child.stdout.take().expect("Failed to open stdout");

    let (result_tx, result_rx) = mpsc::channel::<ShellMessage>();
    start_reader_thread(child_stdout, result_tx);

    // Wait for connection result
    match result_rx.recv() {
        Ok(ShellMessage::Result(msg)) => {
            println!("Connected to {target_node}");
            let _ = msg; // "connected"
        }
        Ok(ShellMessage::Error(err)) => {
            eprintln!("Failed to connect: {err}");
            let _ = child.wait();
            std::process::exit(1);
        }
        Err(_) => {
            eprintln!("Failed to start client node");
            let _ = child.wait();
            std::process::exit(1);
        }
    }

    let mut rl = Editor::<JsHelper, DefaultHistory>::new().expect("Failed to initialize editor");
    rl.set_helper(Some(JsHelper));
    let hist = history_path();
    let _ = rl.load_history(&hist);

    let mut erl_stdin = child_stdin;
    let mut buffer = String::new();

    loop {
        let p = if buffer.is_empty() { prompt.as_str() } else { "...> " };
        let line: String = match rl.readline(p) {
            Ok(l) => l,
            Err(ReadlineError::Interrupted) => {
                if buffer.is_empty() {
                    break;
                }
                buffer.clear();
                continue;
            }
            Err(ReadlineError::Eof) => break,
            Err(e) => {
                eprintln!("Input error: {e}");
                break;
            }
        };

        let trimmed = line.trim();
        if trimmed.is_empty() && buffer.is_empty() {
            continue;
        }

        if buffer.is_empty() {
            buffer = trimmed.to_string();
        } else {
            buffer.push('\n');
            buffer.push_str(trimmed);
        }

        let allocator = Allocator::default();
        let source_type = SourceType::from_path("repl.ts").unwrap();
        let parser_return = Parser::new(&allocator, &buffer, source_type).parse();

        if !parser_return.errors.is_empty() {
            if buffer.matches('{').count() > buffer.matches('}').count()
                || buffer.matches('(').count() > buffer.matches(')').count()
            {
                continue;
            }
            eprintln!("Parse error: {}", parser_return.errors[0]);
            buffer.clear();
            continue;
        }

        let input_source = buffer.clone();

        let mut exprs: Vec<String> = Vec::new();
        for stmt in &parser_return.program.body {
            if let Some(erl) = compiler::compile_stmt_persistent_repl(stmt) {
                exprs.push(erl);
            } else {
                eprintln!("Unsupported statement");
            }
        }
        buffer.clear();

        if exprs.is_empty() {
            continue;
        }

        let eval_str = exprs.join(", ");
        use io::Write as _;
        if writeln!(erl_stdin, "{eval_str}").is_err() {
            eprintln!("VM process exited");
            break;
        }
        if erl_stdin.flush().is_err() {
            eprintln!("VM process exited");
            break;
        }

        match result_rx.recv() {
            Ok(ShellMessage::Result(value)) => {
                println!("\x1b[36m{value}\x1b[0m");
            }
            Ok(ShellMessage::Error(err)) => {
                eprintln!("\x1b[31mError: {}\x1b[0m", friendly_erl_error(&err));
            }
            Err(_) => {
                eprintln!("VM process exited");
                break;
            }
        }

        let _ = rl.add_history_entry(&input_source);
    }

    let _ = rl.save_history(&hist);
    let _ = child.wait();
    println!("Goodbye!");
}
