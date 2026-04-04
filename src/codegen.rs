use std::collections::{BTreeSet, HashMap};
use std::fmt;

use crate::ir::{IrType, lower_program};
use crate::lexer::Span;
use crate::optimize::optimize_program;
use crate::parser::{
    AssignStmt, BinaryOp, Block, CallArg, Expr, ExprKind, Function, Item, LetStmt, Program, Stmt,
    StructDecl, UnaryOp, parse_source,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodegenError {
    pub message: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodegenFailure {
    pub function_name: String,
    pub error: CodegenError,
}

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at line {}, column {}",
            self.message, self.span.line, self.span.column
        )
    }
}

impl std::error::Error for CodegenError {}

pub fn emit_asm_source(source: &str) -> Result<String, CodegenError> {
    let mut program = parse_source(source).map_err(|error| CodegenError {
        message: error.message,
        span: error.span,
    })?;
    optimize_program(&mut program);
    emit_program(&program)
}

pub fn emit_program(program: &Program) -> Result<String, CodegenError> {
    let asm = emit_program_with_context(program).map_err(|failure| failure.error)?;
    Ok(peephole_optimize_asm(&asm))
}

pub fn emit_program_with_context(program: &Program) -> Result<String, CodegenFailure> {
    let mut generator = Generator::new(program);
    let asm = generator.emit()?;
    Ok(peephole_optimize_asm(&asm))
}

pub(crate) fn native_internal_symbol_name(name: &str) -> String {
    if name == "main" {
        return "main".to_string();
    }

    if should_mangle_native_symbol(name) {
        format!("rune_fn_{name}")
    } else {
        name.to_string()
    }
}

fn should_mangle_native_symbol(name: &str) -> bool {
    matches!(
        name,
        "exit"
            | "abort"
            | "malloc"
            | "calloc"
            | "realloc"
            | "free"
            | "memcpy"
            | "memmove"
            | "memset"
            | "strlen"
            | "strcmp"
            | "strncmp"
            | "strcpy"
            | "strncpy"
            | "strcat"
            | "strncat"
            | "printf"
            | "fprintf"
            | "sprintf"
            | "snprintf"
            | "puts"
            | "fputs"
            | "putchar"
            | "getchar"
            | "fopen"
            | "fclose"
            | "fflush"
            | "fread"
            | "fwrite"
            | "remove"
            | "rename"
            | "system"
            | "time"
            | "clock"
            | "sleep"
            | "open"
            | "close"
            | "read"
            | "write"
            | "socket"
            | "bind"
            | "listen"
            | "accept"
            | "connect"
            | "send"
            | "recv"
            | "sendto"
            | "recvfrom"
            | "select"
            | "poll"
    )
}

