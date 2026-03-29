mod compiler;
mod erlang;
mod repl;

use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: juice <file.js> or juice box");
        std::process::exit(1);
    }

    if args[1] == "box" {
        repl::run();
        return;
    }

    let input_path = Path::new(&args[1]);
    let source = fs::read_to_string(input_path).unwrap_or_else(|e| {
        eprintln!("Error reading {}: {e}", input_path.display());
        std::process::exit(1);
    });

    let module_name = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("main");

    // Parse JS
    let allocator = Allocator::default();
    let source_type = SourceType::mjs();
    let parser_return = Parser::new(&allocator, &source, source_type).parse();

    if !parser_return.errors.is_empty() {
        for error in &parser_return.errors {
            eprintln!("Parse error: {error}");
        }
        std::process::exit(1);
    }

    // Compile to Erlang
    let erl_source = compiler::compile(module_name, &parser_return.program);

    // Write .erl file
    let erl_path = format!("{module_name}.erl");
    fs::write(&erl_path, &erl_source).unwrap_or_else(|e| {
        eprintln!("Error writing {erl_path}: {e}");
        std::process::exit(1);
    });

    println!("Generated {erl_path}");

    // Compile with erlc
    let status = Command::new("erlc")
        .arg(&erl_path)
        .status()
        .unwrap_or_else(|e| {
            eprintln!("Error running erlc: {e}");
            std::process::exit(1);
        });

    if !status.success() {
        eprintln!("erlc failed");
        std::process::exit(1);
    }

    println!("Compiled {module_name}.beam");
}
