use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use crate::ir::{IrArg, IrFunction, IrInst, IrProgram, IrType, lower_program};
use crate::optimize::optimize_program;
use crate::parser::{BinaryOp, Item, Program, TypeRef, parse_source};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlvmIrError {
    pub message: String,
}

impl fmt::Display for LlvmIrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for LlvmIrError {}

pub fn emit_llvm_ir_source(source: &str) -> Result<String, LlvmIrError> {
    let mut program = parse_source(source).map_err(|error| LlvmIrError {
        message: error.message,
    })?;
    optimize_program(&mut program);
    emit_llvm_ir(&program)
}

pub fn emit_llvm_ir(program: &Program) -> Result<String, LlvmIrError> {
    let ir = lower_program(program);
    Emitter::new(program, &ir)?.emit()
}

#[derive(Debug, Clone)]
struct FunctionSig {
    is_extern: bool,
    params: Vec<(String, IrType)>,
    ret: IrType,
}

struct Emitter<'a> {
    ir: &'a IrProgram,
    signatures: HashMap<String, FunctionSig>,
    string_pool: BTreeMap<String, String>,
    declared_runtime: BTreeSet<String>,
}

impl<'a> Emitter<'a> {
    fn new(program: &'a Program, ir: &'a IrProgram) -> Result<Self, LlvmIrError> {
        let mut signatures = HashMap::new();
        for item in &program.items {
            let Item::Function(function) = item else {
                continue;
            };
            if function.is_async {
                return Err(LlvmIrError {
                    message: "async functions are not supported by the current LLVM IR backend"
                        .into(),
                });
            }
            let params = function
                .params
                .iter()
                .map(|param| Ok((param.name.clone(), type_ref_to_ir(&param.ty)?)))
                .collect::<Result<Vec<_>, LlvmIrError>>()?;
            let ret = match function.return_type.as_ref() {
                Some(ty) => type_ref_to_ir(ty)?,
                None => IrType::Unit,
            };
            signatures.insert(
                function.name.clone(),
                FunctionSig {
                    is_extern: function.is_extern,
                    params,
                    ret,
                },
            );
        }

        Ok(Self {
            ir,
            signatures,
            string_pool: BTreeMap::new(),
            declared_runtime: BTreeSet::new(),
        })
    }

    fn emit(mut self) -> Result<String, LlvmIrError> {
        let mut out = String::new();
        out.push_str("target triple = \"unknown\"\n\n");

        let mut function_bodies = Vec::new();
        let mut external_decls = Vec::new();
        for function in &self.ir.functions {
            function_bodies.push(self.emit_function(function)?);
        }
        for item in self
            .signatures
            .iter()
            .filter_map(|(name, sig)| {
                if self.ir.functions.iter().any(|function| &function.name == name) {
                    None
                } else {
                    Some((name, sig))
                }
            })
        {
            external_decls.push(self.emit_external_decl(item.0, item.1)?);
        }

        for (value, name) in &self.string_pool {
            let bytes = llvm_string_bytes(value);
            let storage_len = llvm_string_storage_len(value);
            out.push_str(&format!(
                "@{name} = private unnamed_addr constant [{} x i8] c\"{}\"\n",
                storage_len, bytes
            ));
        }
        if !self.string_pool.is_empty() {
            out.push('\n');
        }

        for decl in &self.declared_runtime {
            out.push_str(decl);
            out.push('\n');
        }
        for decl in &external_decls {
            out.push_str(decl);
            out.push('\n');
        }
        if !self.declared_runtime.is_empty() {
            out.push('\n');
        }

        for body in function_bodies {
            out.push_str(&body);
            out.push('\n');
        }

        Ok(out)
    }

    fn emit_function(&mut self, function: &IrFunction) -> Result<String, LlvmIrError> {
        let sig = self
            .signatures
            .get(&function.name)
            .ok_or_else(|| LlvmIrError {
                message: format!("missing signature for function `{}`", function.name),
            })?;
        if sig.is_extern && matches!(sig.ret, IrType::Dynamic) {
            return Err(LlvmIrError {
                message: format!(
                    "extern function `{}` uses a return type not yet supported by the current LLVM IR backend",
                    function.name
                ),
            });
        }
        if sig.is_extern && sig.params.iter().any(|(_, ty)| matches!(ty, IrType::Dynamic)) {
            return Err(LlvmIrError {
                message: format!(
                    "extern function `{}` uses parameter types not yet supported by the current LLVM IR backend",
                    function.name
                ),
            });
        }

        let local_types = function
            .locals
            .iter()
            .map(|local| (local.name.clone(), local.ty.clone()))
            .collect::<HashMap<_, _>>();
        let temp_types = infer_temp_types(function, &self.signatures)?;

        let mut out = String::new();
        out.push_str(&format!(
            "define {} @{}(",
            llvm_function_return_type(sig)?,
            function.name
        ));
        for (index, (name, ty)) in sig.params.iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            match ty {
                IrType::Dynamic if !sig.is_extern => out.push_str(&format!(
                    "i64 %{}.in.tag, i64 %{}.in.payload, i64 %{}.in.extra",
                    name, name, name
                )),
                IrType::String if !sig.is_extern => out.push_str(&format!(
                    "ptr %{}.in.ptr, i64 %{}.in.len",
                    name, name
                )),
                _ => out.push_str(&format!("{} %{}", llvm_extern_type(ty)?, name)),
            }
        }
        out.push_str(") {\nentry:\n");

        for (name, ty) in &sig.params {
            match ty {
                IrType::Dynamic if !sig.is_extern => {
                    out.push_str(&format!("  %{name}.tag = alloca i64\n"));
                    out.push_str(&format!("  %{name}.payload = alloca i64\n"));
                    out.push_str(&format!("  %{name}.extra = alloca i64\n"));
                    out.push_str(&format!("  store i64 %{}.in.tag, ptr %{name}.tag\n", name));
                    out.push_str(&format!(
                        "  store i64 %{}.in.payload, ptr %{name}.payload\n",
                        name
                    ));
                    out.push_str(&format!(
                        "  store i64 %{}.in.extra, ptr %{name}.extra\n",
                        name
                    ));
                }
                IrType::String if !sig.is_extern => {
                    out.push_str(&format!("  %{name}.ptr = alloca ptr\n"));
                    out.push_str(&format!("  %{name}.len = alloca i64\n"));
                    out.push_str(&format!("  store ptr %{}.in.ptr, ptr %{name}.ptr\n", name));
                    out.push_str(&format!("  store i64 %{}.in.len, ptr %{name}.len\n", name));
                }
                _ => {
                    out.push_str(&format!(
                        "  %{name}.addr = alloca {}\n",
                        llvm_scalar_type(ty)?
                    ));
                    out.push_str(&format!(
                        "  store {} %{name}, ptr %{name}.addr\n",
                        llvm_scalar_type(ty)?,
                    ));
                }
            }
        }
        for local in &function.locals {
            if sig.params.iter().any(|(name, _)| name == &local.name) {
                continue;
            }
            if matches!(local.ty, IrType::Unit) {
                return Err(LlvmIrError {
                    message: format!(
                        "local `{}` in `{}` uses a type not yet supported by the current LLVM IR backend",
                        local.name, function.name
                    ),
                });
            }
            match &local.ty {
                IrType::Dynamic => {
                    out.push_str(&format!("  %{}.tag = alloca i64\n", local.name));
                    out.push_str(&format!("  %{}.payload = alloca i64\n", local.name));
                    out.push_str(&format!("  %{}.extra = alloca i64\n", local.name));
                }
                IrType::String => {
                    out.push_str(&format!("  %{}.ptr = alloca ptr\n", local.name));
                    out.push_str(&format!("  %{}.len = alloca i64\n", local.name));
                }
                _ => out.push_str(&format!(
                    "  %{}.addr = alloca {}\n",
                    local.name,
                    llvm_scalar_type(&local.ty)?
                )),
            }
        }

        let mut emitter = FunctionEmitter {
            function_name: &function.name,
            signatures: &self.signatures,
            local_types: &local_types,
            temp_types: &temp_types,
            string_pool: &mut self.string_pool,
            declared_runtime: &mut self.declared_runtime,
            next_reg: 0,
            value_map: HashMap::new(),
            block_terminated: false,
        };

        for inst in &function.instructions {
            emitter.emit_inst(&mut out, inst, &sig.ret)?;
        }

        if sig.ret == IrType::Unit {
            if !emitter.block_terminated {
                out.push_str("  ret void\n");
            }
        }
        out.push_str("}\n");
        Ok(out)
    }

    fn emit_external_decl(&self, name: &str, sig: &FunctionSig) -> Result<String, LlvmIrError> {
        let mut out = format!("declare {} @{}(", llvm_extern_type(&sig.ret)?, name);
        for (index, (_, ty)) in sig.params.iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            out.push_str(llvm_extern_type(ty)?);
        }
        out.push(')');
        Ok(out)
    }
}

