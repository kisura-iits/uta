use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

// Represents our parsed language commands
#[derive(Debug)]
enum Command {
    StartServer {
        port: u16,
        file_descriptor: String,
        is_project: bool,
    },
    EndServer {
        port: u16,
    },
}

fn main() {
    // 1. Get the file path from command line arguments
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: cargo run -- <filename>.web");
        return;
    }

    let file_path = &args[1];
    let path = Path::new(file_path);

    // 2. Strict file extension check
    if path.extension().and_then(|s| s.to_str()) != Some("web") {
        eprintln!("Error: This program strictly only accepts '.web' files.");
        return;
    }

    // 3. Read the file content
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error reading file '{}': {}", file_path, e);
            return;
        }
    };

    println!("Parsing {}...", file_path);
    let commands = parse_web_lang(&content);

    // 4. Execute the parsed commands
    // We use a shared atomic flag so an end_server command can signal a running server thread to stop.
    let running_flag = Arc::new(AtomicBool::new(true));

    for command in commands {
        match command {
            Command::StartServer { port, file_descriptor, is_project } => {
                let flag = Arc::clone(&running_flag);
                println!("🚀 Executing: start_server on port {}, serving '{}'...", port, file_descriptor);
                
                // Run the server in a separate thread so it doesn't block the execution of subsequent commands
                thread::spawn(move || {
                    run_server(port, file_descriptor, is_project, flag);
                });
                
                // Give the server a brief moment to bind to the port before running next commands
                thread::sleep(Duration::from_millis(100));
            }
            Command::EndServer { port } => {
                println!("🛑 Executing: end_server on port {}...", port);
                running_flag.store(false, Ordering::SeqCst);
                
                // Trigger a dummy connection to unblock the TcpListener loop so it can exit cleanly
                if let Ok(mut stream) = TcpStream::connect(format!("127.0.0.1:{}", port)) {
                    let _ = stream.write_all(b"GET / HTTP/1.1\r\n\r\n");
                }
                println!("Server on port {} has been signaled to shut down.", port);
            }
        }
    }

    // Keep the main thread alive for a bit if a server is running
    // In a mature language, this would be replaced by an event-loop or join handles.
    println!("Execution finished. Keeping process alive for 30 seconds (Press Ctrl+C to exit)...");
    thread::sleep(Duration::from_secs(30));
}

/// A rudimentary parser for our .web syntax
fn parse_web_lang(input: &str) -> Vec<Command> {
    let mut commands = Vec::new();

    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue; // Skip empty lines and comments
        }

        if line.starts_with("start_server") {
            // Quick and dirty extraction of what's inside the parentheses
            if let (Some(start), Some(end)) = (line.find('('), line.rfind(')')) {
                let args_str = &line[start + 1..end];
                let args: Vec<&str> = args_str.split(',').map(|s| s.trim()).collect();

                if args.len() == 3 {
                    let port = parse_port(args[0]);
                    let file_descriptor = parse_string(args[1]);
                    let is_project = parse_bool(args[2]);

                    commands.push(Command::StartServer { port, file_descriptor, is_project });
                }
            }
        } else if line.starts_with("end_server") {
            if let (Some(start), Some(end)) = (line.find('('), line.rfind(')')) {
                let args_str = &line[start + 1..end];
                let port = parse_port(args_str);

                commands.push(Command::EndServer { port });
            }
        }
    }
    commands
}

// Parsing Helpers
fn parse_port(s: &str) -> u16 {
    s.replace("port:", "").trim().parse().unwrap_or(8080)
}

fn parse_string(s: &str) -> String {
    let clean = s.replace("file_descriptor:", "");
    clean.trim().trim_matches('"').to_string()
}

fn parse_bool(s: &str) -> bool {
    let clean = s.replace("is_project:", "");
    clean.trim().parse().unwrap_or(false)
}

/// Simple HTTP Server implementation
fn run_server(port: u16, file_descriptor: String, _is_project: bool, running: Arc<AtomicBool>) {
    let listener = match TcpListener::bind(format!("127.0.0.1:{}", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind to port {}: {}", port, e);
            return;
        }
    };

    // Set a timeout so incoming connections don't block indefinitely when shutting down
    let _ = listener.set_nonblocking(false); 

    for stream in listener.incoming() {
        if !running.load(Ordering::SeqCst) {
            break; // Stop accepting connections if end_server was called
        }

        if let Ok(stream) = stream {
            handle_connection(stream, &file_descriptor);
        }
    }
    println!("Server on port {} stopped.", port);
}

fn handle_connection(mut stream: TcpStream, file_to_serve: &str) {
    let buf_reader = BufReader::new(&mut stream);
    let _http_request: Vec<_> = buf_reader
        .lines()
        .map(|result| result.unwrap())
        .take_while(|line| !line.is_empty())
        .collect();

    // Read the file requested by the language syntax, default to a fallback string if missing
    let (status_line, filename) = if Path::new(file_to_serve).exists() {
        ("HTTP/1.1 200 OK", file_to_serve)
    } else {
        ("HTTP/1.1 404 NOT FOUND", "")
    };

    let contents = if !filename.is_empty() {
        fs::read_to_string(filename).unwrap_or_else(|_| "<h1>404 Error</h1>".to_string())
    } else {
        format!("<h1>File '{}' not found locally!</h1><p>Your .web code requested this file, but it doesn't exist.</p>", file_to_serve)
    };

    let length = contents.len();
    let response = format!("{status_line}\r\nContent-Length: {length}\r\n\r\n{contents}");

    let _ = stream.write_all(response.as_bytes());
}