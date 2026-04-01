use std::collections::{BTreeSet, HashMap};
use std::fmt;

use crate::lexer::Span;
use crate::parser::{
    AssignStmt, BinaryOp, Block, CallArg, Expr, ExprKind, Function, Item, LetStmt, PanicStmt,
    ParseError, Program, RaiseStmt, ReturnStmt, Stmt, StructDecl, TypeRef, UnaryOp, WhileStmt,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticError {
    pub message: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticFailure {
    pub function_name: String,
    pub error: SemanticError,
}

impl fmt::Display for SemanticError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at line {}, column {}",
            self.message, self.span.line, self.span.column
        )
    }
}

impl std::error::Error for SemanticError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedProgram {
    pub functions: Vec<CheckedFunction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedFunction {
    pub name: String,
    pub is_extern: bool,
    pub is_async: bool,
    pub return_type: Type,
    pub raises: Option<Type>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Bool,
    Dynamic,
    Exception(String),
    I32,
    I64,
    String,
    Struct(String),
    Unit,
    Unknown(String),
}

impl Type {
    fn from_type_ref(ty: &TypeRef) -> Result<Self, SemanticError> {
        match ty.name.as_str() {
            "bool" => Ok(Type::Bool),
            "dynamic" => Ok(Type::Dynamic),
            "i32" => Ok(Type::I32),
            "i64" => Ok(Type::I64),
            "String" | "str" => Ok(Type::String),
            "unit" => Ok(Type::Unit),
            other => Ok(Type::Unknown(other.to_string())),
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Type::Bool => "bool",
            Type::Dynamic => "dynamic",
            Type::Exception(_) => "exception",
            Type::I32 => "i32",
            Type::I64 => "i64",
            Type::String => "String",
            Type::Struct(_) => "struct",
            Type::Unit => "unit",
            Type::Unknown(_) => "unknown",
        }
    }

    fn display_name(&self) -> String {
        match self {
            Type::Exception(name) => name.clone(),
            Type::Struct(name) => name.clone(),
            Type::Unknown(name) => name.clone(),
            _ => self.name().to_string(),
        }
    }
}

#[derive(Debug, Clone)]
struct FunctionSig {
    is_async: bool,
    params: Vec<(String, Type)>,
    return_type: Type,
    raises: Option<Type>,
}

#[derive(Debug, Clone)]
struct StructSig {
    fields: HashMap<String, Type>,
}

#[derive(Debug)]
struct Analyzer<'a> {
    program: &'a Program,
    exceptions: BTreeSet<String>,
    structs: HashMap<String, StructSig>,
    functions: HashMap<String, FunctionSig>,
}

pub fn check_program(program: &Program) -> Result<CheckedProgram, SemanticError> {
    Analyzer::new(program)
        .check()
        .map_err(|failure| failure.error)
}

pub fn check_program_with_context(program: &Program) -> Result<CheckedProgram, SemanticFailure> {
    Analyzer::new(program).check()
}

pub fn check_program_with_context_all(
    program: &Program,
) -> Result<CheckedProgram, Vec<SemanticFailure>> {
    Analyzer::new(program).check_all()
}

pub fn check_source(source: &str) -> Result<CheckedProgram, SemanticError> {
    let program =
        crate::parser::parse_source(source).map_err(|error: ParseError| SemanticError {
            message: error.message,
            span: error.span,
        })?;
    check_program(&program)
}

impl<'a> Analyzer<'a> {
    fn new(program: &'a Program) -> Self {
        Self {
            program,
            exceptions: BTreeSet::new(),
            structs: HashMap::new(),
            functions: HashMap::new(),
        }
    }

    fn check(mut self) -> Result<CheckedProgram, SemanticFailure> {
        self.collect_exception_declarations()?;
        self.collect_struct_declarations()?;
        self.collect_function_signatures()?;

        let mut checked = Vec::new();
        for item in &self.program.items {
            let Item::Function(function) = item else {
                continue;
            };
            self.check_function(function)
                .map_err(|error| SemanticFailure {
                    function_name: function.name.clone(),
                    error,
                })?;
            let sig = self
                .functions
                .get(&function.name)
                .expect("signature must exist");
            checked.push(CheckedFunction {
                name: function.name.clone(),
                is_extern: function.is_extern,
                is_async: sig.is_async,
                return_type: sig.return_type.clone(),
                raises: sig.raises.clone(),
            });
        }

        Ok(CheckedProgram { functions: checked })
    }

    fn check_all(mut self) -> Result<CheckedProgram, Vec<SemanticFailure>> {
        self.collect_exception_declarations().map_err(|error| vec![error])?;
        self.collect_struct_declarations().map_err(|error| vec![error])?;
        self.collect_function_signatures().map_err(|error| vec![error])?;

        let mut checked = Vec::new();
        let mut failures = Vec::new();
        for item in &self.program.items {
            let Item::Function(function) = item else {
                continue;
            };

            let function_errors = self.check_function_all(function);
            if function_errors.is_empty() {
                let sig = self
                    .functions
                    .get(&function.name)
                    .expect("signature must exist");
                checked.push(CheckedFunction {
                    name: function.name.clone(),
                    is_extern: function.is_extern,
                    is_async: sig.is_async,
                    return_type: sig.return_type.clone(),
                    raises: sig.raises.clone(),
                });
            } else {
                failures.extend(function_errors.into_iter().map(|error| SemanticFailure {
                    function_name: function.name.clone(),
                    error,
                }));
            }
        }

        if failures.is_empty() {
            Ok(CheckedProgram { functions: checked })
        } else {
            Err(failures)
        }
    }

