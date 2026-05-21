use crate::ast::{Expression, RuntimeValue, Statement};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::net::{SocketAddr, TcpListener, TcpStream};
use socket2::{Domain, Socket, Type};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub struct Environment {
    variables: HashMap<String, RuntimeValue>,
    parent: Option<Arc<Mutex<Environment>>>,
}

impl Environment {
    pub fn new() -> Self {
        Self {
            variables: HashMap::new(),
            parent: None,
        }
    }

    pub fn child(parent: Arc<Mutex<Environment>>) -> Self {
        Self {
            variables: HashMap::new(),
            parent: Some(parent),
        }
    }

    pub fn define(&mut self, name: String, value: RuntimeValue) -> Result<(), String> {
        if self.variables.contains_key(&name) {
            return Err(format!("Variable '{}' is already defined in this scope", name));
        }
        self.variables.insert(name, value);
        Ok(())
    }

    pub fn get(&self, name: &str) -> RuntimeValue {
        if let Some(val) = self.variables.get(name) {
            return val.clone();
        }
        if let Some(ref parent) = self.parent {
            return parent.lock().unwrap().get(name);
        }
        RuntimeValue::Null
    }
}

pub struct RuntimeState {
    pub active_coroutines: HashMap<String, thread::JoinHandle<()>>,
    pub servers: HashMap<u16, Arc<AtomicBool>>,
}

impl RuntimeState {
    pub fn new() -> Self {
        Self {
            active_coroutines: HashMap::new(),
            servers: HashMap::new(),
        }
    }

    pub fn stop_server(&mut self, port: u16) {
        if let Some(flag) = self.servers.remove(&port) {
            flag.store(true, Ordering::Relaxed);
        }
    }
}

pub fn execute_program(
    statements: Vec<Statement>,
    env: Arc<Mutex<Environment>>,
    state: &mut RuntimeState,
) -> Result<Option<RuntimeValue>, String> {
    let mut last_return = None;
    for stmt in statements {
        if let Some(val) = execute_statement(stmt, Arc::clone(&env), state)? {
            last_return = Some(val);
            break;
        }
    }
    Ok(last_return)
}

fn execute_statement(
    stmt: Statement,
    env: Arc<Mutex<Environment>>,
    state: &mut RuntimeState,
) -> Result<Option<RuntimeValue>, String> {
    match stmt {
        Statement::VarDeclaration {
            immutable: _,
            name,
            value,
        } => {
            let val = evaluate_expression(value, Arc::clone(&env), state)?;
            env.lock().unwrap().define(name, val)?;
            Ok(None)
        }
        Statement::FuncDeclaration {
            name,
            params,
            return_type,
            body,
        } => {
            let func = RuntimeValue::Function {
                params,
                body,
                return_type,
            };
            env.lock().unwrap().define(name, func)?;
            Ok(None)
        }
        Statement::IfStatement {
            condition,
            then_branch,
        } => {
            let cond_val = evaluate_expression(condition, Arc::clone(&env), state)?;
            if matches!(cond_val, RuntimeValue::Boolean(true)) {
                let local_env = Arc::new(Mutex::new(Environment::child(Arc::clone(&env))));
                return execute_program(then_branch, local_env, state);
            }
            Ok(None)
        }
        Statement::Expression(expr) => {
            evaluate_expression(expr, Arc::clone(&env), state)?;
            Ok(None)
        }
        Statement::Return(expr) => {
            let val = evaluate_expression(expr, Arc::clone(&env), state)?;
            Ok(Some(val))
        }
    }
}

fn evaluate_expression(
    expr: Expression,
    env: Arc<Mutex<Environment>>,
    state: &mut RuntimeState,
) -> Result<RuntimeValue, String> {
    match expr {
        Expression::Literal(val) => Ok(val),
        Expression::Variable(name) => Ok(env.lock().unwrap().get(&name)),
        Expression::BinaryOp { left, op, right } => {
            let l_val = evaluate_expression(*left, Arc::clone(&env), state)?;
            let r_val = evaluate_expression(*right, Arc::clone(&env), state)?;
            Ok(match op.as_str() {
                "!=" => RuntimeValue::Boolean(l_val != r_val),
                "==" => RuntimeValue::Boolean(l_val == r_val),
                _ => RuntimeValue::Null,
            })
        }
        Expression::FunctionCall { name, args } => evaluate_call(&name, &args, env, state),
    }
}

