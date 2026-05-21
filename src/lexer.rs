#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    KwLet,
    KwConst,
    KwDo,
    KwEnd,
    KwIf,
    KwReturn,
    KwTrue,
    KwFalse,
    Identifier(String),
    Number(f64),
    String(String),
    Symbol(char),
    Arrow,
    EqEq,
    NotEq,
    Eof,
}

pub struct Lexer {
    input: Vec<char>,
    pos: usize,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Self {
            input: input.chars().collect(),
            pos: 0,
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, String> {
        let mut tokens = Vec::new();
        loop {
            let token = self.next_token()?;
            let is_eof = matches!(token, Token::Eof);
            tokens.push(token);
            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }

    fn next_token(&mut self) -> Result<Token, String> {
        self.skip_whitespace_and_comments();

        if self.pos >= self.input.len() {
            return Ok(Token::Eof);
        }

        let ch = self.input[self.pos];

        if ch.is_ascii_digit() {
            return Ok(self.read_number());
        }

        if ch == '"' {
            return self.read_string();
        }

        if ch.is_ascii_alphabetic() || ch == '_' {
            return Ok(self.read_identifier_or_keyword());
        }

        if ch == '-' && self.peek_char(1) == '>' {
            self.pos += 2;
            return Ok(Token::Arrow);
        }

        if ch == '=' && self.peek_char(1) == '=' {
            self.pos += 2;
            return Ok(Token::EqEq);
        }

        if ch == '!' && self.peek_char(1) == '=' {
            self.pos += 2;
            return Ok(Token::NotEq);
        }

        if "=;,():<>+-*/".contains(ch) {
            self.pos += 1;
            return Ok(Token::Symbol(ch));
        }

        Err(format!("Unexpected character '{}' at position {}", ch, self.pos))
    }

    fn read_number(&mut self) -> Token {
        let start = self.pos;
        while self.pos < self.input.len() && self.is_number_char(self.input[self.pos]) {
            self.pos += 1;
        }
        let text: String = self.input[start..self.pos].iter().collect();
        let value = text.parse::<f64>().unwrap_or(0.0);
        Token::Number(value)
    }

    fn read_string(&mut self) -> Result<Token, String> {
        self.pos += 1; // opening quote
        let mut value = String::new();
        while self.pos < self.input.len() {
            let ch = self.input[self.pos];
            if ch == '"' {
                self.pos += 1;
                return Ok(Token::String(value));
            }
            if ch == '\\' && self.pos + 1 < self.input.len() {
                self.pos += 1;
                value.push(self.input[self.pos]);
                self.pos += 1;
                continue;
            }
            value.push(ch);
            self.pos += 1;
        }
        Err("Unterminated string literal".to_string())
    }

    fn read_identifier_or_keyword(&mut self) -> Token {
        let start = self.pos;
        while self.pos < self.input.len() {
            let ch = self.input[self.pos];
            if ch.is_ascii_alphanumeric() || ch == '_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        let text: String = self.input[start..self.pos].iter().collect();
        match text.as_str() {
            "let" => Token::KwLet,
            "const" => Token::KwConst,
            "do" => Token::KwDo,
            "end" => Token::KwEnd,
            "if" => Token::KwIf,
            "return" => Token::KwReturn,
            "true" | "True" => Token::KwTrue,
            "false" | "False" => Token::KwFalse,
            _ => Token::Identifier(text),
        }
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            while self.pos < self.input.len() && self.input[self.pos].is_whitespace() {
                self.pos += 1;
            }
            if self.pos + 1 < self.input.len()
                && self.input[self.pos] == '/'
                && self.input[self.pos + 1] == '/'
            {
                self.pos += 2;
                while self.pos < self.input.len() && self.input[self.pos] != '\n' {
                    self.pos += 1;
                }
                continue;
            }
            break;
        }
    }

    fn peek_char(&self, offset: usize) -> char {
        let idx = self.pos + offset;
        if idx < self.input.len() {
            self.input[idx]
        } else {
            '\0'
        }
    }

    fn is_number_char(&self, ch: char) -> bool {
        ch.is_ascii_digit() || ch == '.'
    }
}
