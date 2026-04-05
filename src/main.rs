mod compiler;
mod erlang;
mod project;
mod repl;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;

const VERSION: &str = "0.1.0";

struct Flags {
    emit_erl: bool,
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
        "box" => {
            write_and_compile_erl("juice_shell", &erlang::shell_module());
            repl::run();
        }
        "new" => {
            if positional.len() < 2 {
                eprintln!("Usage: juice new <project-name>");
                std::process::exit(1);
            }
            new_project(&positional[1]);
        }
        "run" => {
            if positional.len() < 2 {
                eprintln!("Usage: juice run <file>");
                std::process::exit(1);
            }
            run_file(Path::new(&positional[1]), &flags);
        }
        "compile" => { compile_project(); }
        "start" => {
            if positional.len() < 2 {
                start_project(&flags);
            } else {
                start_file(Path::new(&positional[1]), &flags);
            }
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
            eprint!("{}", repl::format_parse_error(&source, error));
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
}

fn run_file(input_path: &Path, flags: &Flags) {
    let module_name = compile_and_prepare(input_path, flags);

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

fn start_file(input_path: &Path, flags: &Flags) {
    let module_name = compile_and_prepare(input_path, flags);

    // Generate and compile the shell eval server
    write_and_compile_erl("juice_shell", &erlang::shell_module());

    println!("Starting {}...", module_name);

    repl::run_persistent(&module_name, flags.name.as_deref(), ".");
}

fn connect_node(target: &str) {
    // Generate and compile the remote shell module
    write_and_compile_erl("juice_remote_shell", &erlang::remote_shell_module());

    repl::run_connect(target);
}

fn new_project(name: &str) {
    let cwd = env::current_dir().unwrap_or_else(|e| {
        eprintln!("Error getting current directory: {e}");
        std::process::exit(1);
    });

    project::create_project(&cwd, name).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });

    let sanitized = project::sanitize_name(name);
    println!("Created project '{}' in ./{}", sanitized, name);
    println!();
    println!("  cd {}", name);
    println!("  juice start");
}

fn write_and_compile_erl_to(name: &str, source: &str, out_dir: &Path) {
    let erl_path = out_dir.join(format!("{name}.erl"));
    fs::write(&erl_path, source).unwrap_or_else(|e| {
        eprintln!("Error writing {}: {e}", erl_path.display());
        std::process::exit(1);
    });

    let status = Command::new("erlc")
        .arg("-o")
        .arg(out_dir)
        .arg(&erl_path)
        .status()
        .unwrap_or_else(|e| {
            eprintln!("Error running erlc: {e}");
            std::process::exit(1);
        });

    if !status.success() {
        eprintln!("erlc failed on {}", erl_path.display());
        std::process::exit(1);
    }
}

fn compile_project() -> (String, PathBuf) {
    let root = project::find_project_root().unwrap_or_else(|| {
        eprintln!("No juice.json found in current directory.");
        eprintln!("Run 'juice new <name>' to create a project, or 'juice <file>' to compile a single file.");
        std::process::exit(1);
    });

    let config = project::load_project_config(&root).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });

    let build_dir = root.join("build");
    fs::create_dir_all(&build_dir).unwrap_or_else(|e| {
        eprintln!("Error creating build directory: {e}");
        std::process::exit(1);
    });

    // Collect .ts files from src/
    let src_dir = root.join("src");
    let mut ts_files: Vec<_> = fs::read_dir(&src_dir)
        .unwrap_or_else(|e| {
            eprintln!("Error reading src/ directory: {e}");
            std::process::exit(1);
        })
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("ts") {
                Some(path)
            } else {
                None
            }
        })
        .collect();
    ts_files.sort();

    if ts_files.is_empty() {
        eprintln!("No .ts files found in src/");
        std::process::exit(1);
    }

    let mut needs_supervisor = false;

    for ts_path in &ts_files {
        let source = fs::read_to_string(ts_path).unwrap_or_else(|e| {
            eprintln!("Error reading {}: {e}", ts_path.display());
            std::process::exit(1);
        });

        let module_name = ts_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("main")
            .to_string();

        let allocator = Allocator::default();
        let source_type = SourceType::from_path(ts_path).unwrap_or_else(|_| SourceType::mjs());
        let parser_return = Parser::new(&allocator, &source, source_type).parse();

        if !parser_return.errors.is_empty() {
            for error in &parser_return.errors {
                eprintln!("{}:", ts_path.display());
                eprint!("{}", repl::format_parse_error(&source, error));
            }
            std::process::exit(1);
        }

        let result = compiler::compile(&module_name, &parser_return.program);

        if result.needs_supervisor {
            needs_supervisor = true;
        }

        write_and_compile_erl_to(&module_name, &result.source, &build_dir);
        println!("Compiled {module_name}.beam");
    }

    if needs_supervisor {
        write_and_compile_erl_to("juice_supervisor", &erlang::supervisor_module(), &build_dir);
    }

    // Derive entry module from config.main
    let entry_module = Path::new(&config.main)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("main")
        .to_string();

    (entry_module, build_dir)
}

fn start_project(flags: &Flags) {
    let (entry_module, build_dir) = compile_project();

    // Generate and compile the shell eval server into build/
    write_and_compile_erl_to("juice_shell", &erlang::shell_module(), &build_dir);

    println!("Starting {}...", entry_module);

    let beam_dir = build_dir.to_str().unwrap_or("build");
    repl::run_persistent(&entry_module, flags.name.as_deref(), beam_dir);
}

fn print_usage() {
    eprintln!("juice {VERSION} - JavaScript to BEAM compiler");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  juice new <name>          Create a new project");
    eprintln!("  juice compile             Compile project (requires juice.json)");
    eprintln!("  juice start               Start project with REPL (requires juice.json)");
    eprintln!("  juice <file>              Compile to .beam");
    eprintln!("  juice <file> --emit-erl   Also print generated Erlang");
    eprintln!("  juice run <file>          Compile and execute");
    eprintln!("  juice box                 Interactive REPL");
    eprintln!("  juice start <file>        Start supervised project with REPL");
    eprintln!("  juice connect <node>      Connect to a running Juice node");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -h, --help                Print this help");
    eprintln!("  -v, --version             Print version");
    eprintln!("  --name <node>             Start as a named Erlang node");
}