struct Generator<'a> {
    program: &'a Program,
    function_names: BTreeSet<String>,
    extern_functions: BTreeSet<String>,
    function_params: HashMap<String, Vec<(String, AbiType)>>,
    function_returns: HashMap<String, IrType>,
    function_locals: HashMap<String, HashMap<String, IrType>>,
    struct_layouts: HashMap<String, Vec<(String, IrType)>>,
    string_labels: HashMap<String, String>,
    output: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AbiType {
    Scalar(ScalarKind),
    String,
    CString,
    Dynamic,
    Struct(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScalarKind {
    Bool,
    I32,
    I64,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LocalBinding {
    Scalar {
        offset: i32,
        kind: ScalarKind,
    },
    String {
        ptr_offset: i32,
        len_offset: i32,
    },
    Dynamic {
        tag_offset: i32,
        payload_offset: i32,
        extra_offset: i32,
    },
    Struct {
        name: String,
        fields: HashMap<String, LocalBinding>,
    },
}

const DYNAMIC_TAG_UNIT: i64 = 0;
const DYNAMIC_TAG_BOOL: i64 = 1;
const DYNAMIC_TAG_I32: i64 = 2;
const DYNAMIC_TAG_I64: i64 = 3;
const DYNAMIC_TAG_STRING: i64 = 4;
const DYNAMIC_TAG_JSON: i64 = 5;
const DYNAMIC_CMP_EQ: i64 = 0;
const DYNAMIC_CMP_NE: i64 = 1;
const DYNAMIC_CMP_GT: i64 = 2;
const DYNAMIC_CMP_GE: i64 = 3;
const DYNAMIC_CMP_LT: i64 = 4;
const DYNAMIC_CMP_LE: i64 = 5;

impl AbiType {
    fn from_ir_type(ty: &IrType) -> Result<Self, &'static str> {
        match ty {
            IrType::Bool => Ok(Self::Scalar(ScalarKind::Bool)),
            IrType::I32 => Ok(Self::Scalar(ScalarKind::I32)),
            IrType::I64 => Ok(Self::Scalar(ScalarKind::I64)),
            IrType::Json => Ok(Self::Scalar(ScalarKind::Json)),
            IrType::String => Ok(Self::String),
            IrType::Dynamic => Ok(Self::Dynamic),
            IrType::Struct(name) => Ok(Self::Struct(name.clone())),
            IrType::Unit => Err("unit parameters are not supported by the native backend"),
        }
    }
}

impl LocalBinding {
    fn slot_count(&self) -> i32 {
        match self {
            LocalBinding::Scalar { .. } => 1,
            LocalBinding::String { .. } => 2,
            LocalBinding::Dynamic { .. } => 3,
            LocalBinding::Struct { fields, .. } => fields.values().map(Self::slot_count).sum(),
        }
    }

    fn ir_type(&self) -> IrType {
        match self {
            LocalBinding::Scalar {
                kind: ScalarKind::Bool,
                ..
            } => IrType::Bool,
            LocalBinding::Scalar {
                kind: ScalarKind::I32,
                ..
            } => IrType::I32,
            LocalBinding::Scalar {
                kind: ScalarKind::I64,
                ..
            } => IrType::I64,
            LocalBinding::Scalar {
                kind: ScalarKind::Json,
                ..
            } => IrType::Json,
            LocalBinding::String { .. } => IrType::String,
            LocalBinding::Dynamic { .. } => IrType::Dynamic,
            LocalBinding::Struct { name, .. } => IrType::Struct(name.clone()),
        }
    }
}

impl<'a> Generator<'a> {
    fn new(program: &'a Program) -> Self {
        let ir = lower_program(program);
        let struct_layouts = collect_struct_layouts(program);
        let function_locals = ir
            .functions
            .into_iter()
            .map(|function| {
                (
                    function.name,
                    function
                        .locals
                        .into_iter()
                        .map(|local| (local.name, local.ty))
                        .collect::<HashMap<_, _>>(),
                )
            })
            .collect::<HashMap<_, _>>();
        Self {
            program,
            function_names: BTreeSet::new(),
            extern_functions: BTreeSet::new(),
            function_params: HashMap::new(),
            function_returns: HashMap::new(),
            function_locals,
            struct_layouts,
            string_labels: HashMap::new(),
            output: String::new(),
        }
    }

    fn emit(&mut self) -> Result<String, CodegenFailure> {
        for item in &self.program.items {
            match item {
                Item::Function(function) => {
                    self.collect_function_metadata(function.name.clone(), function)?;
                }
                Item::Struct(decl) => {
                    for method in &decl.methods {
                        self.collect_function_metadata(
                            struct_method_symbol(&decl.name, &method.name),
                            method,
                        )?;
                    }
                }
                _ => {}
            }
        }

        self.output.push_str(".text\n\n");

        for item in &self.program.items {
            match item {
                Item::Function(function) => {
                    if function.is_extern {
                        continue;
                    }
                    self.emit_function_with_symbol(function, &function.name)
                        .map_err(|error| CodegenFailure {
                            function_name: function.name.clone(),
                            error,
                        })?;
                    self.output.push('\n');
                }
                Item::Struct(decl) => {
                    for method in &decl.methods {
                        if method.is_extern {
                            continue;
                        }
                        let synthetic_name = struct_method_symbol(&decl.name, &method.name);
                        self.emit_function_with_symbol(method, &synthetic_name)
                            .map_err(|error| CodegenFailure {
                                function_name: synthetic_name.clone(),
                                error,
                            })?;
                        self.output.push('\n');
                    }
                }
                _ => {}
            }
        }

        if !self.string_labels.is_empty() {
            self.output.push_str(".section .rdata,\"dr\"\n");
            let mut entries = self
                .string_labels
                .iter()
                .map(|(value, label)| (label.as_str(), value.as_str()))
                .collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(right.0));
            for (label, value) in entries {
                self.output.push_str(&format!("{label}:\n"));
                self.output
                    .push_str(&format!("    .ascii \"{}\"\n", escape_ascii(value)));
            }
            self.output.push('\n');
        }

        Ok(std::mem::take(&mut self.output))
    }

    fn collect_function_metadata(
        &mut self,
        registered_name: String,
        function: &Function,
    ) -> Result<(), CodegenFailure> {
        if function.is_async {
            return Err(CodegenFailure {
                function_name: registered_name.clone(),
                error: CodegenError {
                    message: "async functions are not supported by the current native backend"
                        .into(),
                    span: function.span,
                },
            });
        }
        let return_ty = function
            .return_type
            .as_ref()
            .map(|ty| type_ref_to_ir_type(Some(ty)))
            .unwrap_or_else(|| {
                self.function_locals
                    .get(&registered_name)
                    .and_then(|locals| locals.get("__return"))
                    .cloned()
                    .unwrap_or(IrType::Dynamic)
            });
        if registered_name == "main" && return_ty == IrType::Dynamic {
            return Err(CodegenFailure {
                function_name: registered_name.clone(),
                error: CodegenError {
                    message: "dynamic return values are not yet supported for `main` in the native backend"
                        .into(),
                    span: function.span,
                },
            });
        }
        if registered_name == "main" && return_ty == IrType::String {
            return Err(CodegenFailure {
                function_name: registered_name.clone(),
                error: CodegenError {
                    message:
                        "string return values are not yet supported for `main` in the native backend"
                            .into(),
                    span: function.span,
                },
            });
        }
        if registered_name == "main" && return_ty == IrType::Json {
            return Err(CodegenFailure {
                function_name: registered_name.clone(),
                error: CodegenError {
                    message:
                        "Json return values are not yet supported for `main` in the native backend"
                            .into(),
                    span: function.span,
                },
            });
        }
        self.function_names.insert(registered_name.clone());
        if function.is_extern {
            self.extern_functions.insert(registered_name.clone());
        }
        self.function_returns
            .insert(registered_name.clone(), return_ty.clone());
        let param_types = function
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| {
                let ty = if index == 0 && param.name == "self" {
                    method_owner_from_registered_name(&registered_name)
                        .map(IrType::Struct)
                        .or_else(|| {
                            self.function_locals
                                .get(&registered_name)
                                .and_then(|locals| locals.get(&param.name))
                                .cloned()
                        })
                        .unwrap_or_else(|| type_ref_to_ir_type(Some(&param.ty)))
                } else {
                    self.function_locals
                        .get(&registered_name)
                        .and_then(|locals| locals.get(&param.name))
                        .cloned()
                        .unwrap_or_else(|| type_ref_to_ir_type(Some(&param.ty)))
                };
                let abi = if function.is_extern && ty == IrType::String {
                    Ok(AbiType::CString)
                } else {
                    AbiType::from_ir_type(&ty)
                }
                .map_err(|message| CodegenFailure {
                    function_name: registered_name.clone(),
                    error: CodegenError {
                        message: message.into(),
                        span: param.span,
                    },
                })?;
                Ok((param.name.clone(), abi))
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.function_params.insert(registered_name, param_types);
        Ok(())
    }

    fn emit_function_with_symbol(
        &mut self,
        function: &Function,
        registered_name: &str,
    ) -> Result<(), CodegenError> {
        let mut locals = BTreeSet::new();
        collect_locals(&function.body, &mut locals)?;
        let local_types = self
            .function_locals
            .get(registered_name)
            .cloned()
            .unwrap_or_default();

        let param_meta = self
            .function_params
            .get(registered_name)
            .expect("parameter metadata should exist");
        let register_count = param_meta
            .iter()
            .map(|(_, ty)| match ty {
                AbiType::Scalar(_) => 1usize,
                AbiType::String => 2usize,
                AbiType::CString => 1usize,
                AbiType::Dynamic => 3usize,
                AbiType::Struct(_) => 1usize,
            })
            .sum::<usize>()
            + usize::from(matches!(
                self.function_returns.get(registered_name),
                Some(IrType::Struct(_))
            ));
        if register_count > 4 {
            return Err(CodegenError {
                message:
                    "the current native backend supports at most 4 argument registers per function"
                        .into(),
                span: function.span,
            });
        }

        let mut offsets = HashMap::new();
        let mut next_slot = 1_i32;
        for (name, ty) in param_meta {
            match ty {
                AbiType::Scalar(kind) => {
                    offsets.insert(
                        name.clone(),
                        LocalBinding::Scalar {
                            offset: next_slot * 8,
                            kind: *kind,
                        },
                    );
                    next_slot += 1;
                }
                AbiType::String => {
                    offsets.insert(
                        name.clone(),
                        LocalBinding::String {
                            ptr_offset: next_slot * 8,
                            len_offset: (next_slot + 1) * 8,
                        },
                    );
                    next_slot += 2;
                }
                AbiType::CString => {
                    return Err(CodegenError {
                        message: "C string parameters are only supported for extern functions"
                            .into(),
                        span: function.span,
                    });
                }
                AbiType::Dynamic => {
                    offsets.insert(
                        name.clone(),
                        LocalBinding::Dynamic {
                            tag_offset: next_slot * 8,
                            payload_offset: (next_slot + 1) * 8,
                            extra_offset: (next_slot + 2) * 8,
                        },
                    );
                    next_slot += 3;
                }
                AbiType::Struct(struct_name) => {
                    let binding = binding_for_type(
                        next_slot,
                        &IrType::Struct(struct_name.clone()),
                        &self.struct_layouts,
                    );
                    next_slot += binding.slot_count();
                    offsets.insert(name.clone(), binding);
                }
            }
        }
        for name in locals {
            if offsets.contains_key(&name) {
                return Err(CodegenError {
                    message: format!(
                        "shadowing `{name}` is not supported by the current native backend"
                    ),
                    span: function.span,
                });
            }
            let ty = local_types.get(&name).cloned().unwrap_or(IrType::I64);
            let binding = binding_for_type(next_slot, &ty, &self.struct_layouts);
            next_slot += binding.slot_count();
            offsets.insert(name, binding);
        }
        let scratch_offset = next_slot * 8;
        next_slot += FunctionEmitter::SCRATCH_SLOTS;
        let return_out_offset = if matches!(
            self.function_returns.get(registered_name),
            Some(IrType::Struct(_))
        ) {
            let offset = next_slot * 8;
            next_slot += 1;
            Some(offset)
        } else {
            None
        };

        let frame_slots = next_slot - 1;
        let mut stack_size = frame_slots * 8 + 32;
        if stack_size % 16 != 0 {
            stack_size += 8;
        }

        let mut emitter = FunctionEmitter::new(
            registered_name,
            offsets,
            scratch_offset,
            stack_size,
            return_out_offset,
            &self.function_names,
            &self.extern_functions,
            &self.function_params,
            &self.function_returns,
            &self.struct_layouts,
            &mut self.string_labels,
            function.span,
        );

        let symbol_name = native_internal_symbol_name(registered_name);
        self.output
            .push_str(&format!(".globl {symbol_name}\n{symbol_name}:\n"));
        self.output.push_str("    push rbp\n");
        self.output.push_str("    mov rbp, rsp\n");
        if stack_size > 0 {
            self.output
                .push_str(&format!("    sub rsp, {stack_size}\n"));
        }

        let param_regs = ["rcx", "rdx", "r8", "r9"];
        let mut reg_index = 0usize;
        if let Some(offset) = return_out_offset {
            self.output
                .push_str(&format!("    mov QWORD PTR [rbp-{offset}], rcx\n"));
            reg_index = 1;
        }
        for (name, ty) in param_meta {
            match ty {
                AbiType::Scalar(_) => {
                    let LocalBinding::Scalar { offset, .. } = emitter.binding(name)? else {
                        unreachable!();
                    };
                    self.output.push_str(&format!(
                        "    mov QWORD PTR [rbp-{offset}], {}\n",
                        param_regs[reg_index]
                    ));
                    reg_index += 1;
                }
                AbiType::String => {
                    let LocalBinding::String {
                        ptr_offset,
                        len_offset,
                    } = emitter.binding(name)?
                    else {
                        unreachable!();
                    };
                    self.output.push_str(&format!(
                        "    mov QWORD PTR [rbp-{ptr_offset}], {}\n",
                        param_regs[reg_index]
                    ));
                    self.output.push_str(&format!(
                        "    mov QWORD PTR [rbp-{len_offset}], {}\n",
                        param_regs[reg_index + 1]
                    ));
                    reg_index += 2;
                }
                AbiType::CString => {
                    unreachable!("C string ABI is only used for extern functions");
                }
                AbiType::Dynamic => {
                    let LocalBinding::Dynamic {
                        tag_offset,
                        payload_offset,
                        extra_offset,
                    } = emitter.binding(name)?
                    else {
                        unreachable!();
                    };
                    self.output.push_str(&format!(
                        "    mov QWORD PTR [rbp-{tag_offset}], {}\n",
                        param_regs[reg_index]
                    ));
                    self.output.push_str(&format!(
                        "    mov QWORD PTR [rbp-{payload_offset}], {}\n",
                        param_regs[reg_index + 1]
                    ));
                    self.output.push_str(&format!(
                        "    mov QWORD PTR [rbp-{extra_offset}], {}\n",
                        param_regs[reg_index + 2]
                    ));
                    reg_index += 3;
                }
                AbiType::Struct(struct_name) => {
                    let binding = emitter.binding(name)?;
                    emitter.emit_copy_struct_from_ptr(
                        &mut self.output,
                        &binding,
                        param_regs[reg_index],
                        struct_name,
                    )?;
                    reg_index += 1;
                }
            }
        }

        emitter.emit_block(&mut self.output, &function.body)?;

        self.output.push_str(&format!("{symbol_name}.return:\n"));
        if stack_size > 0 {
            self.output
                .push_str(&format!("    add rsp, {stack_size}\n"));
        }
        self.output.push_str("    pop rbp\n");
        self.output.push_str("    ret\n");
        Ok(())
    }
}

fn struct_method_symbol(struct_name: &str, method_name: &str) -> String {
    format!("{struct_name}__{method_name}")
}

fn method_owner_from_registered_name(name: &str) -> Option<String> {
    let (owner, _) = name.split_once("__")?;
    (!owner.is_empty()).then(|| owner.to_string())
}

fn collect_locals(block: &Block, locals: &mut BTreeSet<String>) -> Result<(), CodegenError> {
    for stmt in &block.statements {
        match stmt {
            Stmt::Block(stmt) => collect_locals(&stmt.block, locals)?,
            Stmt::Let(let_stmt) => {
                if !locals.insert(let_stmt.name.clone()) {
                    return Err(CodegenError {
                        message: format!(
                            "duplicate local `{}` is not supported by the current native backend",
                            let_stmt.name
                        ),
                        span: let_stmt.span,
                    });
                }
            }
            Stmt::If(if_stmt) => {
                collect_locals(&if_stmt.then_block, locals)?;
                for elif in &if_stmt.elif_blocks {
                    collect_locals(&elif.block, locals)?;
                }
                if let Some(block) = &if_stmt.else_block {
                    collect_locals(block, locals)?;
                }
            }
            Stmt::While(while_stmt) => collect_locals(&while_stmt.body, locals)?,
            Stmt::Break(_) | Stmt::Continue(_) => {}
            _ => {}
        }
    }
    Ok(())
}

struct FunctionEmitter<'a> {
    function_name: &'a str,
    offsets: HashMap<String, LocalBinding>,
    scratch_offset: i32,
    return_out_offset: Option<i32>,
    label_counter: usize,
    function_names: &'a BTreeSet<String>,
    extern_functions: &'a BTreeSet<String>,
    function_params: &'a HashMap<String, Vec<(String, AbiType)>>,
    function_returns: &'a HashMap<String, IrType>,
    struct_layouts: &'a HashMap<String, Vec<(String, IrType)>>,
    string_labels: &'a mut HashMap<String, String>,
    function_span: Span,
    loop_labels: Vec<(String, String)>,
}

impl<'a> FunctionEmitter<'a> {
    const SCRATCH_SLOTS: i32 = 2;

    fn new(
        function_name: &'a str,
        offsets: HashMap<String, LocalBinding>,
        scratch_offset: i32,
        _stack_size: i32,
        return_out_offset: Option<i32>,
        function_names: &'a BTreeSet<String>,
        extern_functions: &'a BTreeSet<String>,
        function_params: &'a HashMap<String, Vec<(String, AbiType)>>,
        function_returns: &'a HashMap<String, IrType>,
        struct_layouts: &'a HashMap<String, Vec<(String, IrType)>>,
        string_labels: &'a mut HashMap<String, String>,
        function_span: Span,
    ) -> Self {
        Self {
            function_name,
            offsets,
            scratch_offset,
            return_out_offset,
            label_counter: 0,
            function_names,
            extern_functions,
            function_params,
            function_returns,
            struct_layouts,
            string_labels,
            function_span,
            loop_labels: Vec::new(),
        }
    }

    fn binding(&self, name: &str) -> Result<LocalBinding, CodegenError> {
        self.offsets.get(name).cloned().ok_or_else(|| CodegenError {
            message: format!("unknown local `{name}` during code generation"),
            span: self.function_span,
        })
    }

    fn next_label(&mut self, prefix: &str) -> String {
        let label = format!(
            ".L.{}.{}.{}",
            self.function_name, prefix, self.label_counter
        );
        self.label_counter += 1;
        label
    }

    fn emit_block(&mut self, out: &mut String, block: &Block) -> Result<(), CodegenError> {
        for stmt in &block.statements {
            self.emit_stmt(out, stmt)?;
        }
        Ok(())
    }

    fn emit_stmt(&mut self, out: &mut String, stmt: &Stmt) -> Result<(), CodegenError> {
        match stmt {
            Stmt::Block(stmt) => self.emit_block(out, &stmt.block),
            Stmt::Let(stmt) => self.emit_let(out, stmt),
            Stmt::Assign(stmt) => self.emit_assign(out, stmt),
            Stmt::Return(stmt) => {
                let return_ty = self
                    .function_returns
                    .get(self.function_name)
                    .cloned()
                    .unwrap_or(IrType::Unit);
                if let Some(expr) = &stmt.value {
                    if let IrType::Struct(struct_name) = &return_ty {
                        let out_offset = self.return_out_offset.ok_or_else(|| CodegenError {
                            message: "missing struct return out pointer in native backend".into(),
                            span: stmt.span,
                        })?;
                        out.push_str(&format!("    mov rcx, QWORD PTR [rbp-{out_offset}]\n"));
                        self.emit_write_struct_value_to_ptr(
                            out,
                            struct_name,
                            expr,
                            "rcx",
                            stmt.span,
                        )?;
                        out.push_str("    xor eax, eax\n");
                        out.push_str(&format!(
                            "    jmp {}.return\n",
                            native_internal_symbol_name(self.function_name)
                        ));
                        return Ok(());
                    }
                    if return_ty == IrType::Dynamic {
                        self.emit_dynamic_value(out, expr, "rax", "rdx", "r8")?;
                    } else if return_ty == IrType::String {
                        self.emit_string_arg(out, expr, "rax", "rdx", "return value")?;
                    } else {
                        self.emit_expr(out, expr)?;
                    }
                } else {
                    if return_ty == IrType::Dynamic {
                        out.push_str(&format!("    mov rax, {DYNAMIC_TAG_UNIT}\n"));
                        out.push_str("    xor edx, edx\n");
                        out.push_str("    xor r8d, r8d\n");
                    } else if return_ty == IrType::String {
                        out.push_str("    xor eax, eax\n");
                        out.push_str("    xor edx, edx\n");
                    } else if matches!(return_ty, IrType::Struct(_)) {
                        return Err(CodegenError {
                            message: "struct-returning functions must return a struct value".into(),
                            span: stmt.span,
                        });
                    } else {
                        out.push_str("    xor eax, eax\n");
                    }
                }
                out.push_str(&format!(
                    "    jmp {}.return\n",
                    native_internal_symbol_name(self.function_name)
                ));
                Ok(())
            }
            Stmt::If(stmt) => {
                let end_label = self.next_label("ifend");
                let else_label = self.next_label("ifelse");

                self.emit_condition(out, &stmt.condition)?;
                out.push_str(&format!("    je {else_label}\n"));
                self.emit_block(out, &stmt.then_block)?;
                out.push_str(&format!("    jmp {end_label}\n"));
                out.push_str(&format!("{else_label}:\n"));

                for elif in &stmt.elif_blocks {
                    let next_label = self.next_label("elifnext");
                    self.emit_condition(out, &elif.condition)?;
                    out.push_str(&format!("    je {next_label}\n"));
                    self.emit_block(out, &elif.block)?;
                    out.push_str(&format!("    jmp {end_label}\n"));
                    out.push_str(&format!("{next_label}:\n"));
                }

                if let Some(block) = &stmt.else_block {
                    self.emit_block(out, block)?;
                }

                out.push_str(&format!("{end_label}:\n"));
                Ok(())
            }
            Stmt::While(stmt) => {
                let start_label = self.next_label("while");
                let end_label = self.next_label("whileend");
                out.push_str(&format!("{start_label}:\n"));
                self.emit_condition(out, &stmt.condition)?;
                out.push_str(&format!("    je {end_label}\n"));
                self.loop_labels
                    .push((start_label.clone(), end_label.clone()));
                self.emit_block(out, &stmt.body)?;
                self.loop_labels.pop();
                out.push_str(&format!("    jmp {start_label}\n"));
                out.push_str(&format!("{end_label}:\n"));
                Ok(())
            }
            Stmt::Break(stmt) => {
                let (_, break_label) = self.loop_labels.last().ok_or_else(|| CodegenError {
                    message: "`break` is only allowed inside a loop".into(),
                    span: stmt.span,
                })?;
                out.push_str(&format!("    jmp {break_label}\n"));
                Ok(())
            }
            Stmt::Continue(stmt) => {
                let (continue_label, _) =
                    self.loop_labels.last().ok_or_else(|| CodegenError {
                        message: "`continue` is only allowed inside a loop".into(),
                        span: stmt.span,
                    })?;
                out.push_str(&format!("    jmp {continue_label}\n"));
                Ok(())
            }
            Stmt::Expr(stmt) => self.emit_expr_stmt(out, &stmt.expr),
            Stmt::Raise(stmt) => self.emit_raise(out, stmt),
            Stmt::Panic(stmt) => self.emit_panic(out, stmt),
        }
    }

    fn emit_raise(
        &mut self,
        out: &mut String,
        stmt: &crate::parser::RaiseStmt,
    ) -> Result<(), CodegenError> {
        let (type_name, message_expr) = match &stmt.value.kind {
            ExprKind::Call { callee, args } => {
                let ExprKind::Identifier(name) = &callee.kind else {
                    return Err(CodegenError {
                        message:
                            "`raise` requires a direct exception constructor call or string value"
                                .into(),
                        span: stmt.span,
                    });
                };
                let [CallArg::Positional(message_expr)] = args.as_slice() else {
                    return Err(CodegenError {
                        message: format!(
                            "exception `{name}` expects exactly 1 positional message argument"
                        ),
                        span: stmt.span,
                    });
                };
                (name.clone(), message_expr)
            }
            _ => ("String".to_string(), &stmt.value),
        };

        self.emit_string_arg(out, message_expr, "rcx", "rdx", "raise message")?;
        let meta = format!(
            "{type_name} in {} at line {}",
            self.function_name, stmt.span.line
        );
        let meta_label = self.intern_string(&meta);
        out.push_str(&format!("    lea r8, {meta_label}[rip]\n"));
        out.push_str(&format!("    mov r9, {}\n", meta.len()));
        out.push_str("    call rune_rt_raise\n");
        out.push_str("    mov eax, 102\n");
        out.push_str(&format!(
            "    jmp {}.return\n",
            native_internal_symbol_name(self.function_name)
        ));
        Ok(())
    }

    fn emit_panic(
        &mut self,
        out: &mut String,
        stmt: &crate::parser::PanicStmt,
    ) -> Result<(), CodegenError> {
        self.emit_string_arg(out, &stmt.value, "rcx", "rdx", "panic message")?;
        let context = format!("panic in {} at line {}", self.function_name, stmt.span.line);
        let context_label = self.intern_string(&context);
        out.push_str(&format!("    lea r8, {context_label}[rip]\n"));
        out.push_str(&format!("    mov r9, {}\n", context.len()));
        out.push_str("    call rune_rt_panic\n");
        out.push_str("    mov eax, 101\n");
        out.push_str(&format!(
            "    jmp {}.return\n",
            native_internal_symbol_name(self.function_name)
        ));
        Ok(())
    }

    fn emit_zero_division_panic(
        &mut self,
        out: &mut String,
        span: Span,
        operation: &str,
    ) {
        let message = format!("{operation} by zero");
        let message_label = self.intern_string(&message);
        let context = format!("ZeroDivisionError in {} at line {}", self.function_name, span.line);
        let context_label = self.intern_string(&context);
        out.push_str(&format!("    lea rcx, {message_label}[rip]\n"));
        out.push_str(&format!("    mov rdx, {}\n", message.len()));
        out.push_str(&format!("    lea r8, {context_label}[rip]\n"));
        out.push_str(&format!("    mov r9, {}\n", context.len()));
        out.push_str("    call rune_rt_panic\n");
        out.push_str("    mov eax, 101\n");
        out.push_str(&format!(
            "    jmp {}.return\n",
            native_internal_symbol_name(self.function_name)
        ));
    }

    fn emit_expr_stmt(&mut self, out: &mut String, expr: &Expr) -> Result<(), CodegenError> {
        if let ExprKind::Call { callee, args } = &expr.kind
            && let ExprKind::Identifier(name) = &callee.kind
            && (name == "print" || name == "println" || name == "eprint" || name == "eprintln")
        {
            let newline = name == "println" || name == "eprintln";
            let stderr = name == "eprint" || name == "eprintln";
            return self.emit_builtin_print(out, newline, stderr, args, expr.span);
        }
        if let ExprKind::Call { callee, args } = &expr.kind
            && let ExprKind::Identifier(name) = &callee.kind
            && (name == "flush" || name == "eflush")
        {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: format!("`{name}` takes no arguments in the native backend"),
                    span: expr.span,
                });
            }
            out.push_str(if name == "flush" {
                "    call rune_rt_flush_stdout\n"
            } else {
                "    call rune_rt_flush_stderr\n"
            });
            return Ok(());
        }

        self.emit_expr(out, expr)
    }

    fn emit_let(&mut self, out: &mut String, stmt: &LetStmt) -> Result<(), CodegenError> {
        let binding = self.binding(&stmt.name)?;
        match &binding {
            LocalBinding::Scalar { offset, .. } => {
                self.emit_expr(out, &stmt.value)?;
                out.push_str(&format!("    mov QWORD PTR [rbp-{offset}], rax\n"));
            }
            LocalBinding::String {
                ptr_offset,
                len_offset,
            } => {
                self.emit_string_arg(out, &stmt.value, "rax", "rcx", "string let binding")?;
                out.push_str(&format!("    mov QWORD PTR [rbp-{ptr_offset}], rax\n"));
                out.push_str(&format!("    mov QWORD PTR [rbp-{len_offset}], rcx\n"));
            }
            LocalBinding::Dynamic {
                tag_offset,
                payload_offset,
                extra_offset,
            } => {
                self.emit_dynamic_value(out, &stmt.value, "rax", "rcx", "rdx")?;
                out.push_str(&format!("    mov QWORD PTR [rbp-{tag_offset}], rax\n"));
                out.push_str(&format!("    mov QWORD PTR [rbp-{payload_offset}], rcx\n"));
                out.push_str(&format!("    mov QWORD PTR [rbp-{extra_offset}], rdx\n"));
            }
            LocalBinding::Struct { .. } => {
                self.emit_store_struct_value(&binding, out, &stmt.value, stmt.span)?;
            }
        }
        Ok(())
    }

    fn emit_assign(&mut self, out: &mut String, stmt: &AssignStmt) -> Result<(), CodegenError> {
        let binding = self.binding(&stmt.name)?;
        match &binding {
            LocalBinding::Scalar { offset, .. } => {
                self.emit_expr(out, &stmt.value)?;
                out.push_str(&format!("    mov QWORD PTR [rbp-{offset}], rax\n"));
            }
            LocalBinding::String {
                ptr_offset,
                len_offset,
            } => {
                self.emit_string_arg(out, &stmt.value, "rax", "rcx", "string assignment")?;
                out.push_str(&format!("    mov QWORD PTR [rbp-{ptr_offset}], rax\n"));
                out.push_str(&format!("    mov QWORD PTR [rbp-{len_offset}], rcx\n"));
            }
            LocalBinding::Dynamic {
                tag_offset,
                payload_offset,
                extra_offset,
            } => {
                self.emit_dynamic_value(out, &stmt.value, "rax", "rcx", "rdx")?;
                out.push_str(&format!("    mov QWORD PTR [rbp-{tag_offset}], rax\n"));
                out.push_str(&format!("    mov QWORD PTR [rbp-{payload_offset}], rcx\n"));
                out.push_str(&format!("    mov QWORD PTR [rbp-{extra_offset}], rdx\n"));
            }
            LocalBinding::Struct { .. } => {
                self.emit_store_struct_value(&binding, out, &stmt.value, stmt.span)?;
            }
        }
        Ok(())
    }

    fn emit_expr(&mut self, out: &mut String, expr: &Expr) -> Result<(), CodegenError> {
        match &expr.kind {
            ExprKind::Identifier(name) => {
                let binding = self.binding(name).map_err(|_| CodegenError {
                    message: format!("only local variables and parameters are supported in codegen, found `{name}`"),
                    span: expr.span,
                })?;
                match binding {
                    LocalBinding::Scalar { offset, .. } => {
                        out.push_str(&format!("    mov rax, QWORD PTR [rbp-{offset}]\n"));
                        Ok(())
                    }
                    LocalBinding::String { .. } => Err(CodegenError {
                        message: format!(
                            "string value `{name}` cannot be used as a scalar expression in the native backend"
                        ),
                        span: expr.span,
                    }),
                    LocalBinding::Dynamic { .. } => Err(CodegenError {
                        message: format!(
                            "dynamic value `{name}` cannot be used as a scalar expression in the native backend"
                        ),
                        span: expr.span,
                    }),
                    LocalBinding::Struct { name, .. } => Err(CodegenError {
                        message: format!(
                            "struct value `{name}` must be used through field access in the native backend"
                        ),
                        span: expr.span,
                    }),
                }
            }
            ExprKind::Integer(value) => {
                out.push_str(&format!("    mov rax, {value}\n"));
                Ok(())
            }
            ExprKind::String(_) => Err(CodegenError {
                message: "string literals are only supported inside `print` and `println` for now"
                    .into(),
                span: expr.span,
            }),
            ExprKind::Bool(value) => {
                let int_value = if *value { 1 } else { 0 };
                out.push_str(&format!("    mov rax, {int_value}\n"));
                Ok(())
            }
            ExprKind::Unary { op, expr: inner } => match op {
                UnaryOp::Negate => {
                    self.emit_expr(out, inner)?;
                    out.push_str("    neg rax\n");
                    Ok(())
                }
                UnaryOp::Not => {
                    self.emit_condition(out, inner)?;
                    out.push_str("    sete al\n");
                    out.push_str("    movzx rax, al\n");
                    Ok(())
                }
            },
            ExprKind::Binary { left, op, right } => {
                if matches!(op, BinaryOp::And | BinaryOp::Or) {
                    return self.emit_logical_expr(out, left, op, right);
                }
                if self.try_emit_struct_equality(out, expr, left, op, right)? {
                    return Ok(());
                }
                if matches!(op, BinaryOp::EqualEqual | BinaryOp::NotEqual)
                    && self.infer_expr_type(left) == Some(IrType::String)
                    && self.infer_expr_type(right) == Some(IrType::String)
                {
                    self.emit_string_arg(out, left, "rcx", "rdx", "left string operand")?;
                    out.push_str(&format!(
                        "    mov QWORD PTR [rbp-{}], rcx\n",
                        self.scratch_offset
                    ));
                    out.push_str(&format!(
                        "    mov QWORD PTR [rbp-{}], rdx\n",
                        self.scratch_offset + 8
                    ));
                    self.emit_string_arg(out, right, "r8", "r9", "right string operand")?;
                    out.push_str(&format!(
                        "    mov rcx, QWORD PTR [rbp-{}]\n",
                        self.scratch_offset
                    ));
                    out.push_str(&format!(
                        "    mov rdx, QWORD PTR [rbp-{}]\n",
                        self.scratch_offset + 8
                    ));
                    out.push_str("    call rune_rt_string_compare\n");
                    out.push_str("    cmp eax, 0\n");
                    match op {
                        BinaryOp::EqualEqual => out.push_str("    sete al\n"),
                        BinaryOp::NotEqual => out.push_str("    setne al\n"),
                        _ => unreachable!(),
                    }
                    out.push_str("    movzx rax, al\n");
                    return Ok(());
                }
                if matches!(
                    op,
                    BinaryOp::Add
                        | BinaryOp::Subtract
                        | BinaryOp::Multiply
                        | BinaryOp::Divide
                        | BinaryOp::Modulo
                ) && self.infer_expr_type(expr) == Some(IrType::Dynamic)
                {
                    return Err(CodegenError {
                        message: format!(
                            "dynamic `{}` values must be used through dynamic contexts in the native backend",
                            binary_op_name(op)
                        ),
                        span: expr.span,
                    });
                }
                if self.is_dynamic_comparison(op, left, right) {
                    return self.emit_dynamic_compare(out, left, op, right);
                }
                if self.try_emit_simple_binary(out, left, op, right)? {
                    return Ok(());
                }
                self.emit_expr(out, left)?;
                out.push_str("    push rax\n");
                self.emit_expr(out, right)?;
                out.push_str("    mov rcx, rax\n");
                out.push_str("    pop rax\n");
                if matches!(op, BinaryOp::EqualEqual | BinaryOp::NotEqual)
                    && self.infer_expr_type(left) == Some(IrType::Json)
                    && self.infer_expr_type(right) == Some(IrType::Json)
                {
                    out.push_str("    call rune_rt_json_equal\n");
                    if matches!(op, BinaryOp::NotEqual) {
                        out.push_str("    xor eax, 1\n");
                    }
                    out.push_str("    movzx rax, al\n");
                    return Ok(());
                }
                match op {
                    BinaryOp::And | BinaryOp::Or => unreachable!("logical operators lower earlier"),
                    BinaryOp::Add => out.push_str("    add rax, rcx\n"),
                    BinaryOp::Subtract => out.push_str("    sub rax, rcx\n"),
                    BinaryOp::Multiply => out.push_str("    imul rax, rcx\n"),
                    BinaryOp::Divide => {
                        let ok_label = self.next_label("div_ok");
                        out.push_str("    cmp rcx, 0\n");
                        out.push_str(&format!("    jne {ok_label}\n"));
                        self.emit_zero_division_panic(out, expr.span, "division");
                        out.push_str(&format!("{ok_label}:\n"));
                        out.push_str("    cqo\n");
                        out.push_str("    idiv rcx\n");
                    }
                    BinaryOp::Modulo => {
                        let ok_label = self.next_label("mod_ok");
                        out.push_str("    cmp rcx, 0\n");
                        out.push_str(&format!("    jne {ok_label}\n"));
                        self.emit_zero_division_panic(out, expr.span, "modulo");
                        out.push_str(&format!("{ok_label}:\n"));
                        out.push_str("    cqo\n");
                        out.push_str("    idiv rcx\n");
                        out.push_str("    mov rax, rdx\n");
                    }
                    BinaryOp::EqualEqual => {
                        out.push_str("    cmp rax, rcx\n");
                        out.push_str("    sete al\n");
                        out.push_str("    movzx rax, al\n");
                    }
                    BinaryOp::NotEqual => {
                        out.push_str("    cmp rax, rcx\n");
                        out.push_str("    setne al\n");
                        out.push_str("    movzx rax, al\n");
                    }
                    BinaryOp::Greater => {
                        out.push_str("    cmp rax, rcx\n");
                        out.push_str("    setg al\n");
                        out.push_str("    movzx rax, al\n");
                    }
                    BinaryOp::GreaterEqual => {
                        out.push_str("    cmp rax, rcx\n");
                        out.push_str("    setge al\n");
                        out.push_str("    movzx rax, al\n");
                    }
                    BinaryOp::Less => {
                        out.push_str("    cmp rax, rcx\n");
                        out.push_str("    setl al\n");
                        out.push_str("    movzx rax, al\n");
                    }
                    BinaryOp::LessEqual => {
                        out.push_str("    cmp rax, rcx\n");
                        out.push_str("    setle al\n");
                        out.push_str("    movzx rax, al\n");
                    }
                }
                Ok(())
            }
            ExprKind::Call { callee, args } => {
                if matches!(self.infer_expr_type(expr), Some(IrType::Struct(_))) {
                    return Err(CodegenError {
                        message: "struct-returning calls must be used in struct contexts in the native backend"
                            .into(),
                        span: expr.span,
                    });
                }
                self.emit_call(out, callee, args, expr.span)
            }
            ExprKind::Await { .. } => Err(CodegenError {
                message: "`await` is not supported by the current native backend".into(),
                span: expr.span,
            }),
            ExprKind::Field { .. } => self.emit_field_expr(out, expr),
        }
    }

    fn emit_builtin_print(
        &mut self,
        out: &mut String,
        newline: bool,
        stderr: bool,
        args: &[CallArg],
        span: Span,
    ) -> Result<(), CodegenError> {
        for arg in args {
            let expr = match arg {
                CallArg::Positional(expr) => expr,
                CallArg::Keyword { span, .. } => {
                    return Err(CodegenError {
                        message:
                            "`print`-family builtins do not accept keyword arguments in the native backend"
                                .into(),
                        span: *span,
                    });
                }
            };

            match &expr.kind {
                _ if self.infer_expr_type(expr) == Some(IrType::Dynamic) => {
                    self.emit_dynamic_value(out, expr, "rcx", "rdx", "r8")?;
                    out.push_str(if stderr {
                        "    call rune_rt_eprint_dynamic\n"
                    } else {
                        "    call rune_rt_print_dynamic\n"
                    });
                }
                _ if self.infer_expr_type(expr) == Some(IrType::String) => {
                    self.emit_string_arg(out, expr, "rcx", "rdx", "print argument")?;
                    out.push_str(if stderr {
                        "    call rune_rt_eprint_str\n"
                    } else {
                        "    call rune_rt_print_str\n"
                    });
                }
                _ if self.infer_expr_type(expr) == Some(IrType::Json) => {
                    self.emit_into_reg(out, "rcx", expr)?;
                    out.push_str("    call rune_rt_json_stringify\n");
                    self.capture_runtime_string_result(out, "rcx", "rdx");
                    out.push_str(if stderr {
                        "    call rune_rt_eprint_str\n"
                    } else {
                        "    call rune_rt_print_str\n"
                    });
                }
                _ if matches!(self.infer_expr_type(expr), Some(IrType::Struct(_))) => {
                    self.emit_string_arg(out, expr, "rcx", "rdx", "print argument")?;
                    out.push_str(if stderr {
                        "    call rune_rt_eprint_str\n"
                    } else {
                        "    call rune_rt_print_str\n"
                    });
                }
                ExprKind::Integer(_)
                | ExprKind::Identifier(_)
                | ExprKind::Bool(_)
                | ExprKind::Unary { .. }
                | ExprKind::Binary { .. }
                | ExprKind::Call { .. }
                | ExprKind::Field { .. } => {
                    self.emit_expr(out, expr)?;
                    out.push_str("    mov rcx, rax\n");
                    out.push_str(if self.infer_expr_type(expr) == Some(IrType::Bool) {
                        if stderr {
                            "    call rune_rt_eprint_bool\n"
                        } else {
                            "    call rune_rt_print_bool\n"
                        }
                    } else if stderr {
                        "    call rune_rt_eprint_i64\n"
                    } else {
                        "    call rune_rt_print_i64\n"
                    });
                }
                other => {
                    return Err(CodegenError {
                        message: format!(
                            "`print`-family builtins do not yet support {:?} arguments in the native backend",
                            other
                        ),
                        span,
                    });
                }
            }
        }

        if newline {
            out.push_str(if stderr {
                "    call rune_rt_eprint_newline\n"
            } else {
                "    call rune_rt_print_newline\n"
            });
        }

        Ok(())
    }

    fn emit_condition(&mut self, out: &mut String, expr: &Expr) -> Result<(), CodegenError> {
        if self.infer_expr_type(expr) == Some(IrType::Dynamic) {
            self.emit_dynamic_value(out, expr, "rcx", "rdx", "r8")?;
            out.push_str("    call rune_rt_dynamic_truthy\n");
            out.push_str("    cmp rax, 0\n");
            return Ok(());
        }

        self.emit_expr(out, expr)?;
        out.push_str("    cmp rax, 0\n");
        Ok(())
    }

    fn emit_logical_expr(
        &mut self,
        out: &mut String,
        left: &Expr,
        op: &BinaryOp,
        right: &Expr,
    ) -> Result<(), CodegenError> {
        let short_label = self.next_label("logicshort");
        let end_label = self.next_label("logicend");

        self.emit_condition(out, left)?;
        match op {
            BinaryOp::And => out.push_str(&format!("    je {short_label}\n")),
            BinaryOp::Or => out.push_str(&format!("    jne {short_label}\n")),
            _ => unreachable!(),
        }

        self.emit_condition(out, right)?;
        out.push_str("    setne al\n");
        out.push_str("    movzx rax, al\n");
        out.push_str(&format!("    jmp {end_label}\n"));
        out.push_str(&format!("{short_label}:\n"));
        match op {
            BinaryOp::And => out.push_str("    xor eax, eax\n"),
            BinaryOp::Or => out.push_str("    mov eax, 1\n"),
            _ => unreachable!(),
        }
        out.push_str(&format!("{end_label}:\n"));
        Ok(())
    }

    fn emit_call(
        &mut self,
        out: &mut String,
        callee: &Expr,
        args: &[CallArg],
        span: Span,
    ) -> Result<(), CodegenError> {
        let (name, owned_args) = self.resolve_call_target(callee, args, span)?;
        let args = owned_args.as_slice();

        if name == "__rune_builtin_time_now_unix" {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: "`__rune_builtin_time_now_unix` takes no arguments".to_string(),
                    span,
                });
            }
            out.push_str("    call rune_rt_time_now_unix\n");
            return Ok(());
        }

        if name == "__rune_builtin_time_monotonic_ms" {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: "`__rune_builtin_time_monotonic_ms` takes no arguments".to_string(),
                    span,
                });
            }
            out.push_str("    call rune_rt_time_monotonic_ms\n");
            return Ok(());
        }

        if name == "__rune_builtin_time_monotonic_us" {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: "`__rune_builtin_time_monotonic_us` takes no arguments".to_string(),
                    span,
                });
            }
            out.push_str("    call rune_rt_time_monotonic_us\n");
            return Ok(());
        }

        if name == "__rune_builtin_time_sleep_ms" {
            if args.len() != 1 {
                return Err(CodegenError {
                    message: "`__rune_builtin_time_sleep_ms` expects 1 argument".to_string(),
                    span,
                });
            }
            let [CallArg::Positional(expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_time_sleep_ms` does not accept keyword arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", expr)?;
            out.push_str("    call rune_rt_time_sleep_ms\n");
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if name == "__rune_builtin_time_sleep_us" {
            if args.len() != 1 {
                return Err(CodegenError {
                    message: "`__rune_builtin_time_sleep_us` expects 1 argument".to_string(),
                    span,
                });
            }
            let [CallArg::Positional(expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_time_sleep_us` does not accept keyword arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", expr)?;
            out.push_str("    call rune_rt_time_sleep_us\n");
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if name == "__rune_builtin_sum_range" {
            if args.len() != 3 {
                return Err(CodegenError {
                    message: "`__rune_builtin_sum_range` expects 3 arguments".to_string(),
                    span,
                });
            }
            let [
                CallArg::Positional(start_expr),
                CallArg::Positional(stop_expr),
                CallArg::Positional(step_expr),
            ] = args
            else {
                return Err(CodegenError {
                    message: "`__rune_builtin_sum_range` does not accept keyword arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", start_expr)?;
            self.emit_into_reg(out, "rdx", stop_expr)?;
            self.emit_into_reg(out, "r8", step_expr)?;
            out.push_str("    call rune_rt_sum_range\n");
            return Ok(());
        }

        if name == "__rune_builtin_system_pid" {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: "`__rune_builtin_system_pid` takes no arguments".to_string(),
                    span,
                });
            }
            out.push_str("    call rune_rt_system_pid\n");
            return Ok(());
        }

        if name == "__rune_builtin_system_cpu_count" {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: "`__rune_builtin_system_cpu_count` takes no arguments".to_string(),
                    span,
                });
            }
            out.push_str("    call rune_rt_system_cpu_count\n");
            return Ok(());
        }

        if matches!(
            name.as_str(),
            "__rune_builtin_system_platform"
                | "__rune_builtin_system_arch"
                | "__rune_builtin_system_target"
                | "__rune_builtin_system_board"
        ) {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: format!("`{name}` takes no arguments"),
                    span,
                });
            }
            let runtime = match name.as_str() {
                "__rune_builtin_system_platform" => "rune_rt_system_platform",
                "__rune_builtin_system_arch" => "rune_rt_system_arch",
                "__rune_builtin_system_target" => "rune_rt_system_target",
                "__rune_builtin_system_board" => "rune_rt_system_board",
                _ => unreachable!(),
            };
            out.push_str(&format!("    call {runtime}\n"));
            return Ok(());
        }

        if matches!(
            name.as_str(),
            "__rune_builtin_system_is_embedded" | "__rune_builtin_system_is_wasm"
        ) {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: format!("`{name}` takes no arguments"),
                    span,
                });
            }
            let runtime = if name == "__rune_builtin_system_is_embedded" {
                "rune_rt_system_is_embedded"
            } else {
                "rune_rt_system_is_wasm"
            };
            out.push_str(&format!("    call {runtime}\n"));
            out.push_str("    movzx rax, al\n");
            return Ok(());
        }

        if name == "__rune_builtin_system_exit" {
            if args.len() != 1 {
                return Err(CodegenError {
                    message: "`__rune_builtin_system_exit` expects 1 argument".to_string(),
                    span,
                });
            }
            let [CallArg::Positional(expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_system_exit` does not accept keyword arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", expr)?;
            out.push_str("    call rune_rt_system_exit\n");
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if name == "__rune_builtin_env_exists" {
            let [CallArg::Positional(expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_env_exists` expects 1 positional argument"
                        .to_string(),
                    span,
                });
            };
            self.emit_string_arg(out, expr, "rcx", "rdx", "environment variable name")?;
            out.push_str("    call rune_rt_env_exists\n");
            out.push_str("    movzx rax, al\n");
            return Ok(());
        }

        if name == "__rune_builtin_env_get_i32" {
            let [
                CallArg::Positional(name_expr),
                CallArg::Positional(default_expr),
            ] = args
            else {
                return Err(CodegenError {
                    message: "`__rune_builtin_env_get_i32` expects 2 positional arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_string_arg(out, name_expr, "rcx", "rdx", "environment variable name")?;
            self.emit_into_reg(out, "r8d", default_expr)?;
            out.push_str("    call rune_rt_env_get_i32\n");
            return Ok(());
        }

        if name == "__rune_builtin_env_get_bool" {
            let [
                CallArg::Positional(name_expr),
                CallArg::Positional(default_expr),
            ] = args
            else {
                return Err(CodegenError {
                    message: "`__rune_builtin_env_get_bool` expects 2 positional arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_string_arg(out, name_expr, "rcx", "rdx", "environment variable name")?;
            self.emit_into_reg(out, "r8d", default_expr)?;
            out.push_str("    call rune_rt_env_get_bool\n");
            out.push_str("    movzx rax, al\n");
            return Ok(());
        }

        if name == "__rune_builtin_env_get_string" {
            let [
                CallArg::Positional(name_expr),
                CallArg::Positional(default_expr),
            ] = args
            else {
                return Err(CodegenError {
                    message: "`__rune_builtin_env_get_string` expects 2 positional arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_string_arg(out, name_expr, "rcx", "rdx", "environment variable name")?;
            self.emit_string_arg(
                out,
                default_expr,
                "r8",
                "r9",
                "default environment value",
            )?;
            out.push_str("    call rune_rt_env_get_string\n");
            return Ok(());
        }

        if name == "__rune_builtin_env_arg_count" {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: "`__rune_builtin_env_arg_count` takes no arguments".to_string(),
                    span,
                });
            }
            out.push_str("    call rune_rt_env_arg_count\n");
            return Ok(());
        }

        if name == "__rune_builtin_env_arg" {
            let [CallArg::Positional(index_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_env_arg` expects 1 positional argument"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "ecx", index_expr)?;
            out.push_str("    call rune_rt_env_arg\n");
            return Ok(());
        }

        if name == "__rune_builtin_network_tcp_connect" {
            let [
                CallArg::Positional(host_expr),
                CallArg::Positional(port_expr),
            ] = args
            else {
                return Err(CodegenError {
                    message: "`__rune_builtin_network_tcp_connect` expects 2 positional arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_string_arg(out, host_expr, "rcx", "rdx", "TCP host")?;
            self.emit_into_reg(out, "r8d", port_expr)?;
            out.push_str("    call rune_rt_network_tcp_connect\n");
            out.push_str("    movzx rax, al\n");
            return Ok(());
        }

        if name == "__rune_builtin_network_tcp_listen" {
            let [
                CallArg::Positional(host_expr),
                CallArg::Positional(port_expr),
            ] = args
            else {
                return Err(CodegenError {
                    message: "`__rune_builtin_network_tcp_listen` expects 2 positional arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_string_arg(out, host_expr, "rcx", "rdx", "TCP listen host")?;
            self.emit_into_reg(out, "r8d", port_expr)?;
            out.push_str("    call rune_rt_network_tcp_listen\n");
            out.push_str("    movzx rax, al\n");
            return Ok(());
        }

        if name == "__rune_builtin_network_tcp_send" {
            let [
                CallArg::Positional(host_expr),
                CallArg::Positional(port_expr),
                CallArg::Positional(data_expr),
            ] = args
            else {
                return Err(CodegenError {
                    message: "`__rune_builtin_network_tcp_send` expects 3 positional arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_string_arg(out, host_expr, "rcx", "rdx", "TCP send host")?;
            self.emit_into_reg(out, "r8d", port_expr)?;
            self.emit_string_arg(out, data_expr, "r9", "r10", "TCP send data")?;
            out.push_str("    sub rsp, 48\n");
            out.push_str("    mov QWORD PTR [rsp+32], r10\n");
            out.push_str("    call rune_rt_network_tcp_send\n");
            out.push_str("    add rsp, 48\n");
            out.push_str("    movzx rax, al\n");
            return Ok(());
        }

        if name == "__rune_builtin_network_tcp_connect_timeout" {
            let [
                CallArg::Positional(host_expr),
                CallArg::Positional(port_expr),
                CallArg::Positional(timeout_expr),
            ] = args
            else {
                return Err(CodegenError {
                    message:
                        "`__rune_builtin_network_tcp_connect_timeout` expects 3 positional arguments"
                            .to_string(),
                    span,
                });
            };
            self.emit_string_arg(out, host_expr, "rcx", "rdx", "TCP host")?;
            self.emit_into_reg(out, "r8d", port_expr)?;
            self.emit_into_reg(out, "r9d", timeout_expr)?;
            out.push_str("    call rune_rt_network_tcp_connect_timeout\n");
            out.push_str("    movzx rax, al\n");
            return Ok(());
        }

        if name == "__rune_builtin_network_tcp_recv" {
            let [
                CallArg::Positional(host_expr),
                CallArg::Positional(port_expr),
                CallArg::Positional(max_expr),
            ] = args
            else {
                return Err(CodegenError {
                    message: "`__rune_builtin_network_tcp_recv` expects 3 positional arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_string_arg(out, host_expr, "rcx", "rdx", "TCP recv host")?;
            self.emit_into_reg(out, "r8d", port_expr)?;
            self.emit_into_reg(out, "r9d", max_expr)?;
            out.push_str("    call rune_rt_network_tcp_recv\n");
            return Ok(());
        }

        if name == "__rune_builtin_network_tcp_recv_timeout" {
            let [
                CallArg::Positional(host_expr),
                CallArg::Positional(port_expr),
                CallArg::Positional(max_expr),
                CallArg::Positional(timeout_expr),
            ] = args
            else {
                return Err(CodegenError {
                    message:
                        "`__rune_builtin_network_tcp_recv_timeout` expects 4 positional arguments"
                            .to_string(),
                    span,
                });
            };
            self.emit_string_arg(out, host_expr, "rcx", "rdx", "TCP recv host")?;
            self.emit_into_reg(out, "r8d", port_expr)?;
            self.emit_into_reg(out, "r9d", max_expr)?;
            out.push_str("    sub rsp, 48\n");
            self.emit_into_reg(out, "r10d", timeout_expr)?;
            out.push_str("    mov DWORD PTR [rsp+32], r10d\n");
            out.push_str("    call rune_rt_network_tcp_recv_timeout\n");
            out.push_str("    add rsp, 48\n");
            return Ok(());
        }

        if name == "__rune_builtin_network_tcp_request" {
            let [
                CallArg::Positional(host_expr),
                CallArg::Positional(port_expr),
                CallArg::Positional(data_expr),
                CallArg::Positional(max_expr),
                CallArg::Positional(timeout_expr),
            ] = args
            else {
                return Err(CodegenError {
                    message: "`__rune_builtin_network_tcp_request` expects 5 positional arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_string_arg(out, host_expr, "rcx", "rdx", "TCP request host")?;
            self.emit_into_reg(out, "r8d", port_expr)?;
            self.emit_string_arg(out, data_expr, "r9", "r10", "TCP request data")?;
            out.push_str("    sub rsp, 64\n");
            out.push_str("    mov QWORD PTR [rsp+32], r10\n");
            self.emit_into_reg(out, "r10d", max_expr)?;
            out.push_str("    mov DWORD PTR [rsp+40], r10d\n");
            self.emit_into_reg(out, "r10d", timeout_expr)?;
            out.push_str("    mov DWORD PTR [rsp+48], r10d\n");
            out.push_str("    call rune_rt_network_tcp_request\n");
            out.push_str("    add rsp, 64\n");
            return Ok(());
        }

        if name == "__rune_builtin_network_tcp_accept_once" {
            let [
                CallArg::Positional(host_expr),
                CallArg::Positional(port_expr),
                CallArg::Positional(max_expr),
                CallArg::Positional(timeout_expr),
            ] = args
            else {
                return Err(CodegenError {
                    message:
                        "`__rune_builtin_network_tcp_accept_once` expects 4 positional arguments"
                            .to_string(),
                    span,
                });
            };
            self.emit_string_arg(out, host_expr, "rcx", "rdx", "TCP accept host")?;
            self.emit_into_reg(out, "r8d", port_expr)?;
            self.emit_into_reg(out, "r9d", max_expr)?;
            out.push_str("    sub rsp, 48\n");
            self.emit_into_reg(out, "r10d", timeout_expr)?;
            out.push_str("    mov DWORD PTR [rsp+32], r10d\n");
            out.push_str("    call rune_rt_network_tcp_accept_once\n");
            out.push_str("    add rsp, 48\n");
            return Ok(());
        }

        if name == "__rune_builtin_network_tcp_reply_once" {
            let [
                CallArg::Positional(host_expr),
                CallArg::Positional(port_expr),
                CallArg::Positional(data_expr),
                CallArg::Positional(max_expr),
                CallArg::Positional(timeout_expr),
            ] = args
            else {
                return Err(CodegenError {
                    message:
                        "`__rune_builtin_network_tcp_reply_once` expects 5 positional arguments"
                            .to_string(),
                    span,
                });
            };
            self.emit_string_arg(out, host_expr, "rcx", "rdx", "TCP reply host")?;
            self.emit_into_reg(out, "r8d", port_expr)?;
            self.emit_string_arg(out, data_expr, "r9", "r10", "TCP reply data")?;
            out.push_str("    sub rsp, 64\n");
            out.push_str("    mov QWORD PTR [rsp+32], r10\n");
            self.emit_into_reg(out, "r10d", max_expr)?;
            out.push_str("    mov DWORD PTR [rsp+40], r10d\n");
            self.emit_into_reg(out, "r10d", timeout_expr)?;
            out.push_str("    mov DWORD PTR [rsp+48], r10d\n");
            out.push_str("    call rune_rt_network_tcp_reply_once\n");
            out.push_str("    add rsp, 64\n");
            return Ok(());
        }

        if name == "__rune_builtin_network_tcp_server_open" {
            let [CallArg::Positional(host_expr), CallArg::Positional(port_expr)] = args else {
                return Err(CodegenError {
                    message:
                        "`__rune_builtin_network_tcp_server_open` expects 2 positional arguments"
                            .to_string(),
                    span,
                });
            };
            self.emit_string_arg(out, host_expr, "rcx", "rdx", "TCP server host")?;
            self.emit_into_reg(out, "r8d", port_expr)?;
            out.push_str("    call rune_rt_network_tcp_server_open\n");
            return Ok(());
        }

        if name == "__rune_builtin_network_tcp_client_open" {
            let [
                CallArg::Positional(host_expr),
                CallArg::Positional(port_expr),
                CallArg::Positional(timeout_expr),
            ] = args
            else {
                return Err(CodegenError {
                    message:
                        "`__rune_builtin_network_tcp_client_open` expects 3 positional arguments"
                            .to_string(),
                    span,
                });
            };
            self.emit_string_arg(out, host_expr, "rcx", "rdx", "TCP client host")?;
            self.emit_into_reg(out, "r8d", port_expr)?;
            self.emit_into_reg(out, "r9d", timeout_expr)?;
            out.push_str("    call rune_rt_network_tcp_client_open\n");
            return Ok(());
        }

        if name == "__rune_builtin_network_tcp_server_accept" {
            let [
                CallArg::Positional(handle_expr),
                CallArg::Positional(max_expr),
                CallArg::Positional(timeout_expr),
            ] = args
            else {
                return Err(CodegenError {
                    message:
                        "`__rune_builtin_network_tcp_server_accept` expects 3 positional arguments"
                            .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "ecx", handle_expr)?;
            self.emit_into_reg(out, "r8d", max_expr)?;
            self.emit_into_reg(out, "r9d", timeout_expr)?;
            out.push_str("    call rune_rt_network_tcp_server_accept\n");
            return Ok(());
        }

        if name == "__rune_builtin_network_tcp_server_reply" {
            let [
                CallArg::Positional(handle_expr),
                CallArg::Positional(data_expr),
                CallArg::Positional(max_expr),
                CallArg::Positional(timeout_expr),
            ] = args
            else {
                return Err(CodegenError {
                    message:
                        "`__rune_builtin_network_tcp_server_reply` expects 4 positional arguments"
                            .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "ecx", handle_expr)?;
            self.emit_string_arg(out, data_expr, "rdx", "r8", "TCP server reply data")?;
            self.emit_into_reg(out, "r9d", max_expr)?;
            out.push_str("    sub rsp, 48\n");
            self.emit_into_reg(out, "r10d", timeout_expr)?;
            out.push_str("    mov DWORD PTR [rsp+32], r10d\n");
            out.push_str("    call rune_rt_network_tcp_server_reply\n");
            out.push_str("    add rsp, 48\n");
            return Ok(());
        }

        if name == "__rune_builtin_network_tcp_server_close" {
            let [CallArg::Positional(handle_expr)] = args else {
                return Err(CodegenError {
                    message:
                        "`__rune_builtin_network_tcp_server_close` expects 1 positional argument"
                            .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "ecx", handle_expr)?;
            out.push_str("    call rune_rt_network_tcp_server_close\n");
            return Ok(());
        }

        if name == "__rune_builtin_network_tcp_client_send" {
            let [CallArg::Positional(handle_expr), CallArg::Positional(data_expr)] = args else {
                return Err(CodegenError {
                    message:
                        "`__rune_builtin_network_tcp_client_send` expects 2 positional arguments"
                            .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "ecx", handle_expr)?;
            self.emit_string_arg(out, data_expr, "rdx", "r8", "TCP client send data")?;
            out.push_str("    call rune_rt_network_tcp_client_send\n");
            return Ok(());
        }

        if name == "__rune_builtin_network_tcp_client_recv" {
            let [
                CallArg::Positional(handle_expr),
                CallArg::Positional(max_expr),
                CallArg::Positional(timeout_expr),
            ] = args
            else {
                return Err(CodegenError {
                    message:
                        "`__rune_builtin_network_tcp_client_recv` expects 3 positional arguments"
                            .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "ecx", handle_expr)?;
            self.emit_into_reg(out, "edx", max_expr)?;
            self.emit_into_reg(out, "r8d", timeout_expr)?;
            out.push_str("    call rune_rt_network_tcp_client_recv\n");
            return Ok(());
        }

        if name == "__rune_builtin_network_tcp_client_close" {
            let [CallArg::Positional(handle_expr)] = args else {
                return Err(CodegenError {
                    message:
                        "`__rune_builtin_network_tcp_client_close` expects 1 positional argument"
                            .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "ecx", handle_expr)?;
            out.push_str("    call rune_rt_network_tcp_client_close\n");
            return Ok(());
        }

        if name == "__rune_builtin_network_last_error_code" {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: "`__rune_builtin_network_last_error_code` expects 0 positional arguments"
                        .to_string(),
                    span,
                });
            }
            out.push_str("    call rune_rt_network_last_error_code\n");
            return Ok(());
        }

        if name == "__rune_builtin_network_last_error_message" {
            if !args.is_empty() {
                return Err(CodegenError {
                    message:
                        "`__rune_builtin_network_last_error_message` expects 0 positional arguments"
                            .to_string(),
                    span,
                });
            }
            out.push_str("    call rune_rt_network_last_error_message\n");
            return Ok(());
        }

        if name == "__rune_builtin_network_clear_error" {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: "`__rune_builtin_network_clear_error` expects 0 positional arguments"
                        .to_string(),
                    span,
                });
            }
            out.push_str("    call rune_rt_network_clear_error\n");
            return Ok(());
        }

        if name == "__rune_builtin_network_udp_bind" {
            let [
                CallArg::Positional(host_expr),
                CallArg::Positional(port_expr),
            ] = args
            else {
                return Err(CodegenError {
                    message: "`__rune_builtin_network_udp_bind` expects 2 positional arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_string_arg(out, host_expr, "rcx", "rdx", "UDP bind host")?;
            self.emit_into_reg(out, "r8d", port_expr)?;
            out.push_str("    call rune_rt_network_udp_bind\n");
            out.push_str("    movzx rax, al\n");
            return Ok(());
        }

        if name == "__rune_builtin_network_udp_recv" {
            let [
                CallArg::Positional(host_expr),
                CallArg::Positional(port_expr),
                CallArg::Positional(max_expr),
                CallArg::Positional(timeout_expr),
            ] = args
            else {
                return Err(CodegenError {
                    message: "`__rune_builtin_network_udp_recv` expects 4 positional arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_string_arg(out, host_expr, "rcx", "rdx", "UDP recv host")?;
            self.emit_into_reg(out, "r8d", port_expr)?;
            self.emit_into_reg(out, "r9d", max_expr)?;
            out.push_str("    sub rsp, 48\n");
            self.emit_into_reg(out, "r10d", timeout_expr)?;
            out.push_str("    mov DWORD PTR [rsp+32], r10d\n");
            out.push_str("    call rune_rt_network_udp_recv\n");
            out.push_str("    add rsp, 48\n");
            return Ok(());
        }

        if name == "__rune_builtin_network_udp_send" {
            let [
                CallArg::Positional(host_expr),
                CallArg::Positional(port_expr),
                CallArg::Positional(data_expr),
            ] = args
            else {
                return Err(CodegenError {
                    message: "`__rune_builtin_network_udp_send` expects 3 positional arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_string_arg(out, host_expr, "rcx", "rdx", "UDP send host")?;
            self.emit_into_reg(out, "r8d", port_expr)?;
            self.emit_string_arg(out, data_expr, "r9", "r10", "UDP send data")?;
            out.push_str("    sub rsp, 48\n");
            out.push_str("    mov QWORD PTR [rsp+32], r10\n");
            out.push_str("    call rune_rt_network_udp_send\n");
            out.push_str("    add rsp, 48\n");
            out.push_str("    movzx rax, al\n");
            return Ok(());
        }

        if name == "__rune_builtin_fs_exists" {
            let [CallArg::Positional(path_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_fs_exists` expects 1 positional argument"
                        .to_string(),
                    span,
                });
            };
            self.emit_string_arg(out, path_expr, "rcx", "rdx", "filesystem path")?;
            out.push_str("    call rune_rt_fs_exists\n");
            out.push_str("    movzx rax, al\n");
            return Ok(());
        }

        if name == "__rune_builtin_fs_current_dir" {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: "`__rune_builtin_fs_current_dir` takes no arguments".to_string(),
                    span,
                });
            }
            out.push_str("    call rune_rt_fs_current_dir\n");
            return Ok(());
        }

        if name == "__rune_builtin_fs_read_string" {
            let [CallArg::Positional(path_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_fs_read_string` expects 1 positional argument"
                        .to_string(),
                    span,
                });
            };
            self.emit_string_arg(out, path_expr, "rcx", "rdx", "filesystem path")?;
            out.push_str("    call rune_rt_fs_read_string\n");
            return Ok(());
        }

        if matches!(
            name.as_str(),
            "__rune_builtin_fs_write_string" | "__rune_builtin_fs_append_string"
        ) {
            let [
                CallArg::Positional(path_expr),
                CallArg::Positional(content_expr),
            ] = args
            else {
                return Err(CodegenError {
                    message: format!("`{name}` expects 2 positional arguments"),
                    span,
                });
            };
            self.emit_string_arg(out, path_expr, "rcx", "rdx", "filesystem path")?;
            self.emit_string_arg(out, content_expr, "r8", "r9", "filesystem content")?;
            let runtime = match name.as_str() {
                "__rune_builtin_fs_write_string" => "rune_rt_fs_write_string",
                "__rune_builtin_fs_append_string" => "rune_rt_fs_append_string",
                _ => unreachable!(),
            };
            out.push_str(&format!("    call {runtime}\n"));
            out.push_str("    movzx rax, al\n");
            return Ok(());
        }

        if matches!(
            name.as_str(),
            "__rune_builtin_fs_remove"
                | "__rune_builtin_fs_set_current_dir"
                | "__rune_builtin_fs_create_dir"
                | "__rune_builtin_fs_create_dir_all"
                | "__rune_builtin_fs_is_file"
                | "__rune_builtin_fs_is_dir"
        ) {
            let [CallArg::Positional(path_expr)] = args else {
                return Err(CodegenError {
                    message: format!("`{name}` expects 1 positional argument"),
                    span,
                });
            };
            self.emit_string_arg(out, path_expr, "rcx", "rdx", "filesystem path")?;
            let runtime = match name.as_str() {
                "__rune_builtin_fs_remove" => "rune_rt_fs_remove",
                "__rune_builtin_fs_set_current_dir" => "rune_rt_fs_set_current_dir",
                "__rune_builtin_fs_create_dir" => "rune_rt_fs_create_dir",
                "__rune_builtin_fs_create_dir_all" => "rune_rt_fs_create_dir_all",
                "__rune_builtin_fs_is_file" => "rune_rt_fs_is_file",
                "__rune_builtin_fs_is_dir" => "rune_rt_fs_is_dir",
                _ => unreachable!(),
            };
            out.push_str(&format!("    call {runtime}\n"));
            out.push_str("    movzx rax, al\n");
            return Ok(());
        }

        if matches!(
            name.as_str(),
            "__rune_builtin_fs_canonicalize" | "__rune_builtin_fs_file_size"
        ) {
            let [CallArg::Positional(path_expr)] = args else {
                return Err(CodegenError {
                    message: format!("`{name}` expects 1 positional argument"),
                    span,
                });
            };
            self.emit_string_arg(out, path_expr, "rcx", "rdx", "filesystem path")?;
            let runtime = match name.as_str() {
                "__rune_builtin_fs_canonicalize" => "rune_rt_fs_canonicalize",
                "__rune_builtin_fs_file_size" => "rune_rt_fs_file_size",
                _ => unreachable!(),
            };
            out.push_str(&format!("    call {runtime}\n"));
            return Ok(());
        }

        if matches!(name.as_str(), "__rune_builtin_fs_rename" | "__rune_builtin_fs_copy") {
            let [
                CallArg::Positional(from_expr),
                CallArg::Positional(to_expr),
            ] = args
            else {
                return Err(CodegenError {
                    message: format!("`{name}` expects 2 positional arguments"),
                    span,
                });
            };
            self.emit_string_arg(out, from_expr, "rcx", "rdx", "filesystem source path")?;
            self.emit_string_arg(out, to_expr, "r8", "r9", "filesystem destination path")?;
            let runtime = match name.as_str() {
                "__rune_builtin_fs_rename" => "rune_rt_fs_rename",
                "__rune_builtin_fs_copy" => "rune_rt_fs_copy",
                _ => unreachable!(),
            };
            out.push_str(&format!("    call {runtime}\n"));
            out.push_str("    movzx rax, al\n");
            return Ok(());
        }

        if name == "__rune_builtin_json_parse" {
            let [CallArg::Positional(source_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_json_parse` expects 1 positional argument"
                        .to_string(),
                    span,
                });
            };
            self.emit_string_arg(out, source_expr, "rcx", "rdx", "JSON source")?;
            out.push_str("    call rune_rt_json_parse\n");
            return Ok(());
        }

        if name == "__rune_builtin_json_stringify" {
            let [CallArg::Positional(json_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_json_stringify` expects 1 positional argument"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", json_expr)?;
            out.push_str("    call rune_rt_json_stringify\n");
            return Ok(());
        }

        if name == "__rune_builtin_json_kind" {
            let [CallArg::Positional(json_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_json_kind` expects 1 positional argument"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", json_expr)?;
            out.push_str("    call rune_rt_json_kind\n");
            return Ok(());
        }

        if name == "__rune_builtin_json_is_null" {
            let [CallArg::Positional(json_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_json_is_null` expects 1 positional argument"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", json_expr)?;
            out.push_str("    call rune_rt_json_is_null\n");
            out.push_str("    movzx rax, al\n");
            return Ok(());
        }

        if name == "__rune_builtin_json_len" {
            let [CallArg::Positional(json_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_json_len` expects 1 positional argument"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", json_expr)?;
            out.push_str("    call rune_rt_json_len\n");
            return Ok(());
        }

        if name == "__rune_builtin_json_get" {
            let [CallArg::Positional(json_expr), CallArg::Positional(key_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_json_get` expects 2 positional arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", json_expr)?;
            self.emit_string_arg(out, key_expr, "rdx", "r8", "JSON object key")?;
            out.push_str("    call rune_rt_json_get\n");
            return Ok(());
        }

        if name == "__rune_builtin_json_index" {
            let [CallArg::Positional(json_expr), CallArg::Positional(index_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_json_index` expects 2 positional arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", json_expr)?;
            self.emit_into_reg(out, "rdx", index_expr)?;
            out.push_str("    call rune_rt_json_index\n");
            return Ok(());
        }

        if name == "__rune_builtin_json_to_string" {
            let [CallArg::Positional(json_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_json_to_string` expects 1 positional argument"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", json_expr)?;
            out.push_str("    call rune_rt_json_to_string\n");
            return Ok(());
        }

        if name == "__rune_builtin_json_to_i64" {
            let [CallArg::Positional(json_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_json_to_i64` expects 1 positional argument"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", json_expr)?;
            out.push_str("    call rune_rt_json_to_i64\n");
            return Ok(());
        }

        if name == "__rune_builtin_json_to_bool" {
            let [CallArg::Positional(json_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_json_to_bool` expects 1 positional argument"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", json_expr)?;
            out.push_str("    call rune_rt_json_to_bool\n");
            out.push_str("    movzx rax, al\n");
            return Ok(());
        }

        if name == "__rune_builtin_arduino_pin_mode" {
            let [CallArg::Positional(pin_expr), CallArg::Positional(mode_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_arduino_pin_mode` expects 2 positional arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", pin_expr)?;
            self.emit_into_reg(out, "rdx", mode_expr)?;
            out.push_str("    call rune_rt_arduino_pin_mode\n");
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if name == "__rune_builtin_gpio_pin_mode" {
            let [CallArg::Positional(pin_expr), CallArg::Positional(mode_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_gpio_pin_mode` expects 2 positional arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", pin_expr)?;
            self.emit_into_reg(out, "rdx", mode_expr)?;
            out.push_str("    call rune_rt_gpio_pin_mode\n");
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if name == "__rune_builtin_gpio_digital_write" {
            let [CallArg::Positional(pin_expr), CallArg::Positional(value_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_gpio_digital_write` expects 2 positional arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", pin_expr)?;
            self.emit_into_reg(out, "edx", value_expr)?;
            out.push_str("    call rune_rt_gpio_digital_write\n");
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if name == "__rune_builtin_gpio_digital_read" {
            let [CallArg::Positional(pin_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_gpio_digital_read` expects 1 positional argument"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", pin_expr)?;
            out.push_str("    call rune_rt_gpio_digital_read\n");
            out.push_str("    movzx rax, al\n");
            return Ok(());
        }

        if name == "__rune_builtin_gpio_pwm_write" {
            let [CallArg::Positional(pin_expr), CallArg::Positional(value_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_gpio_pwm_write` expects 2 positional arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", pin_expr)?;
            self.emit_into_reg(out, "rdx", value_expr)?;
            out.push_str("    call rune_rt_gpio_pwm_write\n");
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if name == "__rune_builtin_gpio_analog_read" {
            let [CallArg::Positional(pin_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_gpio_analog_read` expects 1 positional argument"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", pin_expr)?;
            out.push_str("    call rune_rt_gpio_analog_read\n");
            return Ok(());
        }

        if matches!(
            name.as_str(),
            "__rune_builtin_gpio_mode_input"
                | "__rune_builtin_gpio_mode_output"
                | "__rune_builtin_gpio_mode_input_pullup"
                | "__rune_builtin_gpio_pwm_duty_max"
                | "__rune_builtin_gpio_analog_max"
        ) {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: format!("`{name}` takes no arguments"),
                    span,
                });
            }
            let runtime = match name.as_str() {
                "__rune_builtin_gpio_mode_input" => "rune_rt_gpio_mode_input",
                "__rune_builtin_gpio_mode_output" => "rune_rt_gpio_mode_output",
                "__rune_builtin_gpio_mode_input_pullup" => "rune_rt_gpio_mode_input_pullup",
                "__rune_builtin_gpio_pwm_duty_max" => "rune_rt_gpio_pwm_duty_max",
                "__rune_builtin_gpio_analog_max" => "rune_rt_gpio_analog_max",
                _ => unreachable!(),
            };
            out.push_str(&format!("    call {runtime}\n"));
            return Ok(());
        }

        if name == "__rune_builtin_arduino_digital_write" {
            let [CallArg::Positional(pin_expr), CallArg::Positional(value_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_arduino_digital_write` expects 2 positional arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", pin_expr)?;
            self.emit_into_reg(out, "edx", value_expr)?;
            out.push_str("    call rune_rt_arduino_digital_write\n");
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if name == "__rune_builtin_arduino_digital_read" {
            let [CallArg::Positional(pin_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_arduino_digital_read` expects 1 positional argument"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", pin_expr)?;
            out.push_str("    call rune_rt_arduino_digital_read\n");
            out.push_str("    movzx rax, al\n");
            return Ok(());
        }

        if name == "__rune_builtin_arduino_analog_write" {
            let [CallArg::Positional(pin_expr), CallArg::Positional(value_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_arduino_analog_write` expects 2 positional arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", pin_expr)?;
            self.emit_into_reg(out, "rdx", value_expr)?;
            out.push_str("    call rune_rt_arduino_analog_write\n");
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if name == "__rune_builtin_arduino_analog_read" {
            let [CallArg::Positional(pin_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_arduino_analog_read` expects 1 positional argument"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", pin_expr)?;
            out.push_str("    call rune_rt_arduino_analog_read\n");
            return Ok(());
        }

        if name == "__rune_builtin_arduino_analog_reference" {
            let [CallArg::Positional(mode_expr)] = args else {
                return Err(CodegenError {
                    message:
                        "`__rune_builtin_arduino_analog_reference` expects 1 positional argument"
                            .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", mode_expr)?;
            out.push_str("    call rune_rt_arduino_analog_reference\n");
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if name == "__rune_builtin_arduino_pulse_in" {
            let [CallArg::Positional(pin_expr), CallArg::Positional(state_expr), CallArg::Positional(timeout_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_arduino_pulse_in` expects 3 positional arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", pin_expr)?;
            self.emit_into_reg(out, "edx", state_expr)?;
            self.emit_into_reg(out, "r8", timeout_expr)?;
            out.push_str("    call rune_rt_arduino_pulse_in\n");
            return Ok(());
        }

        if name == "__rune_builtin_arduino_shift_out" {
            let [CallArg::Positional(data_pin_expr), CallArg::Positional(clock_pin_expr), CallArg::Positional(bit_order_expr), CallArg::Positional(value_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_arduino_shift_out` expects 4 positional arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", data_pin_expr)?;
            self.emit_into_reg(out, "rdx", clock_pin_expr)?;
            self.emit_into_reg(out, "r8", bit_order_expr)?;
            self.emit_into_reg(out, "r9", value_expr)?;
            out.push_str("    call rune_rt_arduino_shift_out\n");
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if name == "__rune_builtin_arduino_shift_in" {
            let [CallArg::Positional(data_pin_expr), CallArg::Positional(clock_pin_expr), CallArg::Positional(bit_order_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_arduino_shift_in` expects 3 positional arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", data_pin_expr)?;
            self.emit_into_reg(out, "rdx", clock_pin_expr)?;
            self.emit_into_reg(out, "r8", bit_order_expr)?;
            out.push_str("    call rune_rt_arduino_shift_in\n");
            return Ok(());
        }

        if name == "__rune_builtin_arduino_tone" {
            let [CallArg::Positional(pin_expr), CallArg::Positional(freq_expr), CallArg::Positional(duration_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_arduino_tone` expects 3 positional arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", pin_expr)?;
            self.emit_into_reg(out, "rdx", freq_expr)?;
            self.emit_into_reg(out, "r8", duration_expr)?;
            out.push_str("    call rune_rt_arduino_tone\n");
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if name == "__rune_builtin_arduino_no_tone" {
            let [CallArg::Positional(pin_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_arduino_no_tone` expects 1 positional argument"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", pin_expr)?;
            out.push_str("    call rune_rt_arduino_no_tone\n");
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if name == "__rune_builtin_arduino_servo_attach" {
            let [CallArg::Positional(pin_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_arduino_servo_attach` expects 1 positional argument"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", pin_expr)?;
            out.push_str("    call rune_rt_arduino_servo_attach\n");
            out.push_str("    movzx eax, al\n");
            return Ok(());
        }

        if name == "__rune_builtin_arduino_servo_detach" {
            let [CallArg::Positional(pin_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_arduino_servo_detach` expects 1 positional argument"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", pin_expr)?;
            out.push_str("    call rune_rt_arduino_servo_detach\n");
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if matches!(
            name.as_str(),
            "__rune_builtin_arduino_servo_write" | "__rune_builtin_arduino_servo_write_us"
        ) {
            let [CallArg::Positional(pin_expr), CallArg::Positional(value_expr)] = args else {
                return Err(CodegenError {
                    message: format!("`{name}` expects 2 positional arguments"),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", pin_expr)?;
            self.emit_into_reg(out, "rdx", value_expr)?;
            let runtime = if name == "__rune_builtin_arduino_servo_write" {
                "rune_rt_arduino_servo_write"
            } else {
                "rune_rt_arduino_servo_write_us"
            };
            out.push_str(&format!("    call {runtime}\n"));
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if name == "__rune_builtin_arduino_delay_ms" {
            let [CallArg::Positional(ms_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_arduino_delay_ms` expects 1 positional argument"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", ms_expr)?;
            out.push_str("    call rune_rt_arduino_delay_ms\n");
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if name == "__rune_builtin_arduino_delay_us" {
            let [CallArg::Positional(us_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_arduino_delay_us` expects 1 positional argument"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", us_expr)?;
            out.push_str("    call rune_rt_arduino_delay_us\n");
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if name == "__rune_builtin_arduino_millis" {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: "`__rune_builtin_arduino_millis` takes no arguments".to_string(),
                    span,
                });
            }
            out.push_str("    call rune_rt_arduino_millis\n");
            return Ok(());
        }

        if name == "__rune_builtin_arduino_micros" {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: "`__rune_builtin_arduino_micros` takes no arguments".to_string(),
                    span,
                });
            }
            out.push_str("    call rune_rt_arduino_micros\n");
            return Ok(());
        }

        if name == "__rune_builtin_arduino_read_line" {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: "`__rune_builtin_arduino_read_line` takes no arguments".to_string(),
                    span,
                });
            }
            out.push_str("    call rune_rt_arduino_read_line\n");
            return Ok(());
        }

        if name == "__rune_builtin_arduino_uart_begin" {
            let [CallArg::Positional(baud_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_arduino_uart_begin` expects 1 positional argument"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", baud_expr)?;
            out.push_str("    call rune_rt_arduino_uart_begin\n");
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if name == "__rune_builtin_arduino_uart_available" {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: "`__rune_builtin_arduino_uart_available` takes no arguments"
                        .to_string(),
                    span,
                });
            }
            out.push_str("    call rune_rt_arduino_uart_available\n");
            return Ok(());
        }

        if name == "__rune_builtin_arduino_uart_read_byte" {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: "`__rune_builtin_arduino_uart_read_byte` takes no arguments"
                        .to_string(),
                    span,
                });
            }
            out.push_str("    call rune_rt_arduino_uart_read_byte\n");
            return Ok(());
        }

        if name == "__rune_builtin_arduino_uart_peek_byte" {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: "`__rune_builtin_arduino_uart_peek_byte` takes no arguments"
                        .to_string(),
                    span,
                });
            }
            out.push_str("    call rune_rt_arduino_uart_peek_byte\n");
            return Ok(());
        }

        if name == "__rune_builtin_arduino_uart_write_byte" {
            let [CallArg::Positional(value_expr)] = args else {
                return Err(CodegenError {
                    message:
                        "`__rune_builtin_arduino_uart_write_byte` expects 1 positional argument"
                            .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", value_expr)?;
            out.push_str("    call rune_rt_arduino_uart_write_byte\n");
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if name == "__rune_builtin_arduino_uart_write" {
            let [CallArg::Positional(text_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_arduino_uart_write` expects 1 positional argument"
                        .to_string(),
                    span,
                });
            };
            self.emit_string_arg(out, text_expr, "rcx", "rdx", "Arduino UART text")?;
            out.push_str("    call rune_rt_arduino_uart_write\n");
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if matches!(
            name.as_str(),
            "__rune_builtin_arduino_interrupts_enable" | "__rune_builtin_arduino_interrupts_disable"
        ) {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: format!("`{name}` expects 0 positional arguments"),
                    span,
                });
            }
            let runtime = if name == "__rune_builtin_arduino_interrupts_enable" {
                "rune_rt_arduino_interrupts_enable"
            } else {
                "rune_rt_arduino_interrupts_disable"
            };
            out.push_str(&format!("    call {runtime}\n"));
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if name == "__rune_builtin_arduino_random_seed" {
            let [CallArg::Positional(seed_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_arduino_random_seed` expects 1 positional argument"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", seed_expr)?;
            out.push_str("    call rune_rt_arduino_random_seed\n");
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if name == "__rune_builtin_arduino_random_i64" {
            let [CallArg::Positional(max_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_arduino_random_i64` expects 1 positional argument"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", max_expr)?;
            out.push_str("    call rune_rt_arduino_random_i64\n");
            return Ok(());
        }

        if name == "__rune_builtin_arduino_random_range" {
            let [CallArg::Positional(min_expr), CallArg::Positional(max_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_arduino_random_range` expects 2 positional arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", min_expr)?;
            self.emit_into_reg(out, "rdx", max_expr)?;
            out.push_str("    call rune_rt_arduino_random_range\n");
            return Ok(());
        }

        if name == "__rune_builtin_serial_open" {
            let [CallArg::Positional(port_expr), CallArg::Positional(baud_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_serial_open` expects 2 positional arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_string_arg(out, port_expr, "rcx", "rdx", "serial port name")?;
            self.emit_into_reg(out, "r8", baud_expr)?;
            out.push_str("    call rune_rt_serial_open\n");
            out.push_str("    movzx eax, al\n");
            return Ok(());
        }

        if name == "__rune_builtin_serial_is_open" {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: "`__rune_builtin_serial_is_open` takes no arguments".to_string(),
                    span,
                });
            }
            out.push_str("    call rune_rt_serial_is_open\n");
            out.push_str("    movzx eax, al\n");
            return Ok(());
        }

        if name == "__rune_builtin_serial_close" {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: "`__rune_builtin_serial_close` takes no arguments".to_string(),
                    span,
                });
            }
            out.push_str("    call rune_rt_serial_close\n");
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if name == "__rune_builtin_serial_flush" {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: "`__rune_builtin_serial_flush` takes no arguments".to_string(),
                    span,
                });
            }
            out.push_str("    call rune_rt_serial_flush\n");
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if name == "__rune_builtin_serial_available" {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: "`__rune_builtin_serial_available` takes no arguments".to_string(),
                    span,
                });
            }
            out.push_str("    call rune_rt_serial_available\n");
            return Ok(());
        }

        if name == "__rune_builtin_serial_read_byte" {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: "`__rune_builtin_serial_read_byte` takes no arguments".to_string(),
                    span,
                });
            }
            out.push_str("    call rune_rt_serial_read_byte\n");
            return Ok(());
        }

        if name == "__rune_builtin_serial_read_byte_timeout" {
            let [CallArg::Positional(timeout_expr)] = args else {
                return Err(CodegenError {
                    message:
                        "`__rune_builtin_serial_read_byte_timeout` expects 1 positional argument"
                            .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", timeout_expr)?;
            out.push_str("    call rune_rt_serial_read_byte_timeout\n");
            return Ok(());
        }

        if name == "__rune_builtin_serial_read_line" {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: "`__rune_builtin_serial_read_line` takes no arguments".to_string(),
                    span,
                });
            }
            out.push_str("    call rune_rt_serial_read_line\n");
            return Ok(());
        }

        if name == "__rune_builtin_serial_read_line_timeout" {
            let [CallArg::Positional(timeout_expr)] = args else {
                return Err(CodegenError {
                    message:
                        "`__rune_builtin_serial_read_line_timeout` expects 1 positional argument"
                            .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", timeout_expr)?;
            out.push_str("    call rune_rt_serial_read_line_timeout\n");
            return Ok(());
        }

        if name == "__rune_builtin_serial_peek_byte" {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: "`__rune_builtin_serial_peek_byte` takes no arguments".to_string(),
                    span,
                });
            }
            out.push_str("    call rune_rt_serial_peek_byte\n");
            return Ok(());
        }

        if name == "__rune_builtin_serial_write" || name == "__rune_builtin_serial_write_line" {
            let [CallArg::Positional(text_expr)] = args else {
                return Err(CodegenError {
                    message: format!("`{name}` expects 1 positional argument"),
                    span,
                });
            };
            self.emit_string_arg(out, text_expr, "rcx", "rdx", "serial text")?;
            let runtime = if name == "__rune_builtin_serial_write" {
                "rune_rt_serial_write"
            } else {
                "rune_rt_serial_write_line"
            };
            out.push_str(&format!("    call {runtime}\n"));
            out.push_str("    movzx eax, al\n");
            return Ok(());
        }

        if name == "__rune_builtin_serial_write_byte" {
            let [CallArg::Positional(value_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_serial_write_byte` expects 1 positional argument"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "rcx", value_expr)?;
            out.push_str("    call rune_rt_serial_write_byte\n");
            out.push_str("    movzx eax, al\n");
            return Ok(());
        }

        if matches!(
            name.as_str(),
            "__rune_builtin_arduino_mode_input"
                | "__rune_builtin_arduino_mode_output"
                | "__rune_builtin_arduino_mode_input_pullup"
                | "__rune_builtin_arduino_led_builtin"
                | "__rune_builtin_arduino_high"
                | "__rune_builtin_arduino_low"
                | "__rune_builtin_arduino_bit_order_lsb_first"
                | "__rune_builtin_arduino_bit_order_msb_first"
                | "__rune_builtin_arduino_analog_ref_default"
                | "__rune_builtin_arduino_analog_ref_internal"
                | "__rune_builtin_arduino_analog_ref_external"
        ) {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: format!("`{name}` takes no arguments"),
                    span,
                });
            }
            let runtime = match name.as_str() {
                "__rune_builtin_arduino_mode_input" => "rune_rt_arduino_mode_input",
                "__rune_builtin_arduino_mode_output" => "rune_rt_arduino_mode_output",
                "__rune_builtin_arduino_mode_input_pullup" => "rune_rt_arduino_mode_input_pullup",
                "__rune_builtin_arduino_led_builtin" => "rune_rt_arduino_led_builtin",
                "__rune_builtin_arduino_high" => "rune_rt_arduino_high",
                "__rune_builtin_arduino_low" => "rune_rt_arduino_low",
                "__rune_builtin_arduino_bit_order_lsb_first" => {
                    "rune_rt_arduino_bit_order_lsb_first"
                }
                "__rune_builtin_arduino_bit_order_msb_first" => {
                    "rune_rt_arduino_bit_order_msb_first"
                }
                "__rune_builtin_arduino_analog_ref_default" => {
                    "rune_rt_arduino_analog_ref_default"
                }
                "__rune_builtin_arduino_analog_ref_internal" => {
                    "rune_rt_arduino_analog_ref_internal"
                }
                "__rune_builtin_arduino_analog_ref_external" => {
                    "rune_rt_arduino_analog_ref_external"
                }
                _ => unreachable!(),
            };
            out.push_str(&format!("    call {runtime}\n"));
            return Ok(());
        }

        if name == "__rune_builtin_terminal_clear" {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: "`__rune_builtin_terminal_clear` takes no arguments".to_string(),
                    span,
                });
            }
            out.push_str("    call rune_rt_terminal_clear\n");
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if name == "__rune_builtin_terminal_move_to" {
            let [CallArg::Positional(row_expr), CallArg::Positional(col_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_terminal_move_to` expects 2 positional arguments"
                        .to_string(),
                    span,
                });
            };
            self.emit_into_reg(out, "ecx", row_expr)?;
            self.emit_into_reg(out, "edx", col_expr)?;
            out.push_str("    call rune_rt_terminal_move_to\n");
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if name == "__rune_builtin_terminal_hide_cursor" {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: "`__rune_builtin_terminal_hide_cursor` takes no arguments"
                        .to_string(),
                    span,
                });
            }
            out.push_str("    call rune_rt_terminal_hide_cursor\n");
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if name == "__rune_builtin_terminal_show_cursor" {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: "`__rune_builtin_terminal_show_cursor` takes no arguments"
                        .to_string(),
                    span,
                });
            }
            out.push_str("    call rune_rt_terminal_show_cursor\n");
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if name == "__rune_builtin_terminal_set_title" {
            let [CallArg::Positional(title_expr)] = args else {
                return Err(CodegenError {
                    message: "`__rune_builtin_terminal_set_title` expects 1 positional argument"
                        .to_string(),
                    span,
                });
            };
            self.emit_string_arg(out, title_expr, "rcx", "rdx", "terminal title")?;
            out.push_str("    call rune_rt_terminal_set_title\n");
            out.push_str("    xor eax, eax\n");
            return Ok(());
        }

        if name == "__rune_builtin_audio_bell" {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: "`__rune_builtin_audio_bell` takes no arguments".to_string(),
                    span,
                });
            }
            out.push_str("    call rune_rt_audio_bell\n");
            out.push_str("    movzx rax, al\n");
            return Ok(());
        }

        if name == "int" {
            let [CallArg::Positional(expr)] = args else {
                return Err(CodegenError {
                    message: "`int` expects 1 positional argument".to_string(),
                    span,
                });
            };
            match self.infer_expr_type(expr) {
                Some(IrType::Bool) => {
                    self.emit_expr(out, expr)?;
                    return Ok(());
                }
                Some(IrType::I32) => {
                    self.emit_expr(out, expr)?;
                    return Ok(());
                }
                Some(IrType::I64) => {
                    self.emit_expr(out, expr)?;
                    return Ok(());
                }
                Some(IrType::String) => {
                    self.emit_string_arg(out, expr, "rcx", "rdx", "string integer conversion")?;
                    out.push_str("    call rune_rt_string_to_i64\n");
                    return Ok(());
                }
                Some(IrType::Dynamic) => {
                    self.emit_dynamic_value(out, expr, "rcx", "rdx", "r8")?;
                    out.push_str("    call rune_rt_dynamic_to_i64\n");
                    return Ok(());
                }
                Some(IrType::Json) => {
                    self.emit_into_reg(out, "rcx", expr)?;
                    out.push_str("    call rune_rt_json_to_i64\n");
                    return Ok(());
                }
                _ => {
                    return Err(CodegenError {
                        message: "`int` conversion is not supported for this expression in the native backend"
                            .into(),
                        span,
                    });
                }
            }
        }

        if name == "str" {
            return Err(CodegenError {
                message: "`str` values must be used in string contexts in the native backend"
                    .into(),
                span,
            });
        }

        if !self.function_names.contains(&name) {
            return Err(CodegenError {
                message: format!(
                    "calls to `{name}` are not supported by the current native backend"
                ),
                span,
            });
        }

        let Some(param_meta) = self.function_params.get(&name) else {
            return Err(CodegenError {
                message: format!("missing parameter metadata for `{name}`"),
                span,
            });
        };
        let callee_return_ty = self
            .function_returns
            .get(&name)
            .cloned()
            .unwrap_or(IrType::Unit);

        let ordered_args = self.resolve_call_args(&name, param_meta, args, span)?;

        let register_count = param_meta
            .iter()
            .map(|(_, ty)| match ty {
                AbiType::Scalar(_) => 1usize,
                AbiType::String => 2usize,
                AbiType::CString => 1usize,
                AbiType::Dynamic => 3usize,
                AbiType::Struct(_) => 1usize,
            })
            .sum::<usize>()
            + usize::from(matches!(callee_return_ty, IrType::Struct(_)));
        if register_count > 4 {
            return Err(CodegenError {
                message: "the current native backend supports at most 4 call arguments".into(),
                span,
            });
        }

        let arg_regs = ["rcx", "rdx", "r8", "r9"];
        let callee_is_extern = self.extern_functions.contains(&name);
        if matches!(callee_return_ty, IrType::Struct(_)) {
            return Err(CodegenError {
                message:
                    "struct-returning calls must be used in struct contexts in the native backend"
                        .into(),
                span,
            });
        }
        let all_simple =
            ordered_args
                .iter()
                .zip(param_meta.iter())
                .all(|(arg, (_, ty))| match ty {
                    AbiType::Scalar(_) => self.simple_operand(arg).is_some(),
                    AbiType::String => {
                        matches!(&arg.kind, ExprKind::String(_) | ExprKind::Identifier(_))
                    }
                    AbiType::CString => self.infer_expr_type(arg) == Some(IrType::String),
                    AbiType::Dynamic => self.infer_expr_type(arg).is_some(),
                    AbiType::Struct(_) => self.struct_arg_base_offset(arg).is_some(),
                });
        if all_simple {
            let mut reg_index = register_count;
            for ((_, ty), arg) in param_meta.iter().zip(ordered_args.iter()).rev() {
                match ty {
                    AbiType::Scalar(kind) => {
                        reg_index -= 1;
                        self.emit_into_reg(
                            out,
                            abi_scalar_register(arg_regs[reg_index], *kind),
                            arg,
                        )?;
                    }
                    AbiType::String => {
                        reg_index -= 2;
                        self.emit_string_arg(
                            out,
                            arg,
                            arg_regs[reg_index],
                            arg_regs[reg_index + 1],
                            "string argument",
                        )?;
                    }
                    AbiType::CString => {
                        reg_index -= 1;
                        self.emit_c_string_arg(
                            out,
                            arg,
                            arg_regs[reg_index],
                            "C string argument",
                        )?;
                    }
                    AbiType::Dynamic => {
                        reg_index -= 3;
                        self.emit_dynamic_value(
                            out,
                            arg,
                            arg_regs[reg_index],
                            arg_regs[reg_index + 1],
                            arg_regs[reg_index + 2],
                        )?;
                    }
                    AbiType::Struct(_) => {
                        reg_index -= 1;
                        self.emit_struct_arg_ptr(out, arg, arg_regs[reg_index], span)?;
                    }
                }
            }
            let target_name = if callee_is_extern {
                name.clone()
            } else {
                native_internal_symbol_name(&name)
            };
            out.push_str(&format!("    call {target_name}\n"));
            if callee_is_extern && callee_return_ty == IrType::String {
                out.push_str("    mov rcx, rax\n");
                out.push_str("    call rune_rt_from_c_string\n");
            }
            return Ok(());
        }

        for ((_, ty), arg) in param_meta.iter().zip(ordered_args.iter()).rev() {
            match ty {
                AbiType::Scalar(_) => {
                    self.emit_expr(out, arg)?;
                    out.push_str("    push rax\n");
                }
                AbiType::String => {
                    self.emit_string_arg(out, arg, "rax", "rcx", "string argument")?;
                    out.push_str("    push rcx\n");
                    out.push_str("    push rax\n");
                }
                AbiType::CString => {
                    self.emit_c_string_arg(out, arg, "rax", "C string argument")?;
                    out.push_str("    push rax\n");
                }
                AbiType::Dynamic => {
                    self.emit_dynamic_value(out, arg, "rax", "rcx", "rdx")?;
                    out.push_str("    push rdx\n");
                    out.push_str("    push rcx\n");
                    out.push_str("    push rax\n");
                }
                AbiType::Struct(_) => {
                    return Err(CodegenError {
                        message: "complex struct call arguments are not yet supported by the native backend"
                            .into(),
                        span: arg.span,
                    });
                }
            }
        }
        for index in 0..register_count {
            out.push_str(&format!("    pop {}\n", arg_regs[index]));
        }
        let target_name = if callee_is_extern {
            name.clone()
        } else {
            native_internal_symbol_name(&name)
        };
        out.push_str(&format!("    call {target_name}\n"));
        if callee_is_extern && callee_return_ty == IrType::String {
            out.push_str("    mov rcx, rax\n");
            out.push_str("    call rune_rt_from_c_string\n");
        }
        Ok(())
    }

    fn resolve_call_target(
        &self,
        callee: &Expr,
        args: &[CallArg],
        span: Span,
    ) -> Result<(String, Vec<CallArg>), CodegenError> {
        match &callee.kind {
            ExprKind::Identifier(name) => Ok((name.clone(), args.to_vec())),
            ExprKind::Field { base, name } => {
                let Some(IrType::Struct(struct_name)) = self.infer_expr_type(base) else {
                    return Err(CodegenError {
                        message: "method calls require a concrete class or struct receiver in the current native backend"
                            .into(),
                        span: callee.span,
                    });
                };
                let synthetic_name = struct_method_symbol(&struct_name, name);
                if !self.function_names.contains(&synthetic_name) {
                    return Err(CodegenError {
                        message: format!(
                            "`{struct_name}` has no method `{name}` in the current native backend"
                        ),
                        span,
                    });
                }
                let mut owned_args = Vec::with_capacity(args.len() + 1);
                owned_args.push(CallArg::Positional((**base).clone()));
                owned_args.extend(args.iter().cloned());
                Ok((synthetic_name, owned_args))
            }
            _ => Err(CodegenError {
                message: "only direct function and method calls are supported by the current native backend"
                    .into(),
                span: callee.span,
            }),
        }
    }

    fn intern_string(&mut self, value: &str) -> String {
        if let Some(label) = self.string_labels.get(value) {
            return label.clone();
        }

        let label = format!(".L.rune.str.{}", self.string_labels.len());
        self.string_labels.insert(value.to_string(), label.clone());
        label
    }

    fn resolve_call_args<'b>(
        &self,
        function_name: &str,
        params: &[(String, AbiType)],
        args: &'b [CallArg],
        span: Span,
    ) -> Result<Vec<&'b Expr>, CodegenError> {
        let mut resolved: Vec<Option<&Expr>> = vec![None; params.len()];
        let mut positional_index = 0usize;
        let mut saw_keyword = false;

        for arg in args {
            match arg {
                CallArg::Positional(expr) => {
                    if saw_keyword {
                        return Err(CodegenError {
                            message: format!(
                                "positional arguments cannot appear after keyword arguments in `{function_name}`"
                            ),
                            span: expr.span,
                        });
                    }
                    if positional_index >= params.len() {
                        return Err(CodegenError {
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
                        return Err(CodegenError {
                            message: format!(
                                "function `{function_name}` has no parameter named `{name}`"
                            ),
                            span: *kw_span,
                        });
                    };
                    if resolved[index].is_some() {
                        return Err(CodegenError {
                            message: format!("parameter `{name}` was provided more than once"),
                            span: *kw_span,
                        });
                    }
                    resolved[index] = Some(value);
                }
            }
        }

        if resolved.iter().any(|arg| arg.is_none()) {
            return Err(CodegenError {
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

    fn try_emit_simple_binary(
        &mut self,
        out: &mut String,
        left: &Expr,
        op: &BinaryOp,
        right: &Expr,
    ) -> Result<bool, CodegenError> {
        if matches!(op, BinaryOp::EqualEqual | BinaryOp::NotEqual)
            && self.infer_expr_type(left) == Some(IrType::Json)
            && self.infer_expr_type(right) == Some(IrType::Json)
        {
            return Ok(false);
        }
        let Some(left_operand) = self.simple_operand(left) else {
            return Ok(false);
        };
        let Some(right_operand) = self.simple_operand(right) else {
            return Ok(false);
        };

        self.emit_load_rax(out, &left_operand);
        match op {
            BinaryOp::And | BinaryOp::Or => return Ok(false),
            BinaryOp::Add => self.emit_binary_op(out, "add", &right_operand),
            BinaryOp::Subtract => self.emit_binary_op(out, "sub", &right_operand),
            BinaryOp::Multiply => {
                if let SimpleOperand::Immediate(value) = &right_operand {
                    out.push_str(&format!("    mov rcx, {value}\n"));
                    out.push_str("    imul rax, rcx\n");
                } else {
                    self.emit_binary_op(out, "imul", &right_operand);
                }
            }
            BinaryOp::Divide | BinaryOp::Modulo => return Ok(false),
            BinaryOp::EqualEqual
            | BinaryOp::NotEqual
            | BinaryOp::Greater
            | BinaryOp::GreaterEqual
            | BinaryOp::Less
            | BinaryOp::LessEqual => {
                out.push_str(&format!("    cmp rax, {}\n", right_operand.render()));
                let setcc = match op {
                    BinaryOp::EqualEqual => "sete",
                    BinaryOp::NotEqual => "setne",
                    BinaryOp::Greater => "setg",
                    BinaryOp::GreaterEqual => "setge",
                    BinaryOp::Less => "setl",
                    BinaryOp::LessEqual => "setle",
                    _ => unreachable!(),
                };
                out.push_str(&format!("    {setcc} al\n"));
                out.push_str("    movzx rax, al\n");
                return Ok(true);
            }
        }
        Ok(true)
    }

    fn emit_into_reg(
        &mut self,
        out: &mut String,
        reg: &str,
        expr: &Expr,
    ) -> Result<(), CodegenError> {
        if let Some(operand) = self.simple_operand(expr) {
            let rendered = if is_32bit_register(reg) {
                operand.render_32()
            } else {
                operand.render()
            };
            out.push_str(&format!("    mov {reg}, {rendered}\n"));
            return Ok(());
        }

        self.emit_expr(out, expr)?;
        if is_32bit_register(reg) {
            out.push_str(&format!("    mov {reg}, eax\n"));
        } else if reg != "rax" {
            out.push_str(&format!("    mov {reg}, rax\n"));
        }
        Ok(())
    }

    fn emit_dynamic_value(
        &mut self,
        out: &mut String,
        expr: &Expr,
        tag_reg: &str,
        payload_reg: &str,
        extra_reg: &str,
    ) -> Result<(), CodegenError> {
        if let ExprKind::Binary { left, op, right } = &expr.kind
            && self.infer_expr_type(expr) == Some(IrType::Dynamic)
        {
            return self.emit_dynamic_binary(out, left, op, right, tag_reg, payload_reg, extra_reg);
        }

        match self.infer_expr_type(expr) {
            Some(IrType::Bool) => {
                self.emit_expr(out, expr)?;
                move_reg(out, payload_reg, "rax");
                out.push_str(&format!("    mov {tag_reg}, {DYNAMIC_TAG_BOOL}\n"));
                out.push_str(&format!("    xor {extra_reg}, {extra_reg}\n"));
                Ok(())
            }
            Some(IrType::I32) => {
                self.emit_expr(out, expr)?;
                move_reg(out, payload_reg, "rax");
                out.push_str(&format!("    mov {tag_reg}, {DYNAMIC_TAG_I32}\n"));
                out.push_str(&format!("    xor {extra_reg}, {extra_reg}\n"));
                Ok(())
            }
            Some(IrType::I64) => {
                self.emit_expr(out, expr)?;
                move_reg(out, payload_reg, "rax");
                out.push_str(&format!("    mov {tag_reg}, {DYNAMIC_TAG_I64}\n"));
                out.push_str(&format!("    xor {extra_reg}, {extra_reg}\n"));
                Ok(())
            }
            Some(IrType::String) => {
                self.emit_string_arg(out, expr, payload_reg, extra_reg, "dynamic string value")?;
                out.push_str(&format!("    mov {tag_reg}, {DYNAMIC_TAG_STRING}\n"));
                Ok(())
            }
            Some(IrType::Json) => {
                self.emit_expr(out, expr)?;
                move_reg(out, payload_reg, "rax");
                out.push_str(&format!("    mov {tag_reg}, {DYNAMIC_TAG_JSON}\n"));
                out.push_str(&format!("    xor {extra_reg}, {extra_reg}\n"));
                Ok(())
            }
            Some(IrType::Dynamic) => {
                match &expr.kind {
                    ExprKind::Identifier(name) => {
                        let LocalBinding::Dynamic {
                            tag_offset,
                            payload_offset,
                            extra_offset,
                        } = self.binding(name)?
                        else {
                            return Err(CodegenError {
                                message: "expected dynamic local binding".into(),
                                span: expr.span,
                            });
                        };
                        out.push_str(&format!("    mov {tag_reg}, QWORD PTR [rbp-{tag_offset}]\n"));
                        out.push_str(&format!(
                            "    mov {payload_reg}, QWORD PTR [rbp-{payload_offset}]\n"
                        ));
                        out.push_str(&format!("    mov {extra_reg}, QWORD PTR [rbp-{extra_offset}]\n"));
                        Ok(())
                    }
                    ExprKind::Call { callee, args } => {
                        self.emit_call(out, callee, args, expr.span)?;
                        move_reg(out, tag_reg, "rax");
                        move_reg(out, payload_reg, "rdx");
                        move_reg(out, extra_reg, "r8");
                        Ok(())
                    }
                    _ => Err(CodegenError {
                        message:
                            "complex dynamic expressions are not yet supported by the native backend"
                                .into(),
                        span: expr.span,
                    }),
                }
            }
            Some(IrType::Unit) => {
                out.push_str(&format!("    mov {tag_reg}, {DYNAMIC_TAG_UNIT}\n"));
                out.push_str(&format!("    xor {payload_reg}, {payload_reg}\n"));
                out.push_str(&format!("    xor {extra_reg}, {extra_reg}\n"));
                Ok(())
            }
            Some(IrType::Struct(_)) => Err(CodegenError {
                message: "struct values are not yet supported inside dynamic contexts in the native backend"
                    .into(),
                span: expr.span,
            }),
            None => Err(CodegenError {
                message: "could not infer a native dynamic value representation for this expression"
                    .into(),
                span: expr.span,
            }),
        }
    }

    fn emit_dynamic_binary(
        &mut self,
        out: &mut String,
        left: &Expr,
        op: &BinaryOp,
        right: &Expr,
        tag_reg: &str,
        payload_reg: &str,
        extra_reg: &str,
    ) -> Result<(), CodegenError> {
        out.push_str("    sub rsp, 80\n");

        self.emit_dynamic_value(out, left, "rax", "rcx", "rdx")?;
        out.push_str("    mov QWORD PTR [rsp], rax\n");
        out.push_str("    mov QWORD PTR [rsp+8], rcx\n");
        out.push_str("    mov QWORD PTR [rsp+16], rdx\n");

        self.emit_dynamic_value(out, right, "rax", "rcx", "rdx")?;
        out.push_str("    mov QWORD PTR [rsp+24], rax\n");
        out.push_str("    mov QWORD PTR [rsp+32], rcx\n");
        out.push_str("    mov QWORD PTR [rsp+40], rdx\n");

        out.push_str("    lea rcx, [rsp]\n");
        out.push_str("    lea rdx, [rsp+24]\n");
        out.push_str("    lea r8, [rsp+48]\n");
        out.push_str(&format!("    mov r9, {}\n", dynamic_binary_opcode(op)));
        out.push_str("    call rune_rt_dynamic_binary\n");

        out.push_str(&format!("    mov {tag_reg}, QWORD PTR [rsp+48]\n"));
        out.push_str(&format!("    mov {payload_reg}, QWORD PTR [rsp+56]\n"));
        out.push_str(&format!("    mov {extra_reg}, QWORD PTR [rsp+64]\n"));
        out.push_str("    add rsp, 80\n");
        Ok(())
    }

    fn emit_dynamic_compare(
        &mut self,
        out: &mut String,
        left: &Expr,
        op: &BinaryOp,
        right: &Expr,
    ) -> Result<(), CodegenError> {
        out.push_str("    sub rsp, 48\n");

        self.emit_dynamic_value(out, left, "rax", "rcx", "rdx")?;
        out.push_str("    mov QWORD PTR [rsp], rax\n");
        out.push_str("    mov QWORD PTR [rsp+8], rcx\n");
        out.push_str("    mov QWORD PTR [rsp+16], rdx\n");

        self.emit_dynamic_value(out, right, "rax", "rcx", "rdx")?;
        out.push_str("    mov QWORD PTR [rsp+24], rax\n");
        out.push_str("    mov QWORD PTR [rsp+32], rcx\n");
        out.push_str("    mov QWORD PTR [rsp+40], rdx\n");

        out.push_str("    lea rcx, [rsp]\n");
        out.push_str("    lea rdx, [rsp+24]\n");
        out.push_str(&format!("    mov r8, {}\n", dynamic_compare_opcode(op)));
        out.push_str("    call rune_rt_dynamic_compare\n");
        out.push_str("    add rsp, 48\n");
        Ok(())
    }

    fn emit_string_arg(
        &mut self,
        out: &mut String,
        expr: &Expr,
        ptr_reg: &str,
        len_reg: &str,
        context: &str,
    ) -> Result<(), CodegenError> {
        if let ExprKind::Call { callee, args } = &expr.kind
            && let ExprKind::Identifier(name) = &callee.kind
            && (name == "str" || name == "repr")
        {
            let display_name = name.as_str();
            let magic_name = if name == "repr" { "__repr__" } else { "__str__" };
            let [CallArg::Positional(value_expr)] = args.as_slice() else {
                return Err(CodegenError {
                    message: format!("`{display_name}` expects 1 positional argument in the native backend"),
                    span: expr.span,
                });
            };
            match self.infer_expr_type(value_expr) {
                Some(IrType::Bool) => {
                    self.emit_expr(out, value_expr)?;
                    out.push_str("    mov ecx, eax\n");
                    out.push_str("    call rune_rt_string_from_bool\n");
                }
                Some(IrType::I32) => {
                    self.emit_expr(out, value_expr)?;
                    out.push_str("    mov ecx, eax\n");
                    out.push_str("    movsxd rcx, ecx\n");
                    out.push_str("    call rune_rt_string_from_i64\n");
                }
                Some(IrType::I64) => {
                    self.emit_expr(out, value_expr)?;
                    out.push_str("    mov rcx, rax\n");
                    out.push_str("    call rune_rt_string_from_i64\n");
                }
                Some(IrType::String) => {
                    return self.emit_string_arg(out, value_expr, ptr_reg, len_reg, context);
                }
                Some(IrType::Dynamic) => {
                    self.emit_dynamic_value(out, value_expr, "rcx", "rdx", "r8")?;
                    out.push_str("    call rune_rt_dynamic_to_string\n");
                }
                Some(IrType::Json) => {
                    self.emit_into_reg(out, "rcx", value_expr)?;
                    out.push_str("    call rune_rt_json_to_string\n");
                }
                Some(IrType::Struct(struct_name)) => {
                    let synthetic_name = struct_method_symbol(&struct_name, magic_name);
                    if self.function_names.contains(&synthetic_name) {
                        if self.function_returns.get(&synthetic_name) != Some(&IrType::String) {
                            return Err(CodegenError {
                                message: format!(
                                    "`{display_name}` on `{struct_name}` requires `{magic_name}`, when defined, to have signature `{magic_name}(self) -> String` in the native backend"
                                ),
                                span: expr.span,
                            });
                        }
                        let callee = Expr {
                            kind: ExprKind::Identifier(synthetic_name),
                            span: expr.span,
                        };
                        let call_args = vec![CallArg::Positional(value_expr.clone())];
                        self.emit_call(out, &callee, &call_args, expr.span)?;
                    } else {
                        let layout = self
                            .struct_layouts
                            .get(&struct_name)
                            .cloned()
                            .ok_or_else(|| CodegenError {
                                message: format!(
                                    "missing struct layout for `{struct_name}` in the native backend"
                                ),
                                span: expr.span,
                            })?;
                        let fallback_expr =
                            build_default_struct_string_expr(value_expr, &struct_name, &layout);
                        return self.emit_string_arg(out, &fallback_expr, ptr_reg, len_reg, context);
                    }
                }
                _ => {
                    return Err(CodegenError {
                        message: format!(
                            "`{display_name}` conversion is not supported for this expression in the native backend"
                        ),
                        span: expr.span,
                    });
                }
            }
            self.capture_runtime_string_result(out, ptr_reg, len_reg);
            return Ok(());
        }

        if let ExprKind::Call { callee, args } = &expr.kind
            && let ExprKind::Identifier(name) = &callee.kind
            && name == "input"
        {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: "`input` expects 0 arguments in the native backend".into(),
                    span: expr.span,
                });
            }
            out.push_str("    call rune_rt_input_line\n");
            self.capture_runtime_string_result(out, ptr_reg, len_reg);
            return Ok(());
        }

        if let ExprKind::Call { callee, args } = &expr.kind
            && let ExprKind::Identifier(name) = &callee.kind
            && matches!(
                name.as_str(),
                "__rune_builtin_env_arg"
                    | "__rune_builtin_env_get_string"
                    | "__rune_builtin_network_tcp_recv"
                    | "__rune_builtin_network_tcp_recv_timeout"
                    | "__rune_builtin_network_tcp_server_accept"
                    | "__rune_builtin_network_tcp_server_reply"
                    | "__rune_builtin_network_last_error_message"
                    | "__rune_builtin_network_tcp_request"
                    | "__rune_builtin_network_udp_recv"
                    | "__rune_builtin_fs_read_string"
                    | "__rune_builtin_serial_read_line"
            )
        {
            if name == "__rune_builtin_fs_read_string" {
                let [CallArg::Positional(path_expr)] = args.as_slice() else {
                    return Err(CodegenError {
                        message:
                            "`__rune_builtin_fs_read_string` expects 1 positional argument in the native backend"
                                .into(),
                        span: expr.span,
                    });
                };
                self.emit_string_arg(out, path_expr, "rcx", "rdx", "filesystem path")?;
                out.push_str("    call rune_rt_fs_read_string\n");
            } else if name == "__rune_builtin_env_arg" {
                let [CallArg::Positional(index_expr)] = args.as_slice() else {
                    return Err(CodegenError {
                        message:
                            "`__rune_builtin_env_arg` expects 1 positional argument in the native backend"
                                .into(),
                        span: expr.span,
                    });
                };
                self.emit_into_reg(out, "ecx", index_expr)?;
                out.push_str("    call rune_rt_env_arg\n");
            } else if name == "__rune_builtin_env_get_string" {
                let [CallArg::Positional(name_expr), CallArg::Positional(default_expr)] =
                    args.as_slice()
                else {
                    return Err(CodegenError {
                        message:
                            "`__rune_builtin_env_get_string` expects 2 positional arguments in the native backend"
                                .into(),
                        span: expr.span,
                    });
                };
                self.emit_string_arg(out, name_expr, "rcx", "rdx", "environment variable name")?;
                self.emit_string_arg(
                    out,
                    default_expr,
                    "r8",
                    "r9",
                    "default environment value",
                )?;
                out.push_str("    call rune_rt_env_get_string\n");
            } else if name == "__rune_builtin_network_last_error_message" {
                if !args.is_empty() {
                    return Err(CodegenError {
                        message:
                            "`__rune_builtin_network_last_error_message` expects 0 positional arguments in the native backend"
                                .into(),
                        span: expr.span,
                    });
                }
                out.push_str("    call rune_rt_network_last_error_message\n");
            } else if name == "__rune_builtin_network_tcp_recv" {
                let [
                    CallArg::Positional(host_expr),
                    CallArg::Positional(port_expr),
                    CallArg::Positional(max_expr),
                ] = args.as_slice()
                else {
                    return Err(CodegenError {
                        message:
                            "`__rune_builtin_network_tcp_recv` expects 3 positional arguments in the native backend"
                                .into(),
                        span: expr.span,
                    });
                };
                self.emit_string_arg(out, host_expr, "rcx", "rdx", "TCP recv host")?;
                self.emit_into_reg(out, "r8d", port_expr)?;
                self.emit_into_reg(out, "r9d", max_expr)?;
                out.push_str("    call rune_rt_network_tcp_recv\n");
            } else if name == "__rune_builtin_network_tcp_recv_timeout" {
                let [
                    CallArg::Positional(host_expr),
                    CallArg::Positional(port_expr),
                    CallArg::Positional(max_expr),
                    CallArg::Positional(timeout_expr),
                ] = args.as_slice()
                else {
                    return Err(CodegenError {
                        message:
                            "`__rune_builtin_network_tcp_recv_timeout` expects 4 positional arguments in the native backend"
                                .into(),
                        span: expr.span,
                    });
                };
                self.emit_string_arg(out, host_expr, "rcx", "rdx", "TCP recv host")?;
                self.emit_into_reg(out, "r8d", port_expr)?;
                self.emit_into_reg(out, "r9d", max_expr)?;
                out.push_str("    sub rsp, 48\n");
                self.emit_into_reg(out, "r10d", timeout_expr)?;
                out.push_str("    mov DWORD PTR [rsp+32], r10d\n");
                out.push_str("    call rune_rt_network_tcp_recv_timeout\n");
                out.push_str("    add rsp, 48\n");
            } else if name == "__rune_builtin_network_tcp_request" {
                let [
                    CallArg::Positional(host_expr),
                    CallArg::Positional(port_expr),
                    CallArg::Positional(data_expr),
                    CallArg::Positional(max_expr),
                    CallArg::Positional(timeout_expr),
                ] = args.as_slice()
                else {
                    return Err(CodegenError {
                        message:
                            "`__rune_builtin_network_tcp_request` expects 5 positional arguments in the native backend"
                                .into(),
                        span: expr.span,
                    });
                };
                self.emit_string_arg(out, host_expr, "rcx", "rdx", "TCP request host")?;
                self.emit_into_reg(out, "r8d", port_expr)?;
                self.emit_string_arg(out, data_expr, "r9", "r10", "TCP request data")?;
                out.push_str("    sub rsp, 64\n");
                out.push_str("    mov QWORD PTR [rsp+32], r10\n");
                self.emit_into_reg(out, "r10d", max_expr)?;
                out.push_str("    mov DWORD PTR [rsp+40], r10d\n");
                self.emit_into_reg(out, "r10d", timeout_expr)?;
                out.push_str("    mov DWORD PTR [rsp+48], r10d\n");
                out.push_str("    call rune_rt_network_tcp_request\n");
                out.push_str("    add rsp, 64\n");
            } else if name == "__rune_builtin_network_tcp_accept_once" {
                let [
                    CallArg::Positional(host_expr),
                    CallArg::Positional(port_expr),
                    CallArg::Positional(max_expr),
                    CallArg::Positional(timeout_expr),
                ] = args.as_slice()
                else {
                    return Err(CodegenError {
                        message:
                            "`__rune_builtin_network_tcp_accept_once` expects 4 positional arguments in the native backend"
                                .into(),
                        span: expr.span,
                    });
                };
                self.emit_string_arg(out, host_expr, "rcx", "rdx", "TCP accept host")?;
                self.emit_into_reg(out, "r8d", port_expr)?;
                self.emit_into_reg(out, "r9d", max_expr)?;
                out.push_str("    sub rsp, 48\n");
                self.emit_into_reg(out, "r10d", timeout_expr)?;
                out.push_str("    mov DWORD PTR [rsp+32], r10d\n");
                out.push_str("    call rune_rt_network_tcp_accept_once\n");
                out.push_str("    add rsp, 48\n");
            } else if name == "__rune_builtin_network_tcp_reply_once" {
                let [
                    CallArg::Positional(host_expr),
                    CallArg::Positional(port_expr),
                    CallArg::Positional(data_expr),
                    CallArg::Positional(max_expr),
                    CallArg::Positional(timeout_expr),
                ] = args.as_slice()
                else {
                    return Err(CodegenError {
                        message:
                            "`__rune_builtin_network_tcp_reply_once` expects 5 positional arguments in the native backend"
                                .into(),
                        span: expr.span,
                    });
                };
                self.emit_string_arg(out, host_expr, "rcx", "rdx", "TCP reply host")?;
                self.emit_into_reg(out, "r8d", port_expr)?;
                self.emit_string_arg(out, data_expr, "r9", "r10", "TCP reply data")?;
                out.push_str("    sub rsp, 64\n");
                out.push_str("    mov QWORD PTR [rsp+32], r10\n");
                self.emit_into_reg(out, "r10d", max_expr)?;
                out.push_str("    mov DWORD PTR [rsp+40], r10d\n");
                self.emit_into_reg(out, "r10d", timeout_expr)?;
                out.push_str("    mov DWORD PTR [rsp+48], r10d\n");
                out.push_str("    call rune_rt_network_tcp_reply_once\n");
                out.push_str("    add rsp, 64\n");
            } else if name == "__rune_builtin_network_tcp_server_open" {
                let [CallArg::Positional(host_expr), CallArg::Positional(port_expr)] =
                    args.as_slice()
                else {
                    return Err(CodegenError {
                        message:
                            "`__rune_builtin_network_tcp_server_open` expects 2 positional arguments in the native backend"
                                .into(),
                        span: expr.span,
                    });
                };
                self.emit_string_arg(out, host_expr, "rcx", "rdx", "TCP server host")?;
                self.emit_into_reg(out, "r8d", port_expr)?;
                out.push_str("    call rune_rt_network_tcp_server_open\n");
            } else if name == "__rune_builtin_network_tcp_client_open" {
                let [
                    CallArg::Positional(host_expr),
                    CallArg::Positional(port_expr),
                    CallArg::Positional(timeout_expr),
                ] = args.as_slice()
                else {
                    return Err(CodegenError {
                        message:
                            "`__rune_builtin_network_tcp_client_open` expects 3 positional arguments in the native backend"
                                .into(),
                        span: expr.span,
                    });
                };
                self.emit_string_arg(out, host_expr, "rcx", "rdx", "TCP client host")?;
                self.emit_into_reg(out, "r8d", port_expr)?;
                self.emit_into_reg(out, "r9d", timeout_expr)?;
                out.push_str("    call rune_rt_network_tcp_client_open\n");
            } else if name == "__rune_builtin_network_tcp_server_accept" {
                let [
                    CallArg::Positional(handle_expr),
                    CallArg::Positional(max_expr),
                    CallArg::Positional(timeout_expr),
                ] = args.as_slice()
                else {
                    return Err(CodegenError {
                        message:
                            "`__rune_builtin_network_tcp_server_accept` expects 3 positional arguments in the native backend"
                                .into(),
                        span: expr.span,
                    });
                };
                self.emit_into_reg(out, "ecx", handle_expr)?;
                self.emit_into_reg(out, "r8d", max_expr)?;
                self.emit_into_reg(out, "r9d", timeout_expr)?;
                out.push_str("    call rune_rt_network_tcp_server_accept\n");
            } else if name == "__rune_builtin_network_tcp_server_reply" {
                let [
                    CallArg::Positional(handle_expr),
                    CallArg::Positional(data_expr),
                    CallArg::Positional(max_expr),
                    CallArg::Positional(timeout_expr),
                ] = args.as_slice()
                else {
                    return Err(CodegenError {
                        message:
                            "`__rune_builtin_network_tcp_server_reply` expects 4 positional arguments in the native backend"
                                .into(),
                        span: expr.span,
                    });
                };
                self.emit_into_reg(out, "ecx", handle_expr)?;
                self.emit_string_arg(out, data_expr, "rdx", "r8", "TCP server reply data")?;
                self.emit_into_reg(out, "r9d", max_expr)?;
                out.push_str("    sub rsp, 48\n");
                self.emit_into_reg(out, "r10d", timeout_expr)?;
                out.push_str("    mov DWORD PTR [rsp+32], r10d\n");
                out.push_str("    call rune_rt_network_tcp_server_reply\n");
                out.push_str("    add rsp, 48\n");
            } else if name == "__rune_builtin_network_tcp_server_close" {
                let [CallArg::Positional(handle_expr)] = args.as_slice() else {
                    return Err(CodegenError {
                        message:
                            "`__rune_builtin_network_tcp_server_close` expects 1 positional argument in the native backend"
                                .into(),
                        span: expr.span,
                    });
                };
                self.emit_into_reg(out, "ecx", handle_expr)?;
                out.push_str("    call rune_rt_network_tcp_server_close\n");
            } else if name == "__rune_builtin_network_tcp_client_send" {
                let [CallArg::Positional(handle_expr), CallArg::Positional(data_expr)] =
                    args.as_slice()
                else {
                    return Err(CodegenError {
                        message:
                            "`__rune_builtin_network_tcp_client_send` expects 2 positional arguments in the native backend"
                                .into(),
                        span: expr.span,
                    });
                };
                self.emit_into_reg(out, "ecx", handle_expr)?;
                self.emit_string_arg(out, data_expr, "rdx", "r8", "TCP client send data")?;
                out.push_str("    call rune_rt_network_tcp_client_send\n");
            } else if name == "__rune_builtin_network_tcp_client_recv" {
                let [
                    CallArg::Positional(handle_expr),
                    CallArg::Positional(max_expr),
                    CallArg::Positional(timeout_expr),
                ] = args.as_slice()
                else {
                    return Err(CodegenError {
                        message:
                            "`__rune_builtin_network_tcp_client_recv` expects 3 positional arguments in the native backend"
                                .into(),
                        span: expr.span,
                    });
                };
                self.emit_into_reg(out, "ecx", handle_expr)?;
                self.emit_into_reg(out, "edx", max_expr)?;
                self.emit_into_reg(out, "r8d", timeout_expr)?;
                out.push_str("    call rune_rt_network_tcp_client_recv\n");
            } else if name == "__rune_builtin_network_tcp_client_close" {
                let [CallArg::Positional(handle_expr)] = args.as_slice() else {
                    return Err(CodegenError {
                        message:
                            "`__rune_builtin_network_tcp_client_close` expects 1 positional argument in the native backend"
                                .into(),
                        span: expr.span,
                    });
                };
                self.emit_into_reg(out, "ecx", handle_expr)?;
                out.push_str("    call rune_rt_network_tcp_client_close\n");
            } else if name == "__rune_builtin_network_last_error_code" {
                if !args.is_empty() {
                    return Err(CodegenError {
                        message:
                            "`__rune_builtin_network_last_error_code` expects 0 positional arguments in the native backend"
                                .into(),
                        span: expr.span,
                    });
                }
                out.push_str("    call rune_rt_network_last_error_code\n");
            } else if name == "__rune_builtin_network_last_error_message" {
                if !args.is_empty() {
                    return Err(CodegenError {
                        message:
                            "`__rune_builtin_network_last_error_message` expects 0 positional arguments in the native backend"
                                .into(),
                        span: expr.span,
                    });
                }
                out.push_str("    call rune_rt_network_last_error_message\n");
            } else if name == "__rune_builtin_network_clear_error" {
                if !args.is_empty() {
                    return Err(CodegenError {
                        message:
                            "`__rune_builtin_network_clear_error` expects 0 positional arguments in the native backend"
                                .into(),
                        span: expr.span,
                    });
                }
                out.push_str("    call rune_rt_network_clear_error\n");
            } else if name == "__rune_builtin_network_udp_recv" {
                let [
                    CallArg::Positional(host_expr),
                    CallArg::Positional(port_expr),
                    CallArg::Positional(max_expr),
                    CallArg::Positional(timeout_expr),
                ] = args.as_slice()
                else {
                    return Err(CodegenError {
                        message:
                            "`__rune_builtin_network_udp_recv` expects 4 positional arguments in the native backend"
                                .into(),
                        span: expr.span,
                    });
                };
                self.emit_string_arg(out, host_expr, "rcx", "rdx", "UDP recv host")?;
                self.emit_into_reg(out, "r8d", port_expr)?;
                self.emit_into_reg(out, "r9d", max_expr)?;
                out.push_str("    sub rsp, 48\n");
                self.emit_into_reg(out, "r10d", timeout_expr)?;
                out.push_str("    mov DWORD PTR [rsp+32], r10d\n");
                out.push_str("    call rune_rt_network_udp_recv\n");
                out.push_str("    add rsp, 48\n");
            } else if name == "__rune_builtin_serial_read_line" {
                if !args.is_empty() {
                    return Err(CodegenError {
                        message:
                            "`__rune_builtin_serial_read_line` expects 0 positional arguments in the native backend"
                                .into(),
                        span: expr.span,
                    });
                }
                out.push_str("    call rune_rt_serial_read_line\n");
            } else if name == "__rune_builtin_serial_read_line_timeout" {
                let [CallArg::Positional(timeout_expr)] = args.as_slice() else {
                    return Err(CodegenError {
                        message:
                            "`__rune_builtin_serial_read_line_timeout` expects 1 positional argument in the native backend"
                                .into(),
                        span: expr.span,
                    });
                };
                self.emit_into_reg(out, "rcx", timeout_expr)?;
                out.push_str("    call rune_rt_serial_read_line_timeout\n");
            } else if name == "__rune_builtin_serial_peek_byte" {
                if !args.is_empty() {
                    return Err(CodegenError {
                        message: "`__rune_builtin_serial_peek_byte` expects 0 positional arguments in the native backend"
                            .into(),
                        span: expr.span,
                    });
                }
                out.push_str("    call rune_rt_serial_peek_byte\n");
                return Ok(());
            } else {
                return Err(CodegenError {
                    message: format!(
                        "unsupported runtime string builtin `{name}` in the native backend"
                    ),
                    span: expr.span,
                });
            }
            self.capture_runtime_string_result(out, ptr_reg, len_reg);
            return Ok(());
        }

        if let ExprKind::Call { callee, args } = &expr.kind
            && let ExprKind::Identifier(name) = &callee.kind
            && matches!(
                name.as_str(),
                "__rune_builtin_json_stringify"
                    | "__rune_builtin_json_kind"
                    | "__rune_builtin_json_to_string"
            )
        {
            let [CallArg::Positional(json_expr)] = args.as_slice() else {
                return Err(CodegenError {
                    message: format!(
                        "`{name}` expects 1 positional argument in the native backend"
                    ),
                    span: expr.span,
                });
            };
            self.emit_into_reg(out, "rcx", json_expr)?;
            let runtime = match name.as_str() {
                "__rune_builtin_json_stringify" => "rune_rt_json_stringify",
                "__rune_builtin_json_kind" => "rune_rt_json_kind",
                "__rune_builtin_json_to_string" => "rune_rt_json_to_string",
                _ => unreachable!(),
            };
            out.push_str(&format!("    call {runtime}\n"));
            self.capture_runtime_string_result(out, ptr_reg, len_reg);
            return Ok(());
        }

        if let ExprKind::Call { callee, args } = &expr.kind
            && let ExprKind::Identifier(name) = &callee.kind
            && matches!(
                name.as_str(),
                "__rune_builtin_system_platform"
                    | "__rune_builtin_system_arch"
                    | "__rune_builtin_system_target"
                    | "__rune_builtin_system_board"
            )
        {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: format!("`{name}` expects 0 positional arguments in the native backend"),
                    span: expr.span,
                });
            }
            let runtime = match name.as_str() {
                "__rune_builtin_system_platform" => "rune_rt_system_platform",
                "__rune_builtin_system_arch" => "rune_rt_system_arch",
                "__rune_builtin_system_target" => "rune_rt_system_target",
                "__rune_builtin_system_board" => "rune_rt_system_board",
                _ => unreachable!(),
            };
            out.push_str(&format!("    call {runtime}\n"));
            self.capture_runtime_string_result(out, ptr_reg, len_reg);
            return Ok(());
        }

        if let ExprKind::Call { callee, args } = &expr.kind
        {
            let (target_name, owned_args) = self.resolve_call_target(callee, args, expr.span)?;
            if self.function_returns.get(&target_name) == Some(&IrType::String) {
                self.emit_call(out, callee, args, expr.span)?;
                if self.extern_functions.contains(&target_name) {
                    self.capture_runtime_string_result(out, ptr_reg, len_reg);
                } else {
                    out.push_str(&format!(
                        "    mov QWORD PTR [rbp-{}], rax\n",
                        self.scratch_offset
                    ));
                    out.push_str(&format!(
                        "    mov QWORD PTR [rbp-{}], rdx\n",
                        self.scratch_offset + 8
                    ));
                    if ptr_reg != "rax" {
                        out.push_str(&format!(
                            "    mov {ptr_reg}, QWORD PTR [rbp-{}]\n",
                            self.scratch_offset
                        ));
                    }
                    if len_reg != "rdx" {
                        out.push_str(&format!(
                            "    mov {len_reg}, QWORD PTR [rbp-{}]\n",
                            self.scratch_offset + 8
                        ));
                    }
                }
                return Ok(());
            }
            let _ = owned_args;
        }

        if let ExprKind::Binary {
            left,
            op: BinaryOp::Add,
            right,
        } = &expr.kind
            && self.infer_expr_type(expr) == Some(IrType::String)
        {
            self.emit_string_arg(out, left, "rcx", "rdx", "left string operand")?;
            self.emit_string_arg(out, right, "r8", "r9", "right string operand")?;
            out.push_str("    call rune_rt_string_concat\n");
            self.capture_runtime_string_result(out, ptr_reg, len_reg);
            return Ok(());
        }

        if let Some(binding) = self.resolve_field_binding(expr) {
            if let LocalBinding::String {
                ptr_offset,
                len_offset,
            } = binding
            {
                out.push_str(&format!(
                    "    mov {ptr_reg}, QWORD PTR [rbp-{ptr_offset}]\n"
                ));
                out.push_str(&format!(
                    "    mov {len_reg}, QWORD PTR [rbp-{len_offset}]\n"
                ));
                return Ok(());
            }
        }

        if let Some(projected) = self.project_constructor_field_expr(expr) {
            return self.emit_string_arg(out, projected, ptr_reg, len_reg, context);
        }

        let ExprKind::String(value) = &expr.kind else {
            if let ExprKind::Identifier(name) = &expr.kind {
                let binding = self.binding(name).map_err(|_| CodegenError {
                    message: format!(
                        "{context} must currently be a string literal or string parameter in the native backend"
                    ),
                    span: expr.span,
                })?;
                let LocalBinding::String {
                    ptr_offset,
                    len_offset,
                } = binding
                else {
                    return Err(CodegenError {
                        message: format!(
                            "{context} expected a string value, found scalar `{name}` in the native backend"
                        ),
                        span: expr.span,
                    });
                };
                out.push_str(&format!(
                    "    mov {ptr_reg}, QWORD PTR [rbp-{ptr_offset}]\n"
                ));
                out.push_str(&format!(
                    "    mov {len_reg}, QWORD PTR [rbp-{len_offset}]\n"
                ));
                return Ok(());
            }
            return Err(CodegenError {
                message: format!(
                    "{context} must currently be a string literal or string parameter in the native backend"
                ),
                span: expr.span,
            });
        };
        let label = self.intern_string(value);
        out.push_str(&format!("    lea {ptr_reg}, {label}[rip]\n"));
        out.push_str(&format!("    mov {len_reg}, {}\n", value.len()));
        Ok(())
    }

    fn capture_runtime_string_result(&self, out: &mut String, ptr_reg: &str, len_reg: &str) {
        out.push_str(&format!(
            "    mov QWORD PTR [rbp-{}], rax\n",
            self.scratch_offset
        ));
        out.push_str("    call rune_rt_last_string_len\n");
        if len_reg != "rax" {
            out.push_str(&format!("    mov {len_reg}, rax\n"));
        }
        out.push_str(&format!(
            "    mov {ptr_reg}, QWORD PTR [rbp-{}]\n",
            self.scratch_offset
        ));
    }

    fn emit_c_string_arg(
        &mut self,
        out: &mut String,
        expr: &Expr,
        target_reg: &str,
        context: &str,
    ) -> Result<(), CodegenError> {
        self.emit_string_arg(out, expr, "rcx", "rdx", context)?;
        out.push_str("    call rune_rt_to_c_string\n");
        if target_reg != "rax" {
            out.push_str(&format!("    mov {target_reg}, rax\n"));
        }
        Ok(())
    }

    fn emit_load_rax(&self, out: &mut String, operand: &SimpleOperand) {
        out.push_str(&format!("    mov rax, {}\n", operand.render()));
    }

    fn emit_binary_op(&self, out: &mut String, op: &str, operand: &SimpleOperand) {
        out.push_str(&format!("    {op} rax, {}\n", operand.render()));
    }

    fn simple_operand(&self, expr: &Expr) -> Option<SimpleOperand> {
        match &expr.kind {
            ExprKind::Identifier(name) => match self.offsets.get(name).cloned() {
                Some(LocalBinding::Scalar { offset, .. }) => Some(SimpleOperand::StackSlot(offset)),
                _ => None,
            },
            ExprKind::Field { .. } => match self.resolve_field_binding(expr) {
                Some(LocalBinding::Scalar { offset, .. }) => Some(SimpleOperand::StackSlot(offset)),
                _ => self
                    .project_constructor_field_expr(expr)
                    .and_then(|projected| self.simple_operand(projected)),
            },
            ExprKind::Integer(value) => Some(SimpleOperand::Immediate(value.clone())),
            ExprKind::Bool(value) => Some(SimpleOperand::Immediate(if *value {
                "1".to_string()
            } else {
                "0".to_string()
            })),
            _ => None,
        }
    }

    fn infer_expr_type(&self, expr: &Expr) -> Option<IrType> {
        match &expr.kind {
            ExprKind::Identifier(name) => self.offsets.get(name).map(LocalBinding::ir_type),
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
            } => self.infer_expr_type(expr),
            ExprKind::Unary {
                op: UnaryOp::Not, ..
            } => Some(IrType::Bool),
            ExprKind::Binary { left, op, right } => {
                let left_ty = self.infer_expr_type(left)?;
                let right_ty = self.infer_expr_type(right)?;
                match op {
                    BinaryOp::And | BinaryOp::Or => Some(IrType::Bool),
                    BinaryOp::Add => {
                        if left_ty == right_ty
                            && matches!(left_ty, IrType::I32 | IrType::I64 | IrType::String)
                        {
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
                }
            }
            ExprKind::Call { callee, .. } => {
                let ExprKind::Identifier(name) = &callee.kind else {
                    return None;
                };
                builtin_return_type(name)
                    .or_else(|| {
                        self.struct_layouts
                            .contains_key(name)
                            .then(|| IrType::Struct(name.clone()))
                    })
                    .or_else(|| self.function_returns.get(name).cloned())
            }
            ExprKind::Await { .. } => None,
            ExprKind::Field { base, name } => {
                if let Some(binding) = self.resolve_field_binding(expr) {
                    return Some(binding.ir_type());
                }
                if let Some(projected) = self.project_constructor_field_expr(expr) {
                    return self.infer_expr_type(projected);
                }
                let IrType::Struct(struct_name) = self.infer_expr_type(base)? else {
                    return None;
                };
                self.struct_layouts
                    .get(&struct_name)
                    .and_then(|fields| fields.iter().find(|(field_name, _)| field_name == name))
                    .map(|(_, ty)| ty.clone())
            }
        }
    }

    fn is_dynamic_comparison(&self, op: &BinaryOp, left: &Expr, right: &Expr) -> bool {
        matches!(
            op,
            BinaryOp::EqualEqual
                | BinaryOp::NotEqual
                | BinaryOp::Greater
                | BinaryOp::GreaterEqual
                | BinaryOp::Less
                | BinaryOp::LessEqual
        ) && (self.infer_expr_type(left) == Some(IrType::Dynamic)
            || self.infer_expr_type(right) == Some(IrType::Dynamic))
    }

    fn emit_field_expr(&mut self, out: &mut String, expr: &Expr) -> Result<(), CodegenError> {
        if let Some(binding) = self.resolve_field_binding(expr) {
            return match binding {
                LocalBinding::Scalar { offset, .. } => {
                    out.push_str(&format!("    mov rax, QWORD PTR [rbp-{offset}]\n"));
                    Ok(())
                }
                LocalBinding::String { .. } => Err(CodegenError {
                    message: "string struct fields must be used in string contexts in the native backend"
                        .into(),
                    span: expr.span,
                }),
                LocalBinding::Dynamic { .. } => Err(CodegenError {
                    message: "dynamic struct fields are not supported by the native backend".into(),
                    span: expr.span,
                }),
                LocalBinding::Struct { .. } => Err(CodegenError {
                    message: "nested struct fields must be accessed through a concrete leaf field in the native backend"
                        .into(),
                    span: expr.span,
                }),
            };
        }

        if let Some(projected) = self.project_constructor_field_expr(expr) {
            return self.emit_expr(out, projected);
        }

        Err(CodegenError {
            message: "field access is only supported for local struct values and constructor projections in the native backend"
                .into(),
            span: expr.span,
        })
    }

    fn try_emit_struct_equality(
        &mut self,
        out: &mut String,
        expr: &Expr,
        left: &Expr,
        op: &BinaryOp,
        right: &Expr,
    ) -> Result<bool, CodegenError> {
        if !matches!(op, BinaryOp::EqualEqual | BinaryOp::NotEqual) {
            return Ok(false);
        }
        let Some(IrType::Struct(struct_name)) = self.infer_expr_type(left) else {
            return Ok(false);
        };
        if self.infer_expr_type(right) != Some(IrType::Struct(struct_name.clone())) {
            return Ok(false);
        }
        if self.function_names.contains(&struct_method_symbol(&struct_name, "__eq__")) {
            let callee = Expr {
                kind: ExprKind::Identifier(struct_method_symbol(&struct_name, "__eq__")),
                span: expr.span,
            };
            let args = vec![CallArg::Positional(left.clone()), CallArg::Positional(right.clone())];
            self.emit_call(out, &callee, &args, expr.span)?;
            if matches!(op, BinaryOp::NotEqual) {
                out.push_str("    xor eax, 1\n");
            }
            out.push_str("    movzx rax, al\n");
            return Ok(true);
        }
        let layout = self
            .struct_layouts
            .get(&struct_name)
            .cloned()
            .ok_or_else(|| CodegenError {
                message: format!("missing struct layout for `{struct_name}` in the native backend"),
                span: expr.span,
            })?;
        let compare_expr = build_default_struct_eq_expr(left, right, &layout, *op);
        self.emit_expr(out, &compare_expr)?;
        Ok(true)
    }

    fn emit_struct_arg_ptr(
        &self,
        out: &mut String,
        expr: &Expr,
        reg: &str,
        span: Span,
    ) -> Result<(), CodegenError> {
        let Some(offset) = self.struct_arg_base_offset(expr) else {
            return Err(CodegenError {
                message: "struct call arguments must currently be local struct values in the native backend"
                    .into(),
                span,
            });
        };
        out.push_str(&format!("    lea {reg}, [rbp-{offset}]\n"));
        Ok(())
    }

    fn struct_arg_base_offset(&self, expr: &Expr) -> Option<i32> {
        match &expr.kind {
            ExprKind::Identifier(name) => {
                let binding = self.offsets.get(name)?;
                Self::struct_binding_base_offset(binding)
            }
            _ => None,
        }
    }

    fn struct_binding_base_offset(binding: &LocalBinding) -> Option<i32> {
        match binding {
            LocalBinding::Struct { fields, .. } => fields
                .values()
                .filter_map(Self::struct_binding_base_offset)
                .max(),
            LocalBinding::Scalar { offset, .. } => Some(*offset),
            LocalBinding::String { ptr_offset, .. } => Some(*ptr_offset),
            LocalBinding::Dynamic { tag_offset, .. } => Some(*tag_offset),
        }
    }

    fn resolve_field_binding(&self, expr: &Expr) -> Option<LocalBinding> {
        match &expr.kind {
            ExprKind::Identifier(name) => self.offsets.get(name).cloned(),
            ExprKind::Field { base, name } => {
                let LocalBinding::Struct { fields, .. } = self.resolve_field_binding(base)? else {
                    return None;
                };
                fields.get(name).cloned()
            }
            _ => None,
        }
    }

    fn project_constructor_field_expr<'b>(&self, expr: &'b Expr) -> Option<&'b Expr> {
        let ExprKind::Field { base, name } = &expr.kind else {
            return None;
        };
        let ExprKind::Call { callee, args } = &base.kind else {
            return None;
        };
        let ExprKind::Identifier(type_name) = &callee.kind else {
            return None;
        };
        self.struct_layouts.get(type_name)?;
        args.iter().find_map(|arg| match arg {
            CallArg::Keyword {
                name: field_name,
                value,
                ..
            } if field_name == name => Some(value),
            _ => None,
        })
    }

    fn emit_store_struct_value(
        &mut self,
        binding: &LocalBinding,
        out: &mut String,
        expr: &Expr,
        span: Span,
    ) -> Result<(), CodegenError> {
        let LocalBinding::Struct { name, fields } = binding else {
            return Err(CodegenError {
                message: "expected struct local binding".into(),
                span,
            });
        };
        match &expr.kind {
            ExprKind::Identifier(other_name) => {
                let other = self.binding(other_name)?;
                let LocalBinding::Struct {
                    name: other_struct,
                    fields: other_fields,
                } = other
                else {
                    return Err(CodegenError {
                        message: format!(
                            "assignment expected struct `{name}`, found `{other_name}`"
                        ),
                        span,
                    });
                };
                if &other_struct != name {
                    return Err(CodegenError {
                        message: format!(
                            "assignment expected struct `{name}`, found `{other_struct}`"
                        ),
                        span,
                    });
                }
                for (field_name, field_binding) in fields {
                    let source_binding = other_fields
                        .get(field_name)
                        .expect("matching struct fields");
                    self.copy_binding_value(out, field_binding, source_binding);
                }
                Ok(())
            }
            ExprKind::Call { callee, args } => {
                let ExprKind::Identifier(callee_name) = &callee.kind else {
                    return Err(CodegenError {
                        message: format!(
                            "struct `{name}` assignment requires a direct `{name}(...)` constructor"
                        ),
                        span,
                    });
                };
                if callee_name != name {
                    if self.function_names.contains(callee_name)
                        && self.function_returns.get(callee_name)
                            == Some(&IrType::Struct(name.clone()))
                    {
                        return self.emit_struct_returning_call_into_binding(
                            out,
                            binding,
                            callee_name,
                            args,
                            span,
                        );
                    }
                    return Err(CodegenError {
                        message: format!(
                            "assignment expected struct `{name}`, found `{callee_name}`"
                        ),
                        span,
                    });
                }
                for (field_name, field_binding) in fields {
                    let value_expr = args
                        .iter()
                        .find_map(|arg| match arg {
                            CallArg::Keyword { name, value, .. } if name == field_name => {
                                Some(value)
                            }
                            _ => None,
                        })
                        .ok_or_else(|| CodegenError {
                            message: format!(
                                "struct `{name}` constructor is missing field `{field_name}`"
                            ),
                            span,
                        })?;
                    self.store_value_into_binding(out, field_binding, value_expr, span)?;
                }
                Ok(())
            }
            _ => Err(CodegenError {
                message: format!(
                    "struct `{name}` values must come from a direct constructor or another local struct value in the native backend"
                ),
                span,
            }),
        }
    }

    fn emit_struct_returning_call_into_binding(
        &mut self,
        out: &mut String,
        binding: &LocalBinding,
        callee_name: &str,
        args: &[CallArg],
        span: Span,
    ) -> Result<(), CodegenError> {
        let Some(param_meta) = self.function_params.get(callee_name) else {
            return Err(CodegenError {
                message: format!("missing parameter metadata for `{callee_name}`"),
                span,
            });
        };
        let ordered_args = self.resolve_call_args(callee_name, param_meta, args, span)?;
        let register_count = param_meta
            .iter()
            .map(|(_, ty)| match ty {
                AbiType::Scalar(_) => 1usize,
                AbiType::String => 2usize,
                AbiType::CString => 1usize,
                AbiType::Dynamic => 3usize,
                AbiType::Struct(_) => 1usize,
            })
            .sum::<usize>()
            + 1;
        if register_count > 4 {
            return Err(CodegenError {
                message: "the current native backend supports at most 4 argument registers per struct-return call"
                    .into(),
                span,
            });
        }
        let arg_regs = ["rcx", "rdx", "r8", "r9"];
        let all_simple =
            ordered_args
                .iter()
                .zip(param_meta.iter())
                .all(|(arg, (_, ty))| match ty {
                    AbiType::Scalar(_) => self.simple_operand(arg).is_some(),
                    AbiType::String => {
                        matches!(&arg.kind, ExprKind::String(_) | ExprKind::Identifier(_))
                    }
                    AbiType::CString => self.infer_expr_type(arg) == Some(IrType::String),
                    AbiType::Dynamic => self.infer_expr_type(arg).is_some(),
                    AbiType::Struct(_) => self.struct_arg_base_offset(arg).is_some(),
                });
        if !all_simple {
            return Err(CodegenError {
                message: "complex struct-return call arguments are not yet supported by the native backend"
                    .into(),
                span,
            });
        }
        let base_offset =
            Self::struct_binding_base_offset(binding).ok_or_else(|| CodegenError {
                message: "expected local struct storage for struct-return call".into(),
                span,
            })?;
        let mut reg_index = register_count;
        for ((_, ty), arg) in param_meta.iter().zip(ordered_args.iter()).rev() {
            match ty {
                AbiType::Scalar(kind) => {
                    reg_index -= 1;
                    self.emit_into_reg(out, abi_scalar_register(arg_regs[reg_index], *kind), arg)?;
                }
                AbiType::String => {
                    reg_index -= 2;
                    self.emit_string_arg(
                        out,
                        arg,
                        arg_regs[reg_index],
                        arg_regs[reg_index + 1],
                        "string argument",
                    )?;
                }
                AbiType::CString => {
                    reg_index -= 1;
                    self.emit_c_string_arg(out, arg, arg_regs[reg_index], "C string argument")?;
                }
                AbiType::Dynamic => {
                    reg_index -= 3;
                    self.emit_dynamic_value(
                        out,
                        arg,
                        arg_regs[reg_index],
                        arg_regs[reg_index + 1],
                        arg_regs[reg_index + 2],
                    )?;
                }
                AbiType::Struct(_) => {
                    reg_index -= 1;
                    self.emit_struct_arg_ptr(out, arg, arg_regs[reg_index], span)?;
                }
            }
        }
        out.push_str(&format!("    lea rcx, [rbp-{base_offset}]\n"));
        out.push_str(&format!(
            "    call {}\n",
            native_internal_symbol_name(callee_name)
        ));
        Ok(())
    }

    fn store_value_into_binding(
        &mut self,
        out: &mut String,
        binding: &LocalBinding,
        expr: &Expr,
        span: Span,
    ) -> Result<(), CodegenError> {
        match binding {
            LocalBinding::Scalar { offset, .. } => {
                self.emit_expr(out, expr)?;
                out.push_str(&format!("    mov QWORD PTR [rbp-{offset}], rax\n"));
                Ok(())
            }
            LocalBinding::String {
                ptr_offset,
                len_offset,
            } => {
                self.emit_string_arg(out, expr, "rax", "rcx", "struct string field")?;
                out.push_str(&format!("    mov QWORD PTR [rbp-{ptr_offset}], rax\n"));
                out.push_str(&format!("    mov QWORD PTR [rbp-{len_offset}], rcx\n"));
                Ok(())
            }
            LocalBinding::Dynamic { .. } => Err(CodegenError {
                message: "dynamic struct fields are not supported by the native backend".into(),
                span,
            }),
            LocalBinding::Struct { .. } => self.emit_store_struct_value(binding, out, expr, span),
        }
    }

    fn copy_binding_value(&self, out: &mut String, dst: &LocalBinding, src: &LocalBinding) {
        match (dst, src) {
            (
                LocalBinding::Scalar {
                    offset: dst_offset, ..
                },
                LocalBinding::Scalar {
                    offset: src_offset, ..
                },
            ) => {
                out.push_str(&format!("    mov rax, QWORD PTR [rbp-{src_offset}]\n"));
                out.push_str(&format!("    mov QWORD PTR [rbp-{dst_offset}], rax\n"));
            }
            (
                LocalBinding::String {
                    ptr_offset: dst_ptr,
                    len_offset: dst_len,
                },
                LocalBinding::String {
                    ptr_offset: src_ptr,
                    len_offset: src_len,
                },
            ) => {
                out.push_str(&format!("    mov rax, QWORD PTR [rbp-{src_ptr}]\n"));
                out.push_str(&format!("    mov QWORD PTR [rbp-{dst_ptr}], rax\n"));
                out.push_str(&format!("    mov rax, QWORD PTR [rbp-{src_len}]\n"));
                out.push_str(&format!("    mov QWORD PTR [rbp-{dst_len}], rax\n"));
            }
            (
                LocalBinding::Struct {
                    fields: dst_fields, ..
                },
                LocalBinding::Struct {
                    fields: src_fields, ..
                },
            ) => {
                for (field_name, dst_field) in dst_fields {
                    let src_field = src_fields.get(field_name).expect("matching struct field");
                    self.copy_binding_value(out, dst_field, src_field);
                }
            }
            _ => {}
        }
    }

    fn emit_copy_struct_from_ptr(
        &self,
        out: &mut String,
        binding: &LocalBinding,
        ptr_reg: &str,
        struct_name: &str,
    ) -> Result<(), CodegenError> {
        let layout = self
            .struct_layouts
            .get(struct_name)
            .ok_or_else(|| CodegenError {
                message: format!("missing struct layout for `{struct_name}`"),
                span: self.function_span,
            })?;
        let LocalBinding::Struct { fields, .. } = binding else {
            return Err(CodegenError {
                message: format!("expected local struct binding for `{struct_name}`"),
                span: self.function_span,
            });
        };
        let mut source_offset = 0i32;
        for (field_name, field_ty) in layout {
            let field_binding = fields.get(field_name).ok_or_else(|| CodegenError {
                message: format!(
                    "missing field binding `{field_name}` for struct parameter `{struct_name}`"
                ),
                span: self.function_span,
            })?;
            self.emit_copy_binding_from_ptr(out, field_binding, field_ty, ptr_reg, source_offset)?;
            source_offset += field_binding.slot_count() * 8;
        }
        Ok(())
    }

    fn emit_copy_binding_from_ptr(
        &self,
        out: &mut String,
        binding: &LocalBinding,
        ty: &IrType,
        ptr_reg: &str,
        source_offset: i32,
    ) -> Result<(), CodegenError> {
        match (binding, ty) {
            (LocalBinding::Scalar { offset, .. }, _) => {
                out.push_str(&format!(
                    "    mov rax, QWORD PTR [{ptr_reg}+{source_offset}]\n"
                ));
                out.push_str(&format!("    mov QWORD PTR [rbp-{offset}], rax\n"));
                Ok(())
            }
            (
                LocalBinding::String {
                    ptr_offset,
                    len_offset,
                },
                IrType::String,
            ) => {
                out.push_str(&format!(
                    "    mov rax, QWORD PTR [{ptr_reg}+{source_offset}]\n"
                ));
                out.push_str(&format!("    mov QWORD PTR [rbp-{ptr_offset}], rax\n"));
                out.push_str(&format!(
                    "    mov rax, QWORD PTR [{ptr_reg}+{}]\n",
                    source_offset + 8
                ));
                out.push_str(&format!("    mov QWORD PTR [rbp-{len_offset}], rax\n"));
                Ok(())
            }
            (LocalBinding::Struct { name, fields }, IrType::Struct(expected_name)) => {
                let nested_layout =
                    self.struct_layouts
                        .get(expected_name)
                        .ok_or_else(|| CodegenError {
                            message: format!("missing nested struct layout for `{expected_name}`"),
                            span: self.function_span,
                        })?;
                if name != expected_name {
                    return Err(CodegenError {
                        message: format!(
                            "struct parameter layout mismatch: expected `{expected_name}`, found `{name}`"
                        ),
                        span: self.function_span,
                    });
                }
                let mut nested_offset = source_offset;
                for (field_name, field_ty) in nested_layout {
                    let nested_binding = fields.get(field_name).ok_or_else(|| CodegenError {
                        message: format!("missing nested field binding `{field_name}`"),
                        span: self.function_span,
                    })?;
                    self.emit_copy_binding_from_ptr(
                        out,
                        nested_binding,
                        field_ty,
                        ptr_reg,
                        nested_offset,
                    )?;
                    nested_offset += nested_binding.slot_count() * 8;
                }
                Ok(())
            }
            _ => Err(CodegenError {
                message: "unsupported struct field layout in native backend".into(),
                span: self.function_span,
            }),
        }
    }

    fn emit_write_struct_value_to_ptr(
        &mut self,
        out: &mut String,
        struct_name: &str,
        expr: &Expr,
        ptr_reg: &str,
        span: Span,
    ) -> Result<(), CodegenError> {
        match &expr.kind {
            ExprKind::Identifier(name) => {
                let binding = self.binding(name)?;
                let LocalBinding::Struct {
                    name: local_name, ..
                } = &binding
                else {
                    return Err(CodegenError {
                        message: format!("return expected struct `{struct_name}`, found `{name}`"),
                        span,
                    });
                };
                if local_name != struct_name {
                    return Err(CodegenError {
                        message: format!(
                            "return expected struct `{struct_name}`, found `{local_name}`"
                        ),
                        span,
                    });
                }
                self.emit_copy_binding_to_ptr(out, &binding, ptr_reg, struct_name, 0)
            }
            ExprKind::Call { callee, args } => {
                let ExprKind::Identifier(callee_name) = &callee.kind else {
                    return Err(CodegenError {
                        message: format!(
                            "return expected struct `{struct_name}` from a direct constructor or function call"
                        ),
                        span,
                    });
                };
                if callee_name == struct_name {
                    let layout = self
                        .struct_layouts
                        .get(struct_name)
                        .ok_or_else(|| CodegenError {
                            message: format!("missing struct layout for `{struct_name}`"),
                            span,
                        })?
                        .clone();
                    let mut offset = 0i32;
                    for (field_name, field_ty) in &layout {
                        let value_expr = args
                            .iter()
                            .find_map(|arg| match arg {
                                CallArg::Keyword { name, value, .. } if name == field_name => Some(value),
                                _ => None,
                            })
                            .ok_or_else(|| CodegenError {
                                message: format!(
                                    "struct `{struct_name}` constructor is missing field `{field_name}`"
                                ),
                                span,
                            })?;
                        self.emit_write_field_expr_to_ptr(
                            out, value_expr, field_ty, ptr_reg, offset, span,
                        )?;
                        offset +=
                            binding_for_type(1, field_ty, &self.struct_layouts).slot_count() * 8;
                    }
                    Ok(())
                } else if self.function_names.contains(callee_name)
                    && self.function_returns.get(callee_name)
                        == Some(&IrType::Struct(struct_name.to_string()))
                {
                    let temp_binding = binding_for_type(
                        self.scratch_offset / 8 + 1,
                        &IrType::Struct(struct_name.to_string()),
                        self.struct_layouts,
                    );
                    self.emit_struct_returning_call_into_binding(
                        out,
                        &temp_binding,
                        callee_name,
                        args,
                        span,
                    )?;
                    self.emit_copy_binding_to_ptr(out, &temp_binding, ptr_reg, struct_name, 0)
                } else {
                    Err(CodegenError {
                        message: format!(
                            "return expected struct `{struct_name}`, found `{callee_name}`"
                        ),
                        span,
                    })
                }
            }
            _ => Err(CodegenError {
                message: format!(
                    "struct `{struct_name}` returns must come from a local struct, constructor, or struct-returning call"
                ),
                span,
            }),
        }
    }

    fn emit_write_field_expr_to_ptr(
        &mut self,
        out: &mut String,
        expr: &Expr,
        ty: &IrType,
        ptr_reg: &str,
        offset: i32,
        span: Span,
    ) -> Result<(), CodegenError> {
        match ty {
            IrType::Bool | IrType::I32 | IrType::I64 | IrType::Json | IrType::Unit => {
                self.emit_expr(out, expr)?;
                out.push_str(&format!("    mov QWORD PTR [{ptr_reg}+{offset}], rax\n"));
                Ok(())
            }
            IrType::String => {
                self.emit_string_arg(out, expr, "rax", "rdx", "struct string field")?;
                out.push_str(&format!("    mov QWORD PTR [{ptr_reg}+{offset}], rax\n"));
                out.push_str(&format!(
                    "    mov QWORD PTR [{ptr_reg}+{}], rdx\n",
                    offset + 8
                ));
                Ok(())
            }
            IrType::Struct(name) => {
                self.emit_write_struct_value_to_ptr(out, name, expr, ptr_reg, span)
            }
            IrType::Dynamic => Err(CodegenError {
                message: "dynamic struct fields are not supported by the native backend".into(),
                span,
            }),
        }
    }

    fn emit_copy_binding_to_ptr(
        &self,
        out: &mut String,
        binding: &LocalBinding,
        ptr_reg: &str,
        struct_name: &str,
        base_offset: i32,
    ) -> Result<(), CodegenError> {
        let layout = self
            .struct_layouts
            .get(struct_name)
            .ok_or_else(|| CodegenError {
                message: format!("missing struct layout for `{struct_name}`"),
                span: self.function_span,
            })?;
        let LocalBinding::Struct { fields, .. } = binding else {
            return Err(CodegenError {
                message: format!("expected local struct binding for `{struct_name}`"),
                span: self.function_span,
            });
        };
        let mut offset = base_offset;
        for (field_name, field_ty) in layout {
            let field_binding = fields.get(field_name).ok_or_else(|| CodegenError {
                message: format!("missing field binding `{field_name}` for `{struct_name}`"),
                span: self.function_span,
            })?;
            self.emit_copy_binding_leaf_to_ptr(out, field_binding, field_ty, ptr_reg, offset)?;
            offset += field_binding.slot_count() * 8;
        }
        Ok(())
    }

    fn emit_copy_binding_leaf_to_ptr(
        &self,
        out: &mut String,
        binding: &LocalBinding,
        ty: &IrType,
        ptr_reg: &str,
        offset: i32,
    ) -> Result<(), CodegenError> {
        match (binding, ty) {
            (LocalBinding::Scalar { offset: src, .. }, _) => {
                out.push_str(&format!("    mov rax, QWORD PTR [rbp-{src}]\n"));
                out.push_str(&format!("    mov QWORD PTR [{ptr_reg}+{offset}], rax\n"));
                Ok(())
            }
            (
                LocalBinding::String {
                    ptr_offset,
                    len_offset,
                },
                IrType::String,
            ) => {
                out.push_str(&format!("    mov rax, QWORD PTR [rbp-{ptr_offset}]\n"));
                out.push_str(&format!("    mov QWORD PTR [{ptr_reg}+{offset}], rax\n"));
                out.push_str(&format!("    mov rax, QWORD PTR [rbp-{len_offset}]\n"));
                out.push_str(&format!(
                    "    mov QWORD PTR [{ptr_reg}+{}], rax\n",
                    offset + 8
                ));
                Ok(())
            }
            (LocalBinding::Struct { name, .. }, IrType::Struct(expected)) => {
                if name != expected {
                    return Err(CodegenError {
                        message: format!(
                            "struct layout mismatch: expected `{expected}`, found `{name}`"
                        ),
                        span: self.function_span,
                    });
                }
                self.emit_copy_binding_to_ptr(out, binding, ptr_reg, expected, offset)
            }
            _ => Err(CodegenError {
                message: "unsupported struct copy layout in native backend".into(),
                span: self.function_span,
            }),
        }
    }
}

#[derive(Debug, Clone)]
enum SimpleOperand {
    Immediate(String),
    StackSlot(i32),
}

impl SimpleOperand {
    fn render(&self) -> String {
        match self {
            SimpleOperand::Immediate(value) => value.clone(),
            SimpleOperand::StackSlot(offset) => format!("QWORD PTR [rbp-{offset}]"),
        }
    }

    fn render_32(&self) -> String {
        match self {
            SimpleOperand::Immediate(value) => value.clone(),
            SimpleOperand::StackSlot(offset) => format!("DWORD PTR [rbp-{offset}]"),
        }
    }
}

fn is_32bit_register(reg: &str) -> bool {
    matches!(
        reg,
        "eax"
            | "ebx"
            | "ecx"
            | "edx"
            | "esi"
            | "edi"
            | "r8d"
            | "r9d"
            | "r10d"
            | "r11d"
            | "r12d"
            | "r13d"
            | "r14d"
            | "r15d"
    )
}

fn abi_scalar_register<'a>(register: &'a str, kind: ScalarKind) -> &'a str {
    match kind {
        ScalarKind::I32 | ScalarKind::Bool => match register {
            "rax" => "eax",
            "rcx" => "ecx",
            "rdx" => "edx",
            "r8" => "r8d",
            "r9" => "r9d",
            other => other,
        },
        ScalarKind::I64 | ScalarKind::Json => register,
    }
}

