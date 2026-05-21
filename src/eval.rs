use crate::ast::{Expression, RuntimeValue, Statement};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
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
        self.try_get(name).unwrap_or(RuntimeValue::Null)
    }

    pub fn try_get(&self, name: &str) -> Option<RuntimeValue> {
        if let Some(val) = self.variables.get(name) {
            return Some(val.clone());
        }
        if let Some(ref parent) = self.parent {
            return parent.lock().unwrap().try_get(name);
        }
        None
    }
}

pub struct RuntimeState {
    pub active_coroutines: HashMap<String, thread::JoinHandle<()>>,
    pub servers: HashMap<u16, Arc<AtomicBool>>,
    pub script_dir: std::path::PathBuf,
}

impl RuntimeState {
    pub fn new(script_dir: std::path::PathBuf) -> Self {
        Self {
            active_coroutines: HashMap::new(),
            servers: HashMap::new(),
            script_dir,
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
            let val = evaluate_expression(expr, Arc::clone(&env), state)?;
            log_script_result(&val);
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
        Expression::Variable(name) => env
            .lock()
            .unwrap()
            .try_get(&name)
            .ok_or_else(|| format!("Undefined variable '{}'", name)),
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

fn is_builtin(name: &str) -> bool {
    matches!(
        name,
        "time_out" | "sleep" | "start_server" | "end_server" | "start_coroutine" | "end_coroutine"
    )
}

fn evaluate_call(
    name: &str,
    args: &[(Option<String>, Expression)],
    env: Arc<Mutex<Environment>>,
    state: &mut RuntimeState,
) -> Result<RuntimeValue, String> {
    if !is_builtin(name) {
        let callee = env.lock().unwrap().get(name);
        if let RuntimeValue::Function {
            params,
            body,
            return_type,
        } = callee
        {
            return call_user_function(params, body, return_type, args, env, state);
        }
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
            let timeout_val = extract_optional_arg(args, "time_out", Arc::clone(&env), state)?;

            let port = runtime_number(port_val, "port")? as u16;
            let file_descriptor = runtime_string(file_val, "file_descriptor")?;
            let file_path = resolve_script_path(&state.script_dir, file_descriptor.clone());

            if !std::path::Path::new(&file_path).is_file() {
                return Err(format!(
                    "file_descriptor '{}' not found (resolved to '{}')",
                    file_descriptor, file_path
                ));
            }

            state.stop_server(port);
            thread::sleep(Duration::from_millis(50));

            let addr: SocketAddr = format!("127.0.0.1:{}", port)
                .parse()
                .map_err(|e| format!("Invalid port {}: {}", port, e))?;
            let listener = bind_listener(addr).map_err(|e| format_bind_error(port, &e))?;

            let shutdown = Arc::new(AtomicBool::new(false));
            state.servers.insert(port, Arc::clone(&shutdown));

            let timeout_ms = match timeout_val {
                RuntimeValue::Number(ms) if ms > 0.0 => Some(ms as u64),
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

            let RuntimeValue::Function {
                params,
                body,
                return_type,
            } = func_val
            else {
                return Err(
                    "start_coroutine requires func: to be a function (use let() do ... end)"
                        .to_string(),
                );
            };
            let child = Arc::new(Mutex::new(Environment::child(Arc::clone(&env))));
            let script_dir = state.script_dir.clone();
            let handle = thread::spawn(move || {
                let mut local_state = RuntimeState::new(script_dir);
                let _ = call_user_function(params, body, return_type, &[], child, &mut local_state);
            });
            state.active_coroutines.insert(routine_name, handle);
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
    return_type: Option<String>,
    args: &[(Option<String>, Expression)],
    env: Arc<Mutex<Environment>>,
    state: &mut RuntimeState,
) -> Result<RuntimeValue, String> {
    let call_env = Arc::new(Mutex::new(Environment::child(Arc::clone(&env))));
    bind_call_arguments(&params, args, Arc::clone(&env), Arc::clone(&call_env), state)?;

    let result = execute_program(body, call_env, state)?;
    let value = result.unwrap_or(RuntimeValue::Null);
    validate_return_type(return_type.as_deref(), &value)?;
    Ok(value)
}

fn bind_call_arguments(
    params: &[String],
    args: &[(Option<String>, Expression)],
    caller_env: Arc<Mutex<Environment>>,
    call_env: Arc<Mutex<Environment>>,
    state: &mut RuntimeState,
) -> Result<(), String> {
    if args.is_empty() {
        return Ok(());
    }

    if args.iter().all(|(label, _)| label.is_some()) {
        for (label, expr) in args {
            let label = label.clone().unwrap();
            if !params.contains(&label) {
                return Err(format!("Unknown argument '{}' for function", label));
            }
            let val = evaluate_expression(expr.clone(), Arc::clone(&caller_env), state)?;
            call_env.lock().unwrap().define(label, val)?;
        }
        return Ok(());
    }

    for (i, (_, expr)) in args.iter().enumerate() {
        if i >= params.len() {
            return Err(format!(
                "Too many arguments: expected {}, got {}",
                params.len(),
                args.len()
            ));
        }
        let val = evaluate_expression(expr.clone(), Arc::clone(&caller_env), state)?;
        call_env.lock().unwrap().define(params[i].clone(), val)?;
    }
    Ok(())
}

fn validate_return_type(return_type: Option<&str>, value: &RuntimeValue) -> Result<(), String> {
    if let Some("script") = return_type {
        if !matches!(value, RuntimeValue::Script(_)) {
            return Err(format!(
                "Function declared -> script must return script HTML, got {}",
                value_type_name(value)
            ));
        }
    }
    Ok(())
}

fn log_script_result(val: &RuntimeValue) {
    if let RuntimeValue::Script(html) = val {
        println!("UTA script output:\n{}", html);
    }
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
    Err(format!("Missing required argument '{}'", label))
}

fn extract_optional_arg(
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
        other => Err(format!(
            "Expected number for '{}', got {}",
            label,
            value_type_name(&other)
        )),
    }
}

fn runtime_string(val: RuntimeValue, label: &str) -> Result<String, String> {
    match val {
        RuntimeValue::String(s) => Ok(s),
        other => Err(format!(
            "Expected string for '{}', got {}",
            label,
            value_type_name(&other)
        )),
    }
}

fn resolve_script_path(script_dir: &std::path::Path, path: String) -> String {
    let candidate = std::path::Path::new(&path);
    let resolved = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        script_dir.join(candidate)
    };
    resolved
        .canonicalize()
        .unwrap_or(resolved)
        .to_string_lossy()
        .into_owned()
}

fn value_type_name(val: &RuntimeValue) -> &'static str {
    match val {
        RuntimeValue::Number(_) => "number",
        RuntimeValue::String(_) => "string",
        RuntimeValue::Boolean(_) => "boolean",
        RuntimeValue::Script(_) => "script",
        RuntimeValue::Function { .. } => "function",
        RuntimeValue::Null => "null/undefined",
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

    let display_path = file_path.strip_prefix(r"\\?\").unwrap_or(&file_path);
    println!("UTA server listening on http://{}", addr);
    println!("  Serving: {}", display_path);
    println!("  Open in browser: http://{}", addr);

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
    let _ = drain_http_request(&mut stream);

    let body = fs::read_to_string(file_path).unwrap_or_else(|e| {
        format!(
            "<html><body><h1>UTA Error</h1><p>Could not read '{}': {}</p></body></html>",
            file_path, e
        )
    });
    let body_bytes = body.as_bytes();

    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body_bytes.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body_bytes);
    let _ = stream.flush();
}

fn drain_http_request(stream: &mut TcpStream) -> std::io::Result<()> {
    let mut buf = [0u8; 1024];
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    stream.set_read_timeout(None)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Expression, Statement};
    use crate::parser::Parser;

    fn env_with_script2_vars() -> (Arc<Mutex<Environment>>, RuntimeState) {
        let src = include_str!("../script2.web");
        let program = Parser::parse(src).unwrap();
        let declarations: Vec<Statement> = program
            .into_iter()
            .filter(|s| matches!(s, Statement::VarDeclaration { .. }))
            .collect();
        let env = Arc::new(Mutex::new(Environment::new()));
        let mut state = RuntimeState::new(std::env::current_dir().unwrap());
        execute_program(declarations, env.clone(), &mut state).unwrap();
        (env, state)
    }

    #[test]
    fn script2_const_variables_are_defined() {
        let (env, _) = env_with_script2_vars();
        let e = env.lock().unwrap();
        assert_eq!(e.get("port"), RuntimeValue::Number(8080.0));
        assert_eq!(
            e.get("file_descriptor"),
            RuntimeValue::String("index.html".into())
        );
        assert_eq!(e.get("is_project"), RuntimeValue::Boolean(false));
    }

    #[test]
    fn user_script_function_returns_html() {
        let src = r#"
let page() -> script do
    return h1("hi");
end;
page();
"#;
        let program = Parser::parse(src).unwrap();
        let env = Arc::new(Mutex::new(Environment::new()));
        let mut state = RuntimeState::new(std::env::current_dir().unwrap());
        execute_program(program, env, &mut state).unwrap();
    }

    #[test]
    fn user_function_can_be_called() {
        let src = r#"
let ping() do
end;
ping();
"#;
        let program = Parser::parse(src).unwrap();
        let env = Arc::new(Mutex::new(Environment::new()));
        let mut state = RuntimeState::new(std::env::current_dir().unwrap());
        execute_program(program, env, &mut state).unwrap();
    }

    #[test]
    fn script2_resolves_variables_in_server_call() {
        let (env, mut state) = env_with_script2_vars();
        let args = vec![
            (
                Some("port".into()),
                Expression::Variable("port".into()),
            ),
            (
                Some("file_descriptor".into()),
                Expression::Variable("file_descriptor".into()),
            ),
            (
                Some("is_project".into()),
                Expression::Variable("is_project".into()),
            ),
        ];
        let port_val = extract_arg(&args, "port", env.clone(), &mut state).unwrap();
        let page_val = extract_arg(&args, "file_descriptor", env, &mut state).unwrap();
        assert_eq!(port_val, RuntimeValue::Number(8080.0));
        assert_eq!(page_val, RuntimeValue::String("index.html".into()));
    }
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
