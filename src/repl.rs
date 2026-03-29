use std::io::{self, BufRead, Write};
use std::process::Command;

use oxc_allocator::Allocator;

use oxc_parser::Parser;
use oxc_span::SourceType;

use crate::compiler;

pub fn run() {
    println!("box v0.1.0 - juice interactive shell");
    println!("Type a JS expression. Ctrl+D to exit.");
    println!();

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    print!("box> ");
    stdout.flush().unwrap();

    let mut buffer = String::new();
    let mut history: Vec<String> = Vec::new();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

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
            for error in &parser_return.errors {
                eprintln!("Parse error: {error}");
            }
            buffer.clear();
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
            // Track variable/function definitions for history
            if let Some(def) = compiler::compile_stmt(stmt) {
                new_defs.push(def);
            }
        }

        buffer.clear();

        if !exprs.is_empty() {
            let mut all_exprs = history.clone();
            all_exprs.extend(exprs);
            let eval_str = format!("{}, halt().", all_exprs.join(", "));
            let output = Command::new("erl")
                .arg("-eval")
                .arg(&eval_str)
                .arg("-noshell")
                .output();

            match output {
                Ok(out) => {
                    if !out.stdout.is_empty() {
                        print!("\x1b[36m{}\x1b[0m", String::from_utf8_lossy(&out.stdout));
                    }
                    if !out.status.success() && !out.stderr.is_empty() {
                        eprint!("{}", String::from_utf8_lossy(&out.stderr));
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