fn move_reg(out: &mut String, dst: &str, src: &str) {
    if dst != src {
        out.push_str(&format!("    mov {dst}, {src}\n"));
    }
}

fn binding_for_type(
    next_slot: i32,
    ty: &IrType,
    struct_layouts: &HashMap<String, Vec<(String, IrType)>>,
) -> LocalBinding {
    match ty {
        IrType::Bool => LocalBinding::Scalar {
            offset: next_slot * 8,
            kind: ScalarKind::Bool,
        },
        IrType::I32 => LocalBinding::Scalar {
            offset: next_slot * 8,
            kind: ScalarKind::I32,
        },
        IrType::Json => LocalBinding::Scalar {
            offset: next_slot * 8,
            kind: ScalarKind::Json,
        },
        IrType::I64 | IrType::Unit => LocalBinding::Scalar {
            offset: next_slot * 8,
            kind: ScalarKind::I64,
        },
        IrType::String => LocalBinding::String {
            ptr_offset: next_slot * 8,
            len_offset: (next_slot + 1) * 8,
        },
        IrType::Dynamic => LocalBinding::Dynamic {
            tag_offset: next_slot * 8,
            payload_offset: (next_slot + 1) * 8,
            extra_offset: (next_slot + 2) * 8,
        },
        IrType::Struct(name) => {
            let fields = struct_layouts
                .get(name)
                .expect("struct layout must exist for native codegen");
            let mut bindings = HashMap::new();
            let mut slot = next_slot;
            let mut staged = Vec::with_capacity(fields.len());
            for (field_name, field_ty) in fields.iter().rev() {
                let binding = binding_for_type(slot, field_ty, struct_layouts);
                slot += binding.slot_count();
                staged.push((field_name.clone(), binding));
            }
            for (field_name, binding) in staged {
                bindings.insert(field_name, binding);
            }
            LocalBinding::Struct {
                name: name.clone(),
                fields: bindings,
            }
        }
    }
}