    fn collect_function_signatures(&mut self) -> Result<(), SemanticFailure> {
        for item in &self.program.items {
            let Item::Function(function) = item else {
                continue;
            };
            if self.functions.contains_key(&function.name) {
                return Err(SemanticFailure {
                    function_name: function.name.clone(),
                    error: SemanticError {
                        message: format!("duplicate function `{}`", function.name),
                        span: function.span,
                    },
                });
            }

            let params = function
                .params
                .iter()
                .map(|param| {
                    let ty = self.resolve_regular_type(&param.ty)?;
                    Ok((param.name.clone(), ty))
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| SemanticFailure {
                    function_name: function.name.clone(),
                    error,
                })?;
            let return_type = match &function.return_type {
                Some(ty) => self
                    .resolve_regular_type(ty)
                    .map_err(|error| SemanticFailure {
                        function_name: function.name.clone(),
                        error,
                    })?,
                None => Type::Unit,
            };
            let raises = match &function.raises {
                Some(ty) => {
                    Some(
                        self.resolve_exception_type(ty)
                            .map_err(|error| SemanticFailure {
                                function_name: function.name.clone(),
                                error,
                            })?,
                    )
                }
                None => None,
            };

            if function.is_extern {
                if function.is_async {
                    return Err(SemanticFailure {
                        function_name: function.name.clone(),
                        error: SemanticError {
                            message: "extern functions cannot be async".to_string(),
                            span: function.span,
                        },
                    });
                }
                if raises.is_some() {
                    return Err(SemanticFailure {
                        function_name: function.name.clone(),
                        error: SemanticError {
                            message: "extern functions cannot declare `raises`".to_string(),
                            span: function.span,
                        },
                    });
                }
                for (param_name, ty) in &params {
                    if !is_supported_extern_ffi_type(ty) {
                        return Err(SemanticFailure {
                            function_name: function.name.clone(),
                            error: SemanticError {
                                message: format!(
                                    "extern function parameter `{param_name}` in `{}` must use bool, i32, i64, String, or unit",
                                    function.name
                                ),
                                span: function.span,
                            },
                        });
                    }
                }
                if !is_supported_extern_ffi_type(&return_type) {
                    return Err(SemanticFailure {
                        function_name: function.name.clone(),
                        error: SemanticError {
                            message: format!(
                                "extern function `{}` must return bool, i32, i64, String, or unit",
                                function.name
                            ),
                            span: function.span,
                        },
                    });
                }
            }

            self.functions.insert(
                function.name.clone(),
                FunctionSig {
                    is_async: function.is_async,
                    params,
                    return_type,
                    raises,
                },
            );
        }
        Ok(())
    }

    fn collect_struct_declarations(&mut self) -> Result<(), SemanticFailure> {
        for item in &self.program.items {
            let Item::Struct(decl) = item else {
                continue;
            };
            if self.structs.contains_key(&decl.name) {
                return Err(SemanticFailure {
                    function_name: decl.name.clone(),
                    error: SemanticError {
                        message: format!("duplicate struct `{}`", decl.name),
                        span: decl.span,
                    },
                });
            }
            self.structs.insert(
                decl.name.clone(),
                StructSig {
                    fields: self.collect_struct_fields(decl)?,
                },
            );
        }
        Ok(())
    }

    fn collect_struct_fields(
        &self,
        decl: &StructDecl,
    ) -> Result<HashMap<String, Type>, SemanticFailure> {
        let mut fields = HashMap::new();
        for field in &decl.fields {
            let ty = self
                .resolve_regular_type(&field.ty)
                .map_err(|error| SemanticFailure {
                    function_name: decl.name.clone(),
                    error,
                })?;
            if matches!(ty, Type::Dynamic | Type::Unit) {
                return Err(SemanticFailure {
                    function_name: decl.name.clone(),
                    error: SemanticError {
                        message: format!(
                            "struct field `{}` in `{}` must use a concrete scalar, string, or struct type",
                            field.name, decl.name
                        ),
                        span: field.span,
                    },
                });
            }
            if fields.insert(field.name.clone(), ty).is_some() {
                return Err(SemanticFailure {
                    function_name: decl.name.clone(),
                    error: SemanticError {
                        message: format!(
                            "duplicate field `{}` in struct `{}`",
                            field.name, decl.name
                        ),
                        span: field.span,
                    },
                });
            }
        }
        Ok(fields)
    }

    fn collect_exception_declarations(&mut self) -> Result<(), SemanticFailure> {
        for item in &self.program.items {
            let Item::Exception(exception) = item else {
                continue;
            };
            if !self.exceptions.insert(exception.name.clone()) {
                return Err(SemanticFailure {
                    function_name: exception.name.clone(),
                    error: SemanticError {
                        message: format!("duplicate exception `{}`", exception.name),
                        span: exception.span,
                    },
                });
            }
        }
        Ok(())
    }

    fn check_function(&self, function: &Function) -> Result<(), SemanticError> {
        if function.is_extern {
            return Ok(());
        }
        let sig = self
            .functions
            .get(&function.name)
            .expect("function signature should exist");

        let mut scope = Scope::default();
        for (name, ty) in &sig.params {
            if scope.values.insert(name.clone(), ty.clone()).is_some() {
                return Err(SemanticError {
                    message: format!("duplicate parameter `{}`", name),
                    span: function.span,
                });
            }
        }

        self.check_block(
            &function.body,
            &mut scope,
            &sig.return_type,
            sig.raises.as_ref(),
            sig.is_async,
        )
    }

    fn check_function_all(&self, function: &Function) -> Vec<SemanticError> {
        if function.is_extern {
            return Vec::new();
        }

        let sig = self
            .functions
            .get(&function.name)
            .expect("function signature should exist");
        let mut scope = Scope::default();
        let mut errors = Vec::new();

        for (name, ty) in &sig.params {
            if scope.values.insert(name.clone(), ty.clone()).is_some() {
                errors.push(SemanticError {
                    message: format!("duplicate parameter `{}`", name),
                    span: function.span,
                });
            }
        }

        self.check_block_collect(
            &function.body,
            &mut scope,
            &sig.return_type,
            sig.raises.as_ref(),
            sig.is_async,
            &mut errors,
        );
        errors
    }

    fn check_block(
        &self,
        block: &Block,
        scope: &mut Scope,
        expected_return: &Type,
        expected_raises: Option<&Type>,
        in_async: bool,
    ) -> Result<(), SemanticError> {
        for stmt in &block.statements {
            self.check_stmt(stmt, scope, expected_return, expected_raises, in_async)?;
        }
        Ok(())
    }

    fn check_block_collect(
        &self,
        block: &Block,
        scope: &mut Scope,
        expected_return: &Type,
        expected_raises: Option<&Type>,
        in_async: bool,
        errors: &mut Vec<SemanticError>,
    ) {
        for stmt in &block.statements {
            self.check_stmt_collect(
                stmt,
                scope,
                expected_return,
                expected_raises,
                in_async,
                errors,
            );
        }
    }

    fn check_stmt(
        &self,
        stmt: &Stmt,
        scope: &mut Scope,
        expected_return: &Type,
        expected_raises: Option<&Type>,
        in_async: bool,
    ) -> Result<(), SemanticError> {
        match stmt {
            Stmt::Let(stmt) => self.check_let(stmt, scope, in_async),
            Stmt::Assign(stmt) => self.check_assign(stmt, scope, in_async),
            Stmt::Return(stmt) => self.check_return(stmt, scope, expected_return, in_async),
            Stmt::If(stmt) => {
                let cond_ty = self.check_expr(&stmt.condition, scope, in_async)?;
                self.expect_condition_type(&cond_ty, stmt.condition.span, "if condition")?;

                let mut then_scope = scope.clone();
                self.check_block(
                    &stmt.then_block,
                    &mut then_scope,
                    expected_return,
                    expected_raises,
                    in_async,
                )?;
                for elif in &stmt.elif_blocks {
                    let elif_ty = self.check_expr(&elif.condition, scope, in_async)?;
                    self.expect_condition_type(&elif_ty, elif.condition.span, "elif condition")?;
                    let mut elif_scope = scope.clone();
                    self.check_block(
                        &elif.block,
                        &mut elif_scope,
                        expected_return,
                        expected_raises,
                        in_async,
                    )?;
                }
                if let Some(block) = &stmt.else_block {
                    let mut else_scope = scope.clone();
                    self.check_block(
                        block,
                        &mut else_scope,
                        expected_return,
                        expected_raises,
                        in_async,
                    )?;
                }
                Ok(())
            }
            Stmt::While(stmt) => {
                self.check_while(stmt, scope, expected_return, expected_raises, in_async)
            }
            Stmt::Raise(stmt) => self.check_raise(stmt, scope, expected_raises, in_async),
            Stmt::Panic(stmt) => self.check_panic(stmt, scope, in_async),
            Stmt::Expr(stmt) => {
                self.check_expr(&stmt.expr, scope, in_async)?;
                Ok(())
            }
        }
    }

    fn check_stmt_collect(
        &self,
        stmt: &Stmt,
        scope: &mut Scope,
        expected_return: &Type,
        expected_raises: Option<&Type>,
        in_async: bool,
        errors: &mut Vec<SemanticError>,
    ) {
        match stmt {
            Stmt::Let(let_stmt) => {
                if let Err(error) = self.check_let(let_stmt, scope, in_async) {
                    errors.push(error);
                }
            }
            Stmt::Assign(assign_stmt) => {
                if let Err(error) = self.check_assign(assign_stmt, scope, in_async) {
                    errors.push(error);
                }
            }
            Stmt::Return(return_stmt) => {
                if let Err(error) = self.check_return(return_stmt, scope, expected_return, in_async)
                {
                    errors.push(error);
                }
            }
            Stmt::If(if_stmt) => {
                if let Err(error) = self
                    .check_expr(&if_stmt.condition, scope, in_async)
                    .and_then(|ty| {
                        self.expect_condition_type(&ty, if_stmt.condition.span, "if condition")
                    })
                {
                    errors.push(error);
                }

                let mut then_scope = scope.clone();
                self.check_block_collect(
                    &if_stmt.then_block,
                    &mut then_scope,
                    expected_return,
                    expected_raises,
                    in_async,
                    errors,
                );

                for elif in &if_stmt.elif_blocks {
                    if let Err(error) = self
                        .check_expr(&elif.condition, scope, in_async)
                        .and_then(|ty| {
                            self.expect_condition_type(
                                &ty,
                                elif.condition.span,
                                "elif condition",
                            )
                        })
                    {
                        errors.push(error);
                    }

                    let mut elif_scope = scope.clone();
                    self.check_block_collect(
                        &elif.block,
                        &mut elif_scope,
                        expected_return,
                        expected_raises,
                        in_async,
                        errors,
                    );
                }

                if let Some(block) = &if_stmt.else_block {
                    let mut else_scope = scope.clone();
                    self.check_block_collect(
                        block,
                        &mut else_scope,
                        expected_return,
                        expected_raises,
                        in_async,
                        errors,
                    );
                }
            }
            Stmt::While(while_stmt) => {
                if let Err(error) = self
                    .check_expr(&while_stmt.condition, scope, in_async)
                    .and_then(|ty| {
                        self.expect_condition_type(
                            &ty,
                            while_stmt.condition.span,
                            "while condition",
                        )
                    })
                {
                    errors.push(error);
                }

                let mut body_scope = scope.clone();
                self.check_block_collect(
                    &while_stmt.body,
                    &mut body_scope,
                    expected_return,
                    expected_raises,
                    in_async,
                    errors,
                );
            }
            Stmt::Raise(raise_stmt) => {
                if let Err(error) = self.check_raise(raise_stmt, scope, expected_raises, in_async)
                {
                    errors.push(error);
                }
            }
            Stmt::Panic(panic_stmt) => {
                if let Err(error) = self.check_panic(panic_stmt, scope, in_async) {
                    errors.push(error);
                }
            }
            Stmt::Expr(expr_stmt) => {
                if let Err(error) = self.check_expr(&expr_stmt.expr, scope, in_async) {
                    errors.push(error);
                }
            }
        }
    }

    fn check_let(
        &self,
        stmt: &LetStmt,
        scope: &mut Scope,
        in_async: bool,
    ) -> Result<(), SemanticError> {
        let value_ty = self.check_expr(&stmt.value, scope, in_async)?;
        let declared_ty = match &stmt.ty {
            Some(ty) => {
                let declared = self.resolve_regular_type(ty)?;
                self.expect_type(&value_ty, &declared, stmt.value.span, "let binding")?;
                declared
            }
            None => Type::Dynamic,
        };

        if scope
            .values
            .insert(stmt.name.clone(), declared_ty)
            .is_some()
        {
            return Err(SemanticError {
                message: format!("duplicate local `{}`", stmt.name),
                span: stmt.span,
            });
        }

        Ok(())
    }

    fn check_assign(
        &self,
        stmt: &AssignStmt,
        scope: &Scope,
        in_async: bool,
    ) -> Result<(), SemanticError> {
        let Some(expected_ty) = scope.values.get(&stmt.name) else {
            return Err(SemanticError {
                message: format!("cannot assign to unknown variable `{}`", stmt.name),
                span: stmt.span,
            });
        };
        let actual_ty = self.check_expr(&stmt.value, scope, in_async)?;
        self.expect_type(&actual_ty, expected_ty, stmt.value.span, "assignment value")
    }

    fn check_return(
        &self,
        stmt: &ReturnStmt,
        scope: &Scope,
        expected_return: &Type,
        in_async: bool,
    ) -> Result<(), SemanticError> {
        let actual = match &stmt.value {
            Some(expr) => self.check_expr(expr, scope, in_async)?,
            None => Type::Unit,
        };
        self.expect_type(&actual, expected_return, stmt.span, "return value")
    }

    fn check_while(
        &self,
        stmt: &WhileStmt,
        scope: &mut Scope,
        expected_return: &Type,
        expected_raises: Option<&Type>,
        in_async: bool,
    ) -> Result<(), SemanticError> {
        let cond_ty = self.check_expr(&stmt.condition, scope, in_async)?;
        self.expect_condition_type(&cond_ty, stmt.condition.span, "while condition")?;

        let mut body_scope = scope.clone();
        self.check_block(
            &stmt.body,
            &mut body_scope,
            expected_return,
            expected_raises,
            in_async,
        )
    }

    fn check_raise(
        &self,
        stmt: &RaiseStmt,
        scope: &Scope,
        expected_raises: Option<&Type>,
        in_async: bool,
    ) -> Result<(), SemanticError> {
        let Some(expected_error) = expected_raises else {
            return Err(SemanticError {
                message: "cannot `raise` in a function without `raises`".to_string(),
                span: stmt.span,
            });
        };

        let actual = self.check_expr(&stmt.value, scope, in_async)?;
        self.expect_type(&actual, expected_error, stmt.span, "raised error")
    }

    fn check_panic(
        &self,
        stmt: &PanicStmt,
        scope: &Scope,
        in_async: bool,
    ) -> Result<(), SemanticError> {
        let value_ty = self.check_expr(&stmt.value, scope, in_async)?;
        self.expect_type(&value_ty, &Type::String, stmt.span, "panic message")
    }

    fn check_expr(
        &self,
        expr: &Expr,
        scope: &Scope,
        in_async: bool,
    ) -> Result<Type, SemanticError> {
        match &expr.kind {
            ExprKind::Identifier(name) => {
                if let Some(ty) = scope.values.get(name) {
                    return Ok(ty.clone());
                }
                if let Some(ty) = builtin_function_type(name) {
                    return Ok(ty);
                }
                if let Some(sig) = self.functions.get(name) {
                    return Ok(Type::Unknown(format!(
                        "fn {}",
                        if sig.is_async { "async" } else { "sync" }
                    )));
                }
                Err(SemanticError {
                    message: format!("unknown identifier `{name}`"),
                    span: expr.span,
                })
            }
            ExprKind::Integer(value) => {
                if value.parse::<i32>().is_ok() {
                    Ok(Type::I32)
                } else {
                    Ok(Type::I64)
                }
            }
            ExprKind::String(_) => Ok(Type::String),
            ExprKind::Bool(_) => Ok(Type::Bool),
            ExprKind::Unary { op, expr: inner } => {
                let inner_ty = self.check_expr(inner, scope, in_async)?;
                match op {
                    UnaryOp::Negate => {
                        if matches!(inner_ty, Type::I32 | Type::I64) {
                            Ok(inner_ty)
                        } else {
                            Err(SemanticError {
                                message: format!(
                                    "unary `-` requires an integer, found `{}`",
                                    inner_ty.name()
                                ),
                                span: expr.span,
                            })
                        }
                    }
                    UnaryOp::Not => {
                        if self.is_truthy_type(&inner_ty) {
                            Ok(Type::Bool)
                        } else {
                            Err(SemanticError {
                                message: format!(
                                    "unary `not` requires `bool` or `dynamic`, found `{}`",
                                    inner_ty.name()
                                ),
                                span: expr.span,
                            })
                        }
                    }
                }
            }
            ExprKind::Binary { left, op, right } => {
                let left_ty = self.check_expr(left, scope, in_async)?;
                let right_ty = self.check_expr(right, scope, in_async)?;
                self.check_binary(op, &left_ty, &right_ty, expr.span)
            }
            ExprKind::Call { callee, args } => {
                self.check_call(callee, args, scope, in_async, expr.span)
            }
            ExprKind::Await { expr: inner } => {
                if !in_async {
                    return Err(SemanticError {
                        message: "`await` is only allowed inside `async def`".to_string(),
                        span: expr.span,
                    });
                }
                self.check_expr(inner, scope, in_async)
            }
            ExprKind::Field { base, name } => {
                let base_ty = self.check_expr(base, scope, in_async)?;
                let Type::Struct(struct_name) = base_ty else {
                    return Err(SemanticError {
                        message: format!(
                            "field access requires a struct value, found `{}`",
                            base_ty.display_name()
                        ),
                        span: expr.span,
                    });
                };
                let Some(struct_sig) = self.structs.get(&struct_name) else {
                    return Err(SemanticError {
                        message: format!("unknown struct `{struct_name}`"),
                        span: expr.span,
                    });
                };
                struct_sig
                    .fields
                    .get(name)
                    .cloned()
                    .ok_or_else(|| SemanticError {
                        message: format!("struct `{struct_name}` has no field `{name}`"),
                        span: expr.span,
                    })
            }
        }
    }

    fn check_binary(
        &self,
        op: &BinaryOp,
        left: &Type,
        right: &Type,
        span: Span,
    ) -> Result<Type, SemanticError> {
        match op {
            BinaryOp::And | BinaryOp::Or => {
                if self.is_truthy_type(left) && self.is_truthy_type(right) {
                    Ok(Type::Bool)
                } else {
                    Err(SemanticError {
                        message: format!(
                            "boolean `{}` requires `bool` or `dynamic` operands, found `{}` and `{}`",
                            match op {
                                BinaryOp::And => "and",
                                BinaryOp::Or => "or",
                                _ => unreachable!(),
                            },
                            left.name(),
                            right.name()
                        ),
                        span,
                    })
                }
            }
            BinaryOp::Add => {
                if left == right && matches!(left, Type::I32 | Type::I64) {
                    Ok(left.clone())
                } else if self.is_integer_pair(left, right) {
                    Ok(Type::I64)
                } else if *left == Type::String && *right == Type::String {
                    Ok(Type::String)
                } else if self.is_dynamic_add_supported(left, right) {
                    Ok(Type::Dynamic)
                } else {
                    Err(SemanticError {
                        message: format!(
                            "binary `+` requires matching integer types, `String + String`, or supported dynamic operands, found `{}` and `{}`",
                            left.name(),
                            right.name()
                        ),
                        span,
                    })
                }
            }
            BinaryOp::Subtract | BinaryOp::Multiply | BinaryOp::Divide | BinaryOp::Modulo => {
                if left == right && matches!(left, Type::I32 | Type::I64) {
                    Ok(left.clone())
                } else if self.is_integer_pair(left, right) {
                    Ok(Type::I64)
                } else if self.is_dynamic_numeric_supported(left, right) {
                    Ok(Type::Dynamic)
                } else {
                    Err(SemanticError {
                        message: format!(
                            "binary arithmetic requires matching integer types or supported dynamic numeric operands, found `{}` and `{}`",
                            left.name(),
                            right.name()
                        ),
                        span,
                    })
                }
            }
            BinaryOp::EqualEqual | BinaryOp::NotEqual => {
                if left == right || self.is_integer_pair(left, right) {
                    Ok(Type::Bool)
                } else if self.is_dynamic_equality_supported(left, right) {
                    Ok(Type::Bool)
                } else {
                    Err(SemanticError {
                        message: format!(
                            "comparison requires matching types, found `{}` and `{}`",
                            left.name(),
                            right.name()
                        ),
                        span,
                    })
                }
            }
            BinaryOp::Greater | BinaryOp::GreaterEqual | BinaryOp::Less | BinaryOp::LessEqual => {
                if (left == right && matches!(left, Type::I32 | Type::I64))
                    || self.is_integer_pair(left, right)
                {
                    Ok(Type::Bool)
                } else if self.is_dynamic_ordering_supported(left, right) {
                    Ok(Type::Bool)
                } else {
                    Err(SemanticError {
                        message: format!(
                            "ordering comparison requires matching integer types, found `{}` and `{}`",
                            left.name(),
                            right.name()
                        ),
                        span,
                    })
                }
            }
        }
    }

    fn check_call(
        &self,
        callee: &Expr,
        args: &[CallArg],
        scope: &Scope,
        in_async: bool,
        span: Span,
    ) -> Result<Type, SemanticError> {
        match &callee.kind {
            ExprKind::Identifier(name) => match name.as_str() {
                "print" | "println" | "eprint" | "eprintln" => {
                    for arg in args {
                        match arg {
                            CallArg::Positional(expr) => {
                                self.check_expr(expr, scope, in_async)?;
                            }
                            CallArg::Keyword { span, .. } => {
                                return Err(SemanticError {
                                    message: format!(
                                        "`{name}` does not accept keyword arguments yet"
                                    ),
                                    span: *span,
                                });
                            }
                        }
                    }
                    Ok(Type::Unit)
                }
                "flush" | "eflush" => {
                    if !args.is_empty() {
                        return Err(SemanticError {
                            message: format!("`{name}` takes no arguments"),
                            span,
                        });
                    }
                    Ok(Type::Unit)
                }
                "input" => {
                    if !args.is_empty() {
                        return Err(SemanticError {
                            message: "`input` takes no arguments".to_string(),
                            span,
                        });
                    }
                    Ok(Type::String)
                }
                "str" => {
                    if args.len() != 1 {
                        return Err(SemanticError {
                            message: "`str` expects 1 argument".to_string(),
                            span,
                        });
                    }
                    let CallArg::Positional(expr) = &args[0] else {
                        return Err(SemanticError {
                            message: "`str` does not accept keyword arguments".to_string(),
                            span,
                        });
                    };
                    let actual = self.check_expr(expr, scope, in_async)?;
                    if matches!(
                        actual,
                        Type::Bool | Type::Dynamic | Type::I32 | Type::I64 | Type::String
                    ) {
                        Ok(Type::String)
                    } else {
                        Err(SemanticError {
                            message: format!(
                                "`str` expects a bool, integer, string, or dynamic value, found `{}`",
                                actual.name()
                            ),
                            span: expr.span,
                        })
                    }
                }
                "int" => {
                    if args.len() != 1 {
                        return Err(SemanticError {
                            message: "`int` expects 1 argument".to_string(),
                            span,
                        });
                    }
                    let CallArg::Positional(expr) = &args[0] else {
                        return Err(SemanticError {
                            message: "`int` does not accept keyword arguments".to_string(),
                            span,
                        });
                    };
                    let actual = self.check_expr(expr, scope, in_async)?;
                    if matches!(
                        actual,
                        Type::Bool | Type::Dynamic | Type::I32 | Type::I64 | Type::String
                    ) {
                        Ok(Type::I64)
                    } else {
                        Err(SemanticError {
                            message: format!(
                                "`int` expects a bool, integer, string, or dynamic value, found `{}`",
                                actual.name()
                            ),
                            span: expr.span,
                        })
                    }
                }
                name if self.structs.contains_key(name) => {
                    let struct_sig = self.structs.get(name).expect("checked above");
                    if args.len() != struct_sig.fields.len() {
                        return Err(SemanticError {
                            message: format!(
                                "struct `{name}` expects {} keyword arguments but got {}",
                                struct_sig.fields.len(),
                                args.len()
                            ),
                            span,
                        });
                    }
                    let mut seen = BTreeSet::new();
                    for arg in args {
                        let CallArg::Keyword {
                            name: field_name,
                            value,
                            span: field_span,
                        } = arg
                        else {
                            return Err(SemanticError {
                                message: format!(
                                    "struct `{name}` construction requires keyword arguments"
                                ),
                                span,
                            });
                        };
                        let Some(expected_ty) = struct_sig.fields.get(field_name) else {
                            return Err(SemanticError {
                                message: format!("struct `{name}` has no field `{field_name}`"),
                                span: *field_span,
                            });
                        };
                        if !seen.insert(field_name.clone()) {
                            return Err(SemanticError {
                                message: format!(
                                    "field `{field_name}` was provided more than once for struct `{name}`"
                                ),
                                span: *field_span,
                            });
                        }
                        let actual = self.check_expr(value, scope, in_async)?;
                        self.expect_type(&actual, expected_ty, value.span, "struct field")?;
                    }
                    Ok(Type::Struct(name.to_string()))
                }
                name if self.exceptions.contains(name) => {
                    if args.len() != 1 {
                        return Err(SemanticError {
                            message: format!("exception `{name}` expects 1 string argument"),
                            span,
                        });
                    }
                    let CallArg::Positional(expr) = &args[0] else {
                        return Err(SemanticError {
                            message: format!(
                                "exception `{name}` does not accept keyword arguments"
                            ),
                            span,
                        });
                    };
                    let actual = self.check_expr(expr, scope, in_async)?;
                    self.expect_type(&actual, &Type::String, expr.span, "exception message")?;
                    Ok(Type::Exception(name.to_string()))
                }
                "__rune_builtin_time_now_unix" => {
                    if !args.is_empty() {
                        return Err(SemanticError {
                            message: "`__rune_builtin_time_now_unix` takes no arguments"
                                .to_string(),
                            span,
                        });
                    }
                    Ok(Type::I64)
                }
                "__rune_builtin_time_monotonic_ms" => {
                    if !args.is_empty() {
                        return Err(SemanticError {
                            message: "`__rune_builtin_time_monotonic_ms` takes no arguments"
                                .to_string(),
                            span,
                        });
                    }
                    Ok(Type::I64)
                }
                "__rune_builtin_time_sleep_ms" => {
                    if args.len() != 1 {
                        return Err(SemanticError {
                            message: "`__rune_builtin_time_sleep_ms` expects 1 argument"
                                .to_string(),
                            span,
                        });
                    }
                    let CallArg::Positional(expr) = &args[0] else {
                        return Err(SemanticError {
                            message:
                                "`__rune_builtin_time_sleep_ms` does not accept keyword arguments"
                                    .to_string(),
                            span,
                        });
                    };
                    let actual = self.check_expr(expr, scope, in_async)?;
                    self.expect_type(&actual, &Type::I64, expr.span, "sleep duration")?;
                    Ok(Type::Unit)
                }
                "__rune_builtin_system_pid" => {
                    if !args.is_empty() {
                        return Err(SemanticError {
                            message: "`__rune_builtin_system_pid` takes no arguments".to_string(),
                            span,
                        });
                    }
                    Ok(Type::I32)
                }
                "__rune_builtin_system_cpu_count" => {
                    if !args.is_empty() {
                        return Err(SemanticError {
                            message: "`__rune_builtin_system_cpu_count` takes no arguments"
                                .to_string(),
                            span,
                        });
                    }
                    Ok(Type::I32)
                }
                "__rune_builtin_system_exit" => {
                    if args.len() != 1 {
                        return Err(SemanticError {
                            message: "`__rune_builtin_system_exit` expects 1 argument".to_string(),
                            span,
                        });
                    }
                    let CallArg::Positional(expr) = &args[0] else {
                        return Err(SemanticError {
                            message:
                                "`__rune_builtin_system_exit` does not accept keyword arguments"
                                    .to_string(),
                            span,
                        });
                    };
                    let actual = self.check_expr(expr, scope, in_async)?;
                    self.expect_type(&actual, &Type::I32, expr.span, "system exit code")?;
                    Ok(Type::Unit)
                }
                "__rune_builtin_env_exists" => {
                    if args.len() != 1 {
                        return Err(SemanticError {
                            message: "`__rune_builtin_env_exists` expects 1 argument".to_string(),
                            span,
                        });
                    }
                    let CallArg::Positional(expr) = &args[0] else {
                        return Err(SemanticError {
                            message:
                                "`__rune_builtin_env_exists` does not accept keyword arguments"
                                    .to_string(),
                            span,
                        });
                    };
                    let actual = self.check_expr(expr, scope, in_async)?;
                    self.expect_type(
                        &actual,
                        &Type::String,
                        expr.span,
                        "environment variable name",
                    )?;
                    Ok(Type::Bool)
                }
                "__rune_builtin_env_get_i32" => {
                    if args.len() != 2 {
                        return Err(SemanticError {
                            message: "`__rune_builtin_env_get_i32` expects 2 arguments".to_string(),
                            span,
                        });
                    }
                    let [
                        CallArg::Positional(name_expr),
                        CallArg::Positional(default_expr),
                    ] = args
                    else {
                        return Err(SemanticError {
                            message:
                                "`__rune_builtin_env_get_i32` does not accept keyword arguments"
                                    .to_string(),
                            span,
                        });
                    };
                    let name_ty = self.check_expr(name_expr, scope, in_async)?;
                    self.expect_type(
                        &name_ty,
                        &Type::String,
                        name_expr.span,
                        "environment variable name",
                    )?;
                    let default_ty = self.check_expr(default_expr, scope, in_async)?;
                    self.expect_type(
                        &default_ty,
                        &Type::I32,
                        default_expr.span,
                        "default environment value",
                    )?;
                    Ok(Type::I32)
                }
                "__rune_builtin_env_get_bool" => {
                    if args.len() != 2 {
                        return Err(SemanticError {
                            message: "`__rune_builtin_env_get_bool` expects 2 arguments"
                                .to_string(),
                            span,
                        });
                    }
                    let [
                        CallArg::Positional(name_expr),
                        CallArg::Positional(default_expr),
                    ] = args
                    else {
                        return Err(SemanticError {
                            message:
                                "`__rune_builtin_env_get_bool` does not accept keyword arguments"
                                    .to_string(),
                            span,
                        });
                    };
                    let name_ty = self.check_expr(name_expr, scope, in_async)?;
                    self.expect_type(
                        &name_ty,
                        &Type::String,
                        name_expr.span,
                        "environment variable name",
                    )?;
                    let default_ty = self.check_expr(default_expr, scope, in_async)?;
                    self.expect_type(
                        &default_ty,
                        &Type::Bool,
                        default_expr.span,
                        "default environment value",
                    )?;
                    Ok(Type::Bool)
                }
                "__rune_builtin_env_arg_count" => {
                    if !args.is_empty() {
                        return Err(SemanticError {
                            message: "`__rune_builtin_env_arg_count` takes no arguments"
                                .to_string(),
                            span,
                        });
                    }
                    Ok(Type::I32)
                }
                "__rune_builtin_network_tcp_connect" => {
                    if args.len() != 2 {
                        return Err(SemanticError {
                            message: "`__rune_builtin_network_tcp_connect` expects 2 arguments"
                                .to_string(),
                            span,
                        });
                    }
                    let [
                        CallArg::Positional(host_expr),
                        CallArg::Positional(port_expr),
                    ] = args
                    else {
                        return Err(SemanticError {
                            message: "`__rune_builtin_network_tcp_connect` does not accept keyword arguments"
                                .to_string(),
                            span,
                        });
                    };
                    let host_ty = self.check_expr(host_expr, scope, in_async)?;
                    self.expect_type(&host_ty, &Type::String, host_expr.span, "TCP host")?;
                    let port_ty = self.check_expr(port_expr, scope, in_async)?;
                    self.expect_type(&port_ty, &Type::I32, port_expr.span, "TCP port")?;
                    Ok(Type::Bool)
                }
                "__rune_builtin_network_tcp_connect_timeout" => {
                    if args.len() != 3 {
                        return Err(SemanticError {
                            message:
                                "`__rune_builtin_network_tcp_connect_timeout` expects 3 arguments"
                                    .to_string(),
                            span,
                        });
                    }
                    let [
                        CallArg::Positional(host_expr),
                        CallArg::Positional(port_expr),
                        CallArg::Positional(timeout_expr),
                    ] = args
                    else {
                        return Err(SemanticError {
                            message: "`__rune_builtin_network_tcp_connect_timeout` does not accept keyword arguments"
                                .to_string(),
                            span,
                        });
                    };
                    let host_ty = self.check_expr(host_expr, scope, in_async)?;
                    self.expect_type(&host_ty, &Type::String, host_expr.span, "TCP host")?;
                    let port_ty = self.check_expr(port_expr, scope, in_async)?;
                    self.expect_type(&port_ty, &Type::I32, port_expr.span, "TCP port")?;
                    let timeout_ty = self.check_expr(timeout_expr, scope, in_async)?;
                    self.expect_type(&timeout_ty, &Type::I32, timeout_expr.span, "TCP timeout")?;
                    Ok(Type::Bool)
                }
                "__rune_builtin_fs_exists" => {
                    if args.len() != 1 {
                        return Err(SemanticError {
                            message: "`__rune_builtin_fs_exists` expects 1 argument".to_string(),
                            span,
                        });
                    }
                    let [CallArg::Positional(path_expr)] = args else {
                        return Err(SemanticError {
                            message: "`__rune_builtin_fs_exists` does not accept keyword arguments"
                                .to_string(),
                            span,
                        });
                    };
                    let path_ty = self.check_expr(path_expr, scope, in_async)?;
                    self.expect_type(&path_ty, &Type::String, path_expr.span, "filesystem path")?;
                    Ok(Type::Bool)
                }
                "__rune_builtin_fs_read_string" => {
                    if args.len() != 1 {
                        return Err(SemanticError {
                            message: "`__rune_builtin_fs_read_string` expects 1 argument"
                                .to_string(),
                            span,
                        });
                    }
                    let [CallArg::Positional(path_expr)] = args else {
                        return Err(SemanticError {
                            message:
                                "`__rune_builtin_fs_read_string` does not accept keyword arguments"
                                    .to_string(),
                            span,
                        });
                    };
                    let path_ty = self.check_expr(path_expr, scope, in_async)?;
                    self.expect_type(&path_ty, &Type::String, path_expr.span, "filesystem path")?;
                    Ok(Type::String)
                }
                "__rune_builtin_fs_write_string" => {
                    if args.len() != 2 {
                        return Err(SemanticError {
                            message: "`__rune_builtin_fs_write_string` expects 2 arguments"
                                .to_string(),
                            span,
                        });
                    }
                    let [
                        CallArg::Positional(path_expr),
                        CallArg::Positional(content_expr),
                    ] = args
                    else {
                        return Err(SemanticError {
                            message:
                                "`__rune_builtin_fs_write_string` does not accept keyword arguments"
                                    .to_string(),
                            span,
                        });
                    };
                    let path_ty = self.check_expr(path_expr, scope, in_async)?;
                    self.expect_type(&path_ty, &Type::String, path_expr.span, "filesystem path")?;
                    let content_ty = self.check_expr(content_expr, scope, in_async)?;
                    self.expect_type(
                        &content_ty,
                        &Type::String,
                        content_expr.span,
                        "filesystem content",
                    )?;
                    Ok(Type::Bool)
                }
                "__rune_builtin_terminal_clear" => {
                    if !args.is_empty() {
                        return Err(SemanticError {
                            message: "`__rune_builtin_terminal_clear` takes no arguments"
                                .to_string(),
                            span,
                        });
                    }
                    Ok(Type::Unit)
                }
                "__rune_builtin_terminal_move_to" => {
                    if args.len() != 2 {
                        return Err(SemanticError {
                            message: "`__rune_builtin_terminal_move_to` expects 2 arguments"
                                .to_string(),
                            span,
                        });
                    }
                    let [CallArg::Positional(row_expr), CallArg::Positional(col_expr)] = args else {
                        return Err(SemanticError {
                            message:
                                "`__rune_builtin_terminal_move_to` does not accept keyword arguments"
                                    .to_string(),
                            span,
                        });
                    };
                    let row_ty = self.check_expr(row_expr, scope, in_async)?;
                    self.expect_type(&row_ty, &Type::I32, row_expr.span, "terminal row")?;
                    let col_ty = self.check_expr(col_expr, scope, in_async)?;
                    self.expect_type(&col_ty, &Type::I32, col_expr.span, "terminal column")?;
                    Ok(Type::Unit)
                }
                "__rune_builtin_terminal_hide_cursor"
                | "__rune_builtin_terminal_show_cursor" => {
                    if !args.is_empty() {
                        return Err(SemanticError {
                            message: format!("`{name}` takes no arguments"),
                            span,
                        });
                    }
                    Ok(Type::Unit)
                }
                "__rune_builtin_terminal_set_title" => {
                    if args.len() != 1 {
                        return Err(SemanticError {
                            message: "`__rune_builtin_terminal_set_title` expects 1 argument"
                                .to_string(),
                            span,
                        });
                    }
                    let [CallArg::Positional(title_expr)] = args else {
                        return Err(SemanticError {
                            message:
                                "`__rune_builtin_terminal_set_title` does not accept keyword arguments"
                                    .to_string(),
                            span,
                        });
                    };
                    let title_ty = self.check_expr(title_expr, scope, in_async)?;
                    self.expect_type(
                        &title_ty,
                        &Type::String,
                        title_expr.span,
                        "terminal title",
                    )?;
                    Ok(Type::Unit)
                }
                "__rune_builtin_audio_bell" => {
                    if !args.is_empty() {
                        return Err(SemanticError {
                            message: "`__rune_builtin_audio_bell` takes no arguments".to_string(),
                            span,
                        });
                    }
                    Ok(Type::Bool)
                }
                _ => {
                    let Some(sig) = self.functions.get(name) else {
                        return Err(SemanticError {
                            message: format!("unknown function `{name}`"),
                            span,
                        });
                    };

                    let resolved = self.resolve_call_args(name, &sig.params, args, span)?;
                    for ((_, expected), arg) in sig.params.iter().zip(resolved.iter()) {
                        let actual = self.check_expr(arg, scope, in_async)?;
                        self.expect_type(&actual, expected, arg.span, "function argument")?;
                    }

                    Ok(sig.return_type.clone())
                }
            },
            _ => Err(SemanticError {
                message: "only direct function calls are currently supported semantically"
                    .to_string(),
                span: callee.span,
            }),
        }
    }

    fn expect_type(
        &self,
        actual: &Type,
        expected: &Type,
        span: Span,
        context: &str,
    ) -> Result<(), SemanticError> {
        if actual == expected {
            Ok(())
        } else if self.can_widen_integer(actual, expected) {
            Ok(())
        } else if *expected == Type::Dynamic && self.can_promote_to_dynamic(actual) {
            Ok(())
        } else {
            Err(SemanticError {
                message: format!(
                    "{context} expected `{}`, found `{}`",
                    expected.display_name(),
                    actual.display_name()
                ),
                span,
            })
        }
    }

    fn can_promote_to_dynamic(&self, actual: &Type) -> bool {
        matches!(
            actual,
            Type::Bool | Type::Dynamic | Type::I32 | Type::I64 | Type::String
        )
    }

    fn is_integer_pair(&self, left: &Type, right: &Type) -> bool {
        matches!(
            (left, right),
            (Type::I32, Type::I64) | (Type::I64, Type::I32)
        )
    }

    fn can_widen_integer(&self, actual: &Type, expected: &Type) -> bool {
        matches!((actual, expected), (Type::I32, Type::I64))
    }

    fn resolve_regular_type(&self, ty: &TypeRef) -> Result<Type, SemanticError> {
        match Type::from_type_ref(ty)? {
            Type::Unknown(name) if self.structs.contains_key(&name) => Ok(Type::Struct(name)),
            Type::Unknown(name) => Err(SemanticError {
                message: format!("unknown type `{name}`"),
                span: ty.span,
            }),
            Type::Exception(name) => Err(SemanticError {
                message: format!("exception type `{name}` is only allowed in `raises`"),
                span: ty.span,
            }),
            other => Ok(other),
        }
    }

    fn resolve_exception_type(&self, ty: &TypeRef) -> Result<Type, SemanticError> {
        match Type::from_type_ref(ty)? {
            Type::Unknown(name) if self.exceptions.contains(&name) => Ok(Type::Exception(name)),
            Type::String => Ok(Type::String),
            Type::Unknown(name) => Err(SemanticError {
                message: format!("unknown exception type `{name}`"),
                span: ty.span,
            }),
            other => Err(SemanticError {
                message: format!(
                    "`raises` expects `String` or a declared exception type, found `{}`",
                    other.display_name()
                ),
                span: ty.span,
            }),
        }
    }

    fn is_dynamic_add_supported(&self, left: &Type, right: &Type) -> bool {
        let supported = |ty: &Type| {
            matches!(
                ty,
                Type::Bool | Type::Dynamic | Type::I32 | Type::I64 | Type::String
            )
        };
        supported(left) && supported(right) && (*left == Type::Dynamic || *right == Type::Dynamic)
    }

    fn is_dynamic_equality_supported(&self, left: &Type, right: &Type) -> bool {
        let supported = |ty: &Type| {
            matches!(
                ty,
                Type::Bool | Type::Dynamic | Type::I32 | Type::I64 | Type::String
            )
        };
        supported(left) && supported(right) && (*left == Type::Dynamic || *right == Type::Dynamic)
    }

    fn is_dynamic_ordering_supported(&self, left: &Type, right: &Type) -> bool {
        let supported =
            |ty: &Type| matches!(ty, Type::Bool | Type::Dynamic | Type::I32 | Type::I64);
        supported(left) && supported(right) && (*left == Type::Dynamic || *right == Type::Dynamic)
    }

    fn is_dynamic_numeric_supported(&self, left: &Type, right: &Type) -> bool {
        let supported =
            |ty: &Type| matches!(ty, Type::Bool | Type::Dynamic | Type::I32 | Type::I64);
        supported(left) && supported(right) && (*left == Type::Dynamic || *right == Type::Dynamic)
    }

    fn is_truthy_type(&self, ty: &Type) -> bool {
        matches!(ty, Type::Bool | Type::Dynamic)
    }

    fn expect_condition_type(
        &self,
        actual: &Type,
        span: Span,
        context: &str,
    ) -> Result<(), SemanticError> {
        if self.is_truthy_type(actual) {
            Ok(())
        } else {
            Err(SemanticError {
                message: format!(
                    "{context} expected `bool` or `dynamic`, found `{}`",
                    actual.display_name()
                ),
                span,
            })
        }
    }

    fn resolve_call_args<'b>(
        &self,
        function_name: &str,
        params: &[(String, Type)],
        args: &'b [CallArg],
        span: Span,
    ) -> Result<Vec<&'b Expr>, SemanticError> {
        let mut resolved: Vec<Option<&Expr>> = vec![None; params.len()];
        let mut positional_index = 0usize;
        let mut saw_keyword = false;