fn evaluate_call(
    name: &str,
    args: &[(Option<String>, Expression)],
    env: Arc<Mutex<Environment>>,
    state: &mut RuntimeState,
) -> Result<RuntimeValue, String> {
    let callee = env.lock().unwrap().get(name);
    if let RuntimeValue::Function {
        params,
        body,
        return_type: _,
    } = callee
    {
        return call_user_function(params, body, args, env, state);
    }

    match name {
        "time_out" => {
            let time_ms = extract_arg(args, "time", Arc::clone(&env), state)?;
            if let RuntimeValue::Number(ms) = time_ms {
                thread::sleep(Duration::from_millis(ms as u64));
            }
            Ok(RuntimeValue::Null)
        }
        "sleep" => {
            let time_ms = extract_arg(args, "time", Arc::clone(&env), state)?;
            if let RuntimeValue::Number(ms) = time_ms {
                thread::spawn(move || {
                    thread::sleep(Duration::from_millis(ms as u64));
                });
            }
            Ok(RuntimeValue::Null)
        }
        "start_server" => {
            let port_val = extract_arg(args, "port", Arc::clone(&env), state)?;
            let file_val = extract_arg(args, "file_descriptor", Arc::clone(&env), state)?;
            let timeout_val = extract_arg(args, "time_out", Arc::clone(&env), state)?;

            let port = runtime_number(port_val, "port")? as u16;
            let file_path = runtime_string(file_val, "file_descriptor")?;

            state.stop_server(port);
            thread::sleep(Duration::from_millis(50));

            let addr: SocketAddr = format!("127.0.0.1:{}", port)
                .parse()
                .map_err(|e| format!("Invalid port {}: {}", port, e))?;
            let listener = bind_listener(addr).map_err(|e| format_bind_error(port, &e))?;

            let shutdown = Arc::new(AtomicBool::new(false));
            state.servers.insert(port, Arc::clone(&shutdown));

            let timeout_ms = match timeout_val {
                RuntimeValue::Number(ms) => Some(ms as u64),
                _ => None,
            };

            thread::spawn(move || run_http_server(listener, port, file_path, shutdown, timeout_ms));

            Ok(RuntimeValue::Null)
        }
        "end_server" => {
            let port_val = extract_arg(args, "port", Arc::clone(&env), state)?;
            let port = runtime_number(port_val, "port")? as u16;
            state.stop_server(port);
            Ok(RuntimeValue::Null)
        }
        "start_coroutine" => {
            let name_val = extract_arg(args, "routine_name", Arc::clone(&env), state)?;
            let func_val = extract_arg(args, "func", Arc::clone(&env), state)?;
            let routine_name = runtime_string(name_val, "routine_name")?;

            if let RuntimeValue::Function { params, body, .. } = func_val {
                let child = Arc::new(Mutex::new(Environment::child(Arc::clone(&env))));
                let handle = thread::spawn(move || {
                    let mut local_state = RuntimeState::new();
                    let _ = call_user_function(params, body, &[], child, &mut local_state);
                });
                state.active_coroutines.insert(routine_name, handle);
            }
            Ok(RuntimeValue::Null)
        }
        "end_coroutine" => {
            let name_val = extract_arg(args, "routine_name", Arc::clone(&env), state)?;
            let routine_name = runtime_string(name_val, "routine_name")?;
            if let Some(handle) = state.active_coroutines.remove(&routine_name) {
                let _ = handle.join();
            }
            Ok(RuntimeValue::Null)
        }
        tag => Ok(html_tag_macro(tag, args, env, state)?),
    }
}

fn call_user_function(
    params: Vec<String>,
    body: Vec<Statement>,
    args: &[(Option<String>, Expression)],
    env: Arc<Mutex<Environment>>,
    state: &mut RuntimeState,
) -> Result<RuntimeValue, String> {
    let call_env = Arc::new(Mutex::new(Environment::child(Arc::clone(&env))));

    if !args.is_empty() && args.iter().all(|(label, _)| label.is_some()) {
        for (label, expr) in args {
            let label = label.clone().unwrap();
            let val = evaluate_expression(expr.clone(), Arc::clone(&env), state)?;
            call_env.lock().unwrap().define(label, val)?;
        }
    } else {
        for (i, (_, expr)) in args.iter().enumerate() {
            if i >= params.len() {
                break;
            }
            let val = evaluate_expression(expr.clone(), Arc::clone(&env), state)?;
            call_env.lock().unwrap().define(params[i].clone(), val)?;
        }
    }

    let result = execute_program(body, call_env, state)?;
    Ok(result.unwrap_or(RuntimeValue::Null))
}