fn dynamic_compare_opcode(op: &BinaryOp) -> i64 {
    match op {
        BinaryOp::EqualEqual => DYNAMIC_CMP_EQ,
        BinaryOp::NotEqual => DYNAMIC_CMP_NE,
        BinaryOp::Greater => DYNAMIC_CMP_GT,
        BinaryOp::GreaterEqual => DYNAMIC_CMP_GE,
        BinaryOp::Less => DYNAMIC_CMP_LT,
        BinaryOp::LessEqual => DYNAMIC_CMP_LE,
        _ => unreachable!("not a comparison operator"),
    }
}

fn dynamic_binary_opcode(op: &BinaryOp) -> i64 {
    match op {
        BinaryOp::Add => 0,
        BinaryOp::Subtract => 1,
        BinaryOp::Multiply => 2,
        BinaryOp::Divide => 3,
        BinaryOp::Modulo => 4,
        _ => unreachable!("not a supported dynamic binary operator"),
    }
}

fn binary_op_name(op: &BinaryOp) -> &'static str {
    match op {
        BinaryOp::And => "and",
        BinaryOp::Add => "+",
        BinaryOp::Or => "or",
        BinaryOp::Subtract => "-",
        BinaryOp::Multiply => "*",
        BinaryOp::Divide => "/",
        BinaryOp::Modulo => "%",
        BinaryOp::EqualEqual => "==",
        BinaryOp::NotEqual => "!=",
        BinaryOp::Greater => ">",
        BinaryOp::GreaterEqual => ">=",
        BinaryOp::Less => "<",
        BinaryOp::LessEqual => "<=",
    }
}

