use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt;

/// Parse a Rune integer literal (decimal, 0x hex, 0o octal, 0b binary, _ separators) to i64.
pub fn parse_integer_literal_str(s: &str) -> i64 {
    let clean = s.replace('_', "");
    if let Some(hex) = clean.strip_prefix("0x").or_else(|| clean.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16).unwrap_or(0)
    } else if let Some(oct) = clean.strip_prefix("0o").or_else(|| clean.strip_prefix("0O")) {
        i64::from_str_radix(oct, 8).unwrap_or(0)
    } else if let Some(bin) = clean.strip_prefix("0b").or_else(|| clean.strip_prefix("0B")) {
        i64::from_str_radix(bin, 2).unwrap_or(0)
    } else {
        clean.parse::<i64>().unwrap_or(0)
    }
}

use crate::frontend::parser::{
    BinaryOp, Block, CallArg, ElifBlock, Expr, ExprKind, Function, IfStmt, Item,
    LetStmt, Program, ReturnStmt, Stmt, StructDecl, TypeRef, UnaryOp, WhileStmt,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrProgram {
    pub functions: Vec<IrFunction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrFunction {
    pub name: String,
    pub locals: Vec<IrLocal>,
    pub instructions: Vec<IrInst>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrLocal {
    pub name: String,
    pub ty: IrType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IrType {
    Bool,
    Dynamic,
    I32,
    I64,
    Json,
    String,
    Struct(String),
    Unit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IrInst {
    ConstInt {
        dst: String,
        value: String,
    },
    ConstBool {
        dst: String,
        value: bool,
    },
    ConstString {
        dst: String,
        value: String,
    },
    Copy {
        dst: String,
        src: String,
    },
    UnaryNeg {
        dst: String,
        src: String,
    },
    UnaryNot {
        dst: String,
        src: String,
    },
    UnaryBitwiseNot {
        dst: String,
        src: String,
    },
    /// Mutate a field on a local struct: `base.field = src`
    SetField {
        base: String,
        field: String,
        src: String,
    },
    Binary {
        dst: String,
        op: BinaryOp,
        left: String,
        right: String,
    },
    Call {
        dst: Option<String>,
        callee: String,
        args: Vec<IrArg>,
    },
    Label(String),
    BranchIf {
        cond: String,
        then_label: String,
        else_label: String,
    },
    Jump(String),
    Return(Option<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrArg {
    pub name: Option<String>,
    pub value: String,
}

pub fn lower_program(program: &Program) -> IrProgram {
    let struct_layouts = collect_struct_layouts(program);
    let struct_methods = collect_struct_methods(program);
    let function_returns = collect_function_returns(program);
    let mut functions = Vec::new();
    for item in &program.items {
        match item {
            Item::Function(function) => {
                if function.is_extern {
                    continue;
                }
                let locals = analyze_locals(function, None, &struct_layouts, &struct_methods, &function_returns);
                let local_types = locals
                    .iter()
                    .map(|local| (local.name.clone(), local.ty.clone()))
                    .collect::<BTreeMap<_, _>>();
                let mut lowerer = Lowerer::new(
                    function.name.clone(),
                    local_types,
                    &struct_layouts,
                    &struct_methods,
                    &function_returns,
                );
                lowerer.lower_block(&function.body);
                functions.push(IrFunction {
                    name: function.name.clone(),
                    locals,
                    instructions: lowerer.instructions,
                });
            }
            Item::Struct(decl) => {
                for method in &decl.methods {
                    if method.is_extern {
                        continue;
                    }
                    let method_name = struct_method_symbol(&decl.name, &method.name);
                    let locals = analyze_locals(
                        method,
                        Some(&decl.name),
                        &struct_layouts,
                        &struct_methods,
                        &function_returns,
                    );
                    let local_types = locals
                        .iter()
                        .map(|local| (local.name.clone(), local.ty.clone()))
                        .collect::<BTreeMap<_, _>>();
                    let mut lowerer = Lowerer::new(
                        method_name.clone(),
                        local_types,
                        &struct_layouts,
                        &struct_methods,
                        &function_returns,
                    );
                    lowerer.lower_block(&method.body);
                    functions.push(IrFunction {
                        name: method_name,
                        locals,
                        instructions: lowerer.instructions,
                    });
                }
            }
            _ => {}
        }
    }
    IrProgram { functions }
}

fn analyze_locals(
    function: &Function,
    method_owner: Option<&str>,
    struct_layouts: &BTreeMap<String, BTreeMap<String, IrType>>,
    struct_methods: &BTreeMap<String, BTreeMap<String, MethodSig>>,
    function_returns: &BTreeMap<String, IrType>,
) -> Vec<IrLocal> {
    let mut infos = BTreeMap::<String, LocalInfo>::new();

    for (index, param) in function.params.iter().enumerate() {
        let ty = if let Some(owner) = method_owner {
            if index == 0 && param.name == "self" {
                IrType::Struct(owner.to_string())
            } else {
                ir_type_from_type_ref(Some(&param.ty))
            }
        } else {
            ir_type_from_type_ref(Some(&param.ty))
        };
        infos.insert(
            param.name.clone(),
            LocalInfo {
                ty: LocalType::Known(ty),
                reassigned: false,
            },
        );
    }

    collect_local_infos(
        &function.body,
        struct_layouts,
        struct_methods,
        function_returns,
        &mut infos,
    );

    infos
        .into_iter()
        .map(|(name, info)| IrLocal {
            name,
            ty: info.ty.specialized(!info.reassigned),
        })
        .collect()
}

fn collect_local_infos(
    block: &Block,
    struct_layouts: &BTreeMap<String, BTreeMap<String, IrType>>,
    struct_methods: &BTreeMap<String, BTreeMap<String, MethodSig>>,
    function_returns: &BTreeMap<String, IrType>,
    infos: &mut BTreeMap<String, LocalInfo>,
) {
    for stmt in &block.statements {
        match stmt {
            Stmt::Block(stmt) => {
                collect_local_infos(
                    &stmt.block,
                    struct_layouts,
                    struct_methods,
                    function_returns,
                    infos,
                )
            }
            Stmt::Let(stmt) => {
                infos.insert(
                    stmt.name.clone(),
                    LocalInfo {
                        ty: infer_declared_or_expr_type(
                    stmt.ty.as_ref(),
                    &stmt.value,
                    infos,
                    struct_layouts,
                    struct_methods,
                    function_returns,
                ),
                        reassigned: false,
                    },
                );
            }
            Stmt::Assign(stmt) => {
                // Infer the rhs type first (immutable borrow) before taking &mut.
                let rhs_ty = infer_expr_type(
                    &stmt.value,
                    infos,
                    struct_layouts,
                    struct_methods,
                    function_returns,
                );
                if let Some(info) = infos.get_mut(&stmt.name) {
                    // If the rhs type is consistent with the current candidate, the
                    // variable's type is stable across reassignment and we can keep the
                    // candidate.  If types diverge, fall back to Dynamic.
                    let candidate_ty = info.ty.specialized(true);
                    let type_consistent = match &rhs_ty {
                        Some(rhs) => *rhs == candidate_ty || candidate_ty == IrType::Dynamic,
                        None => false,
                    };
                    if !type_consistent {
                        info.reassigned = true;
                    }
                }
            }
            Stmt::If(stmt) => {
                collect_local_infos(&stmt.then_block, struct_layouts, struct_methods, function_returns, infos);
                for elif in &stmt.elif_blocks {
                    collect_local_infos(&elif.block, struct_layouts, struct_methods, function_returns, infos);
                }
                if let Some(block) = &stmt.else_block {
                    collect_local_infos(block, struct_layouts, struct_methods, function_returns, infos);
                }
            }
            Stmt::While(stmt) => {
                collect_local_infos(&stmt.body, struct_layouts, struct_methods, function_returns, infos)
            }
            Stmt::FieldAssign(_) => {
                // Field assignment doesn't introduce new locals; the base variable
                // already exists and its type stays the same (struct mutation).
            }
            Stmt::Break(_) | Stmt::Continue(_) | Stmt::Return(_) | Stmt::Raise(_) | Stmt::Panic(_) | Stmt::Expr(_) => {}
        }
    }
}

fn infer_declared_or_expr_type(
    declared: Option<&TypeRef>,
    expr: &Expr,
    infos: &BTreeMap<String, LocalInfo>,
    struct_layouts: &BTreeMap<String, BTreeMap<String, IrType>>,
    struct_methods: &BTreeMap<String, BTreeMap<String, MethodSig>>,
    function_returns: &BTreeMap<String, IrType>,
) -> LocalType {
    match declared {
        Some(ty) => {
            let declared_ir = ir_type_from_type_ref(Some(ty));
            if declared_ir == IrType::Dynamic {
                infer_expr_type(expr, infos, struct_layouts, struct_methods, function_returns)
                    .map(LocalType::Candidate)
                    .unwrap_or(LocalType::Known(IrType::Dynamic))
            } else {
                LocalType::Known(declared_ir)
            }
        }
        None => infer_expr_type(expr, infos, struct_layouts, struct_methods, function_returns)
            .map(LocalType::Candidate)
            .unwrap_or(LocalType::Known(IrType::Dynamic)),
    }
}

fn infer_expr_type(
    expr: &Expr,
    infos: &BTreeMap<String, LocalInfo>,
    struct_layouts: &BTreeMap<String, BTreeMap<String, IrType>>,
    struct_methods: &BTreeMap<String, BTreeMap<String, MethodSig>>,
    function_returns: &BTreeMap<String, IrType>,
) -> Option<IrType> {
    match &expr.kind {
        ExprKind::Identifier(name) => infos.get(name).map(|info| info.ty.specialized(true)),
        ExprKind::Integer(value) => {
            if value.parse::<i32>().is_ok() {
                Some(IrType::I32)
            } else {
                Some(IrType::I64)
            }
        }
        ExprKind::String(_) => Some(IrType::String),
        ExprKind::Bool(_) => Some(IrType::Bool),
        ExprKind::Unary {
            op: UnaryOp::Negate,
            expr,
        } => infer_expr_type(expr, infos, struct_layouts, struct_methods, function_returns),
        ExprKind::Unary {
            op: UnaryOp::Not, ..
        } => Some(IrType::Bool),
        ExprKind::Unary {
            op: UnaryOp::BitwiseNot,
            expr,
        } => infer_expr_type(expr, infos, struct_layouts, struct_methods, function_returns),
        ExprKind::Binary { left, op, right } => {
            let left_ty = infer_expr_type(left, infos, struct_layouts, struct_methods, function_returns)?;
            let right_ty = infer_expr_type(right, infos, struct_layouts, struct_methods, function_returns)?;
            match op {
                BinaryOp::And | BinaryOp::Or => Some(IrType::Bool),
                BinaryOp::Add => {
                    if left_ty == right_ty && matches!(left_ty, IrType::I32 | IrType::I64) {
                        Some(left_ty)
                    } else if matches!(
                        (&left_ty, &right_ty),
                        (
                            IrType::Dynamic,
                            IrType::Bool
                                | IrType::Dynamic
                                | IrType::I32
                                | IrType::I64
                                | IrType::Json
                                | IrType::String
                        ) | (
                            IrType::Bool
                                | IrType::I32
                                | IrType::I64
                                | IrType::Json
                                | IrType::String,
                            IrType::Dynamic
                        )
                    ) {
                        Some(IrType::Dynamic)
                    } else {
                        None
                    }
                }
                BinaryOp::Subtract
                | BinaryOp::Multiply
                | BinaryOp::Divide
                | BinaryOp::Modulo => {
                    if left_ty == right_ty && matches!(left_ty, IrType::I32 | IrType::I64) {
                        Some(left_ty)
                    } else if matches!(
                        (&left_ty, &right_ty),
                        (
                            IrType::Dynamic,
                            IrType::Bool
                                | IrType::Dynamic
                                | IrType::I32
                                | IrType::I64
                                | IrType::Json
                        ) | (
                            IrType::Bool | IrType::I32 | IrType::I64 | IrType::Json,
                            IrType::Dynamic
                        )
                    ) {
                        Some(IrType::Dynamic)
                    } else {
                        None
                    }
                }
                BinaryOp::EqualEqual
                | BinaryOp::NotEqual
                | BinaryOp::Greater
                | BinaryOp::GreaterEqual
                | BinaryOp::Less
                | BinaryOp::LessEqual => Some(IrType::Bool),
                BinaryOp::BitwiseAnd
                | BinaryOp::BitwiseOr
                | BinaryOp::BitwiseXor
                | BinaryOp::ShiftLeft
                | BinaryOp::ShiftRight => {
                    if left_ty == right_ty && matches!(left_ty, IrType::I32 | IrType::I64) {
                        Some(left_ty)
                    } else {
                        Some(IrType::I64)
                    }
                }
            }
        }
        ExprKind::Call { callee, .. } => match &callee.kind {
            ExprKind::Field { base, name } => {
                let IrType::Struct(struct_name) =
                    infer_expr_type(base, infos, struct_layouts, struct_methods, function_returns)?
                else {
                    return None;
                };
                struct_methods
                    .get(&struct_name)
                    .and_then(|methods| methods.get(name))
                    .map(|sig| sig.return_type.clone())
            }
            ExprKind::Identifier(name) if struct_layouts.contains_key(name) => {
                Some(IrType::Struct(name.clone()))
            }
            ExprKind::Identifier(name) => builtin_return_type(name)
                .or_else(|| function_returns.get(name).cloned()),
            _ => None,
        },
        ExprKind::Await { .. } => None,
        ExprKind::Field { base, name } => {
            let IrType::Struct(struct_name) =
                infer_expr_type(base, infos, struct_layouts, struct_methods, function_returns)?
            else {
                return None;
            };
            struct_layouts
                .get(&struct_name)
                .and_then(|fields| fields.get(name))
                .cloned()
        }
    }
}

fn ir_type_from_type_ref(ty: Option<&TypeRef>) -> IrType {
    match ty.map(|ty| ty.name.as_str()) {
        Some("bool") => IrType::Bool,
        Some("i32") => IrType::I32,
        Some("i64") => IrType::I64,
        Some("Json") => IrType::Json,
        Some("String") | Some("str") => IrType::String,
        Some("unit") => IrType::Unit,
        Some("dynamic") | None => IrType::Dynamic,
        Some(name) => IrType::Struct(name.to_string()),
    }
}

fn collect_struct_layouts(program: &Program) -> BTreeMap<String, BTreeMap<String, IrType>> {
    program
        .items
        .iter()
        .filter_map(|item| {
            let Item::Struct(StructDecl { name, fields, .. }) = item else {
                return None;
            };
            Some((
                name.clone(),
                fields
                    .iter()
                    .map(|field| (field.name.clone(), ir_type_from_type_ref(Some(&field.ty))))
                    .collect::<BTreeMap<_, _>>(),
            ))
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LocalInfo {
    ty: LocalType,
    reassigned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LocalType {
    Known(IrType),
    Candidate(IrType),
}

impl LocalType {
    fn specialized(&self, allow_candidate: bool) -> IrType {
        match self {
            LocalType::Known(ty) => ty.clone(),
            LocalType::Candidate(ty) if allow_candidate => ty.clone(),
            LocalType::Candidate(_) => IrType::Dynamic,
        }
    }
}

struct Lowerer {
    function_name: String,
    local_types: BTreeMap<String, IrType>,
    struct_layouts: BTreeMap<String, BTreeMap<String, IrType>>,
    struct_methods: BTreeMap<String, BTreeMap<String, MethodSig>>,
    function_returns: BTreeMap<String, IrType>,
    instructions: Vec<IrInst>,
    temp_counter: usize,
    label_counter: usize,
    loop_labels: Vec<(String, String)>,
}

impl Lowerer {
    fn new(
        function_name: String,
        local_types: BTreeMap<String, IrType>,
        struct_layouts: &BTreeMap<String, BTreeMap<String, IrType>>,
        struct_methods: &BTreeMap<String, BTreeMap<String, MethodSig>>,
        function_returns: &BTreeMap<String, IrType>,
    ) -> Self {
        Self {
            function_name,
            local_types,
            struct_layouts: struct_layouts.clone(),
            struct_methods: struct_methods.clone(),
            function_returns: function_returns.clone(),
            instructions: Vec::new(),
            temp_counter: 0,
            label_counter: 0,
            loop_labels: Vec::new(),
        }
    }

    fn lower_block(&mut self, block: &Block) {
        for stmt in &block.statements {
            self.lower_stmt(stmt);
        }
    }

    fn lower_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Block(stmt) => self.lower_block(&stmt.block),
            Stmt::Let(stmt) => {
                let value = self.lower_expr(&stmt.value);
                self.instructions.push(IrInst::Copy {
                    dst: stmt.name.clone(),
                    src: value,
                });
            }
            Stmt::Assign(stmt) => {
                let value = self.lower_expr(&stmt.value);
                self.instructions.push(IrInst::Copy {
                    dst: stmt.name.clone(),
                    src: value,
                });
            }
            Stmt::Return(stmt) => {
                let value = stmt.value.as_ref().map(|expr| self.lower_expr(expr));
                self.instructions.push(IrInst::Return(value));
            }
            Stmt::Expr(stmt) => {
                let _ = self.lower_expr(&stmt.expr);
            }
            Stmt::If(stmt) => {
                let cond = self.lower_expr(&stmt.condition);
                let then_label = self.next_label("if_then");
                let mut end_label = self.next_label("if_end");
                let else_label = self.next_label("if_else");
                self.instructions.push(IrInst::BranchIf {
                    cond,
                    then_label: then_label.clone(),
                    else_label: else_label.clone(),
                });
                self.instructions.push(IrInst::Label(then_label));
                self.lower_block(&stmt.then_block);
                self.instructions.push(IrInst::Jump(end_label.clone()));
                self.instructions.push(IrInst::Label(else_label));

                let mut remaining_else = stmt.else_block.clone();
                for elif in &stmt.elif_blocks {
                    let elif_cond = self.lower_expr(&elif.condition);
                    let elif_then = self.next_label("elif_then");
                    let elif_else = self.next_label("elif_else");
                    self.instructions.push(IrInst::BranchIf {
                        cond: elif_cond,
                        then_label: elif_then.clone(),
                        else_label: elif_else.clone(),
                    });
                    self.instructions.push(IrInst::Label(elif_then));
                    self.lower_block(&elif.block);
                    self.instructions.push(IrInst::Jump(end_label.clone()));
                    self.instructions.push(IrInst::Label(elif_else));
                    remaining_else = None;
                }

                if let Some(block) = &remaining_else {
                    self.lower_block(block);
                }
                self.instructions
                    .push(IrInst::Label(std::mem::take(&mut end_label)));
            }
            Stmt::While(stmt) => {
                let loop_label = self.next_label("while_loop");
                let body_label = self.next_label("while_body");
                let end_label = self.next_label("while_end");
                self.instructions.push(IrInst::Label(loop_label.clone()));
                let cond = self.lower_expr(&stmt.condition);
                self.instructions.push(IrInst::BranchIf {
                    cond,
                    then_label: body_label.clone(),
                    else_label: end_label.clone(),
                });
                self.instructions.push(IrInst::Label(body_label));
                self.loop_labels
                    .push((loop_label.clone(), end_label.clone()));
                self.lower_block(&stmt.body);
                self.loop_labels.pop();
                self.instructions.push(IrInst::Jump(loop_label));
                self.instructions.push(IrInst::Label(end_label));
            }
            Stmt::Break(_) => {
                let (_, break_label) = self
                    .loop_labels
                    .last()
                    .expect("semantic analysis should reject `break` outside a loop");
                self.instructions.push(IrInst::Jump(break_label.clone()));
            }
            Stmt::Continue(_) => {
                let (continue_label, _) = self
                    .loop_labels
                    .last()
                    .expect("semantic analysis should reject `continue` outside a loop");
                self.instructions.push(IrInst::Jump(continue_label.clone()));
            }
            Stmt::FieldAssign(stmt) => {
                let src = self.lower_expr(&stmt.value);
                // For multi-level paths (a.b.c = v), emit a chain of SetField.
                // The IR models this as sequential field mutations on the base.
                // For a single field this is: SetField { base, field, src }.
                // For nested fields, the intermediate struct must be treated as
                // a mutable local — emit a SetField chain bottom-up.
                if stmt.fields.len() == 1 {
                    self.instructions.push(IrInst::SetField {
                        base: stmt.base.clone(),
                        field: stmt.fields[0].clone(),
                        src,
                    });
                } else {
                    // For a.b.c = v: create temp for each level, insertvalue from inside out.
                    // Emit as nested SetField pairs resolved by backend.
                    // Simple approach: emit one SetField per level; backends handle nesting.
                    let last_field = stmt.fields.last().unwrap().clone();
                    let inner_base = if stmt.fields.len() > 1 {
                        // read the intermediate struct into a temp
                        let mut read_expr = crate::parser::Expr {
                            kind: ExprKind::Identifier(stmt.base.clone()),
                            span: crate::lexer::Span { line: 1, column: 1 },
                        };
                        for f in &stmt.fields[..stmt.fields.len() - 1] {
                            read_expr = crate::parser::Expr {
                                kind: ExprKind::Field {
                                    base: Box::new(read_expr),
                                    name: f.clone(),
                                },
                                span: crate::lexer::Span { line: 1, column: 1 },
                            };
                        }
                        self.lower_expr(&read_expr)
                    } else {
                        stmt.base.clone()
                    };
                    // Just set the inner field on whatever struct the path resolves to.
                    // For now collapse to single-level SetField on the immediate parent.
                    let parent = if stmt.fields.len() > 1 {
                        inner_base
                    } else {
                        stmt.base.clone()
                    };
                    self.instructions.push(IrInst::SetField {
                        base: parent,
                        field: last_field,
                        src,
                    });
                }
            }
            Stmt::Raise(stmt) => {
                let value = self.lower_expr(&stmt.value);
                self.instructions.push(IrInst::Call {
                    dst: None,
                    callee: "raise".to_string(),
                    args: vec![IrArg { name: None, value }],
                });
            }
            Stmt::Panic(stmt) => {
                let value = self.lower_expr(&stmt.value);
                let context = self.next_temp();
                self.instructions.push(IrInst::ConstString {
                    dst: context.clone(),
                    value: format!("panic in {} at line {}", self.function_name, stmt.span.line),
                });
                self.instructions.push(IrInst::Call {
                    dst: None,
                    callee: "panic".to_string(),
                    args: vec![
                        IrArg { name: None, value },
                        IrArg {
                            name: None,
                            value: context,
                        },
                    ],
                });
            }
        }
    }

    fn lower_expr(&mut self, expr: &Expr) -> String {
        match &expr.kind {
            ExprKind::Identifier(name) => name.clone(),
            ExprKind::Integer(value) => {
                let dst = self.next_temp();
                let normalized = parse_integer_literal_str(value).to_string();
                self.instructions.push(IrInst::ConstInt {
                    dst: dst.clone(),
                    value: normalized,
                });
                dst
            }
            ExprKind::String(value) => {
                let dst = self.next_temp();
                self.instructions.push(IrInst::ConstString {
                    dst: dst.clone(),
                    value: value.clone(),
                });
                dst
            }
            ExprKind::Bool(value) => {
                let dst = self.next_temp();
                self.instructions.push(IrInst::ConstBool {
                    dst: dst.clone(),
                    value: *value,
                });
                dst
            }
            ExprKind::Unary { op, expr } => {
                let src = self.lower_expr(expr);
                let dst = self.next_temp();
                match op {
                    UnaryOp::Negate => self.instructions.push(IrInst::UnaryNeg {
                        dst: dst.clone(),
                        src,
                    }),
                    UnaryOp::Not => self.instructions.push(IrInst::UnaryNot {
                        dst: dst.clone(),
                        src,
                    }),
                    UnaryOp::BitwiseNot => self.instructions.push(IrInst::UnaryBitwiseNot {
                        dst: dst.clone(),
                        src,
                    }),
                }
                dst
            }
            ExprKind::Binary { left, op, right } => {
                let left = self.lower_expr(left);
                let right = self.lower_expr(right);
                let dst = self.next_temp();
                self.instructions.push(IrInst::Binary {
                    dst: dst.clone(),
                    op: *op,
                    left,
                    right,
                });
                dst
            }
            ExprKind::Call { callee, args } => {
                let (callee, args) = match &callee.kind {
                    ExprKind::Identifier(name) => (
                        name.clone(),
                        args.iter()
                            .map(|arg| match arg {
                                CallArg::Positional(expr) => IrArg {
                                    name: None,
                                    value: self.lower_expr(expr),
                                },
                                CallArg::Keyword { name, value, .. } => IrArg {
                                    name: Some(name.clone()),
                                    value: self.lower_expr(value),
                                },
                            })
                            .collect::<Vec<_>>(),
                    ),
                    ExprKind::Field { base, name } => {
                        let base_value = self.lower_expr(base);
                        let base_expr_ty = infer_expr_type(
                            base,
                            &self
                                .local_types
                                .iter()
                                .map(|(name, ty)| {
                                    (
                                        name.clone(),
                                        LocalInfo {
                                            ty: LocalType::Known(ty.clone()),
                                            reassigned: false,
                                        },
                                    )
                                })
                                .collect::<BTreeMap<_, _>>(),
                            &self.struct_layouts,
                            &self.struct_methods,
                            &self.function_returns,
                        );
                        let callee_name = if base_expr_ty == Some(IrType::String) {
                            string_method_symbol(name)
                        } else if let Some(IrType::Struct(struct_name)) = base_expr_ty {
                            if self
                                .struct_methods
                                .get(&struct_name)
                                .is_some_and(|methods| methods.contains_key(name))
                            {
                                struct_method_symbol(&struct_name, name)
                            } else {
                                name.clone()
                            }
                        } else {
                            name.clone()
                        };
                        let mut lowered_args = vec![IrArg {
                            name: None,
                            value: base_value,
                        }];
                        lowered_args.extend(args.iter().map(|arg| match arg {
                            CallArg::Positional(expr) => IrArg {
                                name: None,
                                value: self.lower_expr(expr),
                            },
                            CallArg::Keyword { name, value, .. } => IrArg {
                                name: Some(name.clone()),
                                value: self.lower_expr(value),
                            },
                        }));
                        (callee_name, lowered_args)
                    }
                    _ => ("<expr>".to_string(), Vec::new()),
                };
                let dst = self.next_temp();
                self.instructions.push(IrInst::Call {
                    dst: Some(dst.clone()),
                    callee,
                    args,
                });
                dst
            }
            ExprKind::Await { expr } => {
                let value = self.lower_expr(expr);
                let dst = self.next_temp();
                self.instructions.push(IrInst::Call {
                    dst: Some(dst.clone()),
                    callee: "await".to_string(),
                    args: vec![IrArg { name: None, value }],
                });
                dst
            }
            ExprKind::Field { base, name } => {
                let base = self.lower_expr(base);
                let dst = self.next_temp();
                self.instructions.push(IrInst::Call {
                    dst: Some(dst.clone()),
                    callee: format!("field.{name}"),
                    args: vec![IrArg {
                        name: Some("base".to_string()),
                        value: base,
                    }],
                });
                dst
            }
        }
    }

    fn next_temp(&mut self) -> String {
        let value = format!("%t{}", self.temp_counter);
        self.temp_counter += 1;
        value
    }

    fn next_label(&mut self, prefix: &str) -> String {
        let value = format!("{prefix}_{}", self.label_counter);
        self.label_counter += 1;
        value
    }
}

impl fmt::Display for IrProgram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for function in &self.functions {
            writeln!(f, "fn {}:", function.name)?;
            for local in &function.locals {
                writeln!(f, "  local {}: {}", local.name, local.ty)?;
            }
            for inst in &function.instructions {
                writeln!(f, "  {inst}")?;
            }
        }
        Ok(())
    }
}

impl fmt::Display for IrInst {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IrInst::ConstInt { dst, value } => write!(f, "{dst} = const.int {value}"),
            IrInst::ConstBool { dst, value } => write!(f, "{dst} = const.bool {value}"),
            IrInst::ConstString { dst, value } => write!(f, "{dst} = const.str {:?}", value),
            IrInst::Copy { dst, src } => write!(f, "{dst} = copy {src}"),
            IrInst::UnaryNeg { dst, src } => write!(f, "{dst} = neg {src}"),
            IrInst::UnaryNot { dst, src } => write!(f, "{dst} = not {src}"),
            IrInst::UnaryBitwiseNot { dst, src } => write!(f, "{dst} = bnot {src}"),
            IrInst::SetField { base, field, src } => write!(f, "{base}.{field} = {src}"),
            IrInst::Binary {
                dst,
                op,
                left,
                right,
            } => write!(f, "{dst} = {:?} {left}, {right}", op),
            IrInst::Call { dst, callee, args } => {
                if let Some(dst) = dst {
                    write!(f, "{dst} = call {callee}(")?;
                } else {
                    write!(f, "call {callee}(")?;
                }
                for (index, arg) in args.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    if let Some(name) = &arg.name {
                        write!(f, "{name}={}", arg.value)?;
                    } else {
                        write!(f, "{}", arg.value)?;
                    }
                }
                write!(f, ")")
            }
            IrInst::Label(label) => write!(f, "{label}:"),
            IrInst::BranchIf {
                cond,
                then_label,
                else_label,
            } => write!(f, "branch {cond} ? {then_label} : {else_label}"),
            IrInst::Jump(label) => write!(f, "jump {label}"),
            IrInst::Return(Some(value)) => write!(f, "return {value}"),
            IrInst::Return(None) => write!(f, "return"),
        }
    }
}

impl fmt::Display for IrType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IrType::Bool => write!(f, "bool"),
            IrType::Dynamic => write!(f, "dynamic"),
            IrType::I32 => write!(f, "i32"),
            IrType::I64 => write!(f, "i64"),
            IrType::Json => write!(f, "Json"),
            IrType::String => write!(f, "String"),
            IrType::Struct(name) => write!(f, "{name}"),
            IrType::Unit => write!(f, "unit"),
        }
    }
}

fn collect_function_returns(program: &Program) -> BTreeMap<String, IrType> {
    let mut out = BTreeMap::new();
    for item in &program.items {
        match item {
            Item::Function(function) => {
                let ty = function
                    .return_type
                    .as_ref()
                    .map(|ty| ir_type_from_type_ref(Some(ty)))
                    .unwrap_or(IrType::Unit);
                out.insert(function.name.clone(), ty);
            }
            Item::Struct(decl) => {
                for method in &decl.methods {
                    let ty = method
                        .return_type
                        .as_ref()
                        .map(|ty| ir_type_from_type_ref(Some(ty)))
                        .unwrap_or(IrType::Unit);
                    out.insert(struct_method_symbol(&decl.name, &method.name), ty);
                }
            }
            _ => {}
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MethodSig {
    return_type: IrType,
}

fn collect_struct_methods(program: &Program) -> BTreeMap<String, BTreeMap<String, MethodSig>> {
    program
        .items
        .iter()
        .filter_map(|item| {
            let Item::Struct(decl) = item else {
                return None;
            };
            Some((
                decl.name.clone(),
                decl.methods
                    .iter()
                    .map(|method| {
                        (
                            method.name.clone(),
                            MethodSig {
                                return_type: method
                                    .return_type
                                    .as_ref()
                                    .map(|ty| ir_type_from_type_ref(Some(ty)))
                                    .unwrap_or(IrType::Unit),
                            },
                        )
                    })
                    .collect::<BTreeMap<_, _>>(),
            ))
        })
        .collect()
}

fn struct_method_symbol(struct_name: &str, method_name: &str) -> String {
    format!("{struct_name}__{method_name}")
}

fn string_method_symbol(method_name: &str) -> String {
    format!("rune_rt_string_{method_name}")
}

fn builtin_return_type(name: &str) -> Option<IrType> {
    match name {
        "print" | "println" | "eprint" | "eprintln" | "flush" | "eflush" => Some(IrType::Unit),
        "input" => Some(IrType::String),
        "panic" => Some(IrType::Unit),
        "str" => Some(IrType::String),
        "int" => Some(IrType::I64),
        "rune_rt_string_len" => Some(IrType::I64),
        "rune_rt_string_upper"
        | "rune_rt_string_lower"
        | "rune_rt_string_replace"
        | "rune_rt_string_strip" => Some(IrType::String),
        "rune_rt_string_contains"
        | "rune_rt_string_starts_with"
        | "rune_rt_string_ends_with" => Some(IrType::Bool),
        "__rune_builtin_time_has_wall_clock" => Some(IrType::Bool),
        "__rune_builtin_time_now_unix" | "__rune_builtin_time_monotonic_ms" => Some(IrType::I64),
        "__rune_builtin_gpio_mode_input"
        | "__rune_builtin_gpio_mode_output"
        | "__rune_builtin_gpio_mode_input_pullup"
        | "__rune_builtin_gpio_pwm_duty_max"
        | "__rune_builtin_gpio_analog_max" => Some(IrType::I64),
        "__rune_builtin_time_sleep_ms"
        | "__rune_builtin_gpio_pin_mode"
        | "__rune_builtin_gpio_digital_write"
        | "__rune_builtin_gpio_pwm_write"
        | "__rune_builtin_system_exit"
        | "__rune_builtin_terminal_clear"
        | "__rune_builtin_terminal_move_to"
        | "__rune_builtin_terminal_hide_cursor"
        | "__rune_builtin_terminal_show_cursor"
        | "__rune_builtin_terminal_set_title" => Some(IrType::Unit),
        "__rune_builtin_system_pid"
        | "__rune_builtin_system_cpu_count"
        | "__rune_builtin_env_get_i32"
        | "__rune_builtin_env_arg_count" => Some(IrType::I32),
        "__rune_builtin_env_arg"
        | "__rune_builtin_env_get_string"
        | "__rune_builtin_gpio_analog_read"
        | "__rune_builtin_serial_available"
        | "__rune_builtin_serial_read_byte"
        | "__rune_builtin_serial_read_byte_timeout"
        | "__rune_builtin_arduino_uart_peek_byte"
        | "__rune_builtin_serial_peek_byte"
        | "__rune_builtin_serial_read_line"
        | "__rune_builtin_serial_read_line_timeout"
        | "__rune_builtin_network_tcp_recv"
        | "__rune_builtin_network_tcp_recv_timeout"
        | "__rune_builtin_network_tcp_accept_once"
        | "__rune_builtin_network_tcp_reply_once"
        | "__rune_builtin_network_tcp_server_accept"
        | "__rune_builtin_network_tcp_client_recv"
        | "__rune_builtin_network_tcp_server_reply"
        | "__rune_builtin_network_last_error_message"
        | "__rune_builtin_network_tcp_request"
        | "__rune_builtin_network_udp_recv" => Some(IrType::String),
        "__rune_builtin_network_last_error_code" => Some(IrType::I32),
        "__rune_builtin_network_tcp_server_open"
        | "__rune_builtin_network_tcp_client_open" => Some(IrType::I32),
        "__rune_builtin_env_exists"
        | "__rune_builtin_env_get_bool"
        | "__rune_builtin_gpio_digital_read"
        | "__rune_builtin_network_tcp_connect"
        | "__rune_builtin_network_tcp_listen"
        | "__rune_builtin_network_tcp_send"
        | "__rune_builtin_network_tcp_connect_timeout"
        | "__rune_builtin_network_tcp_client_send"
        | "__rune_builtin_network_tcp_server_close"
        | "__rune_builtin_network_tcp_client_close"
        | "__rune_builtin_network_udp_bind"
        | "__rune_builtin_network_udp_send"
        | "__rune_builtin_network_clear_error"
        | "__rune_builtin_serial_flush"
        | "__rune_builtin_serial_write_byte"
        | "__rune_builtin_fs_exists"
        | "__rune_builtin_fs_set_current_dir"
        | "__rune_builtin_fs_write_string"
        | "__rune_builtin_fs_append_string"
        | "__rune_builtin_fs_remove"
        | "__rune_builtin_fs_rename"
        | "__rune_builtin_fs_copy"
        | "__rune_builtin_fs_create_dir"
        | "__rune_builtin_fs_create_dir_all"
        | "__rune_builtin_fs_is_file"
        | "__rune_builtin_fs_is_dir"
        | "__rune_builtin_audio_bell"
        | "__rune_builtin_json_is_null"
        | "__rune_builtin_json_to_bool"
        | "__rune_builtin_arduino_servo_attach" => Some(IrType::Bool),
        "__rune_builtin_arduino_servo_detach"
        | "__rune_builtin_arduino_servo_write"
        | "__rune_builtin_arduino_servo_write_us" => Some(IrType::Unit),
        "__rune_builtin_fs_current_dir"
        | "__rune_builtin_fs_read_string"
        | "__rune_builtin_fs_canonicalize"
        | "__rune_builtin_json_stringify"
        | "__rune_builtin_json_kind"
        | "__rune_builtin_json_to_string" => Some(IrType::String),
        "__rune_builtin_json_parse"
        | "__rune_builtin_json_get"
        | "__rune_builtin_json_index" => Some(IrType::Json),
        "__rune_builtin_fs_file_size"
        | "__rune_builtin_json_len"
        | "__rune_builtin_json_to_i64" => Some(IrType::I64),
        _ => None,
    }
}

// === optimize (merged from optimize.rs) ===

pub fn optimize_program(program: &mut Program) {
    for item in &mut program.items {
        let Item::Function(function) = item else {
            continue;
        };
        optimize_function(function);
    }
}

pub fn prune_program_for_executable(program: &mut Program) {
    prune_program_to_entry_roots(program, &["main", "setup", "loop"]);
}

pub fn prune_program_to_entry_roots(program: &mut Program, entry_roots: &[&str]) {
    let mut function_map = HashMap::new();
    let mut struct_map = HashMap::new();
    let mut exception_names = HashSet::new();
    for item in &program.items {
        match item {
            Item::Function(function) => {
                function_map.insert(function.name.clone(), function);
            }
            Item::Struct(decl) => {
                struct_map.insert(decl.name.clone(), decl);
                for method in &decl.methods {
                    function_map.insert(struct_method_symbol(&decl.name, &method.name), method);
                }
            }
            Item::Import(_) => {}
            Item::Exception(exception) => {
                exception_names.insert(exception.name.clone());
            }
        }
    }

    let mut reachable_functions = HashSet::new();
    let mut reachable_structs = HashSet::new();
    let mut reachable_exceptions = HashSet::new();
    let mut queue = VecDeque::new();

    for root in entry_roots {
        if function_map.contains_key(*root) && reachable_functions.insert((*root).to_string()) {
            queue.push_back(ReachableItem::Function((*root).to_string()));
        }
    }

    while let Some(item) = queue.pop_front() {
        match item {
            ReachableItem::Function(name) => {
                if let Some(function) = function_map.get(&name) {
                    collect_function_type_deps(
                        function,
                        &struct_map,
                        &exception_names,
                        &mut reachable_structs,
                        &mut reachable_exceptions,
                        &mut queue,
                    );
                    collect_block_deps(
                        &function.body,
                        &function_map,
                        &struct_map,
                        &exception_names,
                        &mut reachable_functions,
                        &mut reachable_structs,
                        &mut reachable_exceptions,
                        &mut queue,
                    );
                }
            }
            ReachableItem::Struct(name) => {
                if let Some(decl) = struct_map.get(&name) {
                    for field in &decl.fields {
                        collect_type_ref_deps(&field.ty, &struct_map, &mut reachable_structs, &mut queue);
                    }
                    for method in &decl.methods {
                        let symbol = struct_method_symbol(&decl.name, &method.name);
                        if reachable_functions.insert(symbol.clone()) {
                            queue.push_back(ReachableItem::Function(symbol));
                        }
                    }
                }
            }
        }
    }

    for item in &mut program.items {
        if let Item::Struct(decl) = item {
            decl.methods.retain(|method| {
                reachable_functions.contains(&struct_method_symbol(&decl.name, &method.name))
            });
        }
    }

    program.items.retain(|item| match item {
        Item::Import(_) => true,
        Item::Exception(exception) => reachable_exceptions.contains(&exception.name),
        Item::Function(function) => reachable_functions.contains(&function.name),
        Item::Struct(decl) => reachable_structs.contains(&decl.name),
    });
}

#[derive(Debug, Clone)]
enum ReachableItem {
    Function(String),
    Struct(String),
}

fn collect_function_type_deps(
    function: &Function,
    struct_map: &HashMap<String, &StructDecl>,
    exception_names: &HashSet<String>,
    reachable_structs: &mut HashSet<String>,
    reachable_exceptions: &mut HashSet<String>,
    queue: &mut VecDeque<ReachableItem>,
) {
    for param in &function.params {
        collect_type_ref_deps(&param.ty, struct_map, reachable_structs, queue);
    }
    if let Some(return_type) = &function.return_type {
        collect_type_ref_deps(return_type, struct_map, reachable_structs, queue);
    }
    if let Some(raises) = &function.raises {
        if exception_names.contains(&raises.name) {
            reachable_exceptions.insert(raises.name.clone());
        } else {
            collect_type_ref_deps(raises, struct_map, reachable_structs, queue);
        }
    }
}

fn collect_type_ref_deps(
    ty: &TypeRef,
    struct_map: &HashMap<String, &StructDecl>,
    reachable_structs: &mut HashSet<String>,
    queue: &mut VecDeque<ReachableItem>,
) {
    if struct_map.contains_key(&ty.name) && reachable_structs.insert(ty.name.clone()) {
        queue.push_back(ReachableItem::Struct(ty.name.clone()));
    }
}

fn collect_block_deps(
    block: &Block,
    function_map: &HashMap<String, &Function>,
    struct_map: &HashMap<String, &StructDecl>,
    exception_names: &HashSet<String>,
    reachable_functions: &mut HashSet<String>,
    reachable_structs: &mut HashSet<String>,
    reachable_exceptions: &mut HashSet<String>,
    queue: &mut VecDeque<ReachableItem>,
) {
    for stmt in &block.statements {
        collect_stmt_deps(
            stmt,
            function_map,
            struct_map,
            exception_names,
            reachable_functions,
            reachable_structs,
            reachable_exceptions,
            queue,
        );
    }
}

fn collect_stmt_deps(
    stmt: &Stmt,
    function_map: &HashMap<String, &Function>,
    struct_map: &HashMap<String, &StructDecl>,
    exception_names: &HashSet<String>,
    reachable_functions: &mut HashSet<String>,
    reachable_structs: &mut HashSet<String>,
    reachable_exceptions: &mut HashSet<String>,
    queue: &mut VecDeque<ReachableItem>,
) {
    match stmt {
        Stmt::Block(stmt) => collect_block_deps(
            &stmt.block,
            function_map,
            struct_map,
            exception_names,
            reachable_functions,
            reachable_structs,
            reachable_exceptions,
            queue,
        ),
        Stmt::Let(stmt) => {
            if let Some(ty) = &stmt.ty {
                collect_type_ref_deps(ty, struct_map, reachable_structs, queue);
            }
            collect_expr_deps(
                &stmt.value,
                function_map,
                struct_map,
                exception_names,
                reachable_functions,
                reachable_structs,
                reachable_exceptions,
                queue,
            );
        }
        Stmt::Assign(stmt) => collect_expr_deps(
            &stmt.value,
            function_map,
            struct_map,
            exception_names,
            reachable_functions,
            reachable_structs,
            reachable_exceptions,
            queue,
        ),
        Stmt::Return(stmt) => {
            if let Some(value) = &stmt.value {
                collect_expr_deps(
                    value,
                    function_map,
                    struct_map,
                    exception_names,
                    reachable_functions,
                    reachable_structs,
                    reachable_exceptions,
                    queue,
                );
            }
        }
        Stmt::If(stmt) => {
            collect_expr_deps(
                &stmt.condition,
                function_map,
                struct_map,
                exception_names,
                reachable_functions,
                reachable_structs,
                reachable_exceptions,
                queue,
            );
            collect_block_deps(
                &stmt.then_block,
                function_map,
                struct_map,
                exception_names,
                reachable_functions,
                reachable_structs,
                reachable_exceptions,
                queue,
            );
            for elif in &stmt.elif_blocks {
                collect_expr_deps(
                    &elif.condition,
                    function_map,
                    struct_map,
                    exception_names,
                    reachable_functions,
                    reachable_structs,
                    reachable_exceptions,
                    queue,
                );
                collect_block_deps(
                    &elif.block,
                    function_map,
                    struct_map,
                    exception_names,
                    reachable_functions,
                    reachable_structs,
                    reachable_exceptions,
                    queue,
                );
            }
            if let Some(block) = &stmt.else_block {
                collect_block_deps(
                    block,
                    function_map,
                    struct_map,
                    exception_names,
                    reachable_functions,
                    reachable_structs,
                    reachable_exceptions,
                    queue,
                );
            }
        }
        Stmt::While(stmt) => {
            collect_expr_deps(
                &stmt.condition,
                function_map,
                struct_map,
                exception_names,
                reachable_functions,
                reachable_structs,
                reachable_exceptions,
                queue,
            );
            collect_block_deps(
                &stmt.body,
                function_map,
                struct_map,
                exception_names,
                reachable_functions,
                reachable_structs,
                reachable_exceptions,
                queue,
            );
        }
        Stmt::Raise(stmt) => collect_expr_deps(
            &stmt.value,
            function_map,
            struct_map,
            exception_names,
            reachable_functions,
            reachable_structs,
            reachable_exceptions,
            queue,
        ),
        Stmt::Panic(stmt) => collect_expr_deps(
            &stmt.value,
            function_map,
            struct_map,
            exception_names,
            reachable_functions,
            reachable_structs,
            reachable_exceptions,
            queue,
        ),
        Stmt::Expr(stmt) => collect_expr_deps(
            &stmt.expr,
            function_map,
            struct_map,
            exception_names,
            reachable_functions,
            reachable_structs,
            reachable_exceptions,
            queue,
        ),
        Stmt::FieldAssign(stmt) => collect_expr_deps(
            &stmt.value,
            function_map,
            struct_map,
            exception_names,
            reachable_functions,
            reachable_structs,
            reachable_exceptions,
            queue,
        ),
        Stmt::Break(_) | Stmt::Continue(_) => {}
    }
}

fn collect_expr_deps(
    expr: &Expr,
    function_map: &HashMap<String, &Function>,
    struct_map: &HashMap<String, &StructDecl>,
    exception_names: &HashSet<String>,
    reachable_functions: &mut HashSet<String>,
    reachable_structs: &mut HashSet<String>,
    reachable_exceptions: &mut HashSet<String>,
    queue: &mut VecDeque<ReachableItem>,
) {
    match &expr.kind {
        ExprKind::Identifier(_) | ExprKind::Integer(_) | ExprKind::String(_) | ExprKind::Bool(_) => {}
        ExprKind::Unary { expr, .. } | ExprKind::Await { expr } => collect_expr_deps(
            expr,
            function_map,
            struct_map,
            exception_names,
            reachable_functions,
            reachable_structs,
            reachable_exceptions,
            queue,
        ),
        ExprKind::Binary { left, right, .. } => {
            collect_expr_deps(
                left,
                function_map,
                struct_map,
                exception_names,
                reachable_functions,
                reachable_structs,
                reachable_exceptions,
                queue,
            );
            collect_expr_deps(
                right,
                function_map,
                struct_map,
                exception_names,
                reachable_functions,
                reachable_structs,
                reachable_exceptions,
                queue,
            );
        }
        ExprKind::Field { base, .. } => collect_expr_deps(
            base,
            function_map,
            struct_map,
            exception_names,
            reachable_functions,
            reachable_structs,
            reachable_exceptions,
            queue,
        ),
        ExprKind::Call { callee, args } => {
            for arg in args {
                match arg {
                    CallArg::Positional(expr) => collect_expr_deps(
                        expr,
                        function_map,
                        struct_map,
                        exception_names,
                        reachable_functions,
                        reachable_structs,
                        reachable_exceptions,
                        queue,
                    ),
                    CallArg::Keyword { value, .. } => collect_expr_deps(
                        value,
                        function_map,
                        struct_map,
                        exception_names,
                        reachable_functions,
                        reachable_structs,
                        reachable_exceptions,
                        queue,
                    ),
                }
            }

            match &callee.kind {
                ExprKind::Identifier(name) => {
                    if let Some(function) = function_map.get(name) {
                        if reachable_functions.insert(name.clone()) {
                            queue.push_back(ReachableItem::Function(name.clone()));
                        }
                        collect_function_type_deps(
                            function,
                            struct_map,
                            exception_names,
                            reachable_structs,
                            reachable_exceptions,
                            queue,
                        );
                    } else if exception_names.contains(name) {
                        reachable_exceptions.insert(name.clone());
                    } else if struct_map.contains_key(name) && reachable_structs.insert(name.clone()) {
                        queue.push_back(ReachableItem::Struct(name.clone()));
                    }
                }
                ExprKind::Field { base, name } => {
                    collect_expr_deps(
                        base,
                        function_map,
                        struct_map,
                        exception_names,
                        reachable_functions,
                        reachable_structs,
                        reachable_exceptions,
                        queue,
                    );
                    if let ExprKind::Identifier(struct_name) = &base.kind
                        && struct_map.contains_key(struct_name)
                    {
                        let symbol = struct_method_symbol(struct_name, name);
                        if reachable_functions.insert(symbol.clone()) {
                            queue.push_back(ReachableItem::Function(symbol));
                        }
                        if reachable_structs.insert(struct_name.clone()) {
                            queue.push_back(ReachableItem::Struct(struct_name.clone()));
                        }
                    }
                }
                _ => collect_expr_deps(
                    callee,
                    function_map,
                    struct_map,
                    exception_names,
                    reachable_functions,
                    reachable_structs,
                    reachable_exceptions,
                    queue,
                ),
            }
        }
    }
}

fn optimize_function(function: &mut Function) {
    if function.is_extern {
        return;
    }
    optimize_block(&mut function.body);
    eliminate_dead_pure_lets(&mut function.body);
}

// ---------------------------------------------------------------------------
// Dead pure-let elimination
//
// Removes `let x = <pure-expr>` statements where `x` is never read anywhere
// in the function body.  A pure expression is one with no function calls or
// await expressions — all side effects come from calls, so if there are no
// calls it is safe to drop an unused binding entirely.
//
// If the init expression contains a call (and therefore has observable side
// effects) the binding is kept even when the name is never read, because the
// call must still execute.
// ---------------------------------------------------------------------------

fn eliminate_dead_pure_lets(block: &mut Block) {
    // Collect every name that is READ (appears in an expression context that
    // is not a let-binding target) anywhere within this block tree.
    let mut read_names: HashSet<String> = HashSet::new();
    for stmt in &block.statements {
        collect_reads_stmt(stmt, &mut read_names);
    }

    block.statements.retain(|stmt| {
        if let Stmt::Let(let_stmt) = stmt {
            if !read_names.contains(&let_stmt.name) && is_pure_expr(&let_stmt.value) {
                return false;
            }
        }
        true
    });

    // Recurse so that inner blocks also have their dead lets removed.
    for stmt in &mut block.statements {
        match stmt {
            Stmt::Block(s) => eliminate_dead_pure_lets(&mut s.block),
            Stmt::If(s) => {
                eliminate_dead_pure_lets(&mut s.then_block);
                for elif in &mut s.elif_blocks {
                    eliminate_dead_pure_lets(&mut elif.block);
                }
                if let Some(b) = &mut s.else_block {
                    eliminate_dead_pure_lets(b);
                }
            }
            Stmt::While(s) => eliminate_dead_pure_lets(&mut s.body),
            _ => {}
        }
    }
}

/// Returns `true` when the expression has no observable side effects (no
/// `Call` or `Await` nodes anywhere in its tree).
fn is_pure_expr(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Integer(_) | ExprKind::Bool(_) | ExprKind::String(_) => true,
        ExprKind::Identifier(_) => true,
        ExprKind::Unary { expr: inner, .. } => is_pure_expr(inner),
        ExprKind::Binary { left, right, .. } => is_pure_expr(left) && is_pure_expr(right),
        ExprKind::Field { base, .. } => is_pure_expr(base),
        // Calls have observable side effects; treat them as impure.
        ExprKind::Call { .. } | ExprKind::Await { .. } => false,
    }
}

/// Collect all variable names that are *read* (appear in expression position,
/// not as the target of a `let` or `assign`) within `stmt` and its children.
fn collect_reads_stmt(stmt: &Stmt, reads: &mut HashSet<String>) {
    match stmt {
        Stmt::Block(s) => {
            for inner in &s.block.statements {
                collect_reads_stmt(inner, reads);
            }
        }
        Stmt::Let(s) => collect_reads_expr(&s.value, reads),
        Stmt::Assign(s) => collect_reads_expr(&s.value, reads),
        Stmt::FieldAssign(s) => {
            reads.insert(s.base.clone());
            collect_reads_expr(&s.value, reads);
        }
        Stmt::Return(s) => {
            if let Some(v) = &s.value {
                collect_reads_expr(v, reads);
            }
        }
        Stmt::If(s) => {
            collect_reads_expr(&s.condition, reads);
            for inner in &s.then_block.statements {
                collect_reads_stmt(inner, reads);
            }
            for elif in &s.elif_blocks {
                collect_reads_expr(&elif.condition, reads);
                for inner in &elif.block.statements {
                    collect_reads_stmt(inner, reads);
                }
            }
            if let Some(b) = &s.else_block {
                for inner in &b.statements {
                    collect_reads_stmt(inner, reads);
                }
            }
        }
        Stmt::While(s) => {
            collect_reads_expr(&s.condition, reads);
            for inner in &s.body.statements {
                collect_reads_stmt(inner, reads);
            }
        }
        Stmt::Raise(s) => collect_reads_expr(&s.value, reads),
        Stmt::Panic(s) => collect_reads_expr(&s.value, reads),
        Stmt::Expr(s) => collect_reads_expr(&s.expr, reads),
        Stmt::Break(_) | Stmt::Continue(_) => {}
    }
}

fn collect_reads_expr(expr: &Expr, reads: &mut HashSet<String>) {
    match &expr.kind {
        ExprKind::Identifier(name) => {
            reads.insert(name.clone());
        }
        ExprKind::Integer(_) | ExprKind::Bool(_) | ExprKind::String(_) => {}
        ExprKind::Unary { expr: inner, .. } | ExprKind::Await { expr: inner } => {
            collect_reads_expr(inner, reads);
        }
        ExprKind::Binary { left, right, .. } => {
            collect_reads_expr(left, reads);
            collect_reads_expr(right, reads);
        }
        ExprKind::Field { base, .. } => collect_reads_expr(base, reads),
        ExprKind::Call { callee, args } => {
            collect_reads_expr(callee, reads);
            for arg in args {
                match arg {
                    CallArg::Positional(e) => collect_reads_expr(e, reads),
                    CallArg::Keyword { value, .. } => collect_reads_expr(value, reads),
                }
            }
        }
    }
}

fn optimize_block(block: &mut Block) {
    let mut optimized = Vec::new();
    for mut stmt in std::mem::take(&mut block.statements) {
        optimize_stmt(&mut stmt);
        match fold_control_flow(stmt) {
            ControlFlowFold::Keep(stmt) => optimized.push(stmt),
            ControlFlowFold::Inline(stmts) => optimized.extend(stmts),
            ControlFlowFold::Remove => {}
        }
    }
    block.statements = optimized;
}

fn optimize_stmt(stmt: &mut Stmt) {
    match stmt {
        Stmt::Block(stmt) => optimize_block(&mut stmt.block),
        Stmt::Let(LetStmt { value, .. }) => optimize_expr(value),
        Stmt::Assign(stmt) => optimize_expr(&mut stmt.value),
        Stmt::FieldAssign(stmt) => optimize_expr(&mut stmt.value),
        Stmt::Return(ReturnStmt { value, .. }) => {
            if let Some(expr) = value {
                optimize_expr(expr);
            }
        }
        Stmt::If(IfStmt {
            condition,
            then_block,
            elif_blocks,
            else_block,
            ..
        }) => {
            optimize_expr(condition);
            optimize_block(then_block);
            for ElifBlock {
                condition, block, ..
            } in elif_blocks
            {
                optimize_expr(condition);
                optimize_block(block);
            }
            if let Some(block) = else_block {
                optimize_block(block);
            }
        }
        Stmt::While(WhileStmt {
            condition, body, ..
        }) => {
            optimize_expr(condition);
            optimize_block(body);
        }
        Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::Raise(stmt) => optimize_expr(&mut stmt.value),
        Stmt::Panic(stmt) => optimize_expr(&mut stmt.value),
        Stmt::Expr(stmt) => optimize_expr(&mut stmt.expr),
    }
}

fn optimize_expr(expr: &mut Expr) {
    match &mut expr.kind {
        ExprKind::Unary { expr: inner, .. } => optimize_expr(inner),
        ExprKind::Binary { left, right, .. } => {
            optimize_expr(left);
            optimize_expr(right);
        }
        ExprKind::Call { args, .. } => {
            for arg in args {
                match arg {
                    CallArg::Positional(expr) => optimize_expr(expr),
                    CallArg::Keyword { value, .. } => optimize_expr(value),
                }
            }
        }
        ExprKind::Await { expr } => optimize_expr(expr),
        ExprKind::Field { base, .. } => optimize_expr(base),
        ExprKind::Identifier(_)
        | ExprKind::Integer(_)
        | ExprKind::String(_)
        | ExprKind::Bool(_) => {}
    }

    fold_expr(expr);
}

fn fold_expr(expr: &mut Expr) {
    match &expr.kind {
        ExprKind::Unary {
            op: UnaryOp::Negate,
            expr: inner,
        } => {
            if let ExprKind::Integer(value) = &inner.kind
                && let Ok(number) = value.parse::<i64>()
            {
                expr.kind = ExprKind::Integer((-number).to_string());
            }
        }
        ExprKind::Unary {
            op: UnaryOp::Not,
            expr: inner,
        } => {
            if let ExprKind::Bool(value) = &inner.kind {
                expr.kind = ExprKind::Bool(!value);
            }
        }
        ExprKind::Binary { left, op, right } => {
            if let (Some(lhs), Some(rhs)) = (bool_value(left), bool_value(right)) {
                let folded = match op {
                    BinaryOp::And => Some(ExprKind::Bool(lhs && rhs)),
                    BinaryOp::Or => Some(ExprKind::Bool(lhs || rhs)),
                    _ => None,
                };

                if let Some(kind) = folded {
                    expr.kind = kind;
                    return;
                }
            }
            if let (Some(lhs), Some(rhs)) = (int_value(left), int_value(right)) {
                let folded = match op {
                    BinaryOp::And | BinaryOp::Or => None,
                    BinaryOp::Add => Some(ExprKind::Integer((lhs + rhs).to_string())),
                    BinaryOp::Subtract => Some(ExprKind::Integer((lhs - rhs).to_string())),
                    BinaryOp::Multiply => Some(ExprKind::Integer((lhs * rhs).to_string())),
                    BinaryOp::Divide => {
                        if rhs != 0 {
                            Some(ExprKind::Integer((lhs / rhs).to_string()))
                        } else {
                            None
                        }
                    }
                    BinaryOp::Modulo => {
                        if rhs != 0 {
                            Some(ExprKind::Integer((lhs % rhs).to_string()))
                        } else {
                            None
                        }
                    }
                    BinaryOp::EqualEqual => Some(ExprKind::Bool(lhs == rhs)),
                    BinaryOp::NotEqual => Some(ExprKind::Bool(lhs != rhs)),
                    BinaryOp::Greater => Some(ExprKind::Bool(lhs > rhs)),
                    BinaryOp::GreaterEqual => Some(ExprKind::Bool(lhs >= rhs)),
                    BinaryOp::Less => Some(ExprKind::Bool(lhs < rhs)),
                    BinaryOp::LessEqual => Some(ExprKind::Bool(lhs <= rhs)),
                    BinaryOp::BitwiseAnd => Some(ExprKind::Integer((lhs & rhs).to_string())),
                    BinaryOp::BitwiseOr => Some(ExprKind::Integer((lhs | rhs).to_string())),
                    BinaryOp::BitwiseXor => Some(ExprKind::Integer((lhs ^ rhs).to_string())),
                    BinaryOp::ShiftLeft => {
                        if rhs >= 0 && rhs < 64 {
                            Some(ExprKind::Integer((lhs << rhs).to_string()))
                        } else {
                            None
                        }
                    }
                    BinaryOp::ShiftRight => {
                        if rhs >= 0 && rhs < 64 {
                            Some(ExprKind::Integer((lhs >> rhs).to_string()))
                        } else {
                            None
                        }
                    }
                };

                if let Some(kind) = folded {
                    expr.kind = kind;
                    return;
                }
            }

            match op {
                BinaryOp::And => {
                    if bool_value(left) == Some(true) {
                        expr.kind = right.kind.clone();
                    } else if bool_value(left) == Some(false) || bool_value(right) == Some(false) {
                        expr.kind = ExprKind::Bool(false);
                    }
                }
                BinaryOp::Or => {
                    if bool_value(left) == Some(false) {
                        expr.kind = right.kind.clone();
                    } else if bool_value(left) == Some(true) || bool_value(right) == Some(true) {
                        expr.kind = ExprKind::Bool(true);
                    }
                }
                BinaryOp::Add => {
                    if int_value(right) == Some(0) {
                        expr.kind = left.kind.clone();
                    } else if int_value(left) == Some(0) {
                        expr.kind = right.kind.clone();
                    }
                }
                BinaryOp::Subtract => {
                    if int_value(right) == Some(0) {
                        expr.kind = left.kind.clone();
                    }
                }
                BinaryOp::Multiply => {
                    if int_value(right) == Some(1) {
                        expr.kind = left.kind.clone();
                    } else if int_value(left) == Some(1) {
                        expr.kind = right.kind.clone();
                    } else if int_value(right) == Some(0) || int_value(left) == Some(0) {
                        expr.kind = ExprKind::Integer("0".to_string());
                    }
                }
                BinaryOp::Divide => {
                    if int_value(right) == Some(1) {
                        expr.kind = left.kind.clone();
                    }
                }
                BinaryOp::Modulo => {
                    if int_value(right) == Some(1) {
                        expr.kind = ExprKind::Integer("0".to_string());
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}

fn int_value(expr: &Expr) -> Option<i64> {
    match &expr.kind {
        ExprKind::Integer(value) => value.parse::<i64>().ok(),
        _ => None,
    }
}

fn bool_value(expr: &Expr) -> Option<bool> {
    match &expr.kind {
        ExprKind::Bool(value) => Some(*value),
        _ => None,
    }
}

enum ControlFlowFold {
    Keep(Stmt),
    Inline(Vec<Stmt>),
    Remove,
}

fn fold_control_flow(stmt: Stmt) -> ControlFlowFold {
    match stmt {
        Stmt::Block(stmt) => ControlFlowFold::Inline(stmt.block.statements),
        Stmt::If(if_stmt) => {
            if let Some(value) = bool_value(&if_stmt.condition) {
                if value {
                    return ControlFlowFold::Inline(if_stmt.then_block.statements);
                }
                for elif in if_stmt.elif_blocks {
                    if let Some(elif_value) = bool_value(&elif.condition) {
                        if elif_value {
                            return ControlFlowFold::Inline(elif.block.statements);
                        }
                    } else {
                        let rebuilt = Stmt::If(crate::parser::IfStmt {
                            condition: elif.condition,
                            then_block: elif.block,
                            elif_blocks: Vec::new(),
                            else_block: if_stmt.else_block,
                            span: if_stmt.span,
                        });
                        return ControlFlowFold::Keep(rebuilt);
                    }
                }
                return if let Some(block) = if_stmt.else_block {
                    ControlFlowFold::Inline(block.statements)
                } else {
                    ControlFlowFold::Remove
                };
            }
            ControlFlowFold::Keep(Stmt::If(if_stmt))
        }
        Stmt::While(while_stmt) => {
            if bool_value(&while_stmt.condition) == Some(false) {
                ControlFlowFold::Remove
            } else {
                ControlFlowFold::Keep(Stmt::While(while_stmt))
            }
        }
        other => ControlFlowFold::Keep(other),
    }
}

#[cfg(test)]
mod tests {
    use super::{optimize_program, prune_program_to_entry_roots};
    use crate::frontend::parser::{Item, Stmt, parse_source};

    #[test]
    fn prunes_unreachable_functions_and_structs_from_entry_roots() {
        let mut program = parse_source(
            r"class Used:
    value: i64
    def read(self) -> i64:
        return self.value

class Unused:
    value: i64
    def read(self) -> i64:
        return self.value

def helper() -> i64:
    let item: Used = Used(value=7)
    return item.read()

def dead_helper() -> i64:
    let item: Unused = Unused(value=9)
    return item.read()

def main() -> i32:
    println(helper())
    return 0
",
        )
        .expect("program should parse");

        optimize_program(&mut program);
        prune_program_to_entry_roots(&mut program, &["main", "setup", "loop"]);

        let function_names = program
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Function(function) => Some(function.name.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let struct_names = program
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Struct(decl) => Some(decl.name.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert!(function_names.contains(&"main"));
        assert!(function_names.contains(&"helper"));
        assert!(!function_names.contains(&"dead_helper"));
        assert!(struct_names.contains(&"Used"));
        assert!(!struct_names.contains(&"Unused"));
    }

    #[test]
    fn divide_by_one_folds_to_operand() {
        let mut program = parse_source(
            r"def main() -> i32:
    return 42 / 1
",
        )
        .expect("program should parse");
        optimize_program(&mut program);
        let Item::Function(main_fn) = &program.items[0] else {
            panic!("expected function");
        };
        let Stmt::Return(ret) = &main_fn.body.statements[0] else {
            panic!("expected return");
        };
        let value = ret.value.as_ref().expect("return should have a value");
        assert_eq!(
            format!("{:?}", value.kind),
            format!("{:?}", crate::parser::ExprKind::Integer("42".to_string())),
            "42 / 1 should fold to 42"
        );
    }

    #[test]
    fn dead_pure_let_is_eliminated() {
        let mut program = parse_source(
            r"def main() -> i32:
    let dead = 5 + 3
    return 0
",
        )
        .expect("program should parse");
        optimize_program(&mut program);
        let Item::Function(main_fn) = &program.items[0] else {
            panic!("expected function");
        };
        // After elimination the body should contain only the return statement.
        assert_eq!(
            main_fn.body.statements.len(),
            1,
            "dead pure let should have been removed"
        );
        assert!(matches!(main_fn.body.statements[0], Stmt::Return(_)));
    }

    #[test]
    fn dead_let_with_call_is_kept() {
        let mut program = parse_source(
            r"def main() -> i32:
    let _x = some_side_effect()
    return 0
",
        )
        .expect("program should parse");
        optimize_program(&mut program);
        let Item::Function(main_fn) = &program.items[0] else {
            panic!("expected function");
        };
        // The let must be preserved because the call may have side effects.
        assert_eq!(
            main_fn.body.statements.len(),
            2,
            "let with a call init must not be removed"
        );
    }

    #[test]
    fn used_let_is_not_eliminated() {
        let mut program = parse_source(
            r"def main() -> i32:
    let x = 10
    return x
",
        )
        .expect("program should parse");
        optimize_program(&mut program);
        let Item::Function(main_fn) = &program.items[0] else {
            panic!("expected function");
        };
        // `x` is read by the return, so the let must stay.
        assert_eq!(main_fn.body.statements.len(), 2, "used let must not be removed");
    }
}
