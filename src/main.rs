use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

// ==========================================
// 1. DATA TYPES & RUNTIME VALUES
// ==========================================
#[derive(Debug, Clone, PartialEq)]
enum RuntimeValue {
    Number(f64),             // Supports signed, unsigned, and floats
    String(String),          // Text streams
    Boolean(bool),           // True or False
    Script(String),          // Raw HTML code snippets
    Function {               // User-defined functions
        params: Vec<String>,
        body: Vec<Statement>,
        return_type: Option<String>,
    },
    Null,
}

// ==========================================
// 2. ABSTRACT SYNTAX TREE (AST) DEFINITION
// ==========================================
#[derive(Debug, Clone)]
enum Expression {
    Literal(RuntimeValue),
    Variable(String),
    BinaryOp {
        left: Box<Expression>,
        op: String, // "==", "!=", "+", etc.
        right: Box<Expression>,
    },
    FunctionCall {
        name: String,
        args: Vec<(Option<String>, Expression)>, // Optional named arguments like port: 8080
    },
}

#[derive(Debug, Clone)]
enum Statement {
    VarDeclaration {
        name: String,
        value: Expression,
    },
    FuncDeclaration {
        name: String,
        params: Vec<String>,
        return_type: Option<String>,
        body: Vec<Statement>,
    },
    IfStatement {
        condition: Expression,
        then_branch: Vec<Statement>,
    },
    Expression(Expression),
    Return(Expression),
}

// ==========================================
// 3. RUNTIME ENVIRONMENT (MEMORY & TRACKING)
// ==========================================
struct Environment {
    variables: HashMap<String, RuntimeValue>,
    parent: Option<Arc<Mutex<Environment>>>,
}

impl Environment {
    fn new() -> Self {
        Self { variables: HashMap::new(), parent: None }
    }

    fn define(&mut self, name: String, value: RuntimeValue) {
        // Since variables are immutable by default, we just insert.
        self.variables.insert(name, value);
    }

    fn get(&self, name: &str) -> RuntimeValue {
        if let Some(val) = self.variables.get(name) {
            return val.clone();
        }
        if let Some(ref parent) = self.parent {
            return parent.lock().unwrap().get(name);
        }
        RuntimeValue::Null
    }
}

// Global active threads registry for coroutines and background servers
struct RuntimeState {
    active_threads: HashMap<String, thread::JoinHandle<()>>,
}

// ==========================================
// 4. MAIN PROGRAM ENTRY
// ==========================================
fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: cargo run -- <filename>.web");
        return;
    }

    let file_path = &args[1];
    let path = Path::new(file_path);

    if path.extension().and_then(|s| s.to_str()) != Some("web") {
        eprintln!("UTA Error: Extention must strictly be '.web'");
        return;
    }

    let content = fs::read_to_string(path).expect("Failed to read file");

    println!("--- Initializing UTA Engine ---");
    
    // Global environment
    let env = Arc::new(Mutex::new(Environment::new()));
    let mut state = RuntimeState { active_threads: HashMap::new() };

    // Hardcoding a mock AST representing your program flow to show execution
    let program = mock_parser_ast();

    execute_program(program, env, &mut state);
}

// ==========================================
// 5. EVALUATOR & BUILT-IN EXECUTION ENGINE
// ==========================================
fn execute_program(statements: Vec<Statement>, env: Arc<Mutex<Environment>>, state: &mut RuntimeState) {
    for stmt in statements {
        match stmt {
            Statement::VarDeclaration { name, value } => {
                let val = evaluate_expression(value, Arc::clone(&env));
                env.lock().unwrap().define(name, val);
            }
            Statement::IfStatement { condition, then_branch } => {
                let cond_val = evaluate_expression(condition, Arc::clone(&env));
                if let RuntimeValue::Boolean(true) = cond_val {
                    // Create an explicit local block scope for conditional execution
                    let local_env = Arc::new(Mutex::new(Environment {
                        variables: HashMap::new(),
                        parent: Some(Arc::clone(&env)),
                    }));
                    execute_program(then_branch, local_env, state);
                }
            }
            Statement::Expression(expr) => {
                evaluate_expression(expr, Arc::clone(&env));
            }
            _ => {} // Remaining implementation blocks handled as the syntax lexer expands
        }
    }
}

