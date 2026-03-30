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
    name: Option<String>,
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
        name: None,
    };
    let mut positional: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
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
            "--name" => {
                i += 1;
                if i < args.len() {
                    flags.name = Some(args[i].clone());
                } else {
                    eprintln!("--name requires a value");
                    std::process::exit(1);
                }
            }
            _ => positional.push(args[i].clone()),
        }
        i += 1;
    }

    if positional.is_empty() {
        print_usage();
        std::process::exit(1);
    }

    match positional[0].as_str() {
        "box" => repl::run(),
        "start" => {
            if positional.len() < 2 {
                eprintln!("Usage: juice start <file> [--name <node>]");
                std::process::exit(1);
            }
            start_file(Path::new(&positional[1]), &flags);
        }
        "connect" => {
            if positional.len() < 2 {
                eprintln!("Usage: juice connect <node@host>");
                std::process::exit(1);
            }
            connect_node(&positional[1]);
        }
        path => compile_file(Path::new(path), &flags),
    }
}

fn compile_and_prepare(input_path: &Path, flags: &Flags) -> String {
    let source = fs::read_to_string(input_path).unwrap_or_else(|e| {
        eprintln!("Error reading {}: {e}", input_path.display());
        std::process::exit(1);
    });

    let module_name = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("main")
        .to_string();

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
    let result = compiler::compile(&module_name, &parser_return.program);

    if flags.emit_erl {
        print!("{}", result.source);
    }

    // Write and compile supervisor runtime module if needed
    if result.needs_supervisor {
        write_and_compile_erl("juice_supervisor", &erlang::supervisor_module());
    }

    // Write and compile user module
    write_and_compile_erl(&module_name, &result.source);

    module_name
}

fn write_and_compile_erl(name: &str, source: &str) {
    let erl_path = format!("{name}.erl");
    fs::write(&erl_path, source).unwrap_or_else(|e| {
        eprintln!("Error writing {erl_path}: {e}");
        std::process::exit(1);
    });

    let status = Command::new("erlc")
        .arg(&erl_path)
        .status()
        .unwrap_or_else(|e| {
            eprintln!("Error running erlc: {e}");
            std::process::exit(1);
        });

    if !status.success() {
        eprintln!("erlc failed on {erl_path}");
        std::process::exit(1);
    }
}

fn compile_file(input_path: &Path, flags: &Flags) {
    let module_name = compile_and_prepare(input_path, flags);

    println!("Compiled {module_name}.beam");

    // Run if requested
    if flags.run {
        let status = Command::new("erl")
            .args(["-noshell", "-s", &module_name, "main", "-s", "init", "stop"])
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

fn start_file(input_path: &Path, flags: &Flags) {
    let module_name = compile_and_prepare(input_path, flags);

    // Generate and compile the shell eval server
    write_and_compile_erl("juice_shell", &erlang::shell_module());

    println!("Starting {}...", module_name);

    repl::run_persistent(&module_name, flags.name.as_deref());
}

fn connect_node(target: &str) {
    // Generate and compile the remote shell module
    write_and_compile_erl("juice_remote_shell", &erlang::remote_shell_module());

    repl::run_connect(target);
}

fn print_usage() {
    eprintln!("juice {VERSION} - JavaScript to BEAM compiler");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  juice <file>              Compile to .beam");
    eprintln!("  juice <file> --emit-erl   Also print generated Erlang");
    eprintln!("  juice <file> --run        Compile and execute");
    eprintln!("  juice box                 Interactive REPL");
    eprintln!("  juice start <file>        Start supervised project with REPL");
    eprintln!("  juice connect <node>      Connect to a running Juice node");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -h, --help                Print this help");
    eprintln!("  -v, --version             Print version");
    eprintln!("  --name <node>             Start as a named Erlang node");
}
