use std::io::{self, Write};
use std::process::Command;
use std::sync::mpsc;
use std::thread;

use oxc_allocator::Allocator;

use oxc_parser::Parser;
use oxc_span::SourceType;

use crate::compiler;

fn friendly_erl_error(stderr: &str) -> String {
    // Pattern 1: {{error_type, detail}, stacktrace}  (e.g. unbound_var)
    if let Some(start) = stderr.find("{{") {
        let rest = &stderr[start + 2..];
        if let Some(end) = rest.find('}') {
            let reason = &rest[..end];
            if let Some((_kind, detail)) = reason.split_once(',') {
                let detail = detail.trim().trim_matches('\'');
                if reason.starts_with("unbound_var") {
                    return format!("undefined variable '{detail}'");
                }
            }
            return reason.to_string();
        }
    }
    // Pattern 2: ({error_type, stacktrace})  (e.g. badarg, badarith)
    if let Some(start) = stderr.find("({") {
        let rest = &stderr[start + 2..];
        if let Some(end) = rest.find(',') {
            let error_type = &rest[..end];
            return match error_type {
                "badarg" => "bad argument".to_string(),
                "badarith" => "arithmetic error".to_string(),
                "function_clause" => "no matching function clause".to_string(),
                other => other.to_string(),
            };
        }
    }
    // Fallback: first non-empty line, stripped of crash dump noise
    stderr
        .lines()
        .find(|l| !l.is_empty() && !l.contains("Crash dump") && !l.contains("Runtime terminating"))
        .unwrap_or("runtime error")
        .to_string()
}

/// Drain any remaining pasted lines from the channel
fn drain(rx: &mpsc::Receiver<String>) {
    // Small delay to let the reader thread catch up with pasted input
    thread::sleep(std::time::Duration::from_millis(50));
    while rx.try_recv().is_ok() {}
}

pub fn run() {
    println!("box v0.1.0 - juice interactive shell");
    println!("Type a JS expression. Ctrl+D to exit.");
    println!();

    let mut stdout = io::stdout();

    // Reader thread sends lines through a channel so we can drain on error
    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        use io::BufRead;
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(l) => {
                    if tx.send(l).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    print!("box> ");
    stdout.flush().unwrap();

    let mut buffer = String::new();
    let mut history: Vec<String> = Vec::new();

    while let Ok(line) = rx.recv() {
        let trimmed = line.trim();
        if trimmed.is_empty() && buffer.is_empty() {
            print!("box> ");
            stdout.flush().unwrap();
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
            // If input looks incomplete (unclosed brace/paren), wait for more lines
            if buffer.matches('{').count() > buffer.matches('}').count()
                || buffer.matches('(').count() > buffer.matches(')').count()
            {
                print!("...> ");
                stdout.flush().unwrap();
                continue;
            }
            eprintln!("Parse error: {}", parser_return.errors[0]);
            buffer.clear();
            drain(&rx);
            print!("box> ");
            stdout.flush().unwrap();
            continue;
        }

        let mut exprs: Vec<String> = Vec::new();
        let mut new_defs: Vec<String> = Vec::new();
        for stmt in &parser_return.program.body {
            if let Some(erl) = compiler::compile_stmt_repl(stmt) {
                exprs.push(erl);
            } else {
                eprintln!("Unsupported statement");
            }
            // Only persist variable/function definitions, not side effects
            if matches!(stmt, oxc_ast::ast::Statement::VariableDeclaration(_)) {
                if let Some(def) = compiler::compile_stmt(stmt) {
                    new_defs.push(def);
                }
            }
        }

        buffer.clear();

        if !exprs.is_empty() {
            let mut all_exprs = history.clone();
            all_exprs.extend(exprs);
            // In erl -eval, juice_to_string must be a local fun, not a module function
            let to_string_fun = "JuiceToString = fun(V) when is_integer(V) -> integer_to_list(V); (V) when is_float(V) -> float_to_list(V, [{decimals, 10}, compact]); (V) when is_atom(V) -> atom_to_list(V); (V) when is_list(V) -> V; (V) -> lists:flatten(io_lib:format(\"~p\", [V])) end";
            let mut eval_parts = vec![to_string_fun.to_string()];
            // Rewrite juice_to_string() calls to use the local fun variable
            let rewritten: Vec<String> = all_exprs.iter()
                .map(|e| e.replace("juice_to_string(", "JuiceToString("))
                .collect();
            eval_parts.extend(rewritten);
            let eval_str = format!("{}, halt().", eval_parts.join(", "));
            let output = Command::new("erl")
                .arg("-eval")
                .arg(&eval_str)
                .arg("-noshell")
                .env("ERL_CRASH_DUMP", "/dev/null")
                .output();

            match output {
                Ok(out) => {
                    if !out.stdout.is_empty() {
                        print!("\x1b[36m{}\x1b[0m", String::from_utf8_lossy(&out.stdout));
                    }
                    if !out.status.success() {
                        let stderr = String::from_utf8_lossy(&out.stderr);
                        eprintln!("\x1b[31mError: {}\x1b[0m", friendly_erl_error(&stderr));
                        drain(&rx);
                    } else {
                        history.extend(new_defs);
                    }
                }
                Err(e) => eprintln!("Error running erl: {e}"),
            }
        }

        print!("box> ");
        stdout.flush().unwrap();
    }

    println!("Goodbye!");
}