fn type_ref_to_ir_type(ty: Option<&crate::parser::TypeRef>) -> IrType {
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

fn builtin_return_type(name: &str) -> Option<IrType> {
    match name {
        "print" | "println" | "eprint" | "eprintln" | "flush" | "eflush" => Some(IrType::Unit),
        "input"
        | "__rune_builtin_arduino_read_line"
        | "__rune_builtin_serial_read_line"
        | "__rune_builtin_serial_read_line_timeout" => {
            Some(IrType::String)
        }
        "panic" => Some(IrType::Unit),
        "str" | "repr" => Some(IrType::String),
        "int" => Some(IrType::I64),
        "__rune_builtin_json_parse" => Some(IrType::Json),
        "__rune_builtin_json_stringify"
        | "__rune_builtin_json_kind"
        | "__rune_builtin_json_to_string" => Some(IrType::String),
        "__rune_builtin_system_platform"
        | "__rune_builtin_system_arch"
        | "__rune_builtin_system_target"
        | "__rune_builtin_system_board" => Some(IrType::String),
        "__rune_builtin_json_is_null"
        | "__rune_builtin_json_to_bool"
        | "__rune_builtin_gpio_digital_read"
        | "__rune_builtin_arduino_digital_read"
        | "__rune_builtin_system_is_embedded"
        | "__rune_builtin_system_is_wasm" => Some(IrType::Bool),
        "__rune_builtin_json_len"
        | "__rune_builtin_json_to_i64"
        | "__rune_builtin_gpio_analog_read"
        | "__rune_builtin_arduino_analog_read"
        | "__rune_builtin_arduino_pulse_in"
        | "__rune_builtin_arduino_shift_in"
        | "__rune_builtin_arduino_millis"
        | "__rune_builtin_arduino_micros"
        | "__rune_builtin_arduino_random_i64"
        | "__rune_builtin_arduino_random_range"
        | "__rune_builtin_sum_range" => Some(IrType::I64),
        "__rune_builtin_json_get" | "__rune_builtin_json_index" => Some(IrType::Json),
        "__rune_builtin_time_now_unix"
        | "__rune_builtin_time_monotonic_ms"
        | "__rune_builtin_time_monotonic_us" => Some(IrType::I64),
        "__rune_builtin_time_sleep_ms"
        | "__rune_builtin_gpio_pin_mode"
        | "__rune_builtin_gpio_digital_write"
        | "__rune_builtin_gpio_pwm_write"
        | "__rune_builtin_time_sleep_us"
        | "__rune_builtin_system_exit"
        | "__rune_builtin_terminal_clear"
        | "__rune_builtin_terminal_move_to"
        | "__rune_builtin_terminal_hide_cursor"
        | "__rune_builtin_terminal_show_cursor"
        | "__rune_builtin_terminal_set_title"
        | "__rune_builtin_arduino_pin_mode"
        | "__rune_builtin_arduino_digital_write"
        | "__rune_builtin_arduino_analog_write"
        | "__rune_builtin_arduino_analog_reference"
        | "__rune_builtin_arduino_shift_out"
        | "__rune_builtin_arduino_tone"
        | "__rune_builtin_arduino_no_tone"
        | "__rune_builtin_arduino_servo_detach"
        | "__rune_builtin_arduino_servo_write"
        | "__rune_builtin_arduino_servo_write_us"
        | "__rune_builtin_arduino_delay_ms"
        | "__rune_builtin_arduino_delay_us"
        | "__rune_builtin_arduino_uart_begin"
        | "__rune_builtin_arduino_uart_write_byte"
        | "__rune_builtin_arduino_uart_write"
        | "__rune_builtin_arduino_interrupts_enable"
        | "__rune_builtin_arduino_interrupts_disable"
        | "__rune_builtin_arduino_random_seed"
        | "__rune_builtin_serial_close"
        | "__rune_builtin_serial_flush" => Some(IrType::Unit),
        "__rune_builtin_system_pid"
        | "__rune_builtin_system_cpu_count"
        | "__rune_builtin_env_get_i32"
        | "__rune_builtin_env_arg_count"
        | "__rune_builtin_gpio_mode_input"
        | "__rune_builtin_gpio_mode_output"
        | "__rune_builtin_gpio_mode_input_pullup"
        | "__rune_builtin_gpio_pwm_duty_max"
        | "__rune_builtin_gpio_analog_max"
        | "__rune_builtin_serial_available"
        | "__rune_builtin_serial_read_byte"
        | "__rune_builtin_serial_read_byte_timeout"
        | "__rune_builtin_arduino_uart_peek_byte"
        | "__rune_builtin_serial_peek_byte"
        | "__rune_builtin_arduino_mode_input"
        | "__rune_builtin_arduino_mode_output"
        | "__rune_builtin_arduino_mode_input_pullup"
        | "__rune_builtin_arduino_led_builtin"
        | "__rune_builtin_arduino_high"
        | "__rune_builtin_arduino_low"
        | "__rune_builtin_arduino_bit_order_lsb_first"
        | "__rune_builtin_arduino_bit_order_msb_first"
        | "__rune_builtin_arduino_analog_ref_default"
        | "__rune_builtin_arduino_analog_ref_internal"
        | "__rune_builtin_arduino_analog_ref_external"
        | "__rune_builtin_arduino_uart_available"
        | "__rune_builtin_arduino_uart_read_byte" => Some(IrType::I64),
        "__rune_builtin_serial_open"
        | "__rune_builtin_arduino_servo_attach"
        | "__rune_builtin_serial_is_open"
        | "__rune_builtin_serial_write"
        | "__rune_builtin_serial_write_byte"
        | "__rune_builtin_serial_write_line" => Some(IrType::Bool),
        "__rune_builtin_env_exists"
        | "__rune_builtin_env_get_bool"
        | "__rune_builtin_network_tcp_connect"
        | "__rune_builtin_network_tcp_listen"
        | "__rune_builtin_network_tcp_send"
        | "__rune_builtin_network_tcp_connect_timeout"
        | "__rune_builtin_network_tcp_server_close"
        | "__rune_builtin_network_tcp_client_send"
        | "__rune_builtin_network_tcp_client_close"
        | "__rune_builtin_network_udp_bind"
        | "__rune_builtin_network_udp_send"
        | "__rune_builtin_network_clear_error"
        | "__rune_builtin_fs_exists"
        | "__rune_builtin_fs_set_current_dir"
        | "__rune_builtin_fs_is_file"
        | "__rune_builtin_fs_is_dir"
        | "__rune_builtin_fs_write_string"
        | "__rune_builtin_fs_append_string"
        | "__rune_builtin_fs_remove"
        | "__rune_builtin_fs_rename"
        | "__rune_builtin_fs_copy"
        | "__rune_builtin_fs_create_dir"
        | "__rune_builtin_fs_create_dir_all"
        | "__rune_builtin_audio_bell" => Some(IrType::Bool),
        "__rune_builtin_env_arg"
        | "__rune_builtin_env_get_string"
        | "__rune_builtin_network_tcp_recv"
        | "__rune_builtin_network_tcp_recv_timeout"
        | "__rune_builtin_network_tcp_accept_once"
        | "__rune_builtin_network_tcp_reply_once"
        | "__rune_builtin_network_tcp_server_accept"
        | "__rune_builtin_network_tcp_client_recv"
        | "__rune_builtin_network_tcp_server_reply"
        | "__rune_builtin_network_last_error_message"
        | "__rune_builtin_network_tcp_request"
        | "__rune_builtin_network_udp_recv"
        | "__rune_builtin_fs_current_dir"
        | "__rune_builtin_fs_read_string"
        | "__rune_builtin_fs_canonicalize" => Some(IrType::String),
        "__rune_builtin_network_last_error_code" => Some(IrType::I32),
        "__rune_builtin_network_tcp_server_open"
        | "__rune_builtin_network_tcp_client_open" => Some(IrType::I32),
        "__rune_builtin_fs_file_size" => Some(IrType::I64),
        _ => None,
    }
}

fn collect_struct_layouts(program: &Program) -> HashMap<String, Vec<(String, IrType)>> {
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
                    .map(|field| (field.name.clone(), type_ref_to_ir_type(Some(&field.ty))))
                    .collect::<Vec<_>>(),
            ))
        })
        .collect()
}

