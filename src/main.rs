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

const VERSION: &str = "0.1.0";

struct Flags {
    emit_erl: bool,
    run: bool,
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        print_usage();
        std::process::exit(1);
    }

    // Parse flags and collect positional args
    let mut flags = Flags {
        emit_erl: false,
        run: false,
    };
    let mut positional: Vec<&str> = Vec::new();

    for arg in &args {
        match arg.as_str() {
            "--help" | "-h" => {
                print_usage();
                return;
            }
            "--version" | "-v" => {
                println!("juice {VERSION}");
                return;
            }
            "--emit-erl" => flags.emit_erl = true,
            "--run" => flags.run = true,
            _ => positional.push(arg),
        }
    }

    if positional.is_empty() {
        print_usage();
        std::process::exit(1);
    }

    match positional[0] {
        "box" => repl::run(),
        path => compile_file(Path::new(path), &flags),
    }
}

fn compile_file(input_path: &Path, flags: &Flags) {
    let source = fs::read_to_string(input_path).unwrap_or_else(|e| {
        eprintln!("Error reading {}: {e}", input_path.display());
        std::process::exit(1);
    });

    let module_name = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("main");

    // Parse
    let allocator = Allocator::default();
    let source_type = SourceType::from_path(input_path).unwrap_or_else(|_| SourceType::mjs());
    let parser_return = Parser::new(&allocator, &source, source_type).parse();

    if !parser_return.errors.is_empty() {
        for error in &parser_return.errors {
            eprintln!("Parse error: {error}");
        }
        std::process::exit(1);
    }

    // Compile to Erlang
    let result = compiler::compile(module_name, &parser_return.program);

    if flags.emit_erl {
        print!("{}", result.source);
    }

    // Write and compile supervisor runtime module if needed
    if result.needs_supervisor {
        let sup_source = erlang::supervisor_module();
        fs::write("juice_supervisor.erl", &sup_source).unwrap_or_else(|e| {
            eprintln!("Error writing juice_supervisor.erl: {e}");
            std::process::exit(1);
        });
        let status = Command::new("erlc")
            .arg("juice_supervisor.erl")
            .status()
            .unwrap_or_else(|e| {
                eprintln!("Error running erlc: {e}");
                std::process::exit(1);
            });
        if !status.success() {
            eprintln!("erlc failed on juice_supervisor.erl");
            std::process::exit(1);
        }
    }

    // Write .erl file
    let erl_path = format!("{module_name}.erl");
    fs::write(&erl_path, &result.source).unwrap_or_else(|e| {
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

    // Run if requested
    if flags.run {
        let status = Command::new("erl")
            .args(["-noshell", "-s", module_name, "main", "-s", "init", "stop"])
            .status()
            .unwrap_or_else(|e| {
                eprintln!("Error running erl: {e}");
                std::process::exit(1);
            });

        if !status.success() {
            std::process::exit(status.code().unwrap_or(1));
        }
    }
}

fn print_usage() {
    eprintln!("juice {VERSION} - JavaScript to BEAM compiler");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  juice <file>              Compile to .beam");
    eprintln!("  juice <file> --emit-erl   Also print generated Erlang");
    eprintln!("  juice <file> --run        Compile and execute");
    eprintln!("  juice box                 Interactive REPL");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -h, --help                Print this help");
    eprintln!("  -v, --version             Print version");
}
