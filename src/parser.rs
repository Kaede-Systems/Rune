use crate::lexer::{Span, Token, TokenKind, lex};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Import(ImportDecl),
    Exception(ExceptionDecl),
    Struct(StructDecl),
    Function(Function),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExceptionDecl {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDecl {
    pub level: usize,
    pub module: Vec<String>,
    pub names: Option<Vec<String>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructDecl {
    pub name: String,
    pub fields: Vec<StructField>,
    pub methods: Vec<Function>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructField {
    pub name: String,
    pub ty: TypeRef,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    pub is_extern: bool,
    pub is_async: bool,
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<TypeRef>,
    pub raises: Option<TypeRef>,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    pub ty: TypeRef,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeRef {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub statements: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    Let(LetStmt),
    Assign(AssignStmt),
    Return(ReturnStmt),
    If(IfStmt),
    While(WhileStmt),
    Raise(RaiseStmt),
    Panic(PanicStmt),
    Expr(ExprStmt),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LetStmt {
    pub name: String,
    pub ty: Option<TypeRef>,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignStmt {
    pub name: String,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnStmt {
    pub value: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfStmt {
    pub condition: Expr,
    pub then_block: Block,
    pub elif_blocks: Vec<ElifBlock>,
    pub else_block: Option<Block>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElifBlock {
    pub condition: Expr,
    pub block: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhileStmt {
    pub condition: Expr,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RaiseStmt {
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanicStmt {
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExprStmt {
    pub expr: Expr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExprKind {
    Identifier(String),
    Integer(String),
    String(String),
    Bool(bool),
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<CallArg>,
    },
    Await {
        expr: Box<Expr>,
    },
    Field {
        base: Box<Expr>,
        name: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Negate,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    And,
    Add,
    Or,
    Modulo,
    Subtract,
    Multiply,
    Divide,
    EqualEqual,
    NotEqual,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallArg {
    Positional(Expr),
    Keyword {
        name: String,
        value: Expr,
        span: Span,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at line {}, column {}",
            self.message, self.span.line, self.span.column
        )
    }
}

impl std::error::Error for ParseError {}

pub fn parse_source(source: &str) -> Result<Program, ParseError> {
    let tokens = lex(source).map_err(|error| ParseError {
        message: error.message,
        span: error.span,
    })?;
    parse_tokens(tokens)
}

pub fn parse_tokens(tokens: Vec<Token>) -> Result<Program, ParseError> {
    Parser::new(tokens).parse_program()
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, index: 0 }
    }

    fn parse_program(&mut self) -> Result<Program, ParseError> {
        let mut items = Vec::new();
        self.skip_newlines();

        while !self.at_end() {
            items.push(self.parse_item()?);
            self.skip_newlines();
        }

        Ok(Program { items })
    }

    fn parse_item(&mut self) -> Result<Item, ParseError> {
        match self.peek().kind {
            TokenKind::Import | TokenKind::From => Ok(Item::Import(self.parse_import()?)),
            TokenKind::Exception => Ok(Item::Exception(self.parse_exception()?)),
            TokenKind::Struct | TokenKind::Class => Ok(Item::Struct(self.parse_struct()?)),
            _ => Ok(Item::Function(self.parse_function()?)),
        }
    }

    fn parse_struct(&mut self) -> Result<StructDecl, ParseError> {
        let span = if self.match_simple(&TokenKind::Struct) || self.match_simple(&TokenKind::Class)
        {
            self.previous().span
        } else {
            return Err(ParseError {
                message: "expected `struct` or `class`".to_string(),
                span: self.peek().span,
            });
        };
        let (name, _) = self.expect_identifier("expected class name")?;
        self.expect_simple(TokenKind::Colon, "expected `:` after class name")?;
        self.expect_simple(
            TokenKind::Newline,
            "expected newline after class declaration",
        )?;
        self.expect_simple(TokenKind::Indent, "expected indented class body")?;

        let mut fields = Vec::new();
        let mut methods = Vec::new();
        while !self.check(&TokenKind::Dedent) && !self.at_end() {
            if matches!(
                self.peek().kind,
                TokenKind::Def | TokenKind::Async | TokenKind::Extern
            ) {
                methods.push(self.parse_function()?);
            } else {
                let (field_name, field_span) =
                    self.expect_identifier("expected class field name")?;
                self.expect_simple(TokenKind::Colon, "expected `:` after field name")?;
                let ty = self.parse_type()?;
                self.expect_simple(TokenKind::Newline, "expected newline after class field")?;
                fields.push(StructField {
                    name: field_name,
                    ty,
                    span: field_span,
                });
            }
            self.skip_newlines();
        }

        self.expect_simple(TokenKind::Dedent, "expected end of class body")?;
        Ok(StructDecl {
            name,
            fields,
            methods,
            span,
        })
    }

    fn parse_exception(&mut self) -> Result<ExceptionDecl, ParseError> {
        let span = self.expect_simple(TokenKind::Exception, "expected `exception`")?;
        let (name, _) = self.expect_identifier("expected exception name")?;
        self.expect_simple(
            TokenKind::Newline,
            "expected newline after exception declaration",
        )?;
        Ok(ExceptionDecl { name, span })
    }

    fn parse_import(&mut self) -> Result<ImportDecl, ParseError> {
        if self.match_simple(&TokenKind::Import) {
            let span = self.previous().span;
            let (level, module) = self.parse_module_path_with_level()?;
            self.expect_simple(TokenKind::Newline, "expected newline after import")?;
            return Ok(ImportDecl {
                level,
                module,
                names: None,
                span,
            });
        }

        let span = self.expect_simple(TokenKind::From, "expected `from`")?;
        let (level, module) = self.parse_module_path_with_level()?;
        self.expect_simple(TokenKind::Import, "expected `import` after module path")?;
        let mut names = Vec::new();
        let wrapped = self.match_simple(&TokenKind::LParen);
        if wrapped {
            self.skip_parenthesized_import_layout();
        }
        loop {
            let (name, _) = self.expect_identifier("expected imported name")?;
            names.push(name);
            if !self.match_simple(&TokenKind::Comma) {
                break;
            }
            if wrapped {
                self.skip_parenthesized_import_layout();
                if self.check(&TokenKind::RParen) {
                    break;
                }
            }
        }
        if wrapped {
            self.skip_parenthesized_import_layout();
            self.expect_simple(TokenKind::RParen, "expected `)` after imported names")?;
        }
        self.expect_simple(TokenKind::Newline, "expected newline after import")?;
        Ok(ImportDecl {
            level,
            module,
            names: Some(names),
            span,
        })
    }

    fn parse_module_path_with_level(&mut self) -> Result<(usize, Vec<String>), ParseError> {
        let mut level = 0usize;
        while self.match_simple(&TokenKind::Dot) {
            level += 1;
        }

        let mut module = Vec::new();
        if matches!(self.peek().kind, TokenKind::Identifier(_)) {
            let (first, _) = self.expect_identifier("expected module name")?;
            module.push(first);
            while self.match_simple(&TokenKind::Dot) {
                let (segment, _) = self.expect_identifier("expected module segment after `.`")?;
                module.push(segment);
            }
        }

        if level == 0 && module.is_empty() {
            return Err(ParseError {
                message: "expected module path".to_string(),
                span: self.peek().span,
            });
        }

        Ok((level, module))
    }

    fn parse_function(&mut self) -> Result<Function, ParseError> {
        let start = self.peek().span;
        let is_extern = self.match_simple(&TokenKind::Extern);
        let is_async = self.match_simple(&TokenKind::Async);
        self.expect_simple(TokenKind::Def, "expected `def` or `async def`")?;
        let (name, _) = self.expect_identifier("expected function name")?;
        self.expect_simple(TokenKind::LParen, "expected `(` after function name")?;

        let mut params = Vec::new();
        if !self.check(&TokenKind::RParen) {
            loop {
                let param_span = self.peek().span;
                let (param_name, _) = self.expect_identifier("expected parameter name")?;
                let ty = if self.match_simple(&TokenKind::Colon) {
                    self.parse_type()?
                } else {
                    TypeRef {
                        name: "dynamic".to_string(),
                        span: param_span,
                    }
                };
                params.push(Param {
                    name: param_name,
                    ty,
                    span: param_span,
                });

                if !self.match_simple(&TokenKind::Comma) {
                    break;
                }
            }
        }

        self.expect_simple(TokenKind::RParen, "expected `)` after parameter list")?;

        let return_type = if self.match_simple(&TokenKind::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };

        let raises = if self.match_simple(&TokenKind::Raises) {
            Some(self.parse_type()?)
        } else {
            None
        };

        let body = if is_extern {
            self.expect_simple(
                TokenKind::Newline,
                "expected newline after extern function signature",
            )?;
            Block {
                statements: Vec::new(),
            }
        } else {
            self.expect_simple(TokenKind::Colon, "expected `:` after function signature")?;
            self.expect_simple(
                TokenKind::Newline,
                "expected newline after function signature",
            )?;
            self.parse_block()?
        };

        Ok(Function {
            is_extern,
            is_async,
            name,
            params,
            return_type,
            raises,
            body,
            span: start,
        })
    }

    fn parse_block(&mut self) -> Result<Block, ParseError> {
        self.expect_simple(TokenKind::Indent, "expected indented block")?;
        let mut statements = Vec::new();
        self.skip_newlines();

        while !self.check(&TokenKind::Dedent) && !self.at_end() {
            statements.push(self.parse_stmt()?);
            self.skip_newlines();
        }

        self.expect_simple(TokenKind::Dedent, "expected end of block")?;
        Ok(Block { statements })
    }

    fn parse_stmt(&mut self) -> Result<Stmt, ParseError> {
        match self.peek().kind.clone() {
            TokenKind::Let => Ok(Stmt::Let(self.parse_let_stmt()?)),
            TokenKind::Identifier(_) if self.peek_is_identifier_eq() => {
                Ok(Stmt::Assign(self.parse_assign_stmt()?))
            }
            TokenKind::Return => Ok(Stmt::Return(self.parse_return_stmt()?)),
            TokenKind::If => Ok(Stmt::If(self.parse_if_stmt()?)),
            TokenKind::While => Ok(Stmt::While(self.parse_while_stmt()?)),
            TokenKind::Raise => Ok(Stmt::Raise(self.parse_raise_stmt()?)),
            TokenKind::Panic => Ok(Stmt::Panic(self.parse_panic_stmt()?)),
            _ => Ok(Stmt::Expr(self.parse_expr_stmt()?)),
        }
    }

    fn parse_let_stmt(&mut self) -> Result<LetStmt, ParseError> {
        let span = self.expect_simple(TokenKind::Let, "expected `let`")?;
        let (name, _) = self.expect_identifier("expected variable name")?;
        let ty = if self.match_simple(&TokenKind::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect_simple(TokenKind::Equal, "expected `=` in let statement")?;
        let value = self.parse_expr()?;
        self.expect_simple(TokenKind::Newline, "expected newline after let statement")?;
        Ok(LetStmt {
            name,
            ty,
            value,
            span,
        })
    }

    fn parse_assign_stmt(&mut self) -> Result<AssignStmt, ParseError> {
        let span = self.peek().span;
        let (name, _) = self.expect_identifier("expected assignment target")?;
        self.expect_simple(TokenKind::Equal, "expected `=` in assignment")?;
        let value = self.parse_expr()?;
        self.expect_simple(TokenKind::Newline, "expected newline after assignment")?;
        Ok(AssignStmt { name, value, span })
    }

    fn parse_return_stmt(&mut self) -> Result<ReturnStmt, ParseError> {
        let span = self.expect_simple(TokenKind::Return, "expected `return`")?;
        let value = if self.check(&TokenKind::Newline) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        self.expect_simple(TokenKind::Newline, "expected newline after return")?;
        Ok(ReturnStmt { value, span })
    }

    fn parse_if_stmt(&mut self) -> Result<IfStmt, ParseError> {
        let span = self.expect_simple(TokenKind::If, "expected `if`")?;
        let condition = self.parse_expr()?;
        self.expect_simple(TokenKind::Colon, "expected `:` after if condition")?;
        self.expect_simple(TokenKind::Newline, "expected newline after if condition")?;
        let then_block = self.parse_block()?;

        let mut elif_blocks = Vec::new();
        while self.match_simple(&TokenKind::Elif) {
            let elif_span = self.previous().span;
            let condition = self.parse_expr()?;
            self.expect_simple(TokenKind::Colon, "expected `:` after elif condition")?;
            self.expect_simple(TokenKind::Newline, "expected newline after elif condition")?;
            let block = self.parse_block()?;
            elif_blocks.push(ElifBlock {
                condition,
                block,
                span: elif_span,
            });
        }

        let else_block = if self.match_simple(&TokenKind::Else) {
            self.expect_simple(TokenKind::Colon, "expected `:` after else")?;
            self.expect_simple(TokenKind::Newline, "expected newline after else")?;
            Some(self.parse_block()?)
        } else {
            None
        };

        Ok(IfStmt {
            condition,
            then_block,
            elif_blocks,
            else_block,
            span,
        })
    }

    fn parse_while_stmt(&mut self) -> Result<WhileStmt, ParseError> {
        let span = self.expect_simple(TokenKind::While, "expected `while`")?;
        let condition = self.parse_expr()?;
        self.expect_simple(TokenKind::Colon, "expected `:` after while condition")?;
        self.expect_simple(TokenKind::Newline, "expected newline after while condition")?;
        let body = self.parse_block()?;
        Ok(WhileStmt {
            condition,
            body,
            span,
        })
    }

    fn parse_raise_stmt(&mut self) -> Result<RaiseStmt, ParseError> {
        let span = self.expect_simple(TokenKind::Raise, "expected `raise`")?;
        let value = self.parse_expr()?;
        self.expect_simple(TokenKind::Newline, "expected newline after raise")?;
        Ok(RaiseStmt { value, span })
    }

    fn parse_panic_stmt(&mut self) -> Result<PanicStmt, ParseError> {
        let span = self.expect_simple(TokenKind::Panic, "expected `panic`")?;
        let value = self.parse_expr()?;
        self.expect_simple(TokenKind::Newline, "expected newline after panic")?;
        Ok(PanicStmt { value, span })
    }

    fn parse_expr_stmt(&mut self) -> Result<ExprStmt, ParseError> {
        let expr = self.parse_expr()?;
        self.expect_simple(TokenKind::Newline, "expected newline after expression")?;
        Ok(ExprStmt { expr })
    }

    fn parse_type(&mut self) -> Result<TypeRef, ParseError> {
        let span = self.peek().span;
        let (first, _) = self.expect_identifier("expected type name")?;
        let mut name = first;

        while self.match_simple(&TokenKind::Dot) {
            let (segment, _) = self.expect_identifier("expected type segment after `.`")?;
            name.push('.');
            name.push_str(&segment);
        }

        Ok(TypeRef { name, span })
    }

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_and()?;

        while self.match_simple(&TokenKind::Or) {
            let span = self.previous().span;
            let right = self.parse_and()?;
            expr = Expr {
                kind: ExprKind::Binary {
                    left: Box::new(expr),
                    op: BinaryOp::Or,
                    right: Box::new(right),
                },
                span,
            };
        }

        Ok(expr)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_comparison()?;

        while self.match_simple(&TokenKind::And) {
            let span = self.previous().span;
            let right = self.parse_comparison()?;
            expr = Expr {
                kind: ExprKind::Binary {
                    left: Box::new(expr),
                    op: BinaryOp::And,
                    right: Box::new(right),
                },
                span,
            };
        }

        Ok(expr)
    }

    fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_additive()?;

        loop {
            let op = match self.peek().kind {
                TokenKind::EqualEqual => BinaryOp::EqualEqual,
                TokenKind::NotEqual => BinaryOp::NotEqual,
                TokenKind::Greater => BinaryOp::Greater,
                TokenKind::GreaterEqual => BinaryOp::GreaterEqual,
                TokenKind::Less => BinaryOp::Less,
                TokenKind::LessEqual => BinaryOp::LessEqual,
                _ => break,
            };
            let span = self.advance().span;
            let right = self.parse_additive()?;
            expr = Expr {
                kind: ExprKind::Binary {
                    left: Box::new(expr),
                    op,
                    right: Box::new(right),
                },
                span,
            };
        }

        Ok(expr)
    }

    fn parse_additive(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_multiplicative()?;

        loop {
            let op = match self.peek().kind {
                TokenKind::Plus => BinaryOp::Add,
                TokenKind::Minus => BinaryOp::Subtract,
                _ => break,
            };
            let span = self.advance().span;
            let right = self.parse_multiplicative()?;
            expr = Expr {
                kind: ExprKind::Binary {
                    left: Box::new(expr),
                    op,
                    right: Box::new(right),
                },
                span,
            };
        }

        Ok(expr)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_unary()?;

        loop {
            let op = match self.peek().kind {
                TokenKind::Percent => BinaryOp::Modulo,
                TokenKind::Star => BinaryOp::Multiply,
                TokenKind::Slash => BinaryOp::Divide,
                _ => break,
            };
            let span = self.advance().span;
            let right = self.parse_unary()?;
            expr = Expr {
                kind: ExprKind::Binary {
                    left: Box::new(expr),
                    op,
                    right: Box::new(right),
                },
                span,
            };
        }

        Ok(expr)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        if self.match_simple(&TokenKind::Await) {
            let span = self.previous().span;
            let expr = self.parse_unary()?;
            return Ok(Expr {
                kind: ExprKind::Await {
                    expr: Box::new(expr),
                },
                span,
            });
        }

        if self.match_simple(&TokenKind::Minus) {
            let span = self.previous().span;
            let expr = self.parse_unary()?;
            return Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::Negate,
                    expr: Box::new(expr),
                },
                span,
            });
        }

        if self.match_simple(&TokenKind::Not) {
            let span = self.previous().span;
            let expr = self.parse_unary()?;
            return Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(expr),
                },
                span,
            });
        }

        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_primary()?;

        loop {
            if self.match_simple(&TokenKind::LParen) {
                let mut args = Vec::new();
                if !self.check(&TokenKind::RParen) {
                    loop {
                        if self.peek_is_identifier_eq() {
                            let (name, span) =
                                self.expect_identifier("expected keyword argument name")?;
                            self.expect_simple(
                                TokenKind::Equal,
                                "expected `=` after keyword argument name",
                            )?;
                            let value = self.parse_expr()?;
                            args.push(CallArg::Keyword { name, value, span });
                        } else {
                            args.push(CallArg::Positional(self.parse_expr()?));
                        }
                        if !self.match_simple(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.expect_simple(TokenKind::RParen, "expected `)` after arguments")?;
                let span = expr.span;
                expr = Expr {
                    kind: ExprKind::Call {
                        callee: Box::new(expr),
                        args,
                    },
                    span,
                };
                continue;
            }

            if self.match_simple(&TokenKind::Dot) {
                let (name, span) =
                    self.expect_identifier("expected field or method name after `.`")?;
                expr = Expr {
                    kind: ExprKind::Field {
                        base: Box::new(expr),
                        name,
                    },
                    span,
                };
                continue;
            }

            break;
        }

        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let token = self.advance().clone();
        let kind = match token.kind {
            TokenKind::Identifier(name) => ExprKind::Identifier(name),
            TokenKind::Integer(value) => ExprKind::Integer(value),
            TokenKind::String(value) => ExprKind::String(value),
            TokenKind::True => ExprKind::Bool(true),
            TokenKind::False => ExprKind::Bool(false),
            TokenKind::LParen => {
                let expr = self.parse_expr()?;
                self.expect_simple(TokenKind::RParen, "expected `)` after expression")?;
                return Ok(expr);
            }
            other => {
                return Err(ParseError {
                    message: format!("expected expression, found {other:?}"),
                    span: token.span,
                });
            }
        };

        Ok(Expr {
            kind,
            span: token.span,
        })
    }

    fn expect_identifier(&mut self, message: &str) -> Result<(String, Span), ParseError> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::Identifier(name) => Ok((name, token.span)),
            other => Err(ParseError {
                message: format!("{message}, found {other:?}"),
                span: token.span,
            }),
        }
    }

    fn expect_simple(&mut self, expected: TokenKind, message: &str) -> Result<Span, ParseError> {
        let token = self.advance().clone();
        if token.kind == expected {
            Ok(token.span)
        } else {
            Err(ParseError {
                message: format!("{message}, found {:?}", token.kind),
                span: token.span,
            })
        }
    }

    fn match_simple(&mut self, expected: &TokenKind) -> bool {
        if self.check(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn check(&self, expected: &TokenKind) -> bool {
        !self.at_end() && self.peek().kind == *expected
    }

    fn skip_newlines(&mut self) {
        while self.match_simple(&TokenKind::Newline) {}
    }

    fn skip_parenthesized_import_layout(&mut self) {
        while matches!(
            self.peek().kind,
            TokenKind::Newline | TokenKind::Indent | TokenKind::Dedent
        ) {
            self.advance();
        }
    }

    fn peek_is_identifier_eq(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Identifier(_))
            && self
                .tokens
                .get(self.index + 1)
                .is_some_and(|token| token.kind == TokenKind::Equal)
    }

    fn advance(&mut self) -> &Token {
        if !self.at_end() {
            self.index += 1;
        }
        self.previous()
    }

    fn at_end(&self) -> bool {
        self.peek().kind == TokenKind::Eof
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.index]
    }

    fn previous(&self) -> &Token {
        &self.tokens[self.index - 1]
    }
}
