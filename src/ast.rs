#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeValue {
    Number(f64),
    String(String),
    Boolean(bool),
    Script(String),
    Function {
        params: Vec<String>,
        body: Vec<Statement>,
        return_type: Option<String>,
    },
    Null,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    Literal(RuntimeValue),
    Variable(String),
    BinaryOp {
        left: Box<Expression>,
        op: String,
        right: Box<Expression>,
    },
    FunctionCall {
        name: String,
        args: Vec<(Option<String>, Expression)>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    VarDeclaration {
        immutable: bool,
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
