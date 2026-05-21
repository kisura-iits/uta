mod ast;
mod eval;
mod lexer;
mod parser;

use eval::{execute_program, Environment, RuntimeState};
use parser::Parser;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

fn resolve_script_path(arg: &str) -> Result<PathBuf, String> {
    let path = Path::new(arg);
    if path.is_file() {
        return path
            .canonicalize()
            .map_err(|e| format!("Cannot resolve script path '{}': {}", arg, e));
    }
    let from_cwd = env::current_dir()
        .map_err(|e| format!("Cannot read current directory: {}", e))?
        .join(path);
    if from_cwd.is_file() {
        return from_cwd
            .canonicalize()
            .map_err(|e| format!("Cannot resolve script path '{}': {}", arg, e));
    }
    Err(format!(
        "Script file not found: '{}'. Run from the project folder or pass an absolute path.",
        arg
    ))
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: cargo run -- <script>.web");
        eprintln!("Example: cargo run -- script.web");
        process::exit(1);
    }

    let script_path = match resolve_script_path(&args[1]) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("UTA Error: {}", e);
            process::exit(1);
        }
    };

    if script_path.extension().and_then(|s| s.to_str()) != Some("web") {
        eprintln!("UTA Error: Extension must strictly be '.web'");
        process::exit(1);
    }

    let content = match fs::read_to_string(&script_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("UTA Error: Failed to read '{}': {}", script_path.display(), e);
            process::exit(1);
        }
    };

    println!("--- UTA Engine ---");
    println!("Script: {}", script_path.display());

    let program = match Parser::parse(&content) {
        Ok(stmts) => stmts,
        Err(e) => {
            eprintln!("UTA Parse Error: {}", e);
            process::exit(1);
        }
    };

    let script_dir = script_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let env = Arc::new(Mutex::new(Environment::new()));
    let mut state = RuntimeState::new(script_dir);

    if let Err(e) = execute_program(program, env, &mut state) {
        eprintln!("UTA Runtime Error: {}", e);
        process::exit(1);
    }

    if state.servers.is_empty() {
        println!("UTA: program finished (no server started).");
        return;
    }

    println!("UTA: press Ctrl+C to stop the server.");
    while state
        .servers
        .values()
        .any(|flag| !flag.load(Ordering::Relaxed))
    {
        thread::sleep(Duration::from_millis(100));
    }
    println!("UTA: done.");
}
