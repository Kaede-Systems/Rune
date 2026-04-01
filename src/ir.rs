use std::collections::BTreeMap;
use std::fmt;

use crate::parser::{
    BinaryOp, Block, CallArg, Expr, ExprKind, Function, Item, Program, Stmt, StructDecl, TypeRef,
    UnaryOp,
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
    let functions = program
        .items
        .iter()
        .filter_map(|item| {
            let Item::Function(function) = item else {
                return None;
            };
            if function.is_extern {
                return None;
            }
            let locals = analyze_locals(function, &struct_layouts);
            let mut lowerer = Lowerer::new(function.name.clone());
            lowerer.lower_block(&function.body);
            Some(IrFunction {
                name: function.name.clone(),
                locals,
                instructions: lowerer.instructions,
            })
        })
        .collect();
    IrProgram { functions }
}

fn analyze_locals(
    function: &Function,
    struct_layouts: &BTreeMap<String, BTreeMap<String, IrType>>,
) -> Vec<IrLocal> {
    let mut infos = BTreeMap::<String, LocalInfo>::new();

    for param in &function.params {
        infos.insert(
            param.name.clone(),
            LocalInfo {
                ty: LocalType::Known(ir_type_from_type_ref(Some(&param.ty))),
                reassigned: false,
            },
        );
    }

    collect_local_infos(&function.body, struct_layouts, &mut infos);

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
    infos: &mut BTreeMap<String, LocalInfo>,
) {
    for stmt in &block.statements {
        match stmt {
            Stmt::Let(stmt) => {
                infos.insert(
                    stmt.name.clone(),
                    LocalInfo {
                        ty: infer_declared_or_expr_type(
                            stmt.ty.as_ref(),
                            &stmt.value,
                            infos,
                            struct_layouts,
                        ),
                        reassigned: false,
                    },
                );
            }
            Stmt::Assign(stmt) => {
                if let Some(info) = infos.get_mut(&stmt.name) {
                    info.reassigned = true;
                }
            }
            Stmt::If(stmt) => {
                collect_local_infos(&stmt.then_block, struct_layouts, infos);
                for elif in &stmt.elif_blocks {
                    collect_local_infos(&elif.block, struct_layouts, infos);
                }
                if let Some(block) = &stmt.else_block {
                    collect_local_infos(block, struct_layouts, infos);
                }
            }
            Stmt::While(stmt) => collect_local_infos(&stmt.body, struct_layouts, infos),
            Stmt::Return(_) | Stmt::Raise(_) | Stmt::Panic(_) | Stmt::Expr(_) => {}
        }
    }
}

fn infer_declared_or_expr_type(
    declared: Option<&TypeRef>,
    expr: &Expr,
    infos: &BTreeMap<String, LocalInfo>,
    struct_layouts: &BTreeMap<String, BTreeMap<String, IrType>>,
) -> LocalType {
    match declared {
        Some(ty) => {
            let declared_ir = ir_type_from_type_ref(Some(ty));
            if declared_ir == IrType::Dynamic {
                infer_expr_type(expr, infos, struct_layouts)
                    .map(LocalType::Candidate)
                    .unwrap_or(LocalType::Known(IrType::Dynamic))
            } else {
                LocalType::Known(declared_ir)
            }
        }
        None => infer_expr_type(expr, infos, struct_layouts)
            .map(LocalType::Candidate)
            .unwrap_or(LocalType::Known(IrType::Dynamic)),
    }
}

fn infer_expr_type(
    expr: &Expr,
    infos: &BTreeMap<String, LocalInfo>,
    struct_layouts: &BTreeMap<String, BTreeMap<String, IrType>>,
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
        } => infer_expr_type(expr, infos, struct_layouts),
        ExprKind::Unary {
            op: UnaryOp::Not, ..
        } => Some(IrType::Bool),
        ExprKind::Binary { left, op, right } => {
            let left_ty = infer_expr_type(left, infos, struct_layouts)?;
            let right_ty = infer_expr_type(right, infos, struct_layouts)?;
            match op {
                BinaryOp::And | BinaryOp::Or => Some(IrType::Bool),
                BinaryOp::Add
                | BinaryOp::Subtract
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
                                | IrType::String
                        ) | (
                            IrType::Bool | IrType::I32 | IrType::I64 | IrType::String,
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
            }
        }
        ExprKind::Call { callee, .. } => match &callee.kind {
            ExprKind::Identifier(name) if struct_layouts.contains_key(name) => {
                Some(IrType::Struct(name.clone()))
            }
            _ => None,
        },
        ExprKind::Await { .. } => None,
        ExprKind::Field { base, name } => {
            let IrType::Struct(struct_name) = infer_expr_type(base, infos, struct_layouts)? else {
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
    instructions: Vec<IrInst>,
    temp_counter: usize,
    label_counter: usize,
}

impl Lowerer {
    fn new(function_name: String) -> Self {
        Self {
            function_name,
            instructions: Vec::new(),
            temp_counter: 0,
            label_counter: 0,
        }
    }

    fn lower_block(&mut self, block: &Block) {
        for stmt in &block.statements {
            self.lower_stmt(stmt);
        }
    }

    fn lower_stmt(&mut self, stmt: &Stmt) {
        match stmt {
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
                self.lower_block(&stmt.body);
                self.instructions.push(IrInst::Jump(loop_label));
                self.instructions.push(IrInst::Label(end_label));
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
                self.instructions.push(IrInst::ConstInt {
                    dst: dst.clone(),
                    value: value.clone(),
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
                let callee = match &callee.kind {
                    ExprKind::Identifier(name) => name.clone(),
                    ExprKind::Field { name, .. } => name.clone(),
                    _ => "<expr>".to_string(),
                };
                let args = args
                    .iter()
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
                    .collect::<Vec<_>>();
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
            IrType::String => write!(f, "String"),
            IrType::Struct(name) => write!(f, "{name}"),
            IrType::Unit => write!(f, "unit"),
        }
    }
}