fn build_string_expr(span: Span, value: impl Into<String>) -> Expr {
    Expr {
        kind: ExprKind::String(value.into()),
        span,
    }
}

fn build_bool_expr(span: Span, value: bool) -> Expr {
    Expr {
        kind: ExprKind::Bool(value),
        span,
    }
}

fn build_identifier_expr(span: Span, name: &str) -> Expr {
    Expr {
        kind: ExprKind::Identifier(name.to_string()),
        span,
    }
}

fn build_binary_add_expr(span: Span, left: Expr, right: Expr) -> Expr {
    build_binary_expr(span, left, BinaryOp::Add, right)
}

fn build_binary_expr(span: Span, left: Expr, op: BinaryOp, right: Expr) -> Expr {
    Expr {
        kind: ExprKind::Binary {
            left: Box::new(left),
            op,
            right: Box::new(right),
        },
        span,
    }
}

fn build_str_call_expr(expr: &Expr) -> Expr {
    Expr {
        kind: ExprKind::Call {
            callee: Box::new(build_identifier_expr(expr.span, "str")),
            args: vec![CallArg::Positional(expr.clone())],
        },
        span: expr.span,
    }
}

fn build_default_struct_string_expr(
    base: &Expr,
    struct_name: &str,
    fields: &[(String, IrType)],
) -> Expr {
    let span = base.span;
    let mut rendered = build_string_expr(span, format!("{struct_name}("));
    for (index, (field_name, _)) in fields.iter().enumerate() {
        if index > 0 {
            rendered = build_binary_add_expr(span, rendered, build_string_expr(span, ", "));
        }
        rendered = build_binary_add_expr(
            span,
            rendered,
            build_string_expr(span, format!("{field_name}=")),
        );
        let field_expr = Expr {
            kind: ExprKind::Field {
                base: Box::new(base.clone()),
                name: field_name.clone(),
            },
            span,
        };
        rendered = build_binary_add_expr(span, rendered, build_str_call_expr(&field_expr));
    }
    build_binary_add_expr(span, rendered, build_string_expr(span, ")"))
}

