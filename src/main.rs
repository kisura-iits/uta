use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

// 1. Represents the dynamic types our language supports at runtime
#[derive(Debug, Clone)]
enum RuntimeValue {
    Int(u16),
    Str(String),
    Bool(bool),
}

// Represents our parsed language commands
#[derive(Debug)]
enum Command {
    VariableAssign {
        name: String,
        value: RuntimeValue,
    },
    StartServer {
        port_expr: String, // Can be a raw number or a variable name
        file_expr: String,
        bool_expr: String,
    },
    EndServer {
        port_expr: String,
    },
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: cargo run -- <filename>.web");
        return;
    }

    let file_path = &args[1];
    let path = Path::new(file_path);

    if path.extension().and_then(|s| s.to_str()) != Some("web") {
        eprintln!("Error: This program strictly only accepts '.web' files.");
        return;
    }

    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error reading file '{}': {}", file_path, e);
            return;
        }
    };

    println!("Parsing and executing {}...", file_path);
    let commands = parse_web_lang(&content);

    // This is our Environment / Runtime Memory
    let mut environment: HashMap<String, RuntimeValue> = HashMap::new();
    let running_flag = Arc::new(AtomicBool::new(true));

    // 2. Interpreter Loop
    for command in commands {
        match command {
            Command::VariableAssign { name, value } => {
                println!("💼 Storing variable: {} = {:?}", name, value);
                environment.insert(name, value);
            }
            Command::StartServer { port_expr, file_expr, bool_expr } => {
                // Resolve variables from environment, or fallback to literal parsing
                let port = resolve_port(&port_expr, &environment);
                let file_descriptor = resolve_string(&file_expr, &environment);
                let is_project = resolve_bool(&bool_expr, &environment);

                let flag = Arc::clone(&running_flag);
                println!("🚀 Executing: start_server(port: {}, file_descriptor: \"{}\", is_project: {})", port, file_descriptor, is_project);
                
                thread::spawn(move || {
                    run_server(port, file_descriptor, is_project, flag);
                });
                
                thread::sleep(Duration::from_millis(100));
            }
            Command::EndServer { port_expr } => {
                let port = resolve_port(&port_expr, &environment);
                println!("🛑 Executing: end_server on port {}...", port);
                running_flag.store(false, Ordering::SeqCst);
                
                if let Ok(mut stream) = TcpStream::connect(format!("127.0.0.1:{}", port)) {
                    let _ = stream.write_all(b"GET / HTTP/1.1\r\n\r\n");
                }
            }
        }
    }

    println!("Execution finished. Keeping process alive for 30 seconds...");
    thread::sleep(Duration::from_secs(30));
}

/// A parser that handles variables and functions
fn parse_web_lang(input: &str) -> Vec<Command> {
    let mut commands = Vec::new();

    for line in input.lines() {
        let line = line.trim();
        // Skip empty lines or lines starting with '//'
        if line.is_empty() || line.starts_with("//") {
            continue;
        }

        // Match: const x = 10;
        if line.starts_with("const ") {
            let clean_line = line.trim_end_matches(';');
            let parts: Vec<&str> = clean_line["const ".len()..].split('=').map(|s| s.trim()).collect();
            
            if parts.len() == 2 {
                let var_name = parts[0].to_string();
                let raw_val = parts[1];

                // Determine type dynamically based on syntax literals
                let value = if raw_val.starts_with('"') && raw_val.ends_with('"') {
                    RuntimeValue::Str(raw_val.trim_matches('"').to_string())
                } else if raw_val == "true" || raw_val == "false" {
                    RuntimeValue::Bool(raw_val.parse().unwrap_or(false))
                } else {
                    RuntimeValue::Int(raw_val.parse().unwrap_or(0))
                };

                commands.push(Command::VariableAssign { name: var_name, value });
            }
        } 
        // Match: start_server(...)
        else if line.starts_with("start_server") {
            if let (Some(start), Some(end)) = (line.find('('), line.rfind(')')) {
                let args_str = &line[start + 1..end];
                let args: Vec<&str> = args_str.split(',').map(|s| s.trim()).collect();

                if args.len() == 3 {
                    // Strip labels like "port:" to isolate the variable or literal expressions
                    let port_expr = args[0].replace("port:", "").trim().to_string();
                    let file_expr = args[1].replace("file_descriptor:", "").trim().to_string();
                    let bool_expr = args[2].replace("is_project:", "").trim().to_string();

                    commands.push(Command::StartServer { port_expr, file_expr, bool_expr });
                }
            }
        }
        // Match: end_server(...)
        else if line.starts_with("end_server") {
            if let (Some(start), Some(end)) = (line.find('('), line.rfind(')')) {
                let port_expr = line[start + 1..end].replace("port:", "").trim().to_string();
                commands.push(Command::EndServer { port_expr });
            }
        }
    }
    commands
}

// --- Variable Resolution Helpers ---

fn resolve_port(expr: &str, env: &HashMap<String, RuntimeValue>) -> u16 {
    // If it's a variable in memory, extract it
    if let Some(RuntimeValue::Int(val)) = env.get(expr) {
        return *val;
    }
    // Otherwise, try to parse it as a direct integer literal
    expr.parse().unwrap_or(8080)
}

fn resolve_string(expr: &str, env: &HashMap<String, RuntimeValue>) -> String {
    if let Some(RuntimeValue::Str(val)) = env.get(expr) {
        return val.clone();
    }
    expr.trim_matches('"').to_string()
}

fn resolve_bool(expr: &str, env: &HashMap<String, RuntimeValue>) -> bool {
    if let Some(RuntimeValue::Bool(val)) = env.get(expr) {
        return *val;
    }
    expr.parse().unwrap_or(false)
}

// --- Server Engine ---
fn run_server(port: u16, file_descriptor: String, _is_project: bool, running: Arc<AtomicBool>) {
    let listener = match TcpListener::bind(format!("127.0.0.1:{}", port)) {
        Ok(l) => l,
        Err(_) => return,
    };

    for stream in listener.incoming() {
        if !running.load(Ordering::SeqCst) { break; }
        if let Ok(stream) = stream {
            handle_connection(stream, &file_descriptor);
        }
    }
}

fn handle_connection(mut stream: TcpStream, file_to_serve: &str) {
    let buf_reader = BufReader::new(&mut stream);
    let _req: Vec<_> = buf_reader.lines().map(|r| r.unwrap()).take_while(|l| !l.is_empty()).collect();

    let (status, filename) = if Path::new(file_to_serve).exists() {
        ("HTTP/1.1 200 OK", file_to_serve)
    } else {
        ("HTTP/1.1 404 NOT FOUND", "")
    };

    let contents = if !filename.is_empty() {
        fs::read_to_string(filename).unwrap_or_else(|_| "<h1>Error</h1>".to_string())
    } else {
        format!("<h1>File '{}' not found</h1>", file_to_serve)
    };

    let response = format!("{status}\r\nContent-Length: {}\r\n\r\n{contents}", contents.len());
    let _ = stream.write_all(response.as_bytes());
}