struct FunctionEmitter<'a> {
    function_name: &'a str,
    signatures: &'a HashMap<String, FunctionSig>,
    local_types: &'a HashMap<String, IrType>,
    temp_types: &'a HashMap<String, IrType>,
    string_pool: &'a mut BTreeMap<String, String>,
    declared_runtime: &'a mut BTreeSet<String>,
    next_reg: usize,
    value_map: HashMap<String, String>,
    block_terminated: bool,
}

impl<'a> FunctionEmitter<'a> {
    fn emit_inst(
        &mut self,
        out: &mut String,
        inst: &IrInst,
        expected_return: &IrType,
    ) -> Result<(), LlvmIrError> {
        match inst {
            IrInst::ConstInt { dst, value } => {
                self.value_map.insert(dst.clone(), value.clone());
            }
            IrInst::ConstBool { dst, value } => {
                self.value_map.insert(
                    dst.clone(),
                    if *value {
                        "true".into()
                    } else {
                        "false".into()
                    },
                );
            }
            IrInst::ConstString { dst, value } => {
                let value_ref = self.intern_string_ref(value);
                self.value_map.insert(dst.clone(), value_ref);
            }
            IrInst::Copy { dst, src } => {
                if let Some(local_ty) = self.local_types.get(dst) {
                    let value = if *local_ty == IrType::Dynamic {
                        self.resolve_dynamic_value(src, out)?
                    } else {
                        self.resolve_value(src, local_ty, out)?
                    };
                    match local_ty {
                        IrType::Dynamic => {
                            let (tag, payload, extra) = split_dynamic_value(&value)?;
                            out.push_str(&format!("  store {tag}, ptr %{dst}.tag\n"));
                            out.push_str(&format!("  store {payload}, ptr %{dst}.payload\n"));
                            out.push_str(&format!("  store {extra}, ptr %{dst}.extra\n"));
                        }
                        IrType::String => {
                            let (ptr, len) = split_string_value(&value)?;
                            out.push_str(&format!("  store {ptr}, ptr %{dst}.ptr\n"));
                            out.push_str(&format!("  store {len}, ptr %{dst}.len\n"));
                        }
                        _ => {
                            out.push_str(&format!(
                                "  store {} {}, ptr %{}.addr\n",
                                llvm_scalar_type(local_ty)?,
                                value,
                                dst
                            ));
                        }
                    }
                } else {
                    let ty = self
                        .temp_types
                        .get(src)
                        .or_else(|| self.local_types.get(src))
                        .ok_or_else(|| LlvmIrError {
                            message: format!(
                                "missing source type for `{src}` in `{}`",
                                self.function_name
                            ),
                        })?;
                    let value = self.resolve_value(src, ty, out)?;
                    self.value_map.insert(dst.clone(), value);
                }
            }
            IrInst::UnaryNeg { dst, src } => {
                let ty = self.temp_types.get(dst).ok_or_else(|| LlvmIrError {
                    message: format!("missing temp type for `{dst}`"),
                })?;
                let src_val = self.resolve_value(src, ty, out)?;
                let reg = self.next_reg();
                out.push_str(&format!(
                    "  {reg} = sub {} 0, {src_val}\n",
                    llvm_scalar_type(ty)?
                ));
                self.value_map.insert(dst.clone(), reg);
            }
            IrInst::UnaryNot { dst, src } => {
                let src_ty = self
                    .temp_types
                    .get(src)
                    .or_else(|| self.local_types.get(src))
                    .cloned()
                    .unwrap_or(IrType::Bool);
                let src_val = if src_ty == IrType::Dynamic {
                    self.emit_dynamic_truthy(out, src)?
                } else {
                    self.resolve_value(src, &IrType::Bool, out)?
                };
                let reg = self.next_reg();
                out.push_str(&format!("  {reg} = xor i1 {src_val}, true\n"));
                self.value_map.insert(dst.clone(), reg);
            }
            IrInst::Binary {
                dst,
                op,
                left,
                right,
            } => self.emit_binary(out, dst, op, left, right)?,
            IrInst::Call { dst, callee, args } => {
                self.emit_call(out, dst.as_ref(), callee, args)?
            }
            IrInst::Label(label) => {
                if !self.block_terminated {
                    out.push_str(&format!("  br label %{label}\n"));
                }
                out.push_str(&format!("{label}:\n"));
                self.block_terminated = false;
            }
            IrInst::BranchIf {
                cond,
                then_label,
                else_label,
            } => {
                let cond_ty = self
                    .temp_types
                    .get(cond)
                    .or_else(|| self.local_types.get(cond))
                    .cloned()
                    .unwrap_or(IrType::Bool);
                let cond_val = if cond_ty == IrType::Dynamic {
                    self.emit_dynamic_truthy(out, cond)?
                } else {
                    self.resolve_value(cond, &IrType::Bool, out)?
                };
                out.push_str(&format!(
                    "  br i1 {cond_val}, label %{then_label}, label %{else_label}\n"
                ));
                self.block_terminated = true;
            }
            IrInst::Jump(label) => {
                out.push_str(&format!("  br label %{label}\n"));
                self.block_terminated = true;
            }
            IrInst::Return(value) => match value {
                Some(value) => {
                    let ret_ty = expected_return;
                    let ret_val = if *ret_ty == IrType::Dynamic {
                        self.resolve_dynamic_value(value, out)?
                    } else {
                        self.resolve_value(value, ret_ty, out)?
                    };
                    if *ret_ty == IrType::String {
                        let (ptr, len) = split_string_value(&ret_val)?;
                        let agg0 = self.next_reg();
                        out.push_str(&format!(
                            "  {agg0} = insertvalue {} poison, {ptr}, 0\n",
                            llvm_internal_type(ret_ty)?
                        ));
                        let agg1 = self.next_reg();
                        out.push_str(&format!(
                            "  {agg1} = insertvalue {} {agg0}, {len}, 1\n",
                            llvm_internal_type(ret_ty)?
                        ));
                        out.push_str(&format!(
                            "  ret {} {agg1}\n",
                            llvm_internal_type(ret_ty)?
                        ));
                    } else if *ret_ty == IrType::Dynamic {
                        let (tag, payload, extra) = split_dynamic_value(&ret_val)?;
                        let agg0 = self.next_reg();
                        out.push_str(&format!(
                            "  {agg0} = insertvalue {} poison, {tag}, 0\n",
                            llvm_internal_type(ret_ty)?
                        ));
                        let agg1 = self.next_reg();
                        out.push_str(&format!(
                            "  {agg1} = insertvalue {} {agg0}, {payload}, 1\n",
                            llvm_internal_type(ret_ty)?
                        ));
                        let agg2 = self.next_reg();
                        out.push_str(&format!(
                            "  {agg2} = insertvalue {} {agg1}, {extra}, 2\n",
                            llvm_internal_type(ret_ty)?
                        ));
                        out.push_str(&format!(
                            "  ret {} {agg2}\n",
                            llvm_internal_type(ret_ty)?
                        ));
                    } else {
                        out.push_str(&format!(
                            "  ret {} {}\n",
                            llvm_scalar_type(ret_ty)?,
                            ret_val
                        ));
                    }
                    self.block_terminated = true;
                }
                None => {
                    out.push_str("  ret void\n");
                    self.block_terminated = true;
                }
            },
        }
        Ok(())
    }