fn build_default_struct_eq_expr(
    left: &Expr,
    right: &Expr,
    fields: &[(String, IrType)],
    op: BinaryOp,
) -> Expr {
    let span = left.span;
    let mut rendered = build_bool_expr(span, true);
    for (field_name, _) in fields {
        let left_field = Expr {
            kind: ExprKind::Field {
                base: Box::new(left.clone()),
                name: field_name.clone(),
            },
            span,
        };
        let right_field = Expr {
            kind: ExprKind::Field {
                base: Box::new(right.clone()),
                name: field_name.clone(),
            },
            span,
        };
        let field_eq = build_binary_expr(span, left_field, BinaryOp::EqualEqual, right_field);
        rendered = build_binary_expr(span, rendered, BinaryOp::And, field_eq);
    }
    if matches!(op, BinaryOp::NotEqual) {
        Expr {
            kind: ExprKind::Unary {
                op: UnaryOp::Not,
                expr: Box::new(rendered),
            },
            span,
        }
    } else {
        rendered
    }
}

fn escape_ascii(value: &str) -> String {
    let mut escaped = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other if other.is_ascii_graphic() || other == ' ' => escaped.push(other),
            other => escaped.push_str(&format!("\\x{:02x}", other as u32)),
        }
    }
    escaped
}

