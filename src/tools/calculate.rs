use std::collections::HashMap;
use std::f64::consts::{E, PI};
use serde_json::{json, Value};

const MAX_EXPRESSION_CHARS: usize = 4096;
const MAX_AST_NODES: usize = 512;
const MAX_AST_DEPTH: usize = 64;
const MAX_EXPONENT: f64 = 1000.0;
const MAX_ABS_NUMBER: f64 = 1e100;

#[derive(Debug, PartialEq, Clone)]
enum Token {
    Number(f64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Power,
    LParen,
    RParen,
    Comma,
}

fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }

        if c.is_ascii_digit() || (c == '.' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit()) {
            let start = i;
            let mut has_dot = false;
            let mut has_exp = false;
            while i < chars.len() {
                let cur = chars[i];
                if cur.is_ascii_digit() {
                    i += 1;
                } else if cur == '.' && !has_dot && !has_exp {
                    has_dot = true;
                    i += 1;
                } else if (cur == 'e' || cur == 'E') && !has_exp {
                    has_exp = true;
                    i += 1;
                    if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
                        i += 1;
                    }
                } else {
                    break;
                }
            }
            let s: String = chars[start..i].iter().collect();
            let num: f64 = s.parse().map_err(|_| "Invalid expression".to_string())?;
            tokens.push(Token::Number(num));
            continue;
        }

        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let s: String = chars[start..i].iter().collect();
            tokens.push(Token::Ident(s));
            continue;
        }

        match c {
            '+' => tokens.push(Token::Plus),
            '-' => tokens.push(Token::Minus),
            '*' => {
                if i + 1 < chars.len() && chars[i + 1] == '*' {
                    i += 1;
                    tokens.push(Token::Power);
                } else {
                    tokens.push(Token::Star);
                }
            }
            '/' => tokens.push(Token::Slash),
            '%' => tokens.push(Token::Percent),
            '^' => tokens.push(Token::Power),
            '(' => tokens.push(Token::LParen),
            ')' => tokens.push(Token::RParen),
            ',' => tokens.push(Token::Comma),
            _ => return Err("Invalid expression".to_string()),
        }
        i += 1;
    }

    Ok(tokens)
}

#[derive(Debug)]
enum AstNode {
    Number(f64),
    Variable(String),
    UnaryOp {
        op: Token,
        expr: Box<AstNode>,
    },
    BinaryOp {
        op: Token,
        left: Box<AstNode>,
        right: Box<AstNode>,
    },
    FunctionCall {
        name: String,
        args: Vec<AstNode>,
    },
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Token> {
        if self.pos < self.tokens.len() {
            let t = self.tokens[self.pos].clone();
            self.pos += 1;
            Some(t)
        } else {
            None
        }
    }

    fn parse_expression(&mut self) -> Result<AstNode, String> {
        self.parse_additive()
    }