        for arg in args {
            match arg {
                CallArg::Positional(expr) => {
                    if saw_keyword {
                        return Err(SemanticError {
                            message: format!(
                                "positional arguments cannot appear after keyword arguments in `{function_name}`"
                            ),
                            span: expr.span,
                        });
                    }
                    if positional_index >= params.len() {
                        return Err(SemanticError {
                            message: format!(
                                "function `{function_name}` expects {} arguments but got {}",
                                params.len(),
                                args.len()
                            ),
                            span: expr.span,
                        });
                    }
                    resolved[positional_index] = Some(expr);
                    positional_index += 1;
                }
                CallArg::Keyword {
                    name,
                    value,
                    span: kw_span,
                } => {
                    saw_keyword = true;
                    let Some(index) = params.iter().position(|(param_name, _)| param_name == name)
                    else {
                        return Err(SemanticError {
                            message: format!(
                                "function `{function_name}` has no parameter named `{name}`"
                            ),
                            span: *kw_span,
                        });
                    };
                    if resolved[index].is_some() {
                        return Err(SemanticError {
                            message: format!("parameter `{name}` was provided more than once"),
                            span: *kw_span,
                        });
                    }
                    resolved[index] = Some(value);
                }
            }
        }

        if resolved.iter().any(|arg| arg.is_none()) {
            return Err(SemanticError {
                message: format!(
                    "function `{function_name}` expects {} arguments but got {}",
                    params.len(),
                    args.len()
                ),
                span,
            });
        }

