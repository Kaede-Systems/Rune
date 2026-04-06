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
    Block(BlockStmt),
    Let(LetStmt),
    Assign(AssignStmt),
    FieldAssign(FieldAssignStmt),
    Return(ReturnStmt),
    If(IfStmt),
    While(WhileStmt),
    Break(BreakStmt),
    Continue(ContinueStmt),
    Raise(RaiseStmt),
    Panic(PanicStmt),
    Expr(ExprStmt),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockStmt {
    pub block: Block,
    pub span: Span,
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

/// Assignment to a struct/class field: `base.field = value` or `base.a.b = value`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldAssignStmt {
    /// The root variable name (e.g. `"point"` in `point.x = 5`).
    pub base: String,
    /// Path of field names from the root (e.g. `["x"]` or `["inner", "y"]`).
    pub fields: Vec<String>,
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
pub struct BreakStmt {
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinueStmt {
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
    BitwiseNot,
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
    BitwiseAnd,
    BitwiseOr,
    BitwiseXor,
    ShiftLeft,
    ShiftRight,
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
    synthetic_counter: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            index: 0,
            synthetic_counter: 0,
        }
    }

    fn parse_program(&mut self) -> Result<Program, ParseError> {
        let mut items = Vec::new();
        let mut top_level_statements = Vec::new();
        self.skip_newlines();

        while !self.at_end() {
            if self.peek_starts_top_level_stmt() {
                top_level_statements.push(self.parse_stmt()?);
            } else {
                items.push(self.parse_item()?);
            }
            self.skip_newlines();
        }

        if !top_level_statements.is_empty() {
            if items
                .iter()
                .any(|item| matches!(item, Item::Function(function) if function.name == "main"))
            {
                return Err(ParseError {
                    message: "top-level statements cannot be combined with an explicit `main()`"
                        .to_string(),
                    span: top_level_statements[0].span(),
                });
            }

            let main_span = top_level_statements[0].span();
            if !matches!(top_level_statements.last(), Some(Stmt::Return(_))) {
                top_level_statements.push(Stmt::Return(ReturnStmt {
                    value: Some(Expr {
                        kind: ExprKind::Integer("0".to_string()),
                        span: main_span,
                    }),
                    span: main_span,
                }));
            }

            items.push(Item::Function(Function {
                is_extern: false,
                is_async: false,
                name: "main".to_string(),
                params: Vec::new(),
                return_type: Some(TypeRef {
                    name: "i32".to_string(),
                    span: main_span,
                }),
                raises: None,
                body: Block {
                    statements: top_level_statements,
                },
                span: main_span,
            }));
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
            TokenKind::For => self.parse_for_stmt(),
            TokenKind::Let => Ok(Stmt::Let(self.parse_let_stmt()?)),
            TokenKind::Assert => self.parse_assert_stmt(),
            TokenKind::Match => self.parse_match_stmt(),
            TokenKind::Identifier(_) if self.peek_is_ident_assignment() => {
                self.parse_ident_assignment_stmt()
            }
            TokenKind::Return => Ok(Stmt::Return(self.parse_return_stmt()?)),
            TokenKind::If => Ok(Stmt::If(self.parse_if_stmt()?)),
            TokenKind::While => Ok(Stmt::While(self.parse_while_stmt()?)),
            TokenKind::Break => Ok(Stmt::Break(self.parse_break_stmt()?)),
            TokenKind::Continue => Ok(Stmt::Continue(self.parse_continue_stmt()?)),
            TokenKind::Raise => Ok(Stmt::Raise(self.parse_raise_stmt()?)),
            TokenKind::Panic => Ok(Stmt::Panic(self.parse_panic_stmt()?)),
            _ => Ok(Stmt::Expr(self.parse_expr_stmt()?)),
        }
    }

    /// Returns true when the current token is an identifier followed by any assignment operator
    /// or a field-path assignment (`ident.field ... = value`).
    fn peek_is_ident_assignment(&self) -> bool {
        if !matches!(self.tokens[self.index].kind, TokenKind::Identifier(_)) {
            return false;
        }
        // Walk forward past dots and identifiers to find an assignment operator.
        let mut i = self.index + 1;
        loop {
            let kind = self.tokens.get(i).map(|t| &t.kind);
            match kind {
                Some(TokenKind::Equal) => return true,
                Some(
                    TokenKind::PlusEqual
                    | TokenKind::MinusEqual
                    | TokenKind::StarEqual
                    | TokenKind::SlashEqual
                    | TokenKind::PercentEqual
                    | TokenKind::AmpersandEqual
                    | TokenKind::PipeEqual
                    | TokenKind::CaretEqual,
                ) => return true,
                // Allow dots for field-path assignment
                Some(TokenKind::Dot) => {
                    i += 1;
                    // Must be followed by an identifier
                    match self.tokens.get(i).map(|t| &t.kind) {
                        Some(TokenKind::Identifier(_)) => {
                            i += 1;
                        }
                        _ => return false,
                    }
                }
                _ => return false,
            }
        }
    }

    fn parse_ident_assignment_stmt(&mut self) -> Result<Stmt, ParseError> {
        let span = self.peek().span;
        let (name, _) = self.expect_identifier("expected assignment target")?;

        // Collect optional field path: .field1.field2...
        let mut fields: Vec<String> = Vec::new();
        while self.check(&TokenKind::Dot) {
            self.advance();
            let (field, _) = self.expect_identifier("expected field name after `.`")?;
            fields.push(field);
        }

        // Determine the assignment operator
        let op_token = self.peek().kind.clone();
        self.advance();

        let rhs = self.parse_expr()?;
        self.expect_simple(TokenKind::Newline, "expected newline after assignment")?;

        // For augmented operators, build `name op rhs` (or `base.fields op rhs`)
        let value = match op_token {
            TokenKind::Equal => rhs,
            ref aug => {
                let bin_op = match aug {
                    TokenKind::PlusEqual => BinaryOp::Add,
                    TokenKind::MinusEqual => BinaryOp::Subtract,
                    TokenKind::StarEqual => BinaryOp::Multiply,
                    TokenKind::SlashEqual => BinaryOp::Divide,
                    TokenKind::PercentEqual => BinaryOp::Modulo,
                    TokenKind::AmpersandEqual => BinaryOp::BitwiseAnd,
                    TokenKind::PipeEqual => BinaryOp::BitwiseOr,
                    TokenKind::CaretEqual => BinaryOp::BitwiseXor,
                    _ => unreachable!("peek_is_ident_assignment only returns true for known ops"),
                };
                // Build the current-value expression (name or name.field.field...)
                let current = if fields.is_empty() {
                    ident_expr(&name, span)
                } else {
                    let mut base = ident_expr(&name, span);
                    for f in &fields {
                        base = Expr {
                            kind: ExprKind::Field {
                                base: Box::new(base),
                                name: f.clone(),
                            },
                            span,
                        };
                    }
                    base
                };
                binary_expr(current, bin_op, rhs, span)
            }
        };

        if fields.is_empty() {
            Ok(Stmt::Assign(AssignStmt { name, value, span }))
        } else {
            Ok(Stmt::FieldAssign(FieldAssignStmt { base: name, fields, value, span }))
        }
    }

    fn parse_assert_stmt(&mut self) -> Result<Stmt, ParseError> {
        let span = self.expect_simple(TokenKind::Assert, "expected `assert`")?;
        let condition = self.parse_expr()?;
        // Optional message: assert expr, "msg"
        let message = if self.check(&TokenKind::Comma) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };
        self.expect_simple(TokenKind::Newline, "expected newline after assert")?;

        // Desugar: if not condition: panic(message)
        let msg_expr = message.unwrap_or_else(|| string_expr("assertion failed", span));
        let negated = Expr {
            kind: ExprKind::Unary { op: UnaryOp::Not, expr: Box::new(condition) },
            span,
        };
        Ok(Stmt::If(IfStmt {
            condition: negated,
            then_block: Block {
                statements: vec![Stmt::Panic(PanicStmt { value: msg_expr, span })],
            },
            elif_blocks: Vec::new(),
            else_block: None,
            span,
        }))
    }

    fn parse_match_stmt(&mut self) -> Result<Stmt, ParseError> {
        let span = self.expect_simple(TokenKind::Match, "expected `match`")?;
        let value = self.parse_expr()?;
        self.expect_simple(TokenKind::Colon, "expected `:` after match expression")?;
        self.expect_simple(TokenKind::Newline, "expected newline after match expression")?;
        self.expect_simple(TokenKind::Indent, "expected indented match body")?;

        // Each arm: case <pattern>: NEWLINE INDENT <stmts> DEDENT
        struct Arm {
            pattern: Option<Expr>, // None means wildcard
            body: Block,
        }

        let mut arms: Vec<Arm> = Vec::new();

        while self.check(&TokenKind::Case) {
            self.advance(); // consume `case`
            let pattern: Option<Expr> = match self.peek().kind.clone() {
                TokenKind::Integer(ref raw) => {
                    let s = raw.clone();
                    let tok_span = self.peek().span;
                    self.advance();
                    Some(integer_expr(&s, tok_span))
                }
                TokenKind::Minus => {
                    // Negative integer literal
                    let tok_span = self.peek().span;
                    self.advance(); // consume `-`
                    match self.peek().kind.clone() {
                        TokenKind::Integer(ref raw) => {
                            let s = format!("-{}", raw);
                            self.advance();
                            Some(integer_expr(&s, tok_span))
                        }
                        _ => {
                            return Err(ParseError {
                                message: "expected integer literal after `-` in case pattern"
                                    .to_string(),
                                span: self.peek().span,
                            });
                        }
                    }
                }
                TokenKind::String(ref s) => {
                    let s = s.clone();
                    let tok_span = self.peek().span;
                    self.advance();
                    Some(string_expr(&s, tok_span))
                }
                TokenKind::Identifier(ref name) if name == "_" => {
                    self.advance(); // consume `_`
                    None // wildcard
                }
                _ => {
                    return Err(ParseError {
                        message: "expected integer literal, string literal, or `_` in case pattern"
                            .to_string(),
                        span: self.peek().span,
                    });
                }
            };

            self.expect_simple(TokenKind::Colon, "expected `:` after case pattern")?;
            self.expect_simple(TokenKind::Newline, "expected newline after case pattern")?;
            let body = self.parse_block()?;
            self.skip_newlines();

            arms.push(Arm { pattern, body });
        }

        self.expect_simple(TokenKind::Dedent, "expected end of match body")?;

        if arms.is_empty() {
            return Err(ParseError {
                message: "match statement must have at least one case arm".to_string(),
                span,
            });
        }

        // Desugar to if/elif/else:
        // First arm becomes `if value == pattern:`
        // Middle arms become elif blocks
        // Wildcard arm becomes else block
        // Split arms into non-wildcard and optional wildcard (must be last)
        let wildcard_pos = arms.iter().position(|a| a.pattern.is_none());
        if let Some(pos) = wildcard_pos {
            if pos != arms.len() - 1 {
                return Err(ParseError {
                    message: "wildcard `_` case must be the last arm in a match statement"
                        .to_string(),
                    span,
                });
            }
        }

        // Build equality condition: value == pattern
        // We need to compare `value` against each pattern — clone value expr for each arm.
        let mut arms_iter = arms.into_iter();
        let first_arm = arms_iter.next().expect("at least one arm checked above");

        let (first_condition, first_body, first_else) = if first_arm.pattern.is_none() {
            // Only arm is wildcard — degenerate: just an unconditional else block.
            // Represent as `if true:` with the body, no elif, no else.
            // Actually, the cleanest desugar: emit if true: <body>
            (
                Expr {
                    kind: ExprKind::Bool(true),
                    span,
                },
                first_arm.body,
                None::<Block>,
            )
        } else {
            let cond = binary_expr(
                value.clone(),
                BinaryOp::EqualEqual,
                first_arm.pattern.unwrap(),
                span,
            );
            (cond, first_arm.body, None)
        };

        let mut elif_blocks: Vec<ElifBlock> = Vec::new();
        let mut else_block: Option<Block> = first_else;

        for arm in arms_iter {
            match arm.pattern {
                None => {
                    // wildcard → else
                    else_block = Some(arm.body);
                }
                Some(pat) => {
                    let cond = binary_expr(value.clone(), BinaryOp::EqualEqual, pat, span);
                    elif_blocks.push(ElifBlock {
                        condition: cond,
                        block: arm.body,
                        span,
                    });
                }
            }
        }

        Ok(Stmt::If(IfStmt {
            condition: first_condition,
            then_block: first_body,
            elif_blocks,
            else_block,
            span,
        }))
    }

    fn parse_for_stmt(&mut self) -> Result<Stmt, ParseError> {
        let span = self.expect_simple(TokenKind::For, "expected `for`")?;
        let (name, _) = self.expect_identifier("expected loop variable name")?;
        self.expect_simple(TokenKind::In, "expected `in` after loop variable")?;
        let iterable = self.parse_expr()?;
        let range_args = self.normalize_range_call_expr(&iterable)?;
        self.expect_simple(TokenKind::Colon, "expected `:` after for loop iterable")?;
        self.expect_simple(TokenKind::Newline, "expected newline after for loop header")?;
        let user_body = self.parse_block()?;

        let start_name = self.next_synthetic_name("range_start");
        let stop_name = self.next_synthetic_name("range_stop");
        let step_name = self.next_synthetic_name("range_step");

        let [start_expr, stop_expr, step_expr] = range_args.as_slice() else {
            unreachable!("range normalization always produces three arguments");
        };

        let start_ident = ident_expr(&start_name, span);
        let stop_ident = ident_expr(&stop_name, span);
        let step_ident = ident_expr(&step_name, span);
        let loop_ident = ident_expr(&name, span);

        let mut while_body = user_body.statements;
        while_body.push(Stmt::Assign(AssignStmt {
            name: name.clone(),
            value: binary_expr(loop_ident.clone(), BinaryOp::Add, step_ident.clone(), span),
            span,
        }));

        Ok(Stmt::Block(BlockStmt {
            span,
            block: Block {
                statements: vec![
                    typed_let_stmt(&start_name, "i64", int_call_expr(start_expr.clone(), span), span),
                    typed_let_stmt(&stop_name, "i64", int_call_expr(stop_expr.clone(), span), span),
                    typed_let_stmt(&step_name, "i64", int_call_expr(step_expr.clone(), span), span),
                    Stmt::If(IfStmt {
                        condition: binary_expr(
                            step_ident.clone(),
                            BinaryOp::EqualEqual,
                            integer_expr("0", span),
                            span,
                        ),
                        then_block: Block {
                            statements: vec![Stmt::Panic(PanicStmt {
                                value: string_expr("range step cannot be 0", span),
                                span,
                            })],
                        },
                        elif_blocks: Vec::new(),
                        else_block: None,
                        span,
                    }),
                    typed_let_stmt(&name, "i64", start_ident.clone(), span),
                    Stmt::While(WhileStmt {
                        condition: binary_expr(
                            binary_expr(
                                binary_expr(
                                    step_ident.clone(),
                                    BinaryOp::Greater,
                                    integer_expr("0", span),
                                    span,
                                ),
                                BinaryOp::And,
                                binary_expr(
                                    loop_ident.clone(),
                                    BinaryOp::Less,
                                    stop_ident.clone(),
                                    span,
                                ),
                                span,
                            ),
                            BinaryOp::Or,
                            binary_expr(
                                binary_expr(
                                    step_ident.clone(),
                                    BinaryOp::Less,
                                    integer_expr("0", span),
                                    span,
                                ),
                                BinaryOp::And,
                                binary_expr(
                                    loop_ident.clone(),
                                    BinaryOp::Greater,
                                    stop_ident.clone(),
                                    span,
                                ),
                                span,
                            ),
                            span,
                        ),
                        body: Block {
                            statements: while_body,
                        },
                        span,
                    }),
                ],
            },
        }))
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

    fn parse_break_stmt(&mut self) -> Result<BreakStmt, ParseError> {
        let span = self.expect_simple(TokenKind::Break, "expected `break`")?;
        self.expect_simple(TokenKind::Newline, "expected newline after break")?;
        Ok(BreakStmt { span })
    }

    fn parse_continue_stmt(&mut self) -> Result<ContinueStmt, ParseError> {
        let span = self.expect_simple(TokenKind::Continue, "expected `continue`")?;
        self.expect_simple(TokenKind::Newline, "expected newline after continue")?;
        Ok(ContinueStmt { span })
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
        let mut expr = self.parse_bitwise_or()?;

        while self.match_simple(&TokenKind::And) {
            let span = self.previous().span;
            let right = self.parse_bitwise_or()?;
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

    fn parse_bitwise_or(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_bitwise_xor()?;
        while self.check(&TokenKind::Pipe) {
            let span = self.advance().span;
            let right = self.parse_bitwise_xor()?;
            expr = Expr {
                kind: ExprKind::Binary {
                    left: Box::new(expr),
                    op: BinaryOp::BitwiseOr,
                    right: Box::new(right),
                },
                span,
            };
        }
        Ok(expr)
    }

    fn parse_bitwise_xor(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_bitwise_and()?;
        while self.check(&TokenKind::Caret) {
            let span = self.advance().span;
            let right = self.parse_bitwise_and()?;
            expr = Expr {
                kind: ExprKind::Binary {
                    left: Box::new(expr),
                    op: BinaryOp::BitwiseXor,
                    right: Box::new(right),
                },
                span,
            };
        }
        Ok(expr)
    }

    fn parse_bitwise_and(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_shift()?;
        while self.check(&TokenKind::Ampersand) {
            let span = self.advance().span;
            let right = self.parse_shift()?;
            expr = Expr {
                kind: ExprKind::Binary {
                    left: Box::new(expr),
                    op: BinaryOp::BitwiseAnd,
                    right: Box::new(right),
                },
                span,
            };
        }
        Ok(expr)
    }

    fn parse_shift(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_comparison()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::ShiftLeft => BinaryOp::ShiftLeft,
                TokenKind::ShiftRight => BinaryOp::ShiftRight,
                _ => break,
            };
            let span = self.advance().span;
            let right = self.parse_comparison()?;
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

        if self.match_simple(&TokenKind::Tilde) {
            let span = self.previous().span;
            let expr = self.parse_unary()?;
            return Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::BitwiseNot,
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
                        if self.peek_is_keyword_arg() {
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
                expr = self.rewrite_special_call(expr)?;
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
            TokenKind::FString(value) => return parse_fstring_expr(&value, token.span),
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

    fn peek_starts_top_level_stmt(&self) -> bool {
        matches!(
            self.peek().kind,
            TokenKind::For
                |
            TokenKind::Let
                | TokenKind::Return
                | TokenKind::If
                | TokenKind::Match
                | TokenKind::While
                | TokenKind::Raise
                | TokenKind::Panic
                | TokenKind::Identifier(_)
                | TokenKind::Integer(_)
                | TokenKind::String(_)
                | TokenKind::FString(_)
                | TokenKind::True
                | TokenKind::False
                | TokenKind::LParen
                | TokenKind::Await
                | TokenKind::Minus
                | TokenKind::Not
        )
    }


    /// Returns true for `ident =` in a call argument list (keyword argument syntax).
    fn peek_is_keyword_arg(&self) -> bool {
        matches!(self.tokens[self.index].kind, TokenKind::Identifier(_))
            && self
                .tokens
                .get(self.index + 1)
                .is_some_and(|t| t.kind == TokenKind::Equal)
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

    fn next_synthetic_name(&mut self, prefix: &str) -> String {
        let value = format!("__rune_{prefix}_{}", self.synthetic_counter);
        self.synthetic_counter += 1;
        value
    }

    fn rewrite_special_call(&self, expr: Expr) -> Result<Expr, ParseError> {
        let ExprKind::Call { callee, args } = &expr.kind else {
            return Ok(expr);
        };
        let ExprKind::Identifier(name) = &callee.kind else {
            return Ok(expr);
        };
        if name != "sum" {
            return Ok(expr);
        }
        let [CallArg::Positional(range_expr)] = args.as_slice() else {
            return Ok(expr);
        };
        let normalized = self.normalize_range_call_expr(range_expr)?;
        Ok(Expr {
            kind: ExprKind::Call {
                callee: Box::new(ident_expr("__rune_builtin_sum_range", expr.span)),
                args: normalized.into_iter().map(CallArg::Positional).collect(),
            },
            span: expr.span,
        })
    }

    fn normalize_range_call_expr(&self, expr: &Expr) -> Result<Vec<Expr>, ParseError> {
        let ExprKind::Call { callee, args } = &expr.kind else {
            return Err(ParseError {
                message: "current `for` loops require `range(...)`".to_string(),
                span: expr.span,
            });
        };
        let ExprKind::Identifier(name) = &callee.kind else {
            return Err(ParseError {
                message: "current `for` loops require `range(...)`".to_string(),
                span: callee.span,
            });
        };
        if name != "range" {
            return Err(ParseError {
                message: "current `for` loops require `range(...)`".to_string(),
                span: callee.span,
            });
        }

        let positional = args
            .iter()
            .map(|arg| match arg {
                CallArg::Positional(expr) => Ok(expr.clone()),
                CallArg::Keyword { span, .. } => Err(ParseError {
                    message: "`range(...)` does not accept keyword arguments".to_string(),
                    span: *span,
                }),
            })
            .collect::<Result<Vec<_>, _>>()?;

        match positional.as_slice() {
            [stop] => Ok(vec![
                integer_expr("0", expr.span),
                stop.clone(),
                integer_expr("1", expr.span),
            ]),
            [start, stop] => Ok(vec![
                start.clone(),
                stop.clone(),
                integer_expr("1", expr.span),
            ]),
            [start, stop, step] => Ok(vec![start.clone(), stop.clone(), step.clone()]),
            _ => Err(ParseError {
                message: "`range(...)` expects 1, 2, or 3 positional arguments".to_string(),
                span: expr.span,
            }),
        }
    }
}

impl Stmt {
    fn span(&self) -> Span {
        match self {
            Stmt::Block(stmt) => stmt.span,
            Stmt::Let(stmt) => stmt.span,
            Stmt::Assign(stmt) => stmt.span,
            Stmt::FieldAssign(stmt) => stmt.span,
            Stmt::Return(stmt) => stmt.span,
            Stmt::If(stmt) => stmt.span,
            Stmt::While(stmt) => stmt.span,
            Stmt::Break(stmt) => stmt.span,
            Stmt::Continue(stmt) => stmt.span,
            Stmt::Raise(stmt) => stmt.span,
            Stmt::Panic(stmt) => stmt.span,
            Stmt::Expr(stmt) => stmt.expr.span,
        }
    }
}

fn ident_expr(name: &str, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Identifier(name.to_string()),
        span,
    }
}

fn integer_expr(value: &str, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Integer(value.to_string()),
        span,
    }
}

fn string_expr(value: &str, span: Span) -> Expr {
    Expr {
        kind: ExprKind::String(value.to_string()),
        span,
    }
}

fn call_expr(callee: Expr, args: Vec<CallArg>, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Call {
            callee: Box::new(callee),
            args,
        },
        span,
    }
}

fn binary_expr(left: Expr, op: BinaryOp, right: Expr, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Binary {
            left: Box::new(left),
            op,
            right: Box::new(right),
        },
        span,
    }
}

fn parse_fstring_expr(value: &str, span: Span) -> Result<Expr, ParseError> {
    let segments = parse_fstring_segments(value, span)?;
    let mut parts = Vec::new();
    for segment in segments {
        match segment {
            FStringSegment::Text(text) => {
                if !text.is_empty() {
                    parts.push(string_expr(&text, span));
                }
            }
            FStringSegment::Expr(expr) => {
                let str_call = call_expr(
                    ident_expr("str", span),
                    vec![CallArg::Positional(expr)],
                    span,
                );
                parts.push(str_call);
            }
        }
    }

    if parts.is_empty() {
        return Ok(string_expr("", span));
    }

    let mut iter = parts.into_iter();
    let mut expr = iter.next().expect("parts is not empty");
    for part in iter {
        expr = binary_expr(expr, BinaryOp::Add, part, span);
    }
    Ok(expr)
}

#[derive(Debug)]
enum FStringSegment {
    Text(String),
    Expr(Expr),
}

fn parse_fstring_segments(value: &str, span: Span) -> Result<Vec<FStringSegment>, ParseError> {
    let chars = value.chars().collect::<Vec<_>>();
    let mut index = 0usize;
    let mut text = String::new();
    let mut segments = Vec::new();

    while index < chars.len() {
        let ch = chars[index];
        if ch == '{' {
            if index + 1 < chars.len() && chars[index + 1] == '{' {
                text.push('{');
                index += 2;
                continue;
            }

            if !text.is_empty() {
                segments.push(FStringSegment::Text(std::mem::take(&mut text)));
            }

            index += 1;
            let expr_start = index;
            let mut depth = 1usize;
            let mut in_string = false;
            let mut escaping = false;

            while index < chars.len() {
                let current = chars[index];
                if in_string {
                    if escaping {
                        escaping = false;
                    } else if current == '\\' {
                        escaping = true;
                    } else if current == '"' {
                        in_string = false;
                    }
                    index += 1;
                    continue;
                }

                match current {
                    '"' => {
                        in_string = true;
                        index += 1;
                    }
                    '{' => {
                        depth += 1;
                        index += 1;
                    }
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            let expr_text = chars[expr_start..index].iter().collect::<String>();
                            let expr = parse_inline_expr(expr_text.trim(), span)?;
                            segments.push(FStringSegment::Expr(expr));
                            index += 1;
                            break;
                        }
                        index += 1;
                    }
                    _ => index += 1,
                }
            }

            if depth != 0 {
                return Err(ParseError {
                    message: "unterminated f-string expression".to_string(),
                    span,
                });
            }

            continue;
        }

        if ch == '}' {
            if index + 1 < chars.len() && chars[index + 1] == '}' {
                text.push('}');
                index += 2;
                continue;
            }
            return Err(ParseError {
                message: "single `}` is not allowed in f-string".to_string(),
                span,
            });
        }

        text.push(ch);
        index += 1;
    }

    if !text.is_empty() {
        segments.push(FStringSegment::Text(text));
    }

    Ok(segments)
}

fn parse_inline_expr(source: &str, span: Span) -> Result<Expr, ParseError> {
    let tokens = lex(source).map_err(|error| ParseError {
        message: error.message,
        span: error.span,
    })?;
    let mut parser = Parser::new(tokens);
    let expr = parser.parse_expr()?;
    parser.skip_newlines();
    if !parser.at_end() {
        return Err(ParseError {
            message: "unexpected trailing tokens in f-string expression".to_string(),
            span,
        });
    }
    Ok(expr)
}

fn int_call_expr(value: Expr, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Call {
            callee: Box::new(ident_expr("int", span)),
            args: vec![CallArg::Positional(value)],
        },
        span,
    }
}

fn typed_let_stmt(name: &str, ty_name: &str, value: Expr, span: Span) -> Stmt {
    Stmt::Let(LetStmt {
        name: name.to_string(),
        ty: Some(TypeRef {
            name: ty_name.to_string(),
            span,
        }),
        value,
        span,
    })
}