    fn parse_additive(&mut self) -> Result<AstNode, String> {
        let mut left = self.parse_multiplicative()?;
        while let Some(op) = self.peek() {
            if *op == Token::Plus || *op == Token::Minus {
                let token_op = self.next().unwrap();
                let right = self.parse_multiplicative()?;
                left = AstNode::BinaryOp {
                    op: token_op,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<AstNode, String> {
        let mut left = self.parse_power()?;
        while let Some(op) = self.peek() {
            if *op == Token::Star || *op == Token::Slash || *op == Token::Percent {
                let token_op = self.next().unwrap();
                let right = self.parse_power()?;
                left = AstNode::BinaryOp {
                    op: token_op,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_power(&mut self) -> Result<AstNode, String> {
        let left = self.parse_unary()?;
        if let Some(Token::Power) = self.peek() {
            let token_op = self.next().unwrap();
            let right = self.parse_power()?; // Right-associative
            return Ok(AstNode::BinaryOp {
                op: token_op,
                left: Box::new(left),
                right: Box::new(right),
            });
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<AstNode, String> {
        if let Some(op) = self.peek() {
            if *op == Token::Plus || *op == Token::Minus {
                let token_op = self.next().unwrap();
                let expr = self.parse_unary()?;
                return Ok(AstNode::UnaryOp {
                    op: token_op,
                    expr: Box::new(expr),
                });
            }
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<AstNode, String> {
        match self.next() {
            Some(Token::Number(n)) => Ok(AstNode::Number(n)),
            Some(Token::Ident(name)) => {
                if let Some(Token::LParen) = self.peek() {
                    self.next(); // Consume '('
                    let mut args = Vec::new();
                    if let Some(Token::RParen) = self.peek() {
                        self.next();
                    } else {
                        loop {
                            let arg = self.parse_expression()?;
                            args.push(arg);
                            match self.peek() {
                                Some(Token::Comma) => {
                                    self.next();
                                }
                                Some(Token::RParen) => {
                                    self.next();
                                    break;
                                }
                                _ => return Err("Invalid expression".to_string()),
                            }
                        }
                    }
                    Ok(AstNode::FunctionCall { name, args })
                } else {
                    Ok(AstNode::Variable(name))
                }
            }
            Some(Token::LParen) => {
                let expr = self.parse_expression()?;
                if let Some(Token::RParen) = self.next() {
                    Ok(expr)
                } else {
                    Err("Invalid expression".to_string())
                }
            }
            _ => Err("Invalid expression".to_string()),
        }
    }
}

fn validate_ast(node: &AstNode, count: &mut usize, depth: usize) -> Result<(), String> {
    *count += 1;
    if *count > MAX_AST_NODES {
        return Err("Expression is too complex".to_string());
    }
    if depth > MAX_AST_DEPTH {
        return Err("Expression is nested too deeply".to_string());
    }

    match node {
        AstNode::Number(n) => ensure_finite_number(*n).map(|_| ()),
        AstNode::Variable(_) => Ok(()),
        AstNode::UnaryOp { expr, .. } => validate_ast(expr, count, depth + 1),
        AstNode::BinaryOp { left, right, .. } => {
            validate_ast(left, count, depth + 1)?;
            validate_ast(right, count, depth + 1)
        }
        AstNode::FunctionCall { args, .. } => {
            for arg in args {
                validate_ast(arg, count, depth + 1)?;
            }
            Ok(())
        }
    }
}

fn ensure_finite_number(val: f64) -> Result<f64, String> {
    if !val.is_finite() {
        return Err("Non-finite numbers are not allowed".to_string());
    }
    if val.abs() > MAX_ABS_NUMBER {
        return Err("Numeric magnitude is too large".to_string());
    }
    Ok(val)
}

fn eval_node(node: &AstNode) -> Result<f64, String> {
    match node {
        AstNode::Number(n) => ensure_finite_number(*n),
        AstNode::Variable(name) => match name.as_str() {
            "pi" | "PI" => Ok(PI),
            "e" | "E" => Ok(E),
            _ => Err(format!("Unknown name: {}", name)),
        },
        AstNode::UnaryOp { op, expr } => {
            let val = eval_node(expr)?;
            match op {
                Token::Plus => ensure_finite_number(val),
                Token::Minus => ensure_finite_number(-val),
                _ => Err("Unsupported operator".to_string()),
            }
        }
        AstNode::BinaryOp { op, left, right } => {
            let l = eval_node(left)?;
            let r = eval_node(right)?;
            match op {
                Token::Plus => ensure_finite_number(l + r),
                Token::Minus => ensure_finite_number(l - r),
                Token::Star => ensure_finite_number(l * r),
                Token::Slash => {
                    if r == 0.0 {
                        return Err("Division by zero".to_string());
                    }
                    ensure_finite_number(l / r)
                }
                Token::Percent => {
                    if r == 0.0 {
                        return Err("Division by zero".to_string());
                    }
                    ensure_finite_number(l % r)
                }
                Token::Power => {
                    if r.abs() > MAX_EXPONENT {
                        return Err("Exponent is too large".to_string());
                    }
                    if l.abs() > 1.0 && r > 0.0 {
                        let estimated = r * l.abs().log10();
                        if estimated > MAX_ABS_NUMBER.log10() {
                            return Err("Power result is too large".to_string());
                        }
                    }
                    if l < 0.0 && r.fract() != 0.0 {
                        return Err(
                            "Result is a complex number; only real numbers are supported".to_string(),
                        );
                    }
                    let res = l.powf(r);
                    ensure_finite_number(res)
                }
                _ => Err("Unsupported operator".to_string()),
            }
        }
        AstNode::FunctionCall { name, args } => {
            let fn_name = name.to_lowercase();
            match fn_name.as_str() {
                "sqrt" => {
                    if args.len() != 1 {
                        return Err("sqrt expects 1 arguments".to_string());
                    }
                    let v = eval_node(&args[0])?;
                    if v < 0.0 {
                        return Err("Invalid input for sqrt".to_string());
                    }
                    ensure_finite_number(v.sqrt())
                }
                "sin" => {
                    if args.len() != 1 {
                        return Err("sin expects 1 arguments".to_string());
                    }
                    let v = eval_node(&args[0])?;
                    ensure_finite_number(v.sin())
                }
                "cos" => {
                    if args.len() != 1 {
                        return Err("cos expects 1 arguments".to_string());
                    }
                    let v = eval_node(&args[0])?;
                    ensure_finite_number(v.cos())
                }
                "tan" => {
                    if args.len() != 1 {
                        return Err("tan expects 1 arguments".to_string());
                    }
                    let v = eval_node(&args[0])?;
                    ensure_finite_number(v.tan())
                }
                "log" => {
                    if args.is_empty() || args.len() > 2 {
                        return Err("log expects 1 to 2 arguments".to_string());
                    }
                    let v = eval_node(&args[0])?;
                    if v <= 0.0 {
                        return Err("Invalid input for log".to_string());
                    }
                    if args.len() == 2 {
                        let base = eval_node(&args[1])?;
                        if base <= 0.0 || base == 1.0 {
                            return Err("Invalid input for log".to_string());
                        }
                        ensure_finite_number(v.log(base))
                    } else {
                        ensure_finite_number(v.ln())
                    }
                }
                "log10" => {
                    if args.len() != 1 {
                        return Err("log10 expects 1 arguments".to_string());
                    }
                    let v = eval_node(&args[0])?;
                    if v <= 0.0 {
                        return Err("Invalid input for log10".to_string());
                    }
                    ensure_finite_number(v.log10())
                }
                "log2" => {
                    if args.len() != 1 {
                        return Err("log2 expects 1 arguments".to_string());
                    }
                    let v = eval_node(&args[0])?;
                    if v <= 0.0 {
                        return Err("Invalid input for log2".to_string());
                    }
                    ensure_finite_number(v.log2())
                }
                "ceil" => {
                    if args.len() != 1 {
                        return Err("ceil expects 1 arguments".to_string());
                    }
                    let v = eval_node(&args[0])?;
                    ensure_finite_number(v.ceil())
                }
                "floor" => {
                    if args.len() != 1 {
                        return Err("floor expects 1 arguments".to_string());
                    }
                    let v = eval_node(&args[0])?;
                    ensure_finite_number(v.floor())
                }
                "round" => {
                    if args.is_empty() || args.len() > 2 {
                        return Err("round expects 1 to 2 arguments".to_string());
                    }
                    let v = eval_node(&args[0])?;
                    if args.len() == 2 {
                        let digits = eval_node(&args[1])?;
                        let d = digits.round() as i32;
                        let factor = 10.0_f64.powi(d);
                        ensure_finite_number((v * factor).round() / factor)
                    } else {
                        ensure_finite_number(v.round())
                    }
                }
                "abs" => {
                    if args.len() != 1 {
                        return Err("abs expects 1 arguments".to_string());
                    }
                    let v = eval_node(&args[0])?;
                    ensure_finite_number(v.abs())
                }
                _ => Err(format!("Unknown function: {}", name)),
            }
        }
    }
}

pub fn calculate_expression(expression: &str) -> Result<Value, String> {
    let expr = expression.trim();
    if expr.is_empty() || expr.len() > MAX_EXPRESSION_CHARS {
        return Err("Expression must be between 1 and 200 characters".to_string());
    }

    let tokens = tokenize(expr)?;
    if tokens.is_empty() {
        return Err("Invalid expression".to_string());
    }

    let mut parser = Parser::new(tokens);
    let ast = parser.parse_expression()?;
    if parser.pos < parser.tokens.len() {
        return Err("Invalid expression".to_string());
    }

    let mut node_count = 0;
    validate_ast(&ast, &mut node_count, 1)?;

    let result = eval_node(&ast)?;
    let final_res = ensure_finite_number(result)?;

    let res_json = if final_res.fract() == 0.0 && final_res.abs() < 1e15 {
        json!(final_res as i64)
    } else {
        json!(final_res)
    };

    Ok(json!({
        "expression": expression,
        "result": res_json
    }))
}

pub fn calculate(args: &HashMap<String, Value>) -> Result<Value, String> {
    let expr = match args.get("expression") {
        Some(Value::String(s)) => s.as_str(),
        _ => return Err("expression must be a string".to_string()),
    };
    calculate_expression(expr)
}