fn html_tag_macro(
    tag: &str,
    args: &[(Option<String>, Expression)],
    env: Arc<Mutex<Environment>>,
    state: &mut RuntimeState,
) -> Result<RuntimeValue, String> {
    if args.is_empty() {
        return Ok(RuntimeValue::Script(format!("<{}></{}>", tag, tag)));
    }
    let inner = evaluate_expression(args[0].1.clone(), env, state)?;
    let text = match inner {
        RuntimeValue::String(s) => s,
        RuntimeValue::Script(s) => s,
        other => other.to_display_string(),
    };
    Ok(RuntimeValue::Script(format!("<{}>{}</{}>", tag, text, tag)))
}

fn extract_arg(
    args: &[(Option<String>, Expression)],
    label: &str,
    env: Arc<Mutex<Environment>>,
    state: &mut RuntimeState,
) -> Result<RuntimeValue, String> {
    for (opt_label, expr) in args {
        if let Some(l) = opt_label {
            if l == label {
                return evaluate_expression(expr.clone(), env, state);
            }
        }
    }
    Ok(RuntimeValue::Null)
}

fn runtime_number(val: RuntimeValue, label: &str) -> Result<f64, String> {
    match val {
        RuntimeValue::Number(n) => Ok(n),
        _ => Err(format!("Expected number for '{}'", label)),
    }
}

fn runtime_string(val: RuntimeValue, label: &str) -> Result<String, String> {
    match val {
        RuntimeValue::String(s) => Ok(s),
        _ => Err(format!("Expected string for '{}'", label)),
    }
}

fn bind_listener(addr: SocketAddr) -> std::io::Result<TcpListener> {
    let domain = if addr.is_ipv6() {
        Domain::IPV6
    } else {
        Domain::IPV4
    };
    let socket = Socket::new(domain, Type::STREAM, None)?;
    socket.set_reuse_address(true)?;
    socket.bind(&addr.into())?;
    socket.listen(128)?;
    Ok(socket.into())
}

fn format_bind_error(port: u16, err: &std::io::Error) -> String {
    if err.kind() == std::io::ErrorKind::AddrInUse {
        format!(
            "Port {} is already in use. Stop the other process (often a previous `cargo run`) or use another port.\n  Windows: netstat -ano | findstr :{}",
            port, port
        )
    } else {
        format!("Could not bind 127.0.0.1:{}: {}", port, err)
    }
}

fn run_http_server(
    listener: TcpListener,
    port: u16,
    file_path: String,
    shutdown: Arc<AtomicBool>,
    timeout_ms: Option<u64>,
) {
    let addr = format!("127.0.0.1:{}", port);

    if let Err(e) = listener.set_nonblocking(true) {
        eprintln!("UTA Error: could not set nonblocking listener: {}", e);
        shutdown.store(true, Ordering::Relaxed);
        return;
    }

    println!("UTA server listening on http://{}", addr);

    if let Some(ms) = timeout_ms {
        let shutdown_timeout = Arc::clone(&shutdown);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(ms));
            shutdown_timeout.store(true, Ordering::Relaxed);
            println!("UTA server on port {} timed out after {}ms", port, ms);
        });
    }

    while !shutdown.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let path = file_path.clone();
                thread::spawn(move || serve_http(stream, &path));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(e) => {
                eprintln!("UTA server accept error: {}", e);
                break;
            }
        }
    }

    println!("UTA server on port {} stopped", port);
}

fn serve_http(mut stream: TcpStream, file_path: &str) {
    let content = fs::read_to_string(file_path).unwrap_or_else(|_| {
        format!("<html><body><p>UTA: could not read '{}'</p></body></html>", file_path)
    });

    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        content.len(),
        content
    );

    let _ = stream.write_all(response.as_bytes());
}

impl RuntimeValue {
    fn to_display_string(&self) -> String {
        match self {
            RuntimeValue::Number(n) => n.to_string(),
            RuntimeValue::String(s) => s.clone(),
            RuntimeValue::Boolean(b) => b.to_string(),
            RuntimeValue::Script(s) => s.clone(),
            RuntimeValue::Null => String::new(),
            RuntimeValue::Function { .. } => "<function>".to_string(),
        }
    }
}