        Ok(resolved
            .into_iter()
            .map(|arg| arg.expect("checked above"))
            .collect())
    }
}

fn is_supported_extern_ffi_type(ty: &Type) -> bool {
    matches!(ty, Type::Bool | Type::I32 | Type::I64 | Type::String | Type::Unit)
}

fn builtin_function_type(name: &str) -> Option<Type> {
    match name {
        "print" | "println" | "eprint" | "eprintln" | "flush" | "eflush" => {
            Some(Type::Unknown("builtin".to_string()))
        }
        "input" => Some(Type::Unknown("builtin".to_string())),
        "str" => Some(Type::Unknown("builtin".to_string())),
        "int" => Some(Type::Unknown("builtin".to_string())),
        "__rune_builtin_time_now_unix" => Some(Type::Unknown("builtin".to_string())),
        "__rune_builtin_time_monotonic_ms" => Some(Type::Unknown("builtin".to_string())),
        "__rune_builtin_time_sleep_ms" => Some(Type::Unknown("builtin".to_string())),
        "__rune_builtin_system_pid" => Some(Type::Unknown("builtin".to_string())),
        "__rune_builtin_system_cpu_count" => Some(Type::Unknown("builtin".to_string())),
        "__rune_builtin_system_exit" => Some(Type::Unknown("builtin".to_string())),
        "__rune_builtin_env_exists" => Some(Type::Unknown("builtin".to_string())),
        "__rune_builtin_env_get_i32" => Some(Type::Unknown("builtin".to_string())),
        "__rune_builtin_env_get_bool" => Some(Type::Unknown("builtin".to_string())),
        "__rune_builtin_env_arg_count" => Some(Type::Unknown("builtin".to_string())),
        "__rune_builtin_network_tcp_connect" => Some(Type::Unknown("builtin".to_string())),
        "__rune_builtin_network_tcp_connect_timeout" => Some(Type::Unknown("builtin".to_string())),
        "__rune_builtin_fs_exists" => Some(Type::Unknown("builtin".to_string())),
        "__rune_builtin_fs_read_string" => Some(Type::Unknown("builtin".to_string())),
        "__rune_builtin_fs_write_string" => Some(Type::Unknown("builtin".to_string())),
        "__rune_builtin_terminal_clear" => Some(Type::Unknown("builtin".to_string())),
        "__rune_builtin_terminal_move_to" => Some(Type::Unknown("builtin".to_string())),
        "__rune_builtin_terminal_hide_cursor" => Some(Type::Unknown("builtin".to_string())),
        "__rune_builtin_terminal_show_cursor" => Some(Type::Unknown("builtin".to_string())),
        "__rune_builtin_terminal_set_title" => Some(Type::Unknown("builtin".to_string())),
        "__rune_builtin_audio_bell" => Some(Type::Unknown("builtin".to_string())),
        _ => None,
    }
}

#[derive(Debug, Clone, Default)]
struct Scope {
    values: HashMap<String, Type>,
}