fn peephole_optimize_asm(asm: &str) -> String {
    let mut lines = asm.lines().map(|line| line.to_string()).collect::<Vec<_>>();

    loop {
        let mut changed = false;
        let mut out = Vec::new();
        let mut i = 0usize;

        while i < lines.len() {
            if i + 1 < lines.len()
                && lines[i].trim() == "push rax"
                && lines[i + 1].trim_start().starts_with("pop ")
            {
                let reg = lines[i + 1]
                    .trim_start()
                    .strip_prefix("pop ")
                    .expect("checked above");
                out.push(format!("    mov {reg}, rax"));
                i += 2;
                changed = true;
                continue;
            }

            if i + 1 < lines.len()
                && lines[i].trim() == "xor eax, eax"
                && lines[i + 1].trim() == "xor eax, eax"
            {
                out.push(lines[i].clone());
                i += 2;
                changed = true;
                continue;
            }

            if i + 5 < lines.len()
                && lines[i].trim_start().starts_with("mov rax, ")
                && lines[i + 1].trim() == "push rax"
                && lines[i + 2].trim_start().starts_with("mov rax, ")
                && lines[i + 3].trim() == "mov rcx, rax"
                && lines[i + 4].trim() == "pop rax"
            {
                let rhs = lines[i + 2]
                    .trim_start()
                    .strip_prefix("mov rax, ")
                    .expect("checked above");
                if let Some(op) = lines[i + 5].trim_start().strip_prefix("add rax, rcx") {
                    let _ = op;
                    out.push(lines[i].clone());
                    out.push(format!("    add rax, {rhs}"));
                    i += 6;
                    changed = true;
                    continue;
                }
                if lines[i + 5].trim() == "sub rax, rcx" {
                    out.push(lines[i].clone());
                    out.push(format!("    sub rax, {rhs}"));
                    i += 6;
                    changed = true;
                    continue;
                }
                if lines[i + 5].trim() == "cmp rax, rcx" {
                    out.push(lines[i].clone());
                    out.push(format!("    cmp rax, {rhs}"));
                    i += 6;
                    changed = true;
                    continue;
                }
            }

            if i + 4 < lines.len()
                && lines[i].trim_start().starts_with("mov rax, ")
                && lines[i + 1].trim() == lines[i].trim()
                && lines[i + 2].trim() == "push rax"
                && lines[i + 3].trim_start().starts_with("mov rax, ")
                && lines[i + 4].trim() == "mov rcx, rax"
            {
                out.push(lines[i].clone());
                out.push(lines[i + 2].clone());
                out.push(lines[i + 3].clone());
                out.push(lines[i + 4].clone());
                i += 5;
                changed = true;
                continue;
            }

            if i + 1 < lines.len()
                && lines[i].trim_start().starts_with("jmp ")
                && lines[i + 1].trim_start().starts_with("jmp ")
            {
                out.push(lines[i].clone());
                i += 2;
                changed = true;
                continue;
            }

            if i + 1 < lines.len()
                && lines[i].trim_start().starts_with("jmp ")
                && lines[i + 1].trim_end() == format!("{}:", lines[i].trim_start()[4..].trim())
            {
                i += 1;
                changed = true;
                continue;
            }

            if lines[i].trim_start().starts_with("jmp ") {
                out.push(lines[i].clone());
                i += 1;
                while i < lines.len() {
                    let trimmed = lines[i].trim();
                    if trimmed.is_empty() || trimmed.ends_with(':') || trimmed.starts_with('.') {
                        break;
                    }
                    i += 1;
                    changed = true;
                }
                continue;
            }

            out.push(lines[i].clone());
            i += 1;
        }

        if !changed {
            return out.join("\n") + "\n";
        }
        lines = out;
    }
}
