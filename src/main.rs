mod ast;
mod eval;
mod lexer;
mod parser;

use eval::{execute_program, Environment, RuntimeState};
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;
use parser::Parser;
use std::env;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: cargo run -- <filename>.web");
        return;
    }

    let file_path = &args[1];
    let path = Path::new(file_path);

    if path.extension().and_then(|s| s.to_str()) != Some("web") {
        eprintln!("UTA Error: Extension must strictly be '.web'");
        return;
    }

    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("UTA Error: Failed to read file: {}", e);
            return;
        }
    };

    println!("--- Initializing UTA Engine ---");

    let program = match Parser::parse(&content) {
        Ok(stmts) => stmts,
        Err(e) => {
            eprintln!("UTA Parse Error: {}", e);
            return;
        }
    };

    let script_dir = path
        .canonicalize()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    let env = Arc::new(Mutex::new(Environment::new()));
    let mut state = RuntimeState::new(script_dir);

    if let Err(e) = execute_program(program, env, &mut state) {
        eprintln!("UTA Runtime Error: {}", e);
        return;
    }

    while state
        .servers
        .values()
        .any(|flag| !flag.load(Ordering::Relaxed))
    {
        thread::sleep(Duration::from_millis(100));
    }
}