fn evaluate_expression(expr: Expression, env: Arc<Mutex<Environment>>) -> RuntimeValue {
    match expr {
        Expression::Literal(val) => val,
        Expression::Variable(name) => env.lock().unwrap().get(&name),
        Expression::BinaryOp { left, op, right } => {
            let l_val = evaluate_expression(*left, Arc::clone(&env));
            let r_val = evaluate_expression(*right, Arc::clone(&env));
            
            match op.as_str() {
                "!=" => RuntimeValue::Boolean(l_val != r_val),
                "==" => RuntimeValue::Boolean(l_val == r_val),
                _ => RuntimeValue::Null
            }
        }
        Expression::FunctionCall { name, args } => {
            // Intercept built-in environment functions
            match name.as_str() {
                "time_out" => {
                    let time_ms = extract_arg(&args, "time", Arc::clone(&env));
                    if let RuntimeValue::Number(ms) = time_ms {
                        println!("⏳ [Main Thread] Blocking for {}ms...", ms);
                        thread::sleep(Duration::from_millis(ms as u64));
                    }
                    RuntimeValue::Null
                }
                "sleep" => {
                    let time_ms = extract_arg(&args, "time", Arc::clone(&env));
                    if let RuntimeValue::Number(ms) = time_ms {
                        println!("💤 [Async Thread] Spawning non-blocking sleep for {}ms...", ms);
                        thread::spawn(move || {
                            thread::sleep(Duration::from_millis(ms as u64));
                            println!("💤 [Async Thread] Sleep finished.");
                        });
                    }
                    RuntimeValue::Null
                }
                "h1" => {
                    // Custom macro tag generation engine for type: script
                    let inner = evaluate_expression(args[0].1.clone(), Arc::clone(&env));
                    if let RuntimeValue::String(text) = inner {
                        RuntimeValue::Script(format!("<h1>{}</h1>", text))
                    } else {
                        RuntimeValue::Null
                    }
                }
                _ => RuntimeValue::Null,
            }
        }
    }
}

fn extract_arg(args: &[(Option<String>, Expression)], label: &str, env: Arc<Mutex<Environment>>) -> RuntimeValue {
    for (opt_label, expr) in args {
        if let Some(l) = opt_label {
            if l == label {
                return evaluate_expression(expr.clone(), env);
            }
        }
    }
    RuntimeValue::Null
}

// ==========================================
// 6. SYNTAX VERIFICATION AND MOCK AST GENERATOR
// ==========================================
fn mock_parser_ast() -> Vec<Statement> {
    vec![
        // 1. const x = 10;
        Statement::VarDeclaration {
            name: "x".to_string(),
            value: Expression::Literal(RuntimeValue::Number(10.0)),
        },
        // 2. if x != 10 do ... end;
        Statement::IfStatement {
            condition: Expression::BinaryOp {
                left: Box::new(Expression::Variable("x".to_string())),
                op: "!=".to_string(),
                right: Box::new(Expression::Literal(RuntimeValue::Number(10.0))),
            },
            then_branch: vec![
                Statement::Expression(Expression::FunctionCall {
                    name: "time_out".to_string(),
                    args: vec![(Some("time".to_string()), Expression::Literal(RuntimeValue::Number(500.0)))],
                })
            ],
        },
        // 3. Non-blocking fallback tracking test (sleep(time: 1000))
        Statement::Expression(Expression::FunctionCall {
            name: "sleep".to_string(),
            args: vec![(Some("time".to_string()), Expression::Literal(RuntimeValue::Number(1000.0)))],
        }),
    ]
}