    fn emit_binary(
        &mut self,
        out: &mut String,
        dst: &str,
        op: &BinaryOp,
        left: &str,
        right: &str,
    ) -> Result<(), LlvmIrError> {
        let ty = self.temp_types.get(dst).ok_or_else(|| LlvmIrError {
            message: format!("missing temp type for `{dst}`"),
        })?;
        let left_ty = self
            .temp_types
            .get(left)
            .or_else(|| self.local_types.get(left))
            .cloned()
            .unwrap_or(IrType::Bool);
        let right_ty = self
            .temp_types
            .get(right)
            .or_else(|| self.local_types.get(right))
            .cloned()
            .unwrap_or(IrType::Bool);

        if matches!(
            op,
            BinaryOp::EqualEqual
                | BinaryOp::NotEqual
                | BinaryOp::Greater
                | BinaryOp::GreaterEqual
                | BinaryOp::Less
                | BinaryOp::LessEqual
        ) && (left_ty == IrType::Dynamic || right_ty == IrType::Dynamic)
        {
            let reg = self.emit_dynamic_compare(out, left, right, op)?;
            self.value_map.insert(dst.to_string(), reg);
            return Ok(());
        }
        if *ty == IrType::Dynamic {
            let value = self.emit_dynamic_binary(out, left, right, op)?;
            self.value_map.insert(dst.to_string(), value);
            return Ok(());
        }
        if *ty == IrType::String && matches!(op, BinaryOp::Add) {
            let left_val = self.resolve_value(left, &IrType::String, out)?;
            let right_val = self.resolve_value(right, &IrType::String, out)?;
            let (left_ptr, left_len) = split_string_value(&left_val)?;
            let (right_ptr, right_len) = split_string_value(&right_val)?;
            self.declared_runtime
                .insert("declare ptr @rune_rt_string_concat(ptr, i64, ptr, i64)\n".into());
            self.declared_runtime
                .insert("declare i64 @rune_rt_last_string_len()\n".into());
            let ptr_reg = self.next_reg();
            out.push_str(&format!(
                "  {ptr_reg} = call ptr @rune_rt_string_concat({left_ptr}, {left_len}, {right_ptr}, {right_len})\n"
            ));
            let len_reg = self.next_reg();
            out.push_str(&format!(
                "  {len_reg} = call i64 @rune_rt_last_string_len()\n"
            ));
            self.value_map
                .insert(dst.to_string(), format!("ptr {ptr_reg}, i64 {len_reg}"));
            return Ok(());
        }
        if matches!(op, BinaryOp::And | BinaryOp::Or)
            && (left_ty == IrType::Dynamic || right_ty == IrType::Dynamic)
        {
            let left_val = if left_ty == IrType::Dynamic {
                self.emit_dynamic_truthy(out, left)?
            } else {
                self.resolve_value(left, &IrType::Bool, out)?
            };
            let right_val = if right_ty == IrType::Dynamic {
                self.emit_dynamic_truthy(out, right)?
            } else {
                self.resolve_value(right, &IrType::Bool, out)?
            };
            let reg = self.next_reg();
            let line = match op {
                BinaryOp::And => format!("  {reg} = and i1 {left_val}, {right_val}\n"),
                BinaryOp::Or => format!("  {reg} = or i1 {left_val}, {right_val}\n"),
                _ => unreachable!(),
            };
            out.push_str(&line);
            self.value_map.insert(dst.to_string(), reg);
            return Ok(());
        }
        let op_ty = match op {
            BinaryOp::EqualEqual
            | BinaryOp::NotEqual
            | BinaryOp::Greater
            | BinaryOp::GreaterEqual
            | BinaryOp::Less
            | BinaryOp::LessEqual
            | BinaryOp::And
            | BinaryOp::Or => left_ty,
            _ => ty.clone(),
        };
        if matches!(op_ty, IrType::Dynamic | IrType::String | IrType::Unit) {
            return Err(LlvmIrError {
                message: format!(
                    "operation `{:?}` in `{}` uses unsupported LLVM IR operand types",
                    op, self.function_name
                ),
            });
        }
        let left_val = self.resolve_value(left, &op_ty, out)?;
        let right_val = self.resolve_value(right, &op_ty, out)?;
        let reg = self.next_reg();
        let line = match op {
            BinaryOp::Add => format!(
                "  {reg} = add {} {left_val}, {right_val}\n",
                llvm_scalar_type(&op_ty)?
            ),
            BinaryOp::Subtract => format!(
                "  {reg} = sub {} {left_val}, {right_val}\n",
                llvm_scalar_type(&op_ty)?
            ),
            BinaryOp::Multiply => format!(
                "  {reg} = mul {} {left_val}, {right_val}\n",
                llvm_scalar_type(&op_ty)?
            ),
            BinaryOp::Divide => format!(
                "  {reg} = sdiv {} {left_val}, {right_val}\n",
                llvm_scalar_type(&op_ty)?
            ),
            BinaryOp::Modulo => format!(
                "  {reg} = srem {} {left_val}, {right_val}\n",
                llvm_scalar_type(&op_ty)?
            ),
            BinaryOp::EqualEqual => format!(
                "  {reg} = icmp eq {} {left_val}, {right_val}\n",
                llvm_scalar_type(&op_ty)?
            ),
            BinaryOp::NotEqual => format!(
                "  {reg} = icmp ne {} {left_val}, {right_val}\n",
                llvm_scalar_type(&op_ty)?
            ),
            BinaryOp::Greater => format!(
                "  {reg} = icmp sgt {} {left_val}, {right_val}\n",
                llvm_scalar_type(&op_ty)?
            ),
            BinaryOp::GreaterEqual => format!(
                "  {reg} = icmp sge {} {left_val}, {right_val}\n",
                llvm_scalar_type(&op_ty)?
            ),
            BinaryOp::Less => format!(
                "  {reg} = icmp slt {} {left_val}, {right_val}\n",
                llvm_scalar_type(&op_ty)?
            ),
            BinaryOp::LessEqual => format!(
                "  {reg} = icmp sle {} {left_val}, {right_val}\n",
                llvm_scalar_type(&op_ty)?
            ),
            BinaryOp::And => format!("  {reg} = and i1 {left_val}, {right_val}\n"),
            BinaryOp::Or => format!("  {reg} = or i1 {left_val}, {right_val}\n"),
        };
        out.push_str(&line);
        self.value_map.insert(dst.to_string(), reg);
        Ok(())
    }

