use crate::ast::{Expression, RuntimeValue, Statement};
use crate::lexer::{Token, Lexer};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn parse(source: &str) -> Result<Vec<Statement>, String> {
        let mut lexer = Lexer::new(source);
        let tokens = lexer.tokenize()?;
        let mut parser = Parser { tokens, pos: 0 };
        parser.parse_program()
    }

    fn parse_program(&mut self) -> Result<Vec<Statement>, String> {
        let mut statements = Vec::new();
        while !self.is_at_end() {
            statements.push(self.parse_statement()?);
        }
        Ok(statements)
    }

    fn parse_statement(&mut self) -> Result<Statement, String> {
        match self.current() {
            Token::KwConst => self.parse_var_declaration(true),
            Token::KwLet => {
                if self.peek_is_identifier() && self.peek_ahead_is_symbol('(') {
                    self.parse_func_declaration()
                } else {
                    self.parse_var_declaration(false)
                }
            }
            Token::KwIf => self.parse_if_statement(),
            Token::KwReturn => self.parse_return_statement(),
            _ => {
                let expr = self.parse_expression()?;
                self.expect_symbol(';')?;
                Ok(Statement::Expression(expr))
            }
        }
    }

    fn parse_var_declaration(&mut self, immutable: bool) -> Result<Statement, String> {
        self.advance(); // const | let
        let name = self.expect_identifier()?;
        self.expect_symbol('=')?;
        let value = self.parse_expression()?;
        self.expect_symbol(';')?;
        Ok(Statement::VarDeclaration {
            immutable,
            name,
            value,
        })
    }

    fn parse_func_declaration(&mut self) -> Result<Statement, String> {
        self.advance(); // let
        let name = self.expect_identifier()?;
        self.expect_symbol('(')?;
        let params = self.parse_param_list()?;
        self.expect_symbol(')')?;

        let return_type = if self.check(&Token::Arrow) {
            self.advance();
            Some(self.expect_identifier()?)
        } else {
            None
        };

        self.expect_keyword(Token::KwDo)?;
        let body = self.parse_block()?;
        self.expect_keyword(Token::KwEnd)?;
        self.expect_symbol(';')?;

        Ok(Statement::FuncDeclaration {
            name,
            params,
            return_type,
            body,
        })
    }

    fn parse_if_statement(&mut self) -> Result<Statement, String> {
        self.advance(); // if
        let condition = self.parse_expression()?;
        self.expect_keyword(Token::KwDo)?;
        let then_branch = self.parse_block()?;
        self.expect_keyword(Token::KwEnd)?;
        self.expect_symbol(';')?;
        Ok(Statement::IfStatement {
            condition,
            then_branch,
        })
    }

    fn parse_return_statement(&mut self) -> Result<Statement, String> {
        self.advance(); // return
        let value = self.parse_expression()?;
        self.expect_symbol(';')?;
        Ok(Statement::Return(value))
    }

    fn parse_block(&mut self) -> Result<Vec<Statement>, String> {
        let mut statements = Vec::new();
        while !self.check(&Token::KwEnd) && !self.is_at_end() {
            statements.push(self.parse_statement()?);
        }
        Ok(statements)
    }

    fn parse_param_list(&mut self) -> Result<Vec<String>, String> {
        if self.check(&Token::Symbol(')')) {
            return Ok(Vec::new());
        }
        let mut params = vec![self.expect_identifier()?];
        while self.match_symbol(',') {
            params.push(self.expect_identifier()?);
        }
        Ok(params)
    }

    fn parse_expression(&mut self) -> Result<Expression, String> {
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Result<Expression, String> {
        let mut left = self.parse_primary()?;
        if self.check(&Token::EqEq) || self.check(&Token::NotEq) {
            let op = if self.check(&Token::EqEq) {
                self.advance();
                "==".to_string()
            } else {
                self.advance();
                "!=".to_string()
            };
            let right = self.parse_primary()?;
            left = Expression::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_primary(&mut self) -> Result<Expression, String> {
        match self.current().clone() {
            Token::Number(n) => {
                self.advance();
                Ok(Expression::Literal(RuntimeValue::Number(n)))
            }
            Token::String(s) => {
                self.advance();
                Ok(Expression::Literal(RuntimeValue::String(s)))
            }
            Token::KwTrue => {
                self.advance();
                Ok(Expression::Literal(RuntimeValue::Boolean(true)))
            }
            Token::KwFalse => {
                self.advance();
                Ok(Expression::Literal(RuntimeValue::Boolean(false)))
            }
            Token::Identifier(name) => {
                self.advance();
                if self.check(&Token::Symbol('(')) {
                    self.parse_call(name)
                } else {
                    Ok(Expression::Variable(name))
                }
            }
            _ => Err(format!("Unexpected token in expression: {:?}", self.current())),
        }
    }

    fn parse_call(&mut self, name: String) -> Result<Expression, String> {
        self.expect_symbol('(')?;
        let args = self.parse_arg_list()?;
        self.expect_symbol(')')?;
        Ok(Expression::FunctionCall { name, args })
    }

    fn parse_arg_list(&mut self) -> Result<Vec<(Option<String>, Expression)>, String> {
        if self.check(&Token::Symbol(')')) {
            return Ok(Vec::new());
        }

        let mut args = Vec::new();
        loop {
            let (label, expr) = if self.is_identifier() && self.peek_next_is_symbol(':') {
                let label = self.expect_identifier()?;
                self.expect_symbol(':')?;
                (Some(label), self.parse_expression()?)
            } else {
                (None, self.parse_expression()?)
            };
            args.push((label, expr));

            if !self.match_symbol(',') {
                break;
            }
        }
        Ok(args)
    }

    fn expect_identifier(&mut self) -> Result<String, String> {
        if let Token::Identifier(name) = self.current().clone() {
            self.advance();
            Ok(name)
        } else {
            Err(format!("Expected identifier, found {:?}", self.current()))
        }
    }

    fn expect_symbol(&mut self, expected: char) -> Result<(), String> {
        if self.match_symbol(expected) {
            Ok(())
        } else {
            Err(format!("Expected symbol '{}', found {:?}", expected, self.current()))
        }
    }

    fn expect_keyword(&mut self, expected: Token) -> Result<(), String> {
        if self.check(&expected) {
            self.advance();
            Ok(())
        } else {
            Err(format!("Expected {:?}, found {:?}", expected, self.current()))
        }
    }

    fn match_symbol(&mut self, expected: char) -> bool {
        if self.check(&Token::Symbol(expected)) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn is_identifier(&self) -> bool {
        matches!(self.current(), Token::Identifier(_))
    }

    fn peek_is_identifier(&self) -> bool {
        matches!(self.tokens.get(self.pos + 1), Some(Token::Identifier(_)))
    }

    fn peek_ahead_is_symbol(&self, ch: char) -> bool {
        matches!(
            self.tokens.get(self.pos + 2),
            Some(Token::Symbol(c)) if *c == ch
        )
    }

    fn peek_next_is_symbol(&self, ch: char) -> bool {
        matches!(
            self.tokens.get(self.pos + 1),
            Some(Token::Symbol(c)) if *c == ch
        )
    }

    fn current(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn check(&self, token: &Token) -> bool {
        std::mem::discriminant(self.current()) == std::mem::discriminant(token)
            && self.current() == token
    }

    fn advance(&mut self) {
        if !self.is_at_end() {
            self.pos += 1;
        }
    }

    fn is_at_end(&self) -> bool {
        matches!(self.current(), Token::Eof)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    #[test]
    fn tokenize_start_server_call() {
        let src = r#"start_server(port: 8080, file_descriptor: "index.html", is_project: false);"#;
        let tokens = Lexer::new(src).tokenize().unwrap();
        assert!(matches!(tokens[0], Token::Identifier(_)));
        assert!(matches!(tokens[2], Token::Identifier(_)));
        assert!(matches!(tokens[3], Token::Symbol(':')));
    }

    #[test]
    fn parse_script2() {
        let src = include_str!("../script2.web");
        Parser::parse(src).expect("script2.web should parse");
    }

    #[test]
    fn parse_minimal_server_call() {
        let src = r#"start_server(port: 8080);"#;
        match Parser::parse(src) {
            Ok(_) => {}
            Err(e) => panic!("parse failed: {}", e),
        }
    }
}