    fn emit_call(
        &mut self,
        out: &mut String,
        dst: Option<&String>,
        callee: &str,
        args: &[IrArg],
    ) -> Result<(), LlvmIrError> {
        match callee {
            "str" => {
                let [arg] = args else {
                    return Err(LlvmIrError {
                        message: "`str` expects exactly 1 positional argument in the current LLVM IR backend".into(),
                    });
                };
                if arg.name.is_some() {
                    return Err(LlvmIrError {
                        message:
                            "`str` does not accept keyword arguments in the current LLVM IR backend"
                                .into(),
                    });
                }
                let src_ty = self
                    .temp_types
                    .get(&arg.value)
                    .or_else(|| self.local_types.get(&arg.value))
                    .cloned()
                    .ok_or_else(|| LlvmIrError {
                        message: format!("missing type for `str` argument `{}`", arg.value),
                    })?;
                let rendered = match src_ty {
                    IrType::String => self.resolve_value(&arg.value, &IrType::String, out)?,
                    IrType::Dynamic => {
                        let rendered = self.resolve_dynamic_value(&arg.value, out)?;
                        let (tag, payload, extra) = split_dynamic_value(&rendered)?;
                        self.declared_runtime
                            .insert("declare ptr @rune_rt_dynamic_to_string(i64, i64, i64)\n".into());
                        self.declared_runtime
                            .insert("declare i64 @rune_rt_last_string_len()\n".into());
                        let ptr_reg = self.next_reg();
                        out.push_str(&format!(
                            "  {ptr_reg} = call ptr @rune_rt_dynamic_to_string({tag}, {payload}, {extra})\n"
                        ));
                        let len_reg = self.next_reg();
                        out.push_str(&format!(
                            "  {len_reg} = call i64 @rune_rt_last_string_len()\n"
                        ));
                        format!("ptr {ptr_reg}, i64 {len_reg}")
                    }
                    IrType::I64 => {
                        let value = self.resolve_value(&arg.value, &IrType::I64, out)?;
                        self.declared_runtime
                            .insert("declare ptr @rune_rt_string_from_i64(i64)\n".into());
                        self.declared_runtime
                            .insert("declare i64 @rune_rt_last_string_len()\n".into());
                        let ptr_reg = self.next_reg();
                        out.push_str(&format!(
                            "  {ptr_reg} = call ptr @rune_rt_string_from_i64(i64 {value})\n"
                        ));
                        let len_reg = self.next_reg();
                        out.push_str(&format!(
                            "  {len_reg} = call i64 @rune_rt_last_string_len()\n"
                        ));
                        format!("ptr {ptr_reg}, i64 {len_reg}")
                    }
                    IrType::I32 => {
                        let value = self.resolve_value(&arg.value, &IrType::I32, out)?;
                        let widened = self.next_reg();
                        out.push_str(&format!("  {widened} = sext i32 {value} to i64\n"));
                        self.declared_runtime
                            .insert("declare ptr @rune_rt_string_from_i64(i64)\n".into());
                        self.declared_runtime
                            .insert("declare i64 @rune_rt_last_string_len()\n".into());
                        let ptr_reg = self.next_reg();
                        out.push_str(&format!(
                            "  {ptr_reg} = call ptr @rune_rt_string_from_i64(i64 {widened})\n"
                        ));
                        let len_reg = self.next_reg();
                        out.push_str(&format!(
                            "  {len_reg} = call i64 @rune_rt_last_string_len()\n"
                        ));
                        format!("ptr {ptr_reg}, i64 {len_reg}")
                    }
                    IrType::Bool => {
                        let value = self.resolve_value(&arg.value, &IrType::Bool, out)?;
                        self.declared_runtime
                            .insert("declare ptr @rune_rt_string_from_bool(i1)\n".into());
                        self.declared_runtime
                            .insert("declare i64 @rune_rt_last_string_len()\n".into());
                        let ptr_reg = self.next_reg();
                        out.push_str(&format!(
                            "  {ptr_reg} = call ptr @rune_rt_string_from_bool(i1 {value})\n"
                        ));
                        let len_reg = self.next_reg();
                        out.push_str(&format!(
                            "  {len_reg} = call i64 @rune_rt_last_string_len()\n"
                        ));
                        format!("ptr {ptr_reg}, i64 {len_reg}")
                    }
                    other => {
                        return Err(LlvmIrError {
                            message: format!(
                                "`str` currently supports only bool, i32, i64, dynamic, and String in the LLVM IR backend, found `{}`",
                                match other {
                                    IrType::Bool => "bool",
                                    IrType::Dynamic => "dynamic",
                                    IrType::I32 => "i32",
                                    IrType::I64 => "i64",
                                    IrType::String => "String",
                                    IrType::Struct(_) => "struct",
                                    IrType::Unit => "unit",
                                }
                            ),
                        });
                    }
                };
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), rendered);
                }
                return Ok(());
            }
            "int" => {
                let [arg] = args else {
                    return Err(LlvmIrError {
                        message: "`int` expects exactly 1 positional argument in the current LLVM IR backend".into(),
                    });
                };
                if arg.name.is_some() {
                    return Err(LlvmIrError {
                        message:
                            "`int` does not accept keyword arguments in the current LLVM IR backend"
                                .into(),
                    });
                }
                let src_ty = self
                    .temp_types
                    .get(&arg.value)
                    .or_else(|| self.local_types.get(&arg.value))
                    .cloned()
                    .ok_or_else(|| LlvmIrError {
                        message: format!("missing type for `int` argument `{}`", arg.value),
                    })?;
                let converted = match src_ty {
                    IrType::I64 => self.resolve_value(&arg.value, &IrType::I64, out)?,
                    IrType::I32 => {
                        let value = self.resolve_value(&arg.value, &IrType::I32, out)?;
                        let reg = self.next_reg();
                        out.push_str(&format!("  {reg} = sext i32 {value} to i64\n"));
                        reg
                    }
                    IrType::Bool => {
                        let value = self.resolve_value(&arg.value, &IrType::Bool, out)?;
                        let reg = self.next_reg();
                        out.push_str(&format!("  {reg} = zext i1 {value} to i64\n"));
                        reg
                    }
                    IrType::Dynamic => {
                        let rendered = self.resolve_dynamic_value(&arg.value, out)?;
                        let (tag, payload, extra) = split_dynamic_value(&rendered)?;
                        self.declared_runtime
                            .insert("declare i64 @rune_rt_dynamic_to_i64(i64, i64, i64)\n".into());
                        let reg = self.next_reg();
                        out.push_str(&format!(
                            "  {reg} = call i64 @rune_rt_dynamic_to_i64({tag}, {payload}, {extra})\n"
                        ));
                        reg
                    }
                    _ => {
                        return Err(LlvmIrError {
                            message: "`int` currently supports only bool, i32, i64, and dynamic in the LLVM IR backend".into(),
                        });
                    }
                };
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), converted);
                }
                return Ok(());
            }
            "print" | "println" | "eprint" | "eprintln" => {
                let stderr = callee.starts_with('e');
                let newline = callee.ends_with("ln");
                for arg in args {
                    self.emit_print_arg(out, &arg.value, stderr)?;
                }
                if newline {
                    let decl = if stderr {
                        "declare void @rune_rt_eprint_newline()\n"
                    } else {
                        "declare void @rune_rt_print_newline()\n"
                    };
                    self.declared_runtime.insert(decl.into());
                    out.push_str(if stderr {
                        "  call void @rune_rt_eprint_newline()\n"
                    } else {
                        "  call void @rune_rt_print_newline()\n"
                    });
                }
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "flush" | "eflush" => {
                let decl = if callee == "flush" {
                    "declare void @rune_rt_flush_stdout()\n"
                } else {
                    "declare void @rune_rt_flush_stderr()\n"
                };
                self.declared_runtime.insert(decl.into());
                out.push_str(if callee == "flush" {
                    "  call void @rune_rt_flush_stdout()\n"
                } else {
                    "  call void @rune_rt_flush_stderr()\n"
                });
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "input" => {
                if !args.is_empty() {
                    return Err(LlvmIrError {
                        message: "`input` does not take arguments in the current LLVM IR backend"
                            .into(),
                    });
                }
                let Some(dst) = dst else {
                    return Err(LlvmIrError {
                        message: "`input` result must be used in the current LLVM IR backend"
                            .into(),
                    });
                };
                self.declared_runtime
                    .insert("declare ptr @rune_rt_input_line()\n".into());
                self.declared_runtime
                    .insert("declare i64 @rune_rt_last_string_len()\n".into());
                let ptr_reg = self.next_reg();
                out.push_str(&format!("  {ptr_reg} = call ptr @rune_rt_input_line()\n"));
                let len_reg = self.next_reg();
                out.push_str(&format!(
                    "  {len_reg} = call i64 @rune_rt_last_string_len()\n"
                ));
                self.value_map
                    .insert(dst.clone(), format!("ptr {ptr_reg}, i64 {len_reg}"));
                return Ok(());
            }
            "panic" => {
                let [arg, context] = args else {
                    return Err(LlvmIrError {
                        message: "`panic` expects message and context arguments in the current LLVM IR backend".into(),
                    });
                };
                if arg.name.is_some() || context.name.is_some() {
                    return Err(LlvmIrError {
                        message: "`panic` does not accept keyword arguments in the current LLVM IR backend".into(),
                    });
                }
                let arg_ty = self
                    .temp_types
                    .get(&arg.value)
                    .or_else(|| self.local_types.get(&arg.value))
                    .cloned()
                    .ok_or_else(|| LlvmIrError {
                        message: format!("missing type for `panic` argument `{}`", arg.value),
                    })?;
                if arg_ty != IrType::String {
                    return Err(LlvmIrError {
                        message:
                            "`panic` currently requires a string argument in the LLVM IR backend"
                                .into(),
                    });
                }
                let context_ty = self
                    .temp_types
                    .get(&context.value)
                    .or_else(|| self.local_types.get(&context.value))
                    .cloned()
                    .ok_or_else(|| LlvmIrError {
                        message: format!("missing type for `panic` context `{}`", context.value),
                    })?;
                if context_ty != IrType::String {
                    return Err(LlvmIrError {
                        message: "`panic` context must be a string in the LLVM IR backend".into(),
                    });
                }
                let rendered = self.resolve_value(&arg.value, &arg_ty, out)?;
                let (msg_ptr, msg_len) = split_string_value(&rendered)?;
                let context_rendered = self.resolve_value(&context.value, &context_ty, out)?;
                let (ctx_ptr, ctx_len) = split_string_value(&context_rendered)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_panic(ptr, i64, ptr, i64)\n".into());
                out.push_str(&format!(
                    "  call void @rune_rt_panic({msg_ptr}, {msg_len}, {ctx_ptr}, {ctx_len})\n"
                ));
                out.push_str("  unreachable\n");
                self.block_terminated = true;
                return Ok(());
            }
            "__rune_builtin_time_now_unix" => {
                self.expect_plain_arity(callee, args, 0)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i64 @rune_rt_time_now_unix()\n".into());
                out.push_str(&format!("  {reg} = call i64 @rune_rt_time_now_unix()\n"));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_time_monotonic_ms" => {
                self.expect_plain_arity(callee, args, 0)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i64 @rune_rt_time_monotonic_ms()\n".into());
                out.push_str(&format!(
                    "  {reg} = call i64 @rune_rt_time_monotonic_ms()\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_time_sleep_ms" => {
                self.expect_plain_arity(callee, args, 1)?;
                let ms = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_time_sleep_ms(i64)\n".into());
                out.push_str(&format!("  call void @rune_rt_time_sleep_ms(i64 {ms})\n"));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_system_pid" => {
                self.expect_plain_arity(callee, args, 0)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i32 @rune_rt_system_pid()\n".into());
                out.push_str(&format!("  {reg} = call i32 @rune_rt_system_pid()\n"));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_system_cpu_count" => {
                self.expect_plain_arity(callee, args, 0)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i32 @rune_rt_system_cpu_count()\n".into());
                out.push_str(&format!("  {reg} = call i32 @rune_rt_system_cpu_count()\n"));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_system_exit" => {
                self.expect_plain_arity(callee, args, 1)?;
                let code = self.resolve_value(&args[0].value, &IrType::I32, out)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_system_exit(i32)\n".into());
                out.push_str(&format!("  call void @rune_rt_system_exit(i32 {code})\n"));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_env_exists" => {
                self.expect_plain_arity(callee, args, 1)?;
                let rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&rendered)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i1 @rune_rt_env_exists(ptr, i64)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i1 @rune_rt_env_exists({ptr}, {len})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_env_get_i32" => {
                self.expect_plain_arity(callee, args, 2)?;
                let rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&rendered)?;
                let default = self.resolve_value(&args[1].value, &IrType::I32, out)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i32 @rune_rt_env_get_i32(ptr, i64, i32)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i32 @rune_rt_env_get_i32({ptr}, {len}, i32 {default})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_env_get_bool" => {
                self.expect_plain_arity(callee, args, 2)?;
                let rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&rendered)?;
                let default = self.resolve_value(&args[1].value, &IrType::Bool, out)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i1 @rune_rt_env_get_bool(ptr, i64, i1)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i1 @rune_rt_env_get_bool({ptr}, {len}, i1 {default})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_env_arg_count" => {
                self.expect_plain_arity(callee, args, 0)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i32 @rune_rt_env_arg_count()\n".into());
                out.push_str(&format!("  {reg} = call i32 @rune_rt_env_arg_count()\n"));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_network_tcp_connect" => {
                self.expect_plain_arity(callee, args, 2)?;
                let rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&rendered)?;
                let port = self.resolve_value(&args[1].value, &IrType::I32, out)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i1 @rune_rt_network_tcp_connect(ptr, i64, i32)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i1 @rune_rt_network_tcp_connect({ptr}, {len}, i32 {port})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_network_tcp_connect_timeout" => {
                self.expect_plain_arity(callee, args, 3)?;
                let rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&rendered)?;
                let port = self.resolve_value(&args[1].value, &IrType::I32, out)?;
                let timeout = self.resolve_value(&args[2].value, &IrType::I32, out)?;
                let reg = self.next_reg();
                self.declared_runtime.insert(
                    "declare i1 @rune_rt_network_tcp_connect_timeout(ptr, i64, i32, i32)\n".into(),
                );
                out.push_str(&format!(
                    "  {reg} = call i1 @rune_rt_network_tcp_connect_timeout({ptr}, {len}, i32 {port}, i32 {timeout})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_fs_exists" => {
                self.expect_plain_arity(callee, args, 1)?;
                let rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&rendered)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i1 @rune_rt_fs_exists(ptr, i64)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i1 @rune_rt_fs_exists({ptr}, {len})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_fs_read_string" => {
                self.expect_plain_arity(callee, args, 1)?;
                let rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&rendered)?;
                let ptr_reg = self.next_reg();
                let len_reg = self.next_reg();
                self.declared_runtime
                    .insert("declare ptr @rune_rt_fs_read_string(ptr, i64)\n".into());
                self.declared_runtime
                    .insert("declare i64 @rune_rt_last_string_len()\n".into());
                out.push_str(&format!(
                    "  {ptr_reg} = call ptr @rune_rt_fs_read_string({ptr}, {len})\n"
                ));
                out.push_str(&format!(
                    "  {len_reg} = call i64 @rune_rt_last_string_len()\n"
                ));
                if let Some(dst) = dst {
                    self.value_map
                        .insert(dst.clone(), format!("ptr {ptr_reg}, i64 {len_reg}"));
                }
                return Ok(());
            }
            "__rune_builtin_fs_write_string" => {
                self.expect_plain_arity(callee, args, 2)?;
                let path_rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (path_ptr, path_len) = split_string_value(&path_rendered)?;
                let content_rendered = self.resolve_value(&args[1].value, &IrType::String, out)?;
                let (content_ptr, content_len) = split_string_value(&content_rendered)?;
                let reg = self.next_reg();
                self.declared_runtime.insert(
                    "declare i1 @rune_rt_fs_write_string(ptr, i64, ptr, i64)\n".into(),
                );
                out.push_str(&format!(
                    "  {reg} = call i1 @rune_rt_fs_write_string({path_ptr}, {path_len}, {content_ptr}, {content_len})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_terminal_clear" => {
                self.expect_plain_arity(callee, args, 0)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_terminal_clear()\n".into());
                out.push_str("  call void @rune_rt_terminal_clear()\n");
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_terminal_move_to" => {
                self.expect_plain_arity(callee, args, 2)?;
                let row = self.resolve_value(&args[0].value, &IrType::I32, out)?;
                let col = self.resolve_value(&args[1].value, &IrType::I32, out)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_terminal_move_to(i32, i32)\n".into());
                out.push_str(&format!(
                    "  call void @rune_rt_terminal_move_to(i32 {row}, i32 {col})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_terminal_hide_cursor" => {
                self.expect_plain_arity(callee, args, 0)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_terminal_hide_cursor()\n".into());
                out.push_str("  call void @rune_rt_terminal_hide_cursor()\n");
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_terminal_show_cursor" => {
                self.expect_plain_arity(callee, args, 0)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_terminal_show_cursor()\n".into());
                out.push_str("  call void @rune_rt_terminal_show_cursor()\n");
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_terminal_set_title" => {
                self.expect_plain_arity(callee, args, 1)?;
                let rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&rendered)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_terminal_set_title(ptr, i64)\n".into());
                out.push_str(&format!(
                    "  call void @rune_rt_terminal_set_title({ptr}, {len})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_audio_bell" => {
                self.expect_plain_arity(callee, args, 0)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i1 @rune_rt_audio_bell()\n".into());
                out.push_str(&format!("  {reg} = call i1 @rune_rt_audio_bell()\n"));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            _ => {}
        }

        let sig = self.signatures.get(callee).ok_or_else(|| LlvmIrError {
            message: format!(
                "calls to `{callee}` are not yet supported by the current LLVM IR backend"
            ),
        })?;
        if args.len() != sig.params.len() || args.iter().any(|arg| arg.name.is_some()) {
            return Err(LlvmIrError {
                message: format!(
                    "call shape for `{callee}` is not yet supported by the current LLVM IR backend"
                ),
            });
        }
        let mut rendered_args = Vec::new();
        for (arg, (_, ty)) in args.iter().zip(sig.params.iter()) {
            if matches!(ty, IrType::Unit) {
                return Err(LlvmIrError {
                    message: format!(
                        "call to `{callee}` uses parameter types not yet supported by the current LLVM IR backend"
                    ),
                });
            }
            if *ty == IrType::Dynamic && sig.is_extern {
                return Err(LlvmIrError {
                    message: format!(
                        "extern function `{callee}` uses parameter types not yet supported by the current LLVM IR backend"
                    ),
                });
            } else if *ty == IrType::String && sig.is_extern {
                let rendered = self.resolve_value(&arg.value, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&rendered)?;
                self.declared_runtime
                    .insert("declare ptr @rune_rt_to_c_string(ptr, i64)\n".into());
                let c_ptr = self.next_reg();
                out.push_str(&format!(
                    "  {c_ptr} = call ptr @rune_rt_to_c_string({ptr}, {len})\n"
                ));
                rendered_args.push(format!("ptr {c_ptr}"));
            } else if *ty == IrType::String {
                let rendered = self.resolve_value(&arg.value, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&rendered)?;
                rendered_args.push(ptr.to_string());
                rendered_args.push(len.to_string());
            } else if *ty == IrType::Dynamic {
                let rendered = self.resolve_dynamic_value(&arg.value, out)?;
                let (tag, payload, extra) = split_dynamic_value(&rendered)?;
                rendered_args.push(tag.to_string());
                rendered_args.push(payload.to_string());
                rendered_args.push(extra.to_string());
            } else {
                let value = self.resolve_value(&arg.value, ty, out)?;
                rendered_args.push(format!("{} {}", llvm_scalar_type(ty)?, value));
            }
        }

        if sig.ret == IrType::Unit {
            out.push_str(&format!(
                "  call void @{callee}({})\n",
                rendered_args.join(", ")
            ));
            if let Some(dst) = dst {
                self.value_map.insert(dst.clone(), "0".into());
            }
        } else if sig.ret == IrType::Dynamic && sig.is_extern {
            return Err(LlvmIrError {
                message: format!(
                    "extern function `{callee}` uses a return type not yet supported by the current LLVM IR backend"
                ),
            });
        } else if sig.ret == IrType::String && sig.is_extern {
            let reg = self.next_reg();
            self.declared_runtime
                .insert("declare ptr @rune_rt_from_c_string(ptr)\n".into());
            self.declared_runtime
                .insert("declare i64 @rune_rt_last_string_len()\n".into());
            out.push_str(&format!("  {reg} = call ptr @{callee}({})\n", rendered_args.join(", ")));
            let owned_ptr = self.next_reg();
            out.push_str(&format!(
                "  {owned_ptr} = call ptr @rune_rt_from_c_string(ptr {reg})\n"
            ));
            let len_reg = self.next_reg();
            out.push_str(&format!(
                "  {len_reg} = call i64 @rune_rt_last_string_len()\n"
            ));
            if let Some(dst) = dst {
                self.value_map
                    .insert(dst.clone(), format!("ptr {owned_ptr}, i64 {len_reg}"));
            }
        } else if matches!(sig.ret, IrType::String | IrType::Dynamic) {
            let aggregate_reg = self.next_reg();
            out.push_str(&format!(
                "  {aggregate_reg} = call {} @{callee}({})\n",
                llvm_internal_type(&sig.ret)?,
                rendered_args.join(", ")
            ));
            if sig.ret == IrType::String {
                let ptr_reg = self.next_reg();
                out.push_str(&format!(
                    "  {ptr_reg} = extractvalue {} {aggregate_reg}, 0\n",
                    llvm_internal_type(&sig.ret)?
                ));
                let len_reg = self.next_reg();
                out.push_str(&format!(
                    "  {len_reg} = extractvalue {} {aggregate_reg}, 1\n",
                    llvm_internal_type(&sig.ret)?
                ));
                if let Some(dst) = dst {
                    self.value_map
                        .insert(dst.clone(), format!("ptr {ptr_reg}, i64 {len_reg}"));
                }
            } else if let Some(dst) = dst {
                let tag_reg = self.next_reg();
                out.push_str(&format!(
                    "  {tag_reg} = extractvalue {} {aggregate_reg}, 0\n",
                    llvm_internal_type(&sig.ret)?
                ));
                let payload_reg = self.next_reg();
                out.push_str(&format!(
                    "  {payload_reg} = extractvalue {} {aggregate_reg}, 1\n",
                    llvm_internal_type(&sig.ret)?
                ));
                let extra_reg = self.next_reg();
                out.push_str(&format!(
                    "  {extra_reg} = extractvalue {} {aggregate_reg}, 2\n",
                    llvm_internal_type(&sig.ret)?
                ));
                self.value_map.insert(
                    dst.clone(),
                    format!("i64 {tag_reg}, i64 {payload_reg}, i64 {extra_reg}"),
                );
            }
        } else {
            let reg = self.next_reg();
            out.push_str(&format!(
                "  {reg} = call {} @{callee}({})\n",
                llvm_scalar_type(&sig.ret)?,
                rendered_args.join(", ")
            ));
            if let Some(dst) = dst {
                self.value_map.insert(dst.clone(), reg);
            }
        }
        Ok(())
    }

    fn emit_print_arg(
        &mut self,
        out: &mut String,
        value_name: &str,
        stderr: bool,
    ) -> Result<(), LlvmIrError> {
        let ty = self
            .temp_types
            .get(value_name)
            .or_else(|| self.local_types.get(value_name))
            .ok_or_else(|| LlvmIrError {
                message: format!("missing value type for `{value_name}`"),
            })?;
        match ty {
            IrType::Dynamic => {
                let rendered = self.resolve_value(value_name, ty, out)?;
                let (tag, payload, extra) = split_dynamic_value(&rendered)?;
                let decl = if stderr {
                    "declare void @rune_rt_eprint_dynamic(i64, i64, i64)\n"
                } else {
                    "declare void @rune_rt_print_dynamic(i64, i64, i64)\n"
                };
                self.declared_runtime.insert(decl.into());
                let call = if stderr {
                    format!("  call void @rune_rt_eprint_dynamic({tag}, {payload}, {extra})\n")
                } else {
                    format!("  call void @rune_rt_print_dynamic({tag}, {payload}, {extra})\n")
                };
                out.push_str(&call);
            }
            IrType::String => {
                let rendered = self.resolve_value(value_name, ty, out)?;
                let (ptr, len) = rendered.split_once(", ").ok_or_else(|| LlvmIrError {
                    message: "internal LLVM string rendering bug".into(),
                })?;
                let decl = if stderr {
                    "declare void @rune_rt_eprint_str(ptr, i64)\n"
                } else {
                    "declare void @rune_rt_print_str(ptr, i64)\n"
                };
                self.declared_runtime.insert(decl.into());
                let call = if stderr {
                    format!("  call void @rune_rt_eprint_str({ptr}, {len})\n")
                } else {
                    format!("  call void @rune_rt_print_str({ptr}, {len})\n")
                };
                out.push_str(&call);
            }
            IrType::Bool => {
                let rendered = self.resolve_value(value_name, ty, out)?;
                let zext = self.next_reg();
                out.push_str(&format!("  {zext} = zext i1 {rendered} to i64\n"));
                let decl = if stderr {
                    "declare void @rune_rt_eprint_i64(i64)\n"
                } else {
                    "declare void @rune_rt_print_i64(i64)\n"
                };
                self.declared_runtime.insert(decl.into());
                let call = if stderr {
                    format!("  call void @rune_rt_eprint_i64(i64 {zext})\n")
                } else {
                    format!("  call void @rune_rt_print_i64(i64 {zext})\n")
                };
                out.push_str(&call);
            }
            IrType::I32 => {
                let rendered = self.resolve_value(value_name, ty, out)?;
                let sext = self.next_reg();
                out.push_str(&format!("  {sext} = sext i32 {rendered} to i64\n"));
                let decl = if stderr {
                    "declare void @rune_rt_eprint_i64(i64)\n"
                } else {
                    "declare void @rune_rt_print_i64(i64)\n"
                };
                self.declared_runtime.insert(decl.into());
                let call = if stderr {
                    format!("  call void @rune_rt_eprint_i64(i64 {sext})\n")
                } else {
                    format!("  call void @rune_rt_print_i64(i64 {sext})\n")
                };
                out.push_str(&call);
            }
            IrType::I64 => {
                let rendered = self.resolve_value(value_name, ty, out)?;
                let decl = if stderr {
                    "declare void @rune_rt_eprint_i64(i64)\n"
                } else {
                    "declare void @rune_rt_print_i64(i64)\n"
                };
                self.declared_runtime.insert(decl.into());
                let call = if stderr {
                    format!("  call void @rune_rt_eprint_i64(i64 {rendered})\n")
                } else {
                    format!("  call void @rune_rt_print_i64(i64 {rendered})\n")
                };
                out.push_str(&call);
            }
            _ => {
                return Err(LlvmIrError {
                    message: format!(
                        "printing `{}` values is not yet supported by the current LLVM IR backend",
                        llvm_scalar_type(ty).unwrap_or("unsupported")
                    ),
                });
            }
        }
        Ok(())
    }

    fn emit_dynamic_truthy(&mut self, out: &mut String, name: &str) -> Result<String, LlvmIrError> {
        let rendered = self.resolve_dynamic_value(name, out)?;
        let (tag, payload, extra) = split_dynamic_value(&rendered)?;
        self.declared_runtime
            .insert("declare i1 @rune_rt_dynamic_truthy(i64, i64, i64)\n".into());
        let reg = self.next_reg();
        out.push_str(&format!(
            "  {reg} = call i1 @rune_rt_dynamic_truthy({tag}, {payload}, {extra})\n"
        ));
        Ok(reg)
    }

    fn emit_dynamic_binary(
        &mut self,
        out: &mut String,
        left: &str,
        right: &str,
        op: &BinaryOp,
    ) -> Result<String, LlvmIrError> {
        let left_rendered = self.resolve_dynamic_value(left, out)?;
        let right_rendered = self.resolve_dynamic_value(right, out)?;
        let (left_tag, left_payload, left_extra) = split_dynamic_value(&left_rendered)?;
        let (right_tag, right_payload, right_extra) = split_dynamic_value(&right_rendered)?;

        self.declared_runtime
            .insert("declare void @rune_rt_dynamic_binary(ptr, ptr, ptr, i64)\n".into());
        let left_alloca = self.next_reg();
        let right_alloca = self.next_reg();
        let out_alloca = self.next_reg();
        out.push_str(&format!("  {left_alloca} = alloca [3 x i64]\n"));
        out.push_str(&format!("  {right_alloca} = alloca [3 x i64]\n"));
        out.push_str(&format!("  {out_alloca} = alloca [3 x i64]\n"));
        self.store_dynamic_triplet(out, &left_alloca, left_tag, left_payload, left_extra);
        self.store_dynamic_triplet(out, &right_alloca, right_tag, right_payload, right_extra);
        let op_code = dynamic_binary_opcode(op)?;
        out.push_str(&format!(
            "  call void @rune_rt_dynamic_binary(ptr {left_alloca}, ptr {right_alloca}, ptr {out_alloca}, i64 {op_code})\n"
        ));
        self.load_dynamic_triplet(out, &out_alloca)
    }

    fn emit_dynamic_compare(
        &mut self,
        out: &mut String,
        left: &str,
        right: &str,
        op: &BinaryOp,
    ) -> Result<String, LlvmIrError> {
        let left_rendered = self.resolve_dynamic_value(left, out)?;
        let right_rendered = self.resolve_dynamic_value(right, out)?;
        let (left_tag, left_payload, left_extra) = split_dynamic_value(&left_rendered)?;
        let (right_tag, right_payload, right_extra) = split_dynamic_value(&right_rendered)?;

        self.declared_runtime
            .insert("declare i1 @rune_rt_dynamic_compare(ptr, ptr, i64)\n".into());
        let left_alloca = self.next_reg();
        let right_alloca = self.next_reg();
        out.push_str(&format!("  {left_alloca} = alloca [3 x i64]\n"));
        out.push_str(&format!("  {right_alloca} = alloca [3 x i64]\n"));
        self.store_dynamic_triplet(out, &left_alloca, left_tag, left_payload, left_extra);
        self.store_dynamic_triplet(out, &right_alloca, right_tag, right_payload, right_extra);
        let reg = self.next_reg();
        let op_code = dynamic_compare_opcode(op)?;
        out.push_str(&format!(
            "  {reg} = call i1 @rune_rt_dynamic_compare(ptr {left_alloca}, ptr {right_alloca}, i64 {op_code})\n"
        ));
        Ok(reg)
    }

    fn store_dynamic_triplet(
        &mut self,
        out: &mut String,
        base: &str,
        tag: &str,
        payload: &str,
        extra: &str,
    ) {
        let tag_ptr = self.next_reg();
        out.push_str(&format!(
            "  {tag_ptr} = getelementptr inbounds [3 x i64], ptr {base}, i64 0, i64 0\n"
        ));
        out.push_str(&format!("  store {tag}, ptr {tag_ptr}\n"));
        let payload_ptr = self.next_reg();
        out.push_str(&format!(
            "  {payload_ptr} = getelementptr inbounds [3 x i64], ptr {base}, i64 0, i64 1\n"
        ));
        out.push_str(&format!("  store {payload}, ptr {payload_ptr}\n"));
        let extra_ptr = self.next_reg();
        out.push_str(&format!(
            "  {extra_ptr} = getelementptr inbounds [3 x i64], ptr {base}, i64 0, i64 2\n"
        ));
        out.push_str(&format!("  store {extra}, ptr {extra_ptr}\n"));
    }

    fn load_dynamic_triplet(
        &mut self,
        out: &mut String,
        base: &str,
    ) -> Result<String, LlvmIrError> {
        let tag_ptr = self.next_reg();
        out.push_str(&format!(
            "  {tag_ptr} = getelementptr inbounds [3 x i64], ptr {base}, i64 0, i64 0\n"
        ));
        let tag_reg = self.next_reg();
        out.push_str(&format!("  {tag_reg} = load i64, ptr {tag_ptr}\n"));
        let payload_ptr = self.next_reg();
        out.push_str(&format!(
            "  {payload_ptr} = getelementptr inbounds [3 x i64], ptr {base}, i64 0, i64 1\n"
        ));
        let payload_reg = self.next_reg();
        out.push_str(&format!("  {payload_reg} = load i64, ptr {payload_ptr}\n"));
        let extra_ptr = self.next_reg();
        out.push_str(&format!(
            "  {extra_ptr} = getelementptr inbounds [3 x i64], ptr {base}, i64 0, i64 2\n"
        ));
        let extra_reg = self.next_reg();
        out.push_str(&format!("  {extra_reg} = load i64, ptr {extra_ptr}\n"));
        Ok(format!("i64 {tag_reg}, i64 {payload_reg}, i64 {extra_reg}"))
    }

    fn expect_plain_arity(
        &self,
        callee: &str,
        args: &[IrArg],
        expected: usize,
    ) -> Result<(), LlvmIrError> {
        if args.len() != expected || args.iter().any(|arg| arg.name.is_some()) {
            return Err(LlvmIrError {
                message: format!(
                    "`{callee}` expects {expected} positional argument(s) in the current LLVM IR backend"
                ),
            });
        }
        Ok(())
    }

    fn resolve_value(
        &mut self,
        name: &str,
        ty: &IrType,
        out: &mut String,
    ) -> Result<String, LlvmIrError> {
        if let Some(value) = self.value_map.get(name) {
            return Ok(value.clone());
        }
        if self.local_types.contains_key(name) {
            match ty {
                IrType::Dynamic => {
                    let tag_reg = self.next_reg();
                    out.push_str(&format!("  {tag_reg} = load i64, ptr %{}.tag\n", name));
                    let payload_reg = self.next_reg();
                    out.push_str(&format!(
                        "  {payload_reg} = load i64, ptr %{}.payload\n",
                        name
                    ));
                    let extra_reg = self.next_reg();
                    out.push_str(&format!("  {extra_reg} = load i64, ptr %{}.extra\n", name));
                    Ok(format!(
                        "i64 {tag_reg}, i64 {payload_reg}, i64 {extra_reg}"
                    ))
                }
                IrType::String => {
                    let ptr_reg = self.next_reg();
                    out.push_str(&format!("  {ptr_reg} = load ptr, ptr %{}.ptr\n", name));
                    let len_reg = self.next_reg();
                    out.push_str(&format!("  {len_reg} = load i64, ptr %{}.len\n", name));
                    Ok(format!("ptr {ptr_reg}, i64 {len_reg}"))
                }
                _ => {
                    let reg = self.next_reg();
                    out.push_str(&format!(
                        "  {reg} = load {}, ptr %{}.addr\n",
                        llvm_scalar_type(ty)?,
                        name
                    ));
                    Ok(reg)
                }
            }
        } else {
            Ok(name.to_string())
        }
    }

    fn resolve_dynamic_value(
        &mut self,
        name: &str,
        out: &mut String,
    ) -> Result<String, LlvmIrError> {
        let source_ty = self
            .temp_types
            .get(name)
            .or_else(|| self.local_types.get(name))
            .cloned()
            .unwrap_or(IrType::Dynamic);
        match source_ty {
            IrType::Dynamic => self.resolve_value(name, &IrType::Dynamic, out),
            IrType::Bool => {
                let value = self.resolve_value(name, &IrType::Bool, out)?;
                let payload = self.next_reg();
                out.push_str(&format!("  {payload} = zext i1 {value} to i64\n"));
                Ok(format!("i64 1, i64 {payload}, i64 0"))
            }
            IrType::I32 => {
                let value = self.resolve_value(name, &IrType::I32, out)?;
                let payload = self.next_reg();
                out.push_str(&format!("  {payload} = sext i32 {value} to i64\n"));
                Ok(format!("i64 2, i64 {payload}, i64 0"))
            }
            IrType::I64 => {
                let value = self.resolve_value(name, &IrType::I64, out)?;
                Ok(format!("i64 3, i64 {value}, i64 0"))
            }
            IrType::String => {
                let rendered = self.resolve_value(name, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&rendered)?;
                let ptr_value = ptr
                    .strip_prefix("ptr ")
                    .ok_or_else(|| LlvmIrError {
                        message: "internal LLVM string-to-dynamic rendering bug".into(),
                    })?;
                let ptr_i64 = self.next_reg();
                out.push_str(&format!("  {ptr_i64} = ptrtoint ptr {ptr_value} to i64\n"));
                Ok(format!("i64 4, i64 {ptr_i64}, {len}"))
            }
            IrType::Unit => Ok("i64 0, i64 0, i64 0".into()),
            IrType::Struct(_) => Err(LlvmIrError {
                message: format!(
                    "dynamic struct coercion is not yet supported by the current LLVM IR backend in `{}`",
                    self.function_name
                ),
            }),
        }
    }

    fn next_reg(&mut self) -> String {
        let reg = format!("%r{}", self.next_reg);
        self.next_reg += 1;
        reg
    }

    fn intern_string_ref(&mut self, value: &str) -> String {
        let name = if let Some(existing) = self.string_pool.get(value) {
            existing.clone()
        } else {
            let next = format!(".str.{}", self.string_pool.len());
            self.string_pool.insert(value.to_string(), next.clone());
            next
        };
        let len = llvm_string_len(value);
        format!(
            "ptr getelementptr inbounds ([{len} x i8], ptr @{name}, i64 0, i64 0), i64 {}",
            len - 1
        )
    }
}

fn infer_temp_types(
    function: &IrFunction,
    signatures: &HashMap<String, FunctionSig>,
) -> Result<HashMap<String, IrType>, LlvmIrError> {
    let local_types = function
        .locals
        .iter()
        .map(|local| (local.name.clone(), local.ty.clone()))
        .collect::<HashMap<_, _>>();
    let mut temp_types = HashMap::new();
    for inst in &function.instructions {
        match inst {
            IrInst::ConstInt { dst, value } => {
                let ty = if value.parse::<i32>().is_ok() {
                    IrType::I32
                } else {
                    IrType::I64
                };
                temp_types.insert(dst.clone(), ty);
            }
            IrInst::ConstBool { dst, .. } => {
                temp_types.insert(dst.clone(), IrType::Bool);
            }
            IrInst::ConstString { dst, .. } => {
                temp_types.insert(dst.clone(), IrType::String);
            }
            IrInst::Copy { dst, src } => {
                if !local_types.contains_key(dst) {
                    let ty = temp_types
                        .get(src)
                        .or_else(|| local_types.get(src))
                        .cloned()
                        .ok_or_else(|| LlvmIrError {
                            message: format!("missing type for copy source `{src}`"),
                        })?;
                    temp_types.insert(dst.clone(), ty);
                }
            }
            IrInst::UnaryNeg { dst, src } => {
                let ty = temp_types
                    .get(src)
                    .or_else(|| local_types.get(src))
                    .cloned()
                    .ok_or_else(|| LlvmIrError {
                        message: format!("missing type for unary operand `{src}`"),
                    })?;
                temp_types.insert(dst.clone(), ty);
            }
            IrInst::UnaryNot { dst, .. } => {
                temp_types.insert(dst.clone(), IrType::Bool);
            }
            IrInst::Binary { dst, op, left, .. } => {
                let ty = match op {
                    BinaryOp::EqualEqual
                    | BinaryOp::NotEqual
                    | BinaryOp::Greater
                    | BinaryOp::GreaterEqual
                    | BinaryOp::Less
                    | BinaryOp::LessEqual
                    | BinaryOp::And
                    | BinaryOp::Or => IrType::Bool,
                    _ => temp_types
                        .get(left)
                        .or_else(|| local_types.get(left))
                        .cloned()
                        .ok_or_else(|| LlvmIrError {
                            message: format!("missing type for binary operand `{left}`"),
                        })?,
                };
                temp_types.insert(dst.clone(), ty);
            }
            IrInst::Call { dst, callee, .. } => {
                if let Some(dst) = dst {
                    let ty = builtin_return_type(callee)
                        .or_else(|| signatures.get(callee).map(|sig| sig.ret.clone()))
                        .ok_or_else(|| LlvmIrError {
                            message: format!("missing return type for call `{callee}`"),
                        })?;
                    temp_types.insert(dst.clone(), ty);
                }
            }
            IrInst::Label(_) | IrInst::BranchIf { .. } | IrInst::Jump(_) | IrInst::Return(_) => {}
        }
    }
    Ok(temp_types)
}

fn builtin_return_type(name: &str) -> Option<IrType> {
    match name {
        "print" | "println" | "eprint" | "eprintln" | "flush" | "eflush" => Some(IrType::Unit),
        "input" => Some(IrType::String),
        "panic" => Some(IrType::Unit),
        "str" => Some(IrType::String),
        "int" => Some(IrType::I64),
        "__rune_builtin_time_now_unix" | "__rune_builtin_time_monotonic_ms" => Some(IrType::I64),
        "__rune_builtin_time_sleep_ms"
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
        "__rune_builtin_env_exists"
        | "__rune_builtin_env_get_bool"
        | "__rune_builtin_network_tcp_connect"
        | "__rune_builtin_network_tcp_connect_timeout"
        | "__rune_builtin_fs_exists"
        | "__rune_builtin_fs_write_string"
        | "__rune_builtin_audio_bell" => Some(IrType::Bool),
        "__rune_builtin_fs_read_string" => Some(IrType::String),
        _ => None,
    }
}

fn type_ref_to_ir(ty: &TypeRef) -> Result<IrType, LlvmIrError> {
    match ty.name.as_str() {
        "bool" => Ok(IrType::Bool),
        "i32" => Ok(IrType::I32),
        "i64" => Ok(IrType::I64),
        "unit" => Ok(IrType::Unit),
        "dynamic" => Ok(IrType::Dynamic),
        "String" | "str" => Ok(IrType::String),
        other => Err(LlvmIrError {
            message: format!("type `{other}` is not yet supported by the current LLVM IR backend"),
        }),
    }
}

fn llvm_scalar_type(ty: &IrType) -> Result<&'static str, LlvmIrError> {
    match ty {
        IrType::Bool => Ok("i1"),
        IrType::I32 => Ok("i32"),
        IrType::I64 => Ok("i64"),
        IrType::Unit => Ok("void"),
        IrType::String | IrType::Dynamic | IrType::Struct(_) => Err(LlvmIrError {
            message: "non-scalar type is not yet supported in this LLVM IR position".into(),
        }),
    }
}

fn llvm_extern_type(ty: &IrType) -> Result<&'static str, LlvmIrError> {
    match ty {
        IrType::String => Ok("ptr"),
        _ => llvm_scalar_type(ty),
    }
}

fn llvm_internal_type(ty: &IrType) -> Result<&'static str, LlvmIrError> {
    match ty {
        IrType::String => Ok("{ ptr, i64 }"),
        IrType::Dynamic => Ok("{ i64, i64, i64 }"),
        _ => llvm_scalar_type(ty),
    }
}

fn llvm_function_return_type(sig: &FunctionSig) -> Result<&'static str, LlvmIrError> {
    if matches!(sig.ret, IrType::String | IrType::Dynamic) && !sig.is_extern {
        llvm_internal_type(&sig.ret)
    } else {
        llvm_extern_type(&sig.ret)
    }
}

fn llvm_string_bytes(value: &str) -> String {
    let mut bytes = String::new();
    for byte in value.as_bytes() {
        match byte {
            b'\\' => bytes.push_str("\\5C"),
            b'"' => bytes.push_str("\\22"),
            0x20..=0x7e => bytes.push(*byte as char),
            other => bytes.push_str(&format!("\\{:02X}", other)),
        }
    }
    bytes.push_str("\\00");
    bytes
}

fn llvm_string_len(value: &str) -> usize {
    value.len() + 1
}

fn llvm_string_storage_len(value: &str) -> usize {
    value.as_bytes().len() + 1
}

fn split_string_value(value: &str) -> Result<(&str, &str), LlvmIrError> {
    let marker = ", i64 ";
    let index = value.rfind(marker).ok_or_else(|| LlvmIrError {
        message: "internal LLVM string rendering bug".into(),
    })?;
    let ptr = &value[..index];
    let len = &value[index + 2..];
    Ok((ptr, len))
}

fn split_dynamic_value(value: &str) -> Result<(&str, &str, &str), LlvmIrError> {
    let mut parts = value.split(", ");
    let tag = parts.next().ok_or_else(|| LlvmIrError {
        message: format!("internal LLVM dynamic rendering bug: `{value}`"),
    })?;
    let payload = parts.next().ok_or_else(|| LlvmIrError {
        message: format!("internal LLVM dynamic rendering bug: `{value}`"),
    })?;
    let extra = parts.next().ok_or_else(|| LlvmIrError {
        message: format!("internal LLVM dynamic rendering bug: `{value}`"),
    })?;
    if parts.next().is_some() {
        return Err(LlvmIrError {
            message: format!("internal LLVM dynamic rendering bug: `{value}`"),
        });
    }
    Ok((tag, payload, extra))
}

fn dynamic_binary_opcode(op: &BinaryOp) -> Result<i64, LlvmIrError> {
    match op {
        BinaryOp::Add => Ok(0),
        BinaryOp::Subtract => Ok(1),
        BinaryOp::Multiply => Ok(2),
        BinaryOp::Divide => Ok(3),
        BinaryOp::Modulo => Ok(4),
        _ => Err(LlvmIrError {
            message: format!(
                "dynamic operator `{:?}` is not yet supported by the current LLVM IR backend",
                op
            ),
        }),
    }
}

fn dynamic_compare_opcode(op: &BinaryOp) -> Result<i64, LlvmIrError> {
    match op {
        BinaryOp::EqualEqual => Ok(0),
        BinaryOp::NotEqual => Ok(1),
        BinaryOp::Greater => Ok(2),
        BinaryOp::GreaterEqual => Ok(3),
        BinaryOp::Less => Ok(4),
        BinaryOp::LessEqual => Ok(5),
        _ => Err(LlvmIrError {
            message: format!(
                "dynamic comparison `{:?}` is not yet supported by the current LLVM IR backend",
                op
            ),
        }),
    }
}
