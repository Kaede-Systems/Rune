use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use crate::ir::{IrArg, IrFunction, IrInst, IrProgram, IrType, lower_program};
use crate::ir::optimize_program;
use crate::frontend::parser::{BinaryOp, Item, Program, TypeRef, parse_source};

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
    struct_layouts: HashMap<String, Vec<(String, IrType)>>,
    string_pool: BTreeMap<String, String>,
    declared_runtime: BTreeSet<String>,
}

impl<'a> Emitter<'a> {
    fn new(program: &'a Program, ir: &'a IrProgram) -> Result<Self, LlvmIrError> {
        let mut signatures = HashMap::new();
        let struct_layouts = collect_struct_layouts(program)?;
        for item in &program.items {
            match item {
                Item::Function(function) => {
                    insert_function_signature(&mut signatures, &function.name, function, None)?;
                }
                Item::Struct(decl) => {
                    for method in &decl.methods {
                        let method_name = struct_method_symbol(&decl.name, &method.name);
                        insert_function_signature(
                            &mut signatures,
                            &method_name,
                            method,
                            Some(&decl.name),
                        )?;
                    }
                }
                Item::Import(_) | Item::Exception(_) => {}
            }
        }

        Ok(Self {
            ir,
            signatures,
            struct_layouts,
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
        let temp_types = infer_temp_types(function, &self.signatures, &self.struct_layouts)?;

        let mut out = String::new();
        out.push_str(&format!(
            "define {} @{}(",
            llvm_function_return_type(sig, &self.struct_layouts)?,
            llvm_internal_symbol_name(&function.name, sig)
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
                IrType::Struct(_) if !sig.is_extern => out.push_str(&format!(
                    "{} %{}",
                    llvm_internal_type(ty, &self.struct_layouts)?,
                    name
                )),
                _ => out.push_str(&format!(
                    "{} %{}",
                    llvm_extern_type(ty, &self.struct_layouts)?,
                    name
                )),
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
                IrType::Struct(_) if !sig.is_extern => {
                    let ty = llvm_internal_type(ty, &self.struct_layouts)?;
                    out.push_str(&format!("  %{name}.addr = alloca {ty}\n"));
                    out.push_str(&format!("  store {ty} %{name}, ptr %{name}.addr\n"));
                }
                _ => {
                    let scalar_ty = llvm_scalar_type(ty)?;
                    out.push_str(&format!(
                        "  %{name}.addr = alloca {scalar_ty}\n",
                    ));
                    out.push_str(&format!("  store {scalar_ty} %{name}, ptr %{name}.addr\n"));
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
                IrType::Struct(_) => {
                    let ty = llvm_internal_type(&local.ty, &self.struct_layouts)?;
                    out.push_str(&format!("  %{}.addr = alloca {ty}\n", local.name));
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
            struct_layouts: &self.struct_layouts,
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
        let mut out = format!(
            "declare {} @{}(",
            llvm_extern_type(&sig.ret, &self.struct_layouts)?,
            name
        );
        for (index, (_, ty)) in sig.params.iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            out.push_str(&llvm_extern_type(ty, &self.struct_layouts)?);
        }
        out.push(')');
        Ok(out)
    }
}

fn insert_function_signature(
    signatures: &mut HashMap<String, FunctionSig>,
    registered_name: &str,
    function: &crate::parser::Function,
    method_owner: Option<&str>,
) -> Result<(), LlvmIrError> {
    if function.is_async {
        return Err(LlvmIrError {
            message: "async functions are not supported by the current LLVM IR backend".into(),
        });
    }
    let mut params = Vec::with_capacity(function.params.len());
    for (index, param) in function.params.iter().enumerate() {
        let ty = if index == 0 && param.name == "self" {
            if let Some(owner) = method_owner {
                IrType::Struct(owner.to_string())
            } else {
                type_ref_to_ir(&param.ty)?
            }
        } else {
            type_ref_to_ir(&param.ty)?
        };
        params.push((param.name.clone(), ty));
    }
    let ret = match function.return_type.as_ref() {
        Some(ty) => type_ref_to_ir(ty)?,
        None => IrType::Unit,
    };
    signatures.insert(
        registered_name.to_string(),
        FunctionSig {
            is_extern: function.is_extern,
            params,
            ret,
        },
    );
    Ok(())
}

fn struct_method_symbol(struct_name: &str, method_name: &str) -> String {
    format!("{struct_name}__{method_name}")
}

struct FunctionEmitter<'a> {
    function_name: &'a str,
    signatures: &'a HashMap<String, FunctionSig>,
    struct_layouts: &'a HashMap<String, Vec<(String, IrType)>>,
    local_types: &'a HashMap<String, IrType>,
    temp_types: &'a HashMap<String, IrType>,
    string_pool: &'a mut BTreeMap<String, String>,
    declared_runtime: &'a mut BTreeSet<String>,
    next_reg: usize,
    value_map: HashMap<String, String>,
    block_terminated: bool,
}

impl<'a> FunctionEmitter<'a> {
    fn next_label(&mut self, prefix: &str) -> String {
        let label = format!("{}.{}.{}", self.function_name, prefix, self.next_reg);
        self.next_reg += 1;
        label
    }

    fn emit_runtime_error_code(
        &mut self,
        out: &mut String,
        divisor: &str,
        divisor_ty: &IrType,
        error_code: i32,
    ) -> Result<(), LlvmIrError> {
        let cmp = self.next_reg();
        let trap_label = self.next_label("divzero");
        let ok_label = self.next_label("divok");
        out.push_str(&format!(
            "  {cmp} = icmp eq {} {divisor}, 0\n",
            llvm_scalar_type(divisor_ty)?
        ));
        out.push_str(&format!(
            "  br i1 {cmp}, label %{trap_label}, label %{ok_label}\n"
        ));
        out.push_str(&format!("{trap_label}:\n"));
        self.declared_runtime
            .insert("declare void @rune_rt_fail(i32)\n".into());
        out.push_str(&format!("  call void @rune_rt_fail(i32 {error_code})\n"));
        out.push_str("  unreachable\n");
        out.push_str(&format!("{ok_label}:\n"));
        Ok(())
    }

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
                        IrType::Struct(_) => {
                            out.push_str(&format!(
                                "  store {} {value}, ptr %{}.addr\n",
                                llvm_internal_type(local_ty, self.struct_layouts)?,
                                dst
                            ));
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
            IrInst::UnaryBitwiseNot { dst, src } => {
                let ty = self.temp_types.get(dst).ok_or_else(|| LlvmIrError {
                    message: format!("missing temp type for `{dst}`"),
                })?;
                let src_val = self.resolve_value(src, ty, out)?;
                let reg = self.next_reg();
                out.push_str(&format!(
                    "  {reg} = xor {} {src_val}, -1\n",
                    llvm_scalar_type(ty)?
                ));
                self.value_map.insert(dst.clone(), reg);
            }
            IrInst::SetField { base, field, src } => {
                let base_ty = self.local_types.get(base).cloned().ok_or_else(|| LlvmIrError {
                    message: format!("missing local type for `{base}` in SetField"),
                })?;
                let IrType::Struct(struct_name) = &base_ty else {
                    return Err(LlvmIrError {
                        message: format!("`{base}` is not a struct in SetField"),
                    });
                };
                let struct_name = struct_name.clone();
                let layout = self
                    .struct_layouts
                    .get(&struct_name)
                    .cloned()
                    .ok_or_else(|| LlvmIrError {
                        message: format!("missing struct layout for `{struct_name}` in SetField"),
                    })?;
                let (field_index, field_ty) = layout
                    .iter()
                    .enumerate()
                    .find(|(_, (name, _))| name == field)
                    .map(|(i, (_, ty))| (i, ty.clone()))
                    .ok_or_else(|| LlvmIrError {
                        message: format!("field `{field}` not found in struct `{struct_name}`"),
                    })?;
                let struct_llvm_ty =
                    llvm_internal_type(&base_ty, self.struct_layouts)?;
                // Load current struct value
                let cur_reg = self.next_reg();
                out.push_str(&format!(
                    "  {cur_reg} = load {struct_llvm_ty}, ptr %{base}.addr\n"
                ));
                // Resolve the new field value
                let src_val = self.resolve_value(src, &field_ty, out)?;
                // Insert the new field value
                let new_reg = self.next_reg();
                let field_llvm_ty = llvm_internal_type(&field_ty, self.struct_layouts)?;
                out.push_str(&format!(
                    "  {new_reg} = insertvalue {struct_llvm_ty} {cur_reg}, {field_llvm_ty} {src_val}, {field_index}\n"
                ));
                // Store updated struct back
                out.push_str(&format!(
                    "  store {struct_llvm_ty} {new_reg}, ptr %{base}.addr\n"
                ));
                // Update value_map for base
                self.value_map.insert(base.clone(), new_reg);
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
                            llvm_internal_type(ret_ty, self.struct_layouts)?
                        ));
                        let agg1 = self.next_reg();
                        out.push_str(&format!(
                            "  {agg1} = insertvalue {} {agg0}, {len}, 1\n",
                            llvm_internal_type(ret_ty, self.struct_layouts)?
                        ));
                        out.push_str(&format!(
                            "  ret {} {agg1}\n",
                            llvm_internal_type(ret_ty, self.struct_layouts)?
                        ));
                    } else if *ret_ty == IrType::Dynamic {
                        let (tag, payload, extra) = split_dynamic_value(&ret_val)?;
                        let agg0 = self.next_reg();
                        out.push_str(&format!(
                            "  {agg0} = insertvalue {} poison, {tag}, 0\n",
                            llvm_internal_type(ret_ty, self.struct_layouts)?
                        ));
                        let agg1 = self.next_reg();
                        out.push_str(&format!(
                            "  {agg1} = insertvalue {} {agg0}, {payload}, 1\n",
                            llvm_internal_type(ret_ty, self.struct_layouts)?
                        ));
                        let agg2 = self.next_reg();
                        out.push_str(&format!(
                            "  {agg2} = insertvalue {} {agg1}, {extra}, 2\n",
                            llvm_internal_type(ret_ty, self.struct_layouts)?
                        ));
                        out.push_str(&format!(
                            "  ret {} {agg2}\n",
                            llvm_internal_type(ret_ty, self.struct_layouts)?
                        ));
                    } else if matches!(ret_ty, IrType::Struct(_)) {
                        out.push_str(&format!(
                            "  ret {} {}\n",
                            llvm_internal_type(ret_ty, self.struct_layouts)?,
                            ret_val
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
        if self.try_emit_struct_equality(out, dst, op, left, right, &left_ty, &right_ty)? {
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
        if matches!(
            op,
            BinaryOp::EqualEqual
                | BinaryOp::NotEqual
                | BinaryOp::Greater
                | BinaryOp::GreaterEqual
                | BinaryOp::Less
                | BinaryOp::LessEqual
        ) && left_ty == IrType::String
            && right_ty == IrType::String
        {
            let left_val = self.resolve_value(left, &IrType::String, out)?;
            let right_val = self.resolve_value(right, &IrType::String, out)?;
            let (left_ptr, left_len) = split_string_value(&left_val)?;
            let (right_ptr, right_len) = split_string_value(&right_val)?;
            if matches!(op, BinaryOp::EqualEqual | BinaryOp::NotEqual) {
                self.declared_runtime
                    .insert("declare i1 @rune_rt_string_equal(ptr, i64, ptr, i64)\n".into());
                let eq_reg = self.next_reg();
                out.push_str(&format!(
                    "  {eq_reg} = call i1 @rune_rt_string_equal({left_ptr}, {left_len}, {right_ptr}, {right_len})\n"
                ));
                if matches!(op, BinaryOp::EqualEqual) {
                    self.value_map.insert(dst.to_string(), eq_reg);
                } else {
                    let reg = self.next_reg();
                    out.push_str(&format!("  {reg} = xor i1 {eq_reg}, true\n"));
                    self.value_map.insert(dst.to_string(), reg);
                }
                return Ok(());
            }
            self.declared_runtime.insert(
                "declare i32 @rune_rt_string_compare(ptr, i64, ptr, i64)\n".into(),
            );
            let cmp_reg = self.next_reg();
            out.push_str(&format!(
                "  {cmp_reg} = call i32 @rune_rt_string_compare({left_ptr}, {left_len}, {right_ptr}, {right_len})\n"
            ));
            let reg = self.next_reg();
            let predicate = match op {
                BinaryOp::Greater => "sgt",
                BinaryOp::GreaterEqual => "sge",
                BinaryOp::Less => "slt",
                BinaryOp::LessEqual => "sle",
                _ => unreachable!(),
            };
            out.push_str(&format!("  {reg} = icmp {predicate} i32 {cmp_reg}, 0\n"));
            self.value_map.insert(dst.to_string(), reg);
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
        if matches!(op, BinaryOp::EqualEqual | BinaryOp::NotEqual)
            && left_ty == IrType::Json
            && right_ty == IrType::Json
        {
            let left_val = self.resolve_value(left, &IrType::Json, out)?;
            let right_val = self.resolve_value(right, &IrType::Json, out)?;
            self.declared_runtime
                .insert("declare i1 @rune_rt_json_equal(i64, i64)\n".into());
            let eq_reg = self.next_reg();
            out.push_str(&format!(
                "  {eq_reg} = call i1 @rune_rt_json_equal(i64 {left_val}, i64 {right_val})\n"
            ));
            if matches!(op, BinaryOp::EqualEqual) {
                self.value_map.insert(dst.to_string(), eq_reg);
            } else {
                let reg = self.next_reg();
                out.push_str(&format!("  {reg} = xor i1 {eq_reg}, true\n"));
                self.value_map.insert(dst.to_string(), reg);
            }
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
            BinaryOp::Divide => {
                self.emit_runtime_error_code(out, &right_val, &op_ty, 1001)?;
                format!(
                    "  {reg} = sdiv {} {left_val}, {right_val}\n",
                    llvm_scalar_type(&op_ty)?
                )
            }
            BinaryOp::Modulo => {
                self.emit_runtime_error_code(out, &right_val, &op_ty, 1002)?;
                format!(
                    "  {reg} = srem {} {left_val}, {right_val}\n",
                    llvm_scalar_type(&op_ty)?
                )
            }
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
            BinaryOp::BitwiseAnd => format!(
                "  {reg} = and {} {left_val}, {right_val}\n",
                llvm_scalar_type(&op_ty)?
            ),
            BinaryOp::BitwiseOr => format!(
                "  {reg} = or {} {left_val}, {right_val}\n",
                llvm_scalar_type(&op_ty)?
            ),
            BinaryOp::BitwiseXor => format!(
                "  {reg} = xor {} {left_val}, {right_val}\n",
                llvm_scalar_type(&op_ty)?
            ),
            BinaryOp::ShiftLeft => format!(
                "  {reg} = shl {} {left_val}, {right_val}\n",
                llvm_scalar_type(&op_ty)?
            ),
            BinaryOp::ShiftRight => format!(
                "  {reg} = ashr {} {left_val}, {right_val}\n",
                llvm_scalar_type(&op_ty)?
            ),
        };
        out.push_str(&line);
        self.value_map.insert(dst.to_string(), reg);
        Ok(())
    }

    fn try_emit_struct_equality(
        &mut self,
        out: &mut String,
        dst: &str,
        op: &BinaryOp,
        left: &str,
        right: &str,
        left_ty: &IrType,
        right_ty: &IrType,
    ) -> Result<bool, LlvmIrError> {
        if !matches!(op, BinaryOp::EqualEqual | BinaryOp::NotEqual) {
            return Ok(false);
        }
        let (IrType::Struct(struct_name), IrType::Struct(other_name)) = (left_ty, right_ty) else {
            return Ok(false);
        };
        if struct_name != other_name {
            return Ok(false);
        }
        let reg = if let Some(sig) = self.signatures.get(&struct_method_symbol(struct_name, "__eq__")) {
            if sig.params.len() != 2
                || sig.params[0].1 != IrType::Struct(struct_name.clone())
                || sig.params[1].1 != IrType::Struct(struct_name.clone())
                || sig.ret != IrType::Bool
            {
                return Err(LlvmIrError {
                    message: format!(
                        "`__eq__` on `{struct_name}` must have signature `__eq__(self, other: {struct_name}) -> bool` in the current LLVM IR backend"
                    ),
                });
            }
            let left_val = self.resolve_value(left, left_ty, out)?;
            let right_val = self.resolve_value(right, right_ty, out)?;
            let synthetic_name = struct_method_symbol(struct_name, "__eq__");
            let reg = self.next_reg();
            out.push_str(&format!(
                "  {reg} = call i1 @{}({} {}, {} {})\n",
                llvm_internal_symbol_name(&synthetic_name, sig),
                llvm_internal_type(left_ty, self.struct_layouts)?,
                left_val,
                llvm_internal_type(right_ty, self.struct_layouts)?,
                right_val
            ));
            reg
        } else {
            let left_val = self.resolve_value(left, left_ty, out)?;
            let right_val = self.resolve_value(right, right_ty, out)?;
            self.emit_default_struct_eq_value(out, struct_name, &left_val, &right_val)?
        };
        if matches!(op, BinaryOp::EqualEqual) {
            self.value_map.insert(dst.to_string(), reg);
        } else {
            let neg = self.next_reg();
            out.push_str(&format!("  {neg} = xor i1 {reg}, true\n"));
            self.value_map.insert(dst.to_string(), neg);
        }
        Ok(true)
    }

    fn emit_default_struct_eq_value(
        &mut self,
        out: &mut String,
        struct_name: &str,
        left_val: &str,
        right_val: &str,
    ) -> Result<String, LlvmIrError> {
        let layout = self.struct_layouts.get(struct_name).cloned().ok_or_else(|| LlvmIrError {
            message: format!("missing struct layout for `{struct_name}` in the current LLVM IR backend"),
        })?;
        let struct_ty = llvm_internal_type(&IrType::Struct(struct_name.to_string()), self.struct_layouts)?;
        let mut result = None;
        for (index, (_, field_ty)) in layout.iter().enumerate() {
            let left_field = self.next_reg();
            let right_field = self.next_reg();
            out.push_str(&format!(
                "  {left_field} = extractvalue {struct_ty} {left_val}, {index}\n"
            ));
            out.push_str(&format!(
                "  {right_field} = extractvalue {struct_ty} {right_val}, {index}\n"
            ));
            let field_eq = match field_ty {
                IrType::String => {
                    let left_ptr = self.next_reg();
                    let left_len = self.next_reg();
                    let right_ptr = self.next_reg();
                    let right_len = self.next_reg();
                    let eq_reg = self.next_reg();
                    self.declared_runtime
                        .insert("declare i1 @rune_rt_string_equal(ptr, i64, ptr, i64)\n".into());
                    let string_ty = llvm_internal_type(field_ty, self.struct_layouts)?;
                    out.push_str(&format!(
                        "  {left_ptr} = extractvalue {string_ty} {left_field}, 0\n"
                    ));
                    out.push_str(&format!(
                        "  {left_len} = extractvalue {string_ty} {left_field}, 1\n"
                    ));
                    out.push_str(&format!(
                        "  {right_ptr} = extractvalue {string_ty} {right_field}, 0\n"
                    ));
                    out.push_str(&format!(
                        "  {right_len} = extractvalue {string_ty} {right_field}, 1\n"
                    ));
                    out.push_str(&format!(
                        "  {eq_reg} = call i1 @rune_rt_string_equal({left_ptr}, {left_len}, {right_ptr}, {right_len})\n"
                    ));
                    eq_reg
                }
                IrType::Struct(nested_name) => {
                    self.emit_default_struct_eq_value(out, nested_name, &left_field, &right_field)?
                }
                _ => {
                    let eq_reg = self.next_reg();
                    out.push_str(&format!(
                        "  {eq_reg} = icmp eq {} {}, {}\n",
                        llvm_scalar_type(field_ty)?,
                        left_field,
                        right_field
                    ));
                    eq_reg
                }
            };
            result = Some(if let Some(current) = result {
                let next = self.next_reg();
                out.push_str(&format!("  {next} = and i1 {current}, {field_eq}\n"));
                next
            } else {
                field_eq
            });
        }
        Ok(result.unwrap_or_else(|| "true".to_string()))
    }

    fn emit_call(
        &mut self,
        out: &mut String,
        dst: Option<&String>,
        callee: &str,
        args: &[IrArg],
    ) -> Result<(), LlvmIrError> {
        if let Some(field_name) = callee.strip_prefix("field.") {
            let [base] = args else {
                return Err(LlvmIrError {
                    message: format!("`{callee}` expects exactly 1 argument in the current LLVM IR backend"),
                });
            };
            if base.name.as_deref() != Some("base") {
                return Err(LlvmIrError {
                    message: format!("`{callee}` requires a named `base` receiver in the current LLVM IR backend"),
                });
            }
            let base_ty = self
                .temp_types
                .get(&base.value)
                .or_else(|| self.local_types.get(&base.value))
                .cloned()
                .ok_or_else(|| LlvmIrError {
                    message: format!("missing receiver type for `{callee}`"),
                })?;
            let IrType::Struct(struct_name) = base_ty else {
                return Err(LlvmIrError {
                    message: format!("`{callee}` requires a concrete struct receiver in the current LLVM IR backend"),
                });
            };
            let layout = self.struct_layouts.get(&struct_name).ok_or_else(|| LlvmIrError {
                message: format!("missing struct layout for `{struct_name}`"),
            })?;
            let (field_index, (_, field_ty)) = layout
                .iter()
                .enumerate()
                .find(|(_, (name, _))| name == field_name)
                .ok_or_else(|| LlvmIrError {
                    message: format!("`{struct_name}` has no field `{field_name}`"),
                })?;
            let base_value =
                self.resolve_value(&base.value, &IrType::Struct(struct_name.clone()), out)?;
            let reg = self.next_reg();
            out.push_str(&format!(
                "  {reg} = extractvalue {} {base_value}, {field_index}\n",
                llvm_internal_type(&IrType::Struct(struct_name), self.struct_layouts)?
            ));
            if let Some(dst) = dst {
                match field_ty {
                    IrType::String => {
                        let ptr_reg = self.next_reg();
                        out.push_str(&format!(
                            "  {ptr_reg} = extractvalue {} {reg}, 0\n",
                            llvm_internal_type(field_ty, self.struct_layouts)?
                        ));
                        let len_reg = self.next_reg();
                        out.push_str(&format!(
                            "  {len_reg} = extractvalue {} {reg}, 1\n",
                            llvm_internal_type(field_ty, self.struct_layouts)?
                        ));
                        self.value_map
                            .insert(dst.clone(), format!("ptr {ptr_reg}, i64 {len_reg}"));
                    }
                    IrType::Dynamic => {
                        let tag_reg = self.next_reg();
                        out.push_str(&format!(
                            "  {tag_reg} = extractvalue {} {reg}, 0\n",
                            llvm_internal_type(field_ty, self.struct_layouts)?
                        ));
                        let payload_reg = self.next_reg();
                        out.push_str(&format!(
                            "  {payload_reg} = extractvalue {} {reg}, 1\n",
                            llvm_internal_type(field_ty, self.struct_layouts)?
                        ));
                        let extra_reg = self.next_reg();
                        out.push_str(&format!(
                            "  {extra_reg} = extractvalue {} {reg}, 2\n",
                            llvm_internal_type(field_ty, self.struct_layouts)?
                        ));
                        self.value_map.insert(
                            dst.clone(),
                            format!("i64 {tag_reg}, i64 {payload_reg}, i64 {extra_reg}"),
                        );
                    }
                    _ => {
                        self.value_map.insert(dst.clone(), reg);
                    }
                }
            }
            return Ok(());
        }

        if self.struct_layouts.contains_key(callee) {
            let layout = self.struct_layouts.get(callee).expect("checked above");
            if args.len() != layout.len() || args.iter().any(|arg| arg.name.is_none()) {
                return Err(LlvmIrError {
                    message: format!("constructor call shape for `{callee}` is not yet supported by the current LLVM IR backend"),
                });
            }
            let aggregate_ty =
                llvm_internal_type(&IrType::Struct(callee.to_string()), self.struct_layouts)?;
            let mut aggregate = "poison".to_string();
            for (index, (field_name, field_ty)) in layout.iter().enumerate() {
                let arg = args
                    .iter()
                    .find(|arg| arg.name.as_deref() == Some(field_name))
                    .ok_or_else(|| LlvmIrError {
                        message: format!("missing constructor field `{field_name}` for `{callee}`"),
                    })?;
                let value = self.resolve_value(&arg.value, field_ty, out)?;
                let field_value = match field_ty {
                    IrType::String => {
                        let (ptr, len) = split_string_value(&value)?;
                        let field_agg0 = self.next_reg();
                        out.push_str(&format!(
                            "  {field_agg0} = insertvalue {} poison, {ptr}, 0\n",
                            llvm_internal_type(field_ty, self.struct_layouts)?
                        ));
                        let field_agg1 = self.next_reg();
                        out.push_str(&format!(
                            "  {field_agg1} = insertvalue {} {field_agg0}, {len}, 1\n",
                            llvm_internal_type(field_ty, self.struct_layouts)?
                        ));
                        field_agg1
                    }
                    IrType::Dynamic => {
                        let (tag, payload, extra) = split_dynamic_value(&value)?;
                        let field_agg0 = self.next_reg();
                        out.push_str(&format!(
                            "  {field_agg0} = insertvalue {} poison, {tag}, 0\n",
                            llvm_internal_type(field_ty, self.struct_layouts)?
                        ));
                        let field_agg1 = self.next_reg();
                        out.push_str(&format!(
                            "  {field_agg1} = insertvalue {} {field_agg0}, {payload}, 1\n",
                            llvm_internal_type(field_ty, self.struct_layouts)?
                        ));
                        let field_agg2 = self.next_reg();
                        out.push_str(&format!(
                            "  {field_agg2} = insertvalue {} {field_agg1}, {extra}, 2\n",
                            llvm_internal_type(field_ty, self.struct_layouts)?
                        ));
                        field_agg2
                    }
                    _ => value,
                };
                let reg = self.next_reg();
                out.push_str(&format!(
                    "  {reg} = insertvalue {aggregate_ty} {aggregate}, {} {field_value}, {index}\n",
                    llvm_internal_type(field_ty, self.struct_layouts)?
                ));
                aggregate = reg;
            }
            if let Some(dst) = dst {
                self.value_map.insert(dst.clone(), aggregate);
            }
            return Ok(());
        }

        match callee {
            "str" | "repr" => {
                let display_name = callee;
                let magic_name = if callee == "repr" { "__repr__" } else { "__str__" };
                let [arg] = args else {
                    return Err(LlvmIrError {
                        message: format!(
                            "`{display_name}` expects exactly 1 positional argument in the current LLVM IR backend"
                        ),
                    });
                };
                if arg.name.is_some() {
                    return Err(LlvmIrError {
                        message: format!(
                            "`{display_name}` does not accept keyword arguments in the current LLVM IR backend"
                        ),
                    });
                }
                let src_ty = self
                    .temp_types
                    .get(&arg.value)
                    .or_else(|| self.local_types.get(&arg.value))
                    .cloned()
                    .ok_or_else(|| LlvmIrError {
                        message: format!("missing type for `{display_name}` argument `{}`", arg.value),
                    })?;
                let rendered = match src_ty {
                    IrType::String => self.resolve_value(&arg.value, &IrType::String, out)?,
                    IrType::Json => {
                        let value = self.resolve_value(&arg.value, &IrType::Json, out)?;
                        self.declared_runtime
                            .insert("declare ptr @rune_rt_json_to_string(i64)\n".into());
                        self.declared_runtime
                            .insert("declare i64 @rune_rt_last_string_len()\n".into());
                        let ptr_reg = self.next_reg();
                        out.push_str(&format!(
                            "  {ptr_reg} = call ptr @rune_rt_json_to_string(i64 {value})\n"
                        ));
                        let len_reg = self.next_reg();
                        out.push_str(&format!(
                            "  {len_reg} = call i64 @rune_rt_last_string_len()\n"
                        ));
                        format!("ptr {ptr_reg}, i64 {len_reg}")
                    }
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
                    IrType::Struct(struct_name) => {
                        let synthetic_name = struct_method_symbol(&struct_name, magic_name);
                        if let Some(sig) = self.signatures.get(&synthetic_name) {
                            if sig.params.len() != 1 || sig.ret != IrType::String {
                                return Err(LlvmIrError {
                                    message: format!(
                                        "`{display_name}` on `{struct_name}` requires `{magic_name}`, when defined, to have signature `{magic_name}(self) -> String` in the current LLVM IR backend"
                                    ),
                                });
                            }
                            let value = self.resolve_value(
                                &arg.value,
                                &IrType::Struct(struct_name.clone()),
                                out,
                            )?;
                            let aggregate_reg = self.next_reg();
                            out.push_str(&format!(
                                "  {aggregate_reg} = call {} @{}({} {})\n",
                                llvm_internal_type(&IrType::String, self.struct_layouts)?,
                                llvm_internal_symbol_name(&synthetic_name, sig),
                                llvm_internal_type(&IrType::Struct(struct_name), self.struct_layouts)?,
                                value
                            ));
                            let ptr_reg = self.next_reg();
                            out.push_str(&format!(
                                "  {ptr_reg} = extractvalue {} {aggregate_reg}, 0\n",
                                llvm_internal_type(&IrType::String, self.struct_layouts)?
                            ));
                            let len_reg = self.next_reg();
                            out.push_str(&format!(
                                "  {len_reg} = extractvalue {} {aggregate_reg}, 1\n",
                                llvm_internal_type(&IrType::String, self.struct_layouts)?
                            ));
                            format!("ptr {ptr_reg}, i64 {len_reg}")
                        } else {
                            let value =
                                self.resolve_value(&arg.value, &IrType::Struct(struct_name.clone()), out)?;
                            self.render_default_struct_string_value(out, &struct_name, &value)?
                        }
                    }
                    other => {
                        return Err(LlvmIrError {
                            message: format!(
                                "`{display_name}` currently supports only bool, i32, i64, Json, dynamic, String, and class/struct values in the LLVM IR backend, found `{}`",
                                match other {
                                    IrType::Bool => "bool",
                                    IrType::Dynamic => "dynamic",
                                    IrType::I32 => "i32",
                                    IrType::I64 => "i64",
                                    IrType::Json => "Json",
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
                    IrType::Json => {
                        let value = self.resolve_value(&arg.value, &IrType::Json, out)?;
                        self.declared_runtime
                            .insert("declare i64 @rune_rt_json_to_i64(i64)\n".into());
                        let reg = self.next_reg();
                        out.push_str(&format!(
                            "  {reg} = call i64 @rune_rt_json_to_i64(i64 {value})\n"
                        ));
                        reg
                    }
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
                    IrType::String => {
                        let rendered = self.resolve_value(&arg.value, &IrType::String, out)?;
                        let (ptr, len) = split_string_value(&rendered)?;
                        self.declared_runtime
                            .insert("declare i64 @rune_rt_string_to_i64(ptr, i64)\n".into());
                        let reg = self.next_reg();
                        out.push_str(&format!(
                            "  {reg} = call i64 @rune_rt_string_to_i64({ptr}, {len})\n"
                        ));
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
                            message: "`int` currently supports only bool, i32, i64, Json, String, and dynamic in the LLVM IR backend".into(),
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
            "__rune_builtin_time_has_wall_clock" => {
                self.expect_plain_arity(callee, args, 0)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i1 @rune_rt_time_has_wall_clock()\n".into());
                out.push_str(&format!("  {reg} = call i1 @rune_rt_time_has_wall_clock()\n"));
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
            "__rune_builtin_time_monotonic_us" => {
                self.expect_plain_arity(callee, args, 0)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i64 @rune_rt_time_monotonic_us()\n".into());
                out.push_str(&format!(
                    "  {reg} = call i64 @rune_rt_time_monotonic_us()\n"
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
            "__rune_builtin_sum_range" => {
                self.expect_plain_arity(callee, args, 3)?;
                let start = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                let stop = self.resolve_value(&args[1].value, &IrType::I64, out)?;
                let step = self.resolve_value(&args[2].value, &IrType::I64, out)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i64 @rune_rt_sum_range(i64, i64, i64)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i64 @rune_rt_sum_range(i64 {start}, i64 {stop}, i64 {step})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
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
            "__rune_builtin_system_platform"
            | "__rune_builtin_system_arch"
            | "__rune_builtin_system_target"
            | "__rune_builtin_system_board" => {
                self.expect_plain_arity(callee, args, 0)?;
                let ptr_reg = self.next_reg();
                let len_reg = self.next_reg();
                let runtime = match callee {
                    "__rune_builtin_system_platform" => "rune_rt_system_platform",
                    "__rune_builtin_system_arch" => "rune_rt_system_arch",
                    "__rune_builtin_system_target" => "rune_rt_system_target",
                    "__rune_builtin_system_board" => "rune_rt_system_board",
                    _ => unreachable!(),
                };
                self.declared_runtime
                    .insert(format!("declare ptr @{runtime}()\n"));
                self.declared_runtime
                    .insert("declare i64 @rune_rt_last_string_len()\n".into());
                out.push_str(&format!("  {ptr_reg} = call ptr @{runtime}()\n"));
                out.push_str(&format!("  {len_reg} = call i64 @rune_rt_last_string_len()\n"));
                if let Some(dst) = dst {
                    self.value_map
                        .insert(dst.clone(), format!("ptr {ptr_reg}, i64 {len_reg}"));
                }
                return Ok(());
            }
            "__rune_builtin_system_is_embedded" | "__rune_builtin_system_is_wasm" => {
                self.expect_plain_arity(callee, args, 0)?;
                let reg = self.next_reg();
                let runtime = if callee == "__rune_builtin_system_is_embedded" {
                    "rune_rt_system_is_embedded"
                } else {
                    "rune_rt_system_is_wasm"
                };
                self.declared_runtime
                    .insert(format!("declare i1 @{runtime}()\n"));
                out.push_str(&format!("  {reg} = call i1 @{runtime}()\n"));
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
            "__rune_builtin_env_get_string" => {
                self.expect_plain_arity(callee, args, 2)?;
                let rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&rendered)?;
                let default_rendered = self.resolve_value(&args[1].value, &IrType::String, out)?;
                let (default_ptr, default_len) = split_string_value(&default_rendered)?;
                let ptr_reg = self.next_reg();
                let len_reg = self.next_reg();
                self.declared_runtime
                    .insert("declare ptr @rune_rt_env_get_string(ptr, i64, ptr, i64)\n".into());
                self.declared_runtime
                    .insert("declare i64 @rune_rt_last_string_len()\n".into());
                out.push_str(&format!(
                    "  {ptr_reg} = call ptr @rune_rt_env_get_string({ptr}, {len}, {default_ptr}, {default_len})\n"
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
            "__rune_builtin_time_sleep_us" => {
                self.expect_plain_arity(callee, args, 1)?;
                let us = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_time_sleep_us(i64)\n".into());
                out.push_str(&format!("  call void @rune_rt_time_sleep_us(i64 {us})\n"));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_env_arg" => {
                self.expect_plain_arity(callee, args, 1)?;
                let index = self.resolve_value(&args[0].value, &IrType::I32, out)?;
                let ptr_reg = self.next_reg();
                let len_reg = self.next_reg();
                self.declared_runtime
                    .insert("declare ptr @rune_rt_env_arg(i32)\n".into());
                self.declared_runtime
                    .insert("declare i64 @rune_rt_last_string_len()\n".into());
                out.push_str(&format!("  {ptr_reg} = call ptr @rune_rt_env_arg(i32 {index})\n"));
                out.push_str(&format!(
                    "  {len_reg} = call i64 @rune_rt_last_string_len()\n"
                ));
                if let Some(dst) = dst {
                    self.value_map
                        .insert(dst.clone(), format!("ptr {ptr_reg}, i64 {len_reg}"));
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
            "__rune_builtin_network_tcp_listen" => {
                self.expect_plain_arity(callee, args, 2)?;
                let rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&rendered)?;
                let port = self.resolve_value(&args[1].value, &IrType::I32, out)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i1 @rune_rt_network_tcp_listen(ptr, i64, i32)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i1 @rune_rt_network_tcp_listen({ptr}, {len}, i32 {port})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_network_tcp_send" => {
                self.expect_plain_arity(callee, args, 3)?;
                let rendered_host = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (host_ptr, host_len) = split_string_value(&rendered_host)?;
                let port = self.resolve_value(&args[1].value, &IrType::I32, out)?;
                let rendered_data = self.resolve_value(&args[2].value, &IrType::String, out)?;
                let (data_ptr, data_len) = split_string_value(&rendered_data)?;
                let reg = self.next_reg();
                self.declared_runtime.insert(
                    "declare i1 @rune_rt_network_tcp_send(ptr, i64, i32, ptr, i64)\n".into(),
                );
                out.push_str(&format!(
                    "  {reg} = call i1 @rune_rt_network_tcp_send({host_ptr}, {host_len}, i32 {port}, {data_ptr}, {data_len})\n"
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
            "__rune_builtin_network_tcp_recv" => {
                self.expect_plain_arity(callee, args, 3)?;
                let rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&rendered)?;
                let port = self.resolve_value(&args[1].value, &IrType::I32, out)?;
                let max_bytes = self.resolve_value(&args[2].value, &IrType::I32, out)?;
                let ptr_reg = self.next_reg();
                let len_reg = self.next_reg();
                self.declared_runtime
                    .insert("declare ptr @rune_rt_network_tcp_recv(ptr, i64, i32, i32)\n".into());
                self.declared_runtime
                    .insert("declare i64 @rune_rt_last_string_len()\n".into());
                out.push_str(&format!(
                    "  {ptr_reg} = call ptr @rune_rt_network_tcp_recv({ptr}, {len}, i32 {port}, i32 {max_bytes})\n"
                ));
                out.push_str(&format!("  {len_reg} = call i64 @rune_rt_last_string_len()\n"));
                if let Some(dst) = dst {
                    self.value_map
                        .insert(dst.clone(), format!("ptr {ptr_reg}, i64 {len_reg}"));
                }
                return Ok(());
            }
            "__rune_builtin_network_tcp_recv_timeout" => {
                self.expect_plain_arity(callee, args, 4)?;
                let rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&rendered)?;
                let port = self.resolve_value(&args[1].value, &IrType::I32, out)?;
                let max_bytes = self.resolve_value(&args[2].value, &IrType::I32, out)?;
                let timeout = self.resolve_value(&args[3].value, &IrType::I32, out)?;
                let ptr_reg = self.next_reg();
                let len_reg = self.next_reg();
                self.declared_runtime.insert(
                    "declare ptr @rune_rt_network_tcp_recv_timeout(ptr, i64, i32, i32, i32)\n"
                        .into(),
                );
                self.declared_runtime
                    .insert("declare i64 @rune_rt_last_string_len()\n".into());
                out.push_str(&format!(
                    "  {ptr_reg} = call ptr @rune_rt_network_tcp_recv_timeout({ptr}, {len}, i32 {port}, i32 {max_bytes}, i32 {timeout})\n"
                ));
                out.push_str(&format!("  {len_reg} = call i64 @rune_rt_last_string_len()\n"));
                if let Some(dst) = dst {
                    self.value_map
                        .insert(dst.clone(), format!("ptr {ptr_reg}, i64 {len_reg}"));
                }
                return Ok(());
            }
            "__rune_builtin_network_tcp_request" => {
                self.expect_plain_arity(callee, args, 5)?;
                let rendered_host = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (host_ptr, host_len) = split_string_value(&rendered_host)?;
                let port = self.resolve_value(&args[1].value, &IrType::I32, out)?;
                let rendered_data = self.resolve_value(&args[2].value, &IrType::String, out)?;
                let (data_ptr, data_len) = split_string_value(&rendered_data)?;
                let max_bytes = self.resolve_value(&args[3].value, &IrType::I32, out)?;
                let timeout = self.resolve_value(&args[4].value, &IrType::I32, out)?;
                let ptr_reg = self.next_reg();
                let len_reg = self.next_reg();
                self.declared_runtime.insert(
                    "declare ptr @rune_rt_network_tcp_request(ptr, i64, i32, ptr, i64, i32, i32)\n"
                        .into(),
                );
                self.declared_runtime
                    .insert("declare i64 @rune_rt_last_string_len()\n".into());
                out.push_str(&format!(
                    "  {ptr_reg} = call ptr @rune_rt_network_tcp_request({host_ptr}, {host_len}, i32 {port}, {data_ptr}, {data_len}, i32 {max_bytes}, i32 {timeout})\n"
                ));
                out.push_str(&format!("  {len_reg} = call i64 @rune_rt_last_string_len()\n"));
                if let Some(dst) = dst {
                    self.value_map
                        .insert(dst.clone(), format!("ptr {ptr_reg}, i64 {len_reg}"));
                }
                return Ok(());
            }
            "__rune_builtin_network_tcp_accept_once" => {
                self.expect_plain_arity(callee, args, 4)?;
                let rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&rendered)?;
                let port = self.resolve_value(&args[1].value, &IrType::I32, out)?;
                let max_bytes = self.resolve_value(&args[2].value, &IrType::I32, out)?;
                let timeout = self.resolve_value(&args[3].value, &IrType::I32, out)?;
                let ptr_reg = self.next_reg();
                let len_reg = self.next_reg();
                self.declared_runtime.insert(
                    "declare ptr @rune_rt_network_tcp_accept_once(ptr, i64, i32, i32, i32)\n"
                        .into(),
                );
                self.declared_runtime
                    .insert("declare i64 @rune_rt_last_string_len()\n".into());
                out.push_str(&format!(
                    "  {ptr_reg} = call ptr @rune_rt_network_tcp_accept_once({ptr}, {len}, i32 {port}, i32 {max_bytes}, i32 {timeout})\n"
                ));
                out.push_str(&format!("  {len_reg} = call i64 @rune_rt_last_string_len()\n"));
                if let Some(dst) = dst {
                    self.value_map
                        .insert(dst.clone(), format!("ptr {ptr_reg}, i64 {len_reg}"));
                }
                return Ok(());
            }
            "__rune_builtin_network_tcp_server_open" => {
                self.expect_plain_arity(callee, args, 2)?;
                let rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&rendered)?;
                let port = self.resolve_value(&args[1].value, &IrType::I32, out)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i32 @rune_rt_network_tcp_server_open(ptr, i64, i32)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i32 @rune_rt_network_tcp_server_open({ptr}, {len}, i32 {port})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_network_tcp_client_open" => {
                self.expect_plain_arity(callee, args, 3)?;
                let rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&rendered)?;
                let port = self.resolve_value(&args[1].value, &IrType::I32, out)?;
                let timeout = self.resolve_value(&args[2].value, &IrType::I32, out)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i32 @rune_rt_network_tcp_client_open(ptr, i64, i32, i32)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i32 @rune_rt_network_tcp_client_open({ptr}, {len}, i32 {port}, i32 {timeout})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_network_tcp_server_accept" => {
                self.expect_plain_arity(callee, args, 3)?;
                let handle = self.resolve_value(&args[0].value, &IrType::I32, out)?;
                let max_bytes = self.resolve_value(&args[1].value, &IrType::I32, out)?;
                let timeout = self.resolve_value(&args[2].value, &IrType::I32, out)?;
                let ptr_reg = self.next_reg();
                let len_reg = self.next_reg();
                self.declared_runtime
                    .insert("declare ptr @rune_rt_network_tcp_server_accept(i32, i32, i32)\n".into());
                self.declared_runtime
                    .insert("declare i64 @rune_rt_last_string_len()\n".into());
                out.push_str(&format!(
                    "  {ptr_reg} = call ptr @rune_rt_network_tcp_server_accept(i32 {handle}, i32 {max_bytes}, i32 {timeout})\n"
                ));
                out.push_str(&format!("  {len_reg} = call i64 @rune_rt_last_string_len()\n"));
                if let Some(dst) = dst {
                    self.value_map
                        .insert(dst.clone(), format!("ptr {ptr_reg}, i64 {len_reg}"));
                }
                return Ok(());
            }
            "__rune_builtin_network_tcp_server_reply" => {
                self.expect_plain_arity(callee, args, 4)?;
                let handle = self.resolve_value(&args[0].value, &IrType::I32, out)?;
                let rendered_data = self.resolve_value(&args[1].value, &IrType::String, out)?;
                let (data_ptr, data_len) = split_string_value(&rendered_data)?;
                let max_bytes = self.resolve_value(&args[2].value, &IrType::I32, out)?;
                let timeout = self.resolve_value(&args[3].value, &IrType::I32, out)?;
                let ptr_reg = self.next_reg();
                let len_reg = self.next_reg();
                self.declared_runtime.insert(
                    "declare ptr @rune_rt_network_tcp_server_reply(i32, ptr, i64, i32, i32)\n"
                        .into(),
                );
                self.declared_runtime
                    .insert("declare i64 @rune_rt_last_string_len()\n".into());
                out.push_str(&format!(
                    "  {ptr_reg} = call ptr @rune_rt_network_tcp_server_reply(i32 {handle}, {data_ptr}, {data_len}, i32 {max_bytes}, i32 {timeout})\n"
                ));
                out.push_str(&format!("  {len_reg} = call i64 @rune_rt_last_string_len()\n"));
                if let Some(dst) = dst {
                    self.value_map
                        .insert(dst.clone(), format!("ptr {ptr_reg}, i64 {len_reg}"));
                }
                return Ok(());
            }
            "__rune_builtin_network_tcp_server_close" => {
                self.expect_plain_arity(callee, args, 1)?;
                let handle = self.resolve_value(&args[0].value, &IrType::I32, out)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i1 @rune_rt_network_tcp_server_close(i32)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i1 @rune_rt_network_tcp_server_close(i32 {handle})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_network_tcp_client_send" => {
                self.expect_plain_arity(callee, args, 2)?;
                let handle = self.resolve_value(&args[0].value, &IrType::I32, out)?;
                let rendered_data = self.resolve_value(&args[1].value, &IrType::String, out)?;
                let (data_ptr, data_len) = split_string_value(&rendered_data)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i1 @rune_rt_network_tcp_client_send(i32, ptr, i64)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i1 @rune_rt_network_tcp_client_send(i32 {handle}, {data_ptr}, {data_len})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_network_tcp_client_recv" => {
                self.expect_plain_arity(callee, args, 3)?;
                let handle = self.resolve_value(&args[0].value, &IrType::I32, out)?;
                let max_bytes = self.resolve_value(&args[1].value, &IrType::I32, out)?;
                let timeout = self.resolve_value(&args[2].value, &IrType::I32, out)?;
                let ptr_reg = self.next_reg();
                let len_reg = self.next_reg();
                self.declared_runtime
                    .insert("declare ptr @rune_rt_network_tcp_client_recv(i32, i32, i32)\n".into());
                self.declared_runtime
                    .insert("declare i64 @rune_rt_last_string_len()\n".into());
                out.push_str(&format!(
                    "  {ptr_reg} = call ptr @rune_rt_network_tcp_client_recv(i32 {handle}, i32 {max_bytes}, i32 {timeout})\n"
                ));
                out.push_str(&format!("  {len_reg} = call i64 @rune_rt_last_string_len()\n"));
                if let Some(dst) = dst {
                    self.value_map
                        .insert(dst.clone(), format!("ptr {ptr_reg}, i64 {len_reg}"));
                }
                return Ok(());
            }
            "__rune_builtin_network_tcp_client_close" => {
                self.expect_plain_arity(callee, args, 1)?;
                let handle = self.resolve_value(&args[0].value, &IrType::I32, out)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i1 @rune_rt_network_tcp_client_close(i32)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i1 @rune_rt_network_tcp_client_close(i32 {handle})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_network_last_error_code" => {
                self.expect_plain_arity(callee, args, 0)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i32 @rune_rt_network_last_error_code()\n".into());
                out.push_str(&format!(
                    "  {reg} = call i32 @rune_rt_network_last_error_code()\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_network_last_error_message" => {
                self.expect_plain_arity(callee, args, 0)?;
                let ptr_reg = self.next_reg();
                let len_reg = self.next_reg();
                self.declared_runtime
                    .insert("declare ptr @rune_rt_network_last_error_message()\n".into());
                self.declared_runtime
                    .insert("declare i64 @rune_rt_last_string_len()\n".into());
                out.push_str(&format!(
                    "  {ptr_reg} = call ptr @rune_rt_network_last_error_message()\n"
                ));
                out.push_str(&format!("  {len_reg} = call i64 @rune_rt_last_string_len()\n"));
                if let Some(dst) = dst {
                    self.value_map
                        .insert(dst.clone(), format!("ptr {ptr_reg}, i64 {len_reg}"));
                }
                return Ok(());
            }
            "__rune_builtin_network_clear_error" => {
                self.expect_plain_arity(callee, args, 0)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_network_clear_error()\n".into());
                out.push_str("  call void @rune_rt_network_clear_error()\n");
                return Ok(());
            }
            "__rune_builtin_network_tcp_reply_once" => {
                self.expect_plain_arity(callee, args, 5)?;
                let rendered_host = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (host_ptr, host_len) = split_string_value(&rendered_host)?;
                let port = self.resolve_value(&args[1].value, &IrType::I32, out)?;
                let rendered_data = self.resolve_value(&args[2].value, &IrType::String, out)?;
                let (data_ptr, data_len) = split_string_value(&rendered_data)?;
                let max_bytes = self.resolve_value(&args[3].value, &IrType::I32, out)?;
                let timeout = self.resolve_value(&args[4].value, &IrType::I32, out)?;
                let ptr_reg = self.next_reg();
                let len_reg = self.next_reg();
                self.declared_runtime.insert(
                    "declare ptr @rune_rt_network_tcp_reply_once(ptr, i64, i32, ptr, i64, i32, i32)\n"
                        .into(),
                );
                self.declared_runtime
                    .insert("declare i64 @rune_rt_last_string_len()\n".into());
                out.push_str(&format!(
                    "  {ptr_reg} = call ptr @rune_rt_network_tcp_reply_once({host_ptr}, {host_len}, i32 {port}, {data_ptr}, {data_len}, i32 {max_bytes}, i32 {timeout})\n"
                ));
                out.push_str(&format!("  {len_reg} = call i64 @rune_rt_last_string_len()\n"));
                if let Some(dst) = dst {
                    self.value_map
                        .insert(dst.clone(), format!("ptr {ptr_reg}, i64 {len_reg}"));
                }
                return Ok(());
            }
            "__rune_builtin_network_udp_bind" => {
                self.expect_plain_arity(callee, args, 2)?;
                let rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&rendered)?;
                let port = self.resolve_value(&args[1].value, &IrType::I32, out)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i1 @rune_rt_network_udp_bind(ptr, i64, i32)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i1 @rune_rt_network_udp_bind({ptr}, {len}, i32 {port})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_network_udp_send" => {
                self.expect_plain_arity(callee, args, 3)?;
                let rendered_host = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (host_ptr, host_len) = split_string_value(&rendered_host)?;
                let port = self.resolve_value(&args[1].value, &IrType::I32, out)?;
                let rendered_data = self.resolve_value(&args[2].value, &IrType::String, out)?;
                let (data_ptr, data_len) = split_string_value(&rendered_data)?;
                let reg = self.next_reg();
                self.declared_runtime.insert(
                    "declare i1 @rune_rt_network_udp_send(ptr, i64, i32, ptr, i64)\n".into(),
                );
                out.push_str(&format!(
                    "  {reg} = call i1 @rune_rt_network_udp_send({host_ptr}, {host_len}, i32 {port}, {data_ptr}, {data_len})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_network_udp_recv" => {
                self.expect_plain_arity(callee, args, 4)?;
                let rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&rendered)?;
                let port = self.resolve_value(&args[1].value, &IrType::I32, out)?;
                let max_bytes = self.resolve_value(&args[2].value, &IrType::I32, out)?;
                let timeout = self.resolve_value(&args[3].value, &IrType::I32, out)?;
                let ptr_reg = self.next_reg();
                let len_reg = self.next_reg();
                self.declared_runtime.insert(
                    "declare ptr @rune_rt_network_udp_recv(ptr, i64, i32, i32, i32)\n".into(),
                );
                self.declared_runtime
                    .insert("declare i64 @rune_rt_last_string_len()\n".into());
                out.push_str(&format!(
                    "  {ptr_reg} = call ptr @rune_rt_network_udp_recv({ptr}, {len}, i32 {port}, i32 {max_bytes}, i32 {timeout})\n"
                ));
                out.push_str(&format!("  {len_reg} = call i64 @rune_rt_last_string_len()\n"));
                if let Some(dst) = dst {
                    self.value_map
                        .insert(dst.clone(), format!("ptr {ptr_reg}, i64 {len_reg}"));
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
            "__rune_builtin_fs_current_dir" => {
                self.expect_plain_arity(callee, args, 0)?;
                let ptr_reg = self.next_reg();
                let len_reg = self.next_reg();
                self.declared_runtime
                    .insert("declare ptr @rune_rt_fs_current_dir()\n".into());
                self.declared_runtime
                    .insert("declare i64 @rune_rt_last_string_len()\n".into());
                out.push_str(&format!("  {ptr_reg} = call ptr @rune_rt_fs_current_dir()\n"));
                out.push_str(&format!("  {len_reg} = call i64 @rune_rt_last_string_len()\n"));
                if let Some(dst) = dst {
                    self.value_map
                        .insert(dst.clone(), format!("ptr {ptr_reg}, i64 {len_reg}"));
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
            "__rune_builtin_fs_canonicalize" => {
                self.expect_plain_arity(callee, args, 1)?;
                let rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&rendered)?;
                let ptr_reg = self.next_reg();
                let len_reg = self.next_reg();
                self.declared_runtime
                    .insert("declare ptr @rune_rt_fs_canonicalize(ptr, i64)\n".into());
                self.declared_runtime
                    .insert("declare i64 @rune_rt_last_string_len()\n".into());
                out.push_str(&format!(
                    "  {ptr_reg} = call ptr @rune_rt_fs_canonicalize({ptr}, {len})\n"
                ));
                out.push_str(&format!("  {len_reg} = call i64 @rune_rt_last_string_len()\n"));
                if let Some(dst) = dst {
                    self.value_map
                        .insert(dst.clone(), format!("ptr {ptr_reg}, i64 {len_reg}"));
                }
                return Ok(());
            }
            "__rune_builtin_fs_write_string" | "__rune_builtin_fs_append_string" => {
                self.expect_plain_arity(callee, args, 2)?;
                let path_rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (path_ptr, path_len) = split_string_value(&path_rendered)?;
                let content_rendered = self.resolve_value(&args[1].value, &IrType::String, out)?;
                let (content_ptr, content_len) = split_string_value(&content_rendered)?;
                let reg = self.next_reg();
                let runtime = match callee {
                    "__rune_builtin_fs_write_string" => "rune_rt_fs_write_string",
                    "__rune_builtin_fs_append_string" => "rune_rt_fs_append_string",
                    _ => unreachable!(),
                };
                self.declared_runtime
                    .insert(format!("declare i1 @{runtime}(ptr, i64, ptr, i64)\n"));
                out.push_str(&format!(
                    "  {reg} = call i1 @{runtime}({path_ptr}, {path_len}, {content_ptr}, {content_len})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_fs_remove"
            | "__rune_builtin_fs_set_current_dir"
            | "__rune_builtin_fs_create_dir"
            | "__rune_builtin_fs_create_dir_all"
            | "__rune_builtin_fs_is_file"
            | "__rune_builtin_fs_is_dir" => {
                self.expect_plain_arity(callee, args, 1)?;
                let rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&rendered)?;
                let reg = self.next_reg();
                let runtime = match callee {
                    "__rune_builtin_fs_remove" => "rune_rt_fs_remove",
                    "__rune_builtin_fs_set_current_dir" => "rune_rt_fs_set_current_dir",
                    "__rune_builtin_fs_create_dir" => "rune_rt_fs_create_dir",
                    "__rune_builtin_fs_create_dir_all" => "rune_rt_fs_create_dir_all",
                    "__rune_builtin_fs_is_file" => "rune_rt_fs_is_file",
                    "__rune_builtin_fs_is_dir" => "rune_rt_fs_is_dir",
                    _ => unreachable!(),
                };
                self.declared_runtime
                    .insert(format!("declare i1 @{runtime}(ptr, i64)\n"));
                out.push_str(&format!("  {reg} = call i1 @{runtime}({ptr}, {len})\n"));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_fs_rename" | "__rune_builtin_fs_copy" => {
                self.expect_plain_arity(callee, args, 2)?;
                let from_rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (from_ptr, from_len) = split_string_value(&from_rendered)?;
                let to_rendered = self.resolve_value(&args[1].value, &IrType::String, out)?;
                let (to_ptr, to_len) = split_string_value(&to_rendered)?;
                let reg = self.next_reg();
                let runtime = match callee {
                    "__rune_builtin_fs_rename" => "rune_rt_fs_rename",
                    "__rune_builtin_fs_copy" => "rune_rt_fs_copy",
                    _ => unreachable!(),
                };
                self.declared_runtime
                    .insert(format!("declare i1 @{runtime}(ptr, i64, ptr, i64)\n"));
                out.push_str(&format!(
                    "  {reg} = call i1 @{runtime}({from_ptr}, {from_len}, {to_ptr}, {to_len})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_fs_file_size" => {
                self.expect_plain_arity(callee, args, 1)?;
                let rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&rendered)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i64 @rune_rt_fs_file_size(ptr, i64)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i64 @rune_rt_fs_file_size({ptr}, {len})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_json_parse" => {
                self.expect_plain_arity(callee, args, 1)?;
                let rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&rendered)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i64 @rune_rt_json_parse(ptr, i64)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i64 @rune_rt_json_parse({ptr}, {len})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_json_stringify" => {
                self.expect_plain_arity(callee, args, 1)?;
                let value = self.resolve_value(&args[0].value, &IrType::Json, out)?;
                let ptr_reg = self.next_reg();
                let len_reg = self.next_reg();
                self.declared_runtime
                    .insert("declare ptr @rune_rt_json_stringify(i64)\n".into());
                self.declared_runtime
                    .insert("declare i64 @rune_rt_last_string_len()\n".into());
                out.push_str(&format!(
                    "  {ptr_reg} = call ptr @rune_rt_json_stringify(i64 {value})\n"
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
            "__rune_builtin_json_kind" => {
                self.expect_plain_arity(callee, args, 1)?;
                let value = self.resolve_value(&args[0].value, &IrType::Json, out)?;
                let ptr_reg = self.next_reg();
                let len_reg = self.next_reg();
                self.declared_runtime
                    .insert("declare ptr @rune_rt_json_kind(i64)\n".into());
                self.declared_runtime
                    .insert("declare i64 @rune_rt_last_string_len()\n".into());
                out.push_str(&format!(
                    "  {ptr_reg} = call ptr @rune_rt_json_kind(i64 {value})\n"
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
            "__rune_builtin_json_is_null" => {
                self.expect_plain_arity(callee, args, 1)?;
                let value = self.resolve_value(&args[0].value, &IrType::Json, out)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i1 @rune_rt_json_is_null(i64)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i1 @rune_rt_json_is_null(i64 {value})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_json_len" => {
                self.expect_plain_arity(callee, args, 1)?;
                let value = self.resolve_value(&args[0].value, &IrType::Json, out)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i64 @rune_rt_json_len(i64)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i64 @rune_rt_json_len(i64 {value})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_json_get" => {
                self.expect_plain_arity(callee, args, 2)?;
                let value = self.resolve_value(&args[0].value, &IrType::Json, out)?;
                let key = self.resolve_value(&args[1].value, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&key)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i64 @rune_rt_json_get(i64, ptr, i64)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i64 @rune_rt_json_get(i64 {value}, {ptr}, {len})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_json_index" => {
                self.expect_plain_arity(callee, args, 2)?;
                let value = self.resolve_value(&args[0].value, &IrType::Json, out)?;
                let index = self.resolve_value(&args[1].value, &IrType::I64, out)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i64 @rune_rt_json_index(i64, i64)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i64 @rune_rt_json_index(i64 {value}, i64 {index})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_json_to_string" => {
                self.expect_plain_arity(callee, args, 1)?;
                let value = self.resolve_value(&args[0].value, &IrType::Json, out)?;
                let ptr_reg = self.next_reg();
                let len_reg = self.next_reg();
                self.declared_runtime
                    .insert("declare ptr @rune_rt_json_to_string(i64)\n".into());
                self.declared_runtime
                    .insert("declare i64 @rune_rt_last_string_len()\n".into());
                out.push_str(&format!(
                    "  {ptr_reg} = call ptr @rune_rt_json_to_string(i64 {value})\n"
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
            "__rune_builtin_json_to_i64" => {
                self.expect_plain_arity(callee, args, 1)?;
                let value = self.resolve_value(&args[0].value, &IrType::Json, out)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i64 @rune_rt_json_to_i64(i64)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i64 @rune_rt_json_to_i64(i64 {value})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_json_to_bool" => {
                self.expect_plain_arity(callee, args, 1)?;
                let value = self.resolve_value(&args[0].value, &IrType::Json, out)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i1 @rune_rt_json_to_bool(i64)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i1 @rune_rt_json_to_bool(i64 {value})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_arduino_pin_mode" => {
                self.expect_plain_arity(callee, args, 2)?;
                let pin = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                let mode = self.resolve_value(&args[1].value, &IrType::I64, out)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_arduino_pin_mode(i64, i64)\n".into());
                out.push_str(&format!(
                    "  call void @rune_rt_arduino_pin_mode(i64 {pin}, i64 {mode})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_gpio_pin_mode" => {
                self.expect_plain_arity(callee, args, 2)?;
                let pin = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                let mode = self.resolve_value(&args[1].value, &IrType::I64, out)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_gpio_pin_mode(i64, i64)\n".into());
                out.push_str(&format!(
                    "  call void @rune_rt_gpio_pin_mode(i64 {pin}, i64 {mode})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_gpio_digital_write" => {
                self.expect_plain_arity(callee, args, 2)?;
                let pin = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                let value = self.resolve_value(&args[1].value, &IrType::Bool, out)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_gpio_digital_write(i64, i1)\n".into());
                out.push_str(&format!(
                    "  call void @rune_rt_gpio_digital_write(i64 {pin}, i1 {value})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_gpio_digital_read" => {
                self.expect_plain_arity(callee, args, 1)?;
                let pin = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i1 @rune_rt_gpio_digital_read(i64)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i1 @rune_rt_gpio_digital_read(i64 {pin})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_gpio_pwm_write" => {
                self.expect_plain_arity(callee, args, 2)?;
                let pin = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                let value = self.resolve_value(&args[1].value, &IrType::I64, out)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_gpio_pwm_write(i64, i64)\n".into());
                out.push_str(&format!(
                    "  call void @rune_rt_gpio_pwm_write(i64 {pin}, i64 {value})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_gpio_analog_read" => {
                self.expect_plain_arity(callee, args, 1)?;
                let pin = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i64 @rune_rt_gpio_analog_read(i64)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i64 @rune_rt_gpio_analog_read(i64 {pin})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_gpio_mode_input"
            | "__rune_builtin_gpio_mode_output"
            | "__rune_builtin_gpio_mode_input_pullup"
            | "__rune_builtin_gpio_pwm_duty_max"
            | "__rune_builtin_gpio_analog_max" => {
                self.expect_plain_arity(callee, args, 0)?;
                let reg = self.next_reg();
                let runtime = match callee {
                    "__rune_builtin_gpio_mode_input" => "rune_rt_gpio_mode_input",
                    "__rune_builtin_gpio_mode_output" => "rune_rt_gpio_mode_output",
                    "__rune_builtin_gpio_mode_input_pullup" => "rune_rt_gpio_mode_input_pullup",
                    "__rune_builtin_gpio_pwm_duty_max" => "rune_rt_gpio_pwm_duty_max",
                    "__rune_builtin_gpio_analog_max" => "rune_rt_gpio_analog_max",
                    _ => unreachable!(),
                };
                self.declared_runtime
                    .insert(format!("declare i64 @{runtime}()\n"));
                out.push_str(&format!("  {reg} = call i64 @{runtime}()\n"));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_arduino_digital_write" => {
                self.expect_plain_arity(callee, args, 2)?;
                let pin = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                let value = self.resolve_value(&args[1].value, &IrType::Bool, out)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_arduino_digital_write(i64, i1)\n".into());
                out.push_str(&format!(
                    "  call void @rune_rt_arduino_digital_write(i64 {pin}, i1 {value})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_arduino_digital_read" => {
                self.expect_plain_arity(callee, args, 1)?;
                let pin = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i1 @rune_rt_arduino_digital_read(i64)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i1 @rune_rt_arduino_digital_read(i64 {pin})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_arduino_analog_write" => {
                self.expect_plain_arity(callee, args, 2)?;
                let pin = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                let value = self.resolve_value(&args[1].value, &IrType::I64, out)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_arduino_analog_write(i64, i64)\n".into());
                out.push_str(&format!(
                    "  call void @rune_rt_arduino_analog_write(i64 {pin}, i64 {value})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_arduino_analog_read" => {
                self.expect_plain_arity(callee, args, 1)?;
                let pin = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i64 @rune_rt_arduino_analog_read(i64)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i64 @rune_rt_arduino_analog_read(i64 {pin})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_arduino_analog_reference" => {
                self.expect_plain_arity(callee, args, 1)?;
                let mode = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_arduino_analog_reference(i64)\n".into());
                out.push_str(&format!(
                    "  call void @rune_rt_arduino_analog_reference(i64 {mode})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_arduino_pulse_in" => {
                self.expect_plain_arity(callee, args, 3)?;
                let pin = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                let state = self.resolve_value(&args[1].value, &IrType::Bool, out)?;
                let timeout = self.resolve_value(&args[2].value, &IrType::I64, out)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i64 @rune_rt_arduino_pulse_in(i64, i1, i64)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i64 @rune_rt_arduino_pulse_in(i64 {pin}, i1 {state}, i64 {timeout})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_arduino_shift_out" => {
                self.expect_plain_arity(callee, args, 4)?;
                let data_pin = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                let clock_pin = self.resolve_value(&args[1].value, &IrType::I64, out)?;
                let bit_order = self.resolve_value(&args[2].value, &IrType::I64, out)?;
                let value = self.resolve_value(&args[3].value, &IrType::I64, out)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_arduino_shift_out(i64, i64, i64, i64)\n".into());
                out.push_str(&format!(
                    "  call void @rune_rt_arduino_shift_out(i64 {data_pin}, i64 {clock_pin}, i64 {bit_order}, i64 {value})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_arduino_shift_in" => {
                self.expect_plain_arity(callee, args, 3)?;
                let data_pin = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                let clock_pin = self.resolve_value(&args[1].value, &IrType::I64, out)?;
                let bit_order = self.resolve_value(&args[2].value, &IrType::I64, out)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i64 @rune_rt_arduino_shift_in(i64, i64, i64)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i64 @rune_rt_arduino_shift_in(i64 {data_pin}, i64 {clock_pin}, i64 {bit_order})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_arduino_tone" => {
                self.expect_plain_arity(callee, args, 3)?;
                let pin = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                let frequency = self.resolve_value(&args[1].value, &IrType::I64, out)?;
                let duration = self.resolve_value(&args[2].value, &IrType::I64, out)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_arduino_tone(i64, i64, i64)\n".into());
                out.push_str(&format!(
                    "  call void @rune_rt_arduino_tone(i64 {pin}, i64 {frequency}, i64 {duration})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_arduino_no_tone" => {
                self.expect_plain_arity(callee, args, 1)?;
                let pin = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_arduino_no_tone(i64)\n".into());
                out.push_str(&format!(
                    "  call void @rune_rt_arduino_no_tone(i64 {pin})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_arduino_servo_attach" => {
                self.expect_plain_arity(callee, args, 1)?;
                let pin = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i1 @rune_rt_arduino_servo_attach(i64)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i1 @rune_rt_arduino_servo_attach(i64 {pin})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_arduino_servo_detach" => {
                self.expect_plain_arity(callee, args, 1)?;
                let pin = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_arduino_servo_detach(i64)\n".into());
                out.push_str(&format!(
                    "  call void @rune_rt_arduino_servo_detach(i64 {pin})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_arduino_servo_write"
            | "__rune_builtin_arduino_servo_write_us" => {
                self.expect_plain_arity(callee, args, 2)?;
                let pin = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                let value = self.resolve_value(&args[1].value, &IrType::I64, out)?;
                let runtime = if callee == "__rune_builtin_arduino_servo_write" {
                    "rune_rt_arduino_servo_write"
                } else {
                    "rune_rt_arduino_servo_write_us"
                };
                self.declared_runtime
                    .insert(format!("declare void @{runtime}(i64, i64)\n"));
                out.push_str(&format!("  call void @{runtime}(i64 {pin}, i64 {value})\n"));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_arduino_delay_ms" => {
                self.expect_plain_arity(callee, args, 1)?;
                let ms = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_arduino_delay_ms(i64)\n".into());
                out.push_str(&format!(
                    "  call void @rune_rt_arduino_delay_ms(i64 {ms})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_arduino_delay_us" => {
                self.expect_plain_arity(callee, args, 1)?;
                let us = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_arduino_delay_us(i64)\n".into());
                out.push_str(&format!(
                    "  call void @rune_rt_arduino_delay_us(i64 {us})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_arduino_millis" => {
                self.expect_plain_arity(callee, args, 0)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i64 @rune_rt_arduino_millis()\n".into());
                out.push_str(&format!("  {reg} = call i64 @rune_rt_arduino_millis()\n"));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_arduino_micros" => {
                self.expect_plain_arity(callee, args, 0)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i64 @rune_rt_arduino_micros()\n".into());
                out.push_str(&format!("  {reg} = call i64 @rune_rt_arduino_micros()\n"));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_arduino_read_line" => {
                self.expect_plain_arity(callee, args, 0)?;
                let ptr_reg = self.next_reg();
                let len_reg = self.next_reg();
                self.declared_runtime
                    .insert("declare ptr @rune_rt_arduino_read_line()\n".into());
                self.declared_runtime
                    .insert("declare i64 @rune_rt_last_string_len()\n".into());
                out.push_str(&format!("  {ptr_reg} = call ptr @rune_rt_arduino_read_line()\n"));
                out.push_str(&format!("  {len_reg} = call i64 @rune_rt_last_string_len()\n"));
                if let Some(dst) = dst {
                    self.value_map
                        .insert(dst.clone(), format!("ptr {ptr_reg}, i64 {len_reg}"));
                }
                return Ok(());
            }
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
            | "__rune_builtin_arduino_analog_ref_external" => {
                self.expect_plain_arity(callee, args, 0)?;
                let reg = self.next_reg();
                let runtime = match callee {
                    "__rune_builtin_arduino_mode_input" => "rune_rt_arduino_mode_input",
                    "__rune_builtin_arduino_mode_output" => "rune_rt_arduino_mode_output",
                    "__rune_builtin_arduino_mode_input_pullup" => {
                        "rune_rt_arduino_mode_input_pullup"
                    }
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
                self.declared_runtime
                    .insert(format!("declare i64 @{runtime}()\n"));
                out.push_str(&format!("  {reg} = call i64 @{runtime}()\n"));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_arduino_uart_begin" => {
                self.expect_plain_arity(callee, args, 1)?;
                let baud = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_arduino_uart_begin(i64)\n".into());
                out.push_str(&format!(
                    "  call void @rune_rt_arduino_uart_begin(i64 {baud})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_arduino_uart_available" => {
                self.expect_plain_arity(callee, args, 0)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i64 @rune_rt_arduino_uart_available()\n".into());
                out.push_str(&format!("  {reg} = call i64 @rune_rt_arduino_uart_available()\n"));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_arduino_uart_read_byte" => {
                self.expect_plain_arity(callee, args, 0)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i64 @rune_rt_arduino_uart_read_byte()\n".into());
                out.push_str(&format!("  {reg} = call i64 @rune_rt_arduino_uart_read_byte()\n"));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_arduino_uart_peek_byte" => {
                self.expect_plain_arity(callee, args, 0)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i64 @rune_rt_arduino_uart_peek_byte()\n".into());
                out.push_str(&format!("  {reg} = call i64 @rune_rt_arduino_uart_peek_byte()\n"));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_arduino_uart_write_byte" => {
                self.expect_plain_arity(callee, args, 1)?;
                let value = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_arduino_uart_write_byte(i64)\n".into());
                out.push_str(&format!(
                    "  call void @rune_rt_arduino_uart_write_byte(i64 {value})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_arduino_uart_write" => {
                self.expect_plain_arity(callee, args, 1)?;
                let rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&rendered)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_arduino_uart_write(ptr, i64)\n".into());
                out.push_str(&format!(
                    "  call void @rune_rt_arduino_uart_write({ptr}, {len})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_arduino_interrupts_enable"
            | "__rune_builtin_arduino_interrupts_disable" => {
                self.expect_plain_arity(callee, args, 0)?;
                let runtime = if callee == "__rune_builtin_arduino_interrupts_enable" {
                    "rune_rt_arduino_interrupts_enable"
                } else {
                    "rune_rt_arduino_interrupts_disable"
                };
                self.declared_runtime
                    .insert(format!("declare void @{runtime}()\n"));
                out.push_str(&format!("  call void @{runtime}()\n"));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_arduino_random_seed" => {
                self.expect_plain_arity(callee, args, 1)?;
                let seed = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_arduino_random_seed(i64)\n".into());
                out.push_str(&format!("  call void @rune_rt_arduino_random_seed(i64 {seed})\n"));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_arduino_random_i64" => {
                self.expect_plain_arity(callee, args, 1)?;
                let max_value = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i64 @rune_rt_arduino_random_i64(i64)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i64 @rune_rt_arduino_random_i64(i64 {max_value})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_arduino_random_range" => {
                self.expect_plain_arity(callee, args, 2)?;
                let min_value = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                let max_value = self.resolve_value(&args[1].value, &IrType::I64, out)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i64 @rune_rt_arduino_random_range(i64, i64)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i64 @rune_rt_arduino_random_range(i64 {min_value}, i64 {max_value})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_serial_open" => {
                self.expect_plain_arity(callee, args, 2)?;
                let rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&rendered)?;
                let baud = self.resolve_value(&args[1].value, &IrType::I64, out)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i1 @rune_rt_serial_open(ptr, i64, i64)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i1 @rune_rt_serial_open({ptr}, {len}, i64 {baud})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_serial_is_open" => {
                self.expect_plain_arity(callee, args, 0)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i1 @rune_rt_serial_is_open()\n".into());
                out.push_str(&format!("  {reg} = call i1 @rune_rt_serial_is_open()\n"));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_serial_close" => {
                self.expect_plain_arity(callee, args, 0)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_serial_close()\n".into());
                out.push_str("  call void @rune_rt_serial_close()\n");
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_serial_flush" => {
                self.expect_plain_arity(callee, args, 0)?;
                self.declared_runtime
                    .insert("declare void @rune_rt_serial_flush()\n".into());
                out.push_str("  call void @rune_rt_serial_flush()\n");
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), "0".into());
                }
                return Ok(());
            }
            "__rune_builtin_serial_available" => {
                self.expect_plain_arity(callee, args, 0)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i64 @rune_rt_serial_available()\n".into());
                out.push_str(&format!("  {reg} = call i64 @rune_rt_serial_available()\n"));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_serial_read_byte" => {
                self.expect_plain_arity(callee, args, 0)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i64 @rune_rt_serial_read_byte()\n".into());
                out.push_str(&format!("  {reg} = call i64 @rune_rt_serial_read_byte()\n"));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_serial_read_byte_timeout" => {
                self.expect_plain_arity(callee, args, 1)?;
                let timeout = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i64 @rune_rt_serial_read_byte_timeout(i64)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i64 @rune_rt_serial_read_byte_timeout(i64 {timeout})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_serial_peek_byte" => {
                self.expect_plain_arity(callee, args, 0)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i64 @rune_rt_serial_peek_byte()\n".into());
                out.push_str(&format!("  {reg} = call i64 @rune_rt_serial_peek_byte()\n"));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_serial_read_line" => {
                self.expect_plain_arity(callee, args, 0)?;
                let ptr_reg = self.next_reg();
                let len_reg = self.next_reg();
                self.declared_runtime
                    .insert("declare ptr @rune_rt_serial_read_line()\n".into());
                self.declared_runtime
                    .insert("declare i64 @rune_rt_last_string_len()\n".into());
                out.push_str(&format!("  {ptr_reg} = call ptr @rune_rt_serial_read_line()\n"));
                out.push_str(&format!("  {len_reg} = call i64 @rune_rt_last_string_len()\n"));
                if let Some(dst) = dst {
                    self.value_map
                        .insert(dst.clone(), format!("ptr {ptr_reg}, i64 {len_reg}"));
                }
                return Ok(());
            }
            "__rune_builtin_serial_read_line_timeout" => {
                self.expect_plain_arity(callee, args, 1)?;
                let timeout = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                let ptr_reg = self.next_reg();
                let len_reg = self.next_reg();
                self.declared_runtime
                    .insert("declare ptr @rune_rt_serial_read_line_timeout(i64)\n".into());
                self.declared_runtime
                    .insert("declare i64 @rune_rt_last_string_len()\n".into());
                out.push_str(&format!(
                    "  {ptr_reg} = call ptr @rune_rt_serial_read_line_timeout(i64 {timeout})\n"
                ));
                out.push_str(&format!("  {len_reg} = call i64 @rune_rt_last_string_len()\n"));
                if let Some(dst) = dst {
                    self.value_map
                        .insert(dst.clone(), format!("ptr {ptr_reg}, i64 {len_reg}"));
                }
                return Ok(());
            }
            "__rune_builtin_serial_write" | "__rune_builtin_serial_write_line" => {
                self.expect_plain_arity(callee, args, 1)?;
                let rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&rendered)?;
                let runtime = if callee == "__rune_builtin_serial_write" {
                    "rune_rt_serial_write"
                } else {
                    "rune_rt_serial_write_line"
                };
                let reg = self.next_reg();
                self.declared_runtime
                    .insert(format!("declare i1 @{runtime}(ptr, i64)\n"));
                out.push_str(&format!("  {reg} = call i1 @{runtime}({ptr}, {len})\n"));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "__rune_builtin_serial_write_byte" => {
                self.expect_plain_arity(callee, args, 1)?;
                let value = self.resolve_value(&args[0].value, &IrType::I64, out)?;
                let reg = self.next_reg();
                self.declared_runtime
                    .insert("declare i1 @rune_rt_serial_write_byte(i64)\n".into());
                out.push_str(&format!(
                    "  {reg} = call i1 @rune_rt_serial_write_byte(i64 {value})\n"
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
            "rune_rt_string_len" => {
                if args.len() != 1 {
                    return Err(LlvmIrError {
                        message: "`rune_rt_string_len` expects 1 argument".into(),
                    });
                }
                let rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (_, len) = split_string_value(&rendered)?;
                // len is already `i64 <value>` — extract just the value token
                let len_val = len.trim_start_matches("i64 ");
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), len_val.to_string());
                }
                return Ok(());
            }
            "rune_rt_string_upper" | "rune_rt_string_lower" | "rune_rt_string_strip" => {
                if args.len() != 1 {
                    return Err(LlvmIrError {
                        message: format!("`{callee}` expects 1 argument"),
                    });
                }
                let rendered = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (ptr, len) = split_string_value(&rendered)?;
                self.declared_runtime
                    .insert(format!("declare ptr @{callee}(ptr, i64)\n"));
                self.declared_runtime
                    .insert("declare i64 @rune_rt_last_string_len()\n".into());
                let ptr_reg = self.next_reg();
                out.push_str(&format!("  {ptr_reg} = call ptr @{callee}({ptr}, {len})\n"));
                let len_reg = self.next_reg();
                out.push_str(&format!(
                    "  {len_reg} = call i64 @rune_rt_last_string_len()\n"
                ));
                if let Some(dst) = dst {
                    self.value_map
                        .insert(dst.clone(), format!("ptr {ptr_reg}, i64 {len_reg}"));
                }
                return Ok(());
            }
            "rune_rt_string_contains"
            | "rune_rt_string_starts_with"
            | "rune_rt_string_ends_with" => {
                if args.len() != 2 {
                    return Err(LlvmIrError {
                        message: format!("`{callee}` expects 2 arguments"),
                    });
                }
                let s = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (s_ptr, s_len) = split_string_value(&s)?;
                let needle = self.resolve_value(&args[1].value, &IrType::String, out)?;
                let (n_ptr, n_len) = split_string_value(&needle)?;
                self.declared_runtime
                    .insert(format!("declare i1 @{callee}(ptr, i64, ptr, i64)\n"));
                let reg = self.next_reg();
                out.push_str(&format!(
                    "  {reg} = call i1 @{callee}({s_ptr}, {s_len}, {n_ptr}, {n_len})\n"
                ));
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), reg);
                }
                return Ok(());
            }
            "rune_rt_string_replace" => {
                if args.len() != 3 {
                    return Err(LlvmIrError {
                        message: "`rune_rt_string_replace` expects 3 arguments".into(),
                    });
                }
                let s = self.resolve_value(&args[0].value, &IrType::String, out)?;
                let (s_ptr, s_len) = split_string_value(&s)?;
                let from = self.resolve_value(&args[1].value, &IrType::String, out)?;
                let (f_ptr, f_len) = split_string_value(&from)?;
                let to = self.resolve_value(&args[2].value, &IrType::String, out)?;
                let (t_ptr, t_len) = split_string_value(&to)?;
                self.declared_runtime
                    .insert("declare ptr @rune_rt_string_replace(ptr, i64, ptr, i64, ptr, i64)\n".into());
                self.declared_runtime
                    .insert("declare i64 @rune_rt_last_string_len()\n".into());
                let ptr_reg = self.next_reg();
                out.push_str(&format!(
                    "  {ptr_reg} = call ptr @rune_rt_string_replace({s_ptr}, {s_len}, {f_ptr}, {f_len}, {t_ptr}, {t_len})\n"
                ));
                let len_reg = self.next_reg();
                out.push_str(&format!(
                    "  {len_reg} = call i64 @rune_rt_last_string_len()\n"
                ));
                if let Some(dst) = dst {
                    self.value_map
                        .insert(dst.clone(), format!("ptr {ptr_reg}, i64 {len_reg}"));
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
        let ordered_args = resolve_llvm_call_args(callee, &sig.params, args)?;
        let mut rendered_args = Vec::new();
        for (arg, (_, ty)) in ordered_args.iter().zip(sig.params.iter()) {
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
            } else if matches!(ty, IrType::Struct(_)) && !sig.is_extern {
                let value = self.resolve_value(&arg.value, ty, out)?;
                rendered_args.push(format!(
                    "{} {}",
                    llvm_internal_type(ty, self.struct_layouts)?,
                    value
                ));
            } else {
                let value = self.resolve_value(&arg.value, ty, out)?;
                rendered_args.push(format!("{} {}", llvm_scalar_type(ty)?, value));
            }
        }

        if sig.ret == IrType::Unit {
            out.push_str(&format!(
                "  call void @{}({})\n",
                llvm_internal_symbol_name(callee, sig),
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
            out.push_str(&format!(
                "  {reg} = call ptr @{}({})\n",
                llvm_internal_symbol_name(callee, sig),
                rendered_args.join(", ")
            ));
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
        } else if matches!(sig.ret, IrType::String | IrType::Dynamic | IrType::Struct(_)) {
            let aggregate_reg = self.next_reg();
            out.push_str(&format!(
                "  {aggregate_reg} = call {} @{}({})\n",
                llvm_internal_type(&sig.ret, self.struct_layouts)?,
                llvm_internal_symbol_name(callee, sig),
                rendered_args.join(", ")
            ));
            if sig.ret == IrType::String {
                let ptr_reg = self.next_reg();
                out.push_str(&format!(
                    "  {ptr_reg} = extractvalue {} {aggregate_reg}, 0\n",
                    llvm_internal_type(&sig.ret, self.struct_layouts)?
                ));
                let len_reg = self.next_reg();
                out.push_str(&format!(
                    "  {len_reg} = extractvalue {} {aggregate_reg}, 1\n",
                    llvm_internal_type(&sig.ret, self.struct_layouts)?
                ));
                if let Some(dst) = dst {
                    self.value_map
                        .insert(dst.clone(), format!("ptr {ptr_reg}, i64 {len_reg}"));
                }
            } else if matches!(sig.ret, IrType::Struct(_)) {
                if let Some(dst) = dst {
                    self.value_map.insert(dst.clone(), aggregate_reg);
                }
            } else if let Some(dst) = dst {
                let tag_reg = self.next_reg();
                out.push_str(&format!(
                    "  {tag_reg} = extractvalue {} {aggregate_reg}, 0\n",
                    llvm_internal_type(&sig.ret, self.struct_layouts)?
                ));
                let payload_reg = self.next_reg();
                out.push_str(&format!(
                    "  {payload_reg} = extractvalue {} {aggregate_reg}, 1\n",
                    llvm_internal_type(&sig.ret, self.struct_layouts)?
                ));
                let extra_reg = self.next_reg();
                out.push_str(&format!(
                    "  {extra_reg} = extractvalue {} {aggregate_reg}, 2\n",
                    llvm_internal_type(&sig.ret, self.struct_layouts)?
                ));
                self.value_map.insert(
                    dst.clone(),
                    format!("i64 {tag_reg}, i64 {payload_reg}, i64 {extra_reg}"),
                );
            }
        } else {
            let reg = self.next_reg();
            out.push_str(&format!(
                "  {reg} = call {} @{}({})\n",
                llvm_scalar_type(&sig.ret)?,
                llvm_internal_symbol_name(callee, sig),
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
                let (ptr, len) = split_string_value(&rendered)?;
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
            IrType::Json => {
                let rendered = self.resolve_value(value_name, ty, out)?;
                self.declared_runtime
                    .insert("declare ptr @rune_rt_json_stringify(i64)\n".into());
                self.declared_runtime
                    .insert("declare i64 @rune_rt_last_string_len()\n".into());
                let ptr_reg = self.next_reg();
                out.push_str(&format!(
                    "  {ptr_reg} = call ptr @rune_rt_json_stringify(i64 {rendered})\n"
                ));
                let len_reg = self.next_reg();
                out.push_str(&format!(
                    "  {len_reg} = call i64 @rune_rt_last_string_len()\n"
                ));
                let decl = if stderr {
                    "declare void @rune_rt_eprint_str(ptr, i64)\n"
                } else {
                    "declare void @rune_rt_print_str(ptr, i64)\n"
                };
                self.declared_runtime.insert(decl.into());
                let call = if stderr {
                    format!("  call void @rune_rt_eprint_str(ptr {ptr_reg}, i64 {len_reg})\n")
                } else {
                    format!("  call void @rune_rt_print_str(ptr {ptr_reg}, i64 {len_reg})\n")
                };
                out.push_str(&call);
            }
            IrType::Bool => {
                let rendered = self.resolve_value(value_name, ty, out)?;
                let zext = self.next_reg();
                out.push_str(&format!("  {zext} = zext i1 {rendered} to i64\n"));
                let decl = if stderr {
                    "declare void @rune_rt_eprint_bool(i64)\n"
                } else {
                    "declare void @rune_rt_print_bool(i64)\n"
                };
                self.declared_runtime.insert(decl.into());
                let call = if stderr {
                    format!("  call void @rune_rt_eprint_bool(i64 {zext})\n")
                } else {
                    format!("  call void @rune_rt_print_bool(i64 {zext})\n")
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
            IrType::Struct(struct_name) => {
                let rendered = self.resolve_value(value_name, ty, out)?;
                let string_value =
                    self.render_default_struct_string_value(out, struct_name, &rendered)?;
                let (ptr, len) = split_string_value(&string_value)?;
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

    fn concat_string_values(
        &mut self,
        out: &mut String,
        left: String,
        right: String,
    ) -> Result<String, LlvmIrError> {
        let (left_ptr, left_len) = split_string_value(&left)?;
        let (right_ptr, right_len) = split_string_value(&right)?;
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
        Ok(format!("ptr {ptr_reg}, i64 {len_reg}"))
    }

    fn render_ir_value_as_string(
        &mut self,
        out: &mut String,
        ty: &IrType,
        value: String,
    ) -> Result<String, LlvmIrError> {
        match ty {
            IrType::String => {
                if value.starts_with("ptr ") && value.contains(", i64 ") {
                    Ok(value)
                } else {
                    let ptr_reg = self.next_reg();
                    out.push_str(&format!(
                        "  {ptr_reg} = extractvalue {} {value}, 0\n",
                        llvm_internal_type(&IrType::String, self.struct_layouts)?
                    ));
                    let len_reg = self.next_reg();
                    out.push_str(&format!(
                        "  {len_reg} = extractvalue {} {value}, 1\n",
                        llvm_internal_type(&IrType::String, self.struct_layouts)?
                    ));
                    Ok(format!("ptr {ptr_reg}, i64 {len_reg}"))
                }
            }
            IrType::Dynamic => {
                let (tag, payload, extra): (String, String, String) =
                    if value.starts_with("i64 ") && value.contains(", i64 ") {
                    let (tag, payload, extra) = split_dynamic_value(&value)?;
                    (tag.to_string(), payload.to_string(), extra.to_string())
                } else {
                    let tag_reg = self.next_reg();
                    out.push_str(&format!(
                        "  {tag_reg} = extractvalue {} {value}, 0\n",
                        llvm_internal_type(&IrType::Dynamic, self.struct_layouts)?
                    ));
                    let payload_reg = self.next_reg();
                    out.push_str(&format!(
                        "  {payload_reg} = extractvalue {} {value}, 1\n",
                        llvm_internal_type(&IrType::Dynamic, self.struct_layouts)?
                    ));
                    let extra_reg = self.next_reg();
                    out.push_str(&format!(
                        "  {extra_reg} = extractvalue {} {value}, 2\n",
                        llvm_internal_type(&IrType::Dynamic, self.struct_layouts)?
                    ));
                    (
                        format!("i64 {tag_reg}"),
                        format!("i64 {payload_reg}"),
                        format!("i64 {extra_reg}"),
                    )
                };
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
                Ok(format!("ptr {ptr_reg}, i64 {len_reg}"))
            }
            IrType::Bool => {
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
                Ok(format!("ptr {ptr_reg}, i64 {len_reg}"))
            }
            IrType::I32 => {
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
                Ok(format!("ptr {ptr_reg}, i64 {len_reg}"))
            }
            IrType::I64 => {
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
                Ok(format!("ptr {ptr_reg}, i64 {len_reg}"))
            }
            IrType::Json => {
                self.declared_runtime
                    .insert("declare ptr @rune_rt_json_to_string(i64)\n".into());
                self.declared_runtime
                    .insert("declare i64 @rune_rt_last_string_len()\n".into());
                let ptr_reg = self.next_reg();
                out.push_str(&format!(
                    "  {ptr_reg} = call ptr @rune_rt_json_to_string(i64 {value})\n"
                ));
                let len_reg = self.next_reg();
                out.push_str(&format!(
                    "  {len_reg} = call i64 @rune_rt_last_string_len()\n"
                ));
                Ok(format!("ptr {ptr_reg}, i64 {len_reg}"))
            }
            IrType::Struct(struct_name) => self.render_default_struct_string_value(out, struct_name, &value),
            IrType::Unit => Err(LlvmIrError {
                message: "unit values cannot be rendered as strings in the current LLVM IR backend".into(),
            }),
        }
    }

    fn render_default_struct_string_value(
        &mut self,
        out: &mut String,
        struct_name: &str,
        value: &str,
    ) -> Result<String, LlvmIrError> {
        if let Some(sig) = self.signatures.get(&struct_method_symbol(struct_name, "__str__")) {
            if sig.params.len() != 1 || sig.ret != IrType::String {
                return Err(LlvmIrError {
                    message: format!(
                        "`str` on `{struct_name}` requires `__str__`, when defined, to have signature `__str__(self) -> String` in the current LLVM IR backend"
                    ),
                });
            }
            let aggregate_reg = self.next_reg();
            let synthetic_name = struct_method_symbol(struct_name, "__str__");
            out.push_str(&format!(
                "  {aggregate_reg} = call {} @{}({} {})\n",
                llvm_internal_type(&IrType::String, self.struct_layouts)?,
                llvm_internal_symbol_name(&synthetic_name, sig),
                llvm_internal_type(&IrType::Struct(struct_name.to_string()), self.struct_layouts)?,
                value
            ));
            return self.render_ir_value_as_string(out, &IrType::String, aggregate_reg);
        }

        let layout = self
            .struct_layouts
            .get(struct_name)
            .cloned()
            .ok_or_else(|| LlvmIrError {
                message: format!("missing struct layout for `{struct_name}` in the current LLVM IR backend"),
            })?;
        let struct_ty = llvm_internal_type(&IrType::Struct(struct_name.to_string()), self.struct_layouts)?;
        let mut rendered = self.intern_string_ref(&format!("{struct_name}("));
        for (index, (field_name, field_ty)) in layout.iter().enumerate() {
            if index > 0 {
                let separator = self.intern_string_ref(", ");
                rendered = self.concat_string_values(out, rendered, separator)?;
            }
            let label = self.intern_string_ref(&format!("{field_name}="));
            rendered = self.concat_string_values(
                out,
                rendered,
                label,
            )?;
            let field_reg = self.next_reg();
            out.push_str(&format!(
                "  {field_reg} = extractvalue {struct_ty} {value}, {index}\n"
            ));
            let field_rendered = self.render_ir_value_as_string(out, field_ty, field_reg)?;
            rendered = self.concat_string_values(out, rendered, field_rendered)?;
        }
        let suffix = self.intern_string_ref(")");
        self.concat_string_values(out, rendered, suffix)
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
                IrType::Struct(_) => {
                    let reg = self.next_reg();
                    out.push_str(&format!(
                        "  {reg} = load {}, ptr %{}.addr\n",
                        llvm_internal_type(ty, self.struct_layouts)?,
                        name
                    ));
                    Ok(reg)
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
            IrType::Json => {
                let value = self.resolve_value(name, &IrType::Json, out)?;
                Ok(format!("i64 5, i64 {value}, i64 0"))
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
    struct_layouts: &HashMap<String, Vec<(String, IrType)>>,
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
            IrInst::UnaryBitwiseNot { dst, src } => {
                let ty = temp_types
                    .get(src)
                    .or_else(|| local_types.get(src))
                    .cloned()
                    .ok_or_else(|| LlvmIrError {
                        message: format!("missing type for bitwise-not operand `{src}`"),
                    })?;
                temp_types.insert(dst.clone(), ty);
            }
            IrInst::SetField { .. } => {}
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
                        .or_else(|| field_call_return_type(callee, inst, &temp_types, &local_types, struct_layouts))
                        .or_else(|| {
                            if struct_layouts.contains_key(callee) {
                                Some(IrType::Struct(callee.to_string()))
                            } else {
                                None
                            }
                        })
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

fn field_call_return_type(
    callee: &str,
    inst: &IrInst,
    temp_types: &HashMap<String, IrType>,
    local_types: &HashMap<String, IrType>,
    struct_layouts: &HashMap<String, Vec<(String, IrType)>>,
) -> Option<IrType> {
    let field_name = callee.strip_prefix("field.")?;
    let IrInst::Call { args, .. } = inst else {
        return None;
    };
    let base = args.iter().find(|arg| arg.name.as_deref() == Some("base"))?;
    let base_ty = temp_types
        .get(&base.value)
        .or_else(|| local_types.get(&base.value))
        .cloned()?;
    let IrType::Struct(struct_name) = base_ty else {
        return None;
    };
    struct_layouts
        .get(&struct_name)?
        .iter()
        .find(|(name, _)| name == field_name)
        .map(|(_, ty)| ty.clone())
}

fn builtin_return_type(name: &str) -> Option<IrType> {
    match name {
        "print" | "println" | "eprint" | "eprintln" | "flush" | "eflush" => Some(IrType::Unit),
        "rune_rt_string_len" => Some(IrType::I64),
        "rune_rt_string_upper"
        | "rune_rt_string_lower"
        | "rune_rt_string_replace"
        | "rune_rt_string_strip" => Some(IrType::String),
        "rune_rt_string_contains"
        | "rune_rt_string_starts_with"
        | "rune_rt_string_ends_with" => Some(IrType::Bool),
        "input"
        | "__rune_builtin_arduino_read_line"
        | "__rune_builtin_serial_read_line"
        | "__rune_builtin_serial_read_line_timeout" => {
            Some(IrType::String)
        }
        "panic" | "__rune_builtin_serial_flush" => Some(IrType::Unit),
        "str" | "repr" => Some(IrType::String),
        "int" => Some(IrType::I64),
        "__rune_builtin_gpio_mode_input"
        | "__rune_builtin_gpio_mode_output"
        | "__rune_builtin_gpio_mode_input_pullup"
        | "__rune_builtin_gpio_pwm_duty_max"
        | "__rune_builtin_gpio_analog_max" => Some(IrType::I64),
        "__rune_builtin_json_parse" => Some(IrType::Json),
        "__rune_builtin_json_stringify"
        | "__rune_builtin_env_arg"
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
        | "__rune_builtin_json_kind"
        | "__rune_builtin_json_to_string"
        | "__rune_builtin_system_platform"
        | "__rune_builtin_system_arch"
        | "__rune_builtin_system_target"
        | "__rune_builtin_system_board" => Some(IrType::String),
        "__rune_builtin_json_is_null"
        | "__rune_builtin_json_to_bool"
        | "__rune_builtin_gpio_digital_read"
        | "__rune_builtin_arduino_servo_attach"
        | "__rune_builtin_arduino_digital_read"
        | "__rune_builtin_time_has_wall_clock"
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
        | "__rune_builtin_serial_close" => Some(IrType::Unit),
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
        | "__rune_builtin_arduino_uart_available"
        | "__rune_builtin_serial_available"
        | "__rune_builtin_serial_read_byte"
        | "__rune_builtin_serial_read_byte_timeout"
        | "__rune_builtin_arduino_uart_peek_byte"
        | "__rune_builtin_serial_peek_byte"
        | "__rune_builtin_arduino_uart_read_byte" => Some(IrType::I64),
        "__rune_builtin_serial_open"
        | "__rune_builtin_serial_is_open"
        | "__rune_builtin_serial_write"
        | "__rune_builtin_serial_write_byte"
        | "__rune_builtin_serial_write_line" => Some(IrType::Bool),
        "__rune_builtin_system_pid"
        | "__rune_builtin_system_cpu_count"
        | "__rune_builtin_env_get_i32"
        | "__rune_builtin_env_arg_count"
        | "__rune_builtin_network_last_error_code"
        | "__rune_builtin_network_tcp_server_open"
        | "__rune_builtin_network_tcp_client_open" => Some(IrType::I32),
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
        | "__rune_builtin_fs_write_string"
        | "__rune_builtin_fs_append_string"
        | "__rune_builtin_fs_remove"
        | "__rune_builtin_fs_rename"
        | "__rune_builtin_fs_copy"
        | "__rune_builtin_fs_create_dir"
        | "__rune_builtin_fs_create_dir_all"
        | "__rune_builtin_fs_is_file"
        | "__rune_builtin_fs_is_dir"
        | "__rune_builtin_audio_bell" => Some(IrType::Bool),
        "__rune_builtin_fs_current_dir"
        | "__rune_builtin_fs_read_string"
        | "__rune_builtin_fs_canonicalize" => Some(IrType::String),
        "__rune_builtin_fs_file_size" => Some(IrType::I64),
        _ => None,
    }
}

fn type_ref_to_ir(ty: &TypeRef) -> Result<IrType, LlvmIrError> {
    match ty.name.as_str() {
        "bool" => Ok(IrType::Bool),
        "i32" => Ok(IrType::I32),
        "i64" => Ok(IrType::I64),
        "Json" => Ok(IrType::Json),
        "unit" => Ok(IrType::Unit),
        "dynamic" => Ok(IrType::Dynamic),
        "String" | "str" => Ok(IrType::String),
        other => Ok(IrType::Struct(other.to_string())),
    }
}

fn llvm_internal_symbol_name(name: &str, sig: &FunctionSig) -> String {
    if sig.is_extern || name == "main" {
        return name.to_string();
    }

    if crate::codegen::native_internal_symbol_name(name) != name {
        crate::codegen::native_internal_symbol_name(name)
    } else {
        name.to_string()
    }
}

fn llvm_scalar_type(ty: &IrType) -> Result<&'static str, LlvmIrError> {
    match ty {
        IrType::Bool => Ok("i1"),
        IrType::I32 => Ok("i32"),
        IrType::I64 => Ok("i64"),
        IrType::Json => Ok("i64"),
        IrType::Unit => Ok("void"),
        IrType::String | IrType::Dynamic | IrType::Struct(_) => Err(LlvmIrError {
            message: "non-scalar type is not yet supported in this LLVM IR position".into(),
        }),
    }
}

fn llvm_extern_type(
    ty: &IrType,
    struct_layouts: &HashMap<String, Vec<(String, IrType)>>,
) -> Result<String, LlvmIrError> {
    match ty {
        IrType::String => Ok("ptr".into()),
        IrType::Struct(_) => Err(LlvmIrError {
            message: "extern struct ABI is not yet supported in the current LLVM IR backend"
                .into(),
        }),
        _ => {
            let _ = struct_layouts;
            Ok(llvm_scalar_type(ty)?.into())
        }
    }
}

fn llvm_internal_type(
    ty: &IrType,
    struct_layouts: &HashMap<String, Vec<(String, IrType)>>,
) -> Result<String, LlvmIrError> {
    match ty {
        IrType::String => Ok("{ ptr, i64 }".into()),
        IrType::Dynamic => Ok("{ i64, i64, i64 }".into()),
        IrType::Struct(name) => llvm_struct_type(name, struct_layouts),
        _ => Ok(llvm_scalar_type(ty)?.into()),
    }
}

fn llvm_function_return_type(
    sig: &FunctionSig,
    struct_layouts: &HashMap<String, Vec<(String, IrType)>>,
) -> Result<String, LlvmIrError> {
    if matches!(sig.ret, IrType::String | IrType::Dynamic | IrType::Struct(_)) && !sig.is_extern {
        llvm_internal_type(&sig.ret, struct_layouts)
    } else {
        llvm_extern_type(&sig.ret, struct_layouts)
    }
}

fn llvm_struct_type(
    name: &str,
    struct_layouts: &HashMap<String, Vec<(String, IrType)>>,
) -> Result<String, LlvmIrError> {
    let layout = struct_layouts.get(name).ok_or_else(|| LlvmIrError {
        message: format!("missing struct layout for `{name}`"),
    })?;
    let mut parts = Vec::with_capacity(layout.len());
    for (_, ty) in layout {
        parts.push(llvm_internal_type(ty, struct_layouts)?);
    }
    Ok(format!("{{ {} }}", parts.join(", ")))
}

fn collect_struct_layouts(
    program: &Program,
) -> Result<HashMap<String, Vec<(String, IrType)>>, LlvmIrError> {
    let mut layouts = HashMap::new();
    for item in &program.items {
        let Item::Struct(decl) = item else {
            continue;
        };
        let mut fields = Vec::with_capacity(decl.fields.len());
        for field in &decl.fields {
            fields.push((field.name.clone(), type_ref_to_ir(&field.ty)?));
        }
        layouts.insert(decl.name.clone(), fields);
    }
    Ok(layouts)
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

fn resolve_llvm_call_args<'a>(
    callee: &str,
    params: &[(String, IrType)],
    args: &'a [IrArg],
) -> Result<Vec<&'a IrArg>, LlvmIrError> {
    let mut resolved: Vec<Option<&IrArg>> = vec![None; params.len()];
    let mut positional_index = 0usize;
    let mut saw_keyword = false;

    for arg in args {
        match &arg.name {
            None => {
                if saw_keyword {
                    return Err(LlvmIrError {
                        message: format!(
                            "positional arguments cannot appear after keyword arguments in `{callee}`"
                        ),
                    });
                }
                if positional_index >= params.len() {
                    return Err(LlvmIrError {
                        message: format!(
                            "call shape for `{callee}` is not yet supported by the current LLVM IR backend"
                        ),
                    });
                }
                resolved[positional_index] = Some(arg);
                positional_index += 1;
            }
            Some(name) => {
                saw_keyword = true;
                let Some(index) = params.iter().position(|(param_name, _)| param_name == name) else {
                    return Err(LlvmIrError {
                        message: format!("function `{callee}` has no parameter named `{name}`"),
                    });
                };
                if resolved[index].is_some() {
                    return Err(LlvmIrError {
                        message: format!("parameter `{name}` was provided more than once"),
                    });
                }
                resolved[index] = Some(arg);
            }
        }
    }

    if resolved.iter().any(|arg| arg.is_none()) {
        return Err(LlvmIrError {
            message: format!(
                "call shape for `{callee}` is not yet supported by the current LLVM IR backend"
            ),
        });
    }

    Ok(resolved
        .into_iter()
        .map(|arg| arg.expect("checked above"))
        .collect())
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

// === llvm_backend (merged from llvm_backend.rs) ===

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::driver::toolchain::find_packaged_llvm_tool_for_target;

/// Optimization level for LLVM-backed builds.
///
/// The default, `Full`, runs the complete LLVM optimization pipeline
/// (`default<O3>`). This maximizes both execution speed and binary quality:
/// the same passes that improve speed (inlining, dead-code elimination,
/// constant propagation) also reduce the amount of code that ends up in the
/// binary.
///
/// `MinSize` pushes further by switching LLVM to its size-first mode
/// (`default<Oz>`), which trades some runtime performance for the smallest
/// possible output. Use it when flash or disk space is the primary constraint.
///
/// AVR (`avr-atmega328p-arduino-uno`) always uses `default<Oz>` regardless of
/// this setting because the ATmega328P has only 32 KiB of flash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LlvmOptLevel {
    /// Full LLVM optimization pipeline. Uses `default<O3>` in opt and `-O3`
    /// in llc. Dead-code elimination, inlining, and constant propagation all
    /// run, which improves both speed and binary size simultaneously.
    /// This is the default for all host and cross-compiled targets.
    #[default]
    Full,
    /// Aggressive size-first optimization. Uses `default<Oz>` in opt and
    /// `-Oz` in llc. Produces the smallest possible binary at the cost of
    /// some runtime performance. Enabled with `--size` on the CLI.
    MinSize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlvmBackendError {
    pub message: String,
}

impl fmt::Display for LlvmBackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for LlvmBackendError {}

impl From<LlvmIrError> for LlvmBackendError {
    fn from(value: LlvmIrError) -> Self {
        Self {
            message: value.message,
        }
    }
}

pub fn emit_object_file(
    program: &Program,
    target_triple: &str,
    output_path: &Path,
    opt_level: LlvmOptLevel,
) -> Result<String, LlvmBackendError> {
    let llvm_ir = emit_llvm_ir(program)?;
    emit_object_file_from_ir(&llvm_ir, target_triple, output_path, opt_level)?;
    Ok(llvm_ir)
}

pub fn emit_assembly_file(
    program: &Program,
    target_triple: &str,
    output_path: &Path,
    opt_level: LlvmOptLevel,
) -> Result<String, LlvmBackendError> {
    let llvm_ir = emit_llvm_ir(program)?;
    emit_assembly_file_from_ir(&llvm_ir, target_triple, output_path, opt_level)?;
    Ok(llvm_ir)
}

pub fn emit_object_file_from_ir(
    llvm_ir: &str,
    target_triple: &str,
    output_path: &Path,
    opt_level: LlvmOptLevel,
) -> Result<(), LlvmBackendError> {
    emit_codegen_artifact_from_ir(llvm_ir, target_triple, output_path, "obj", opt_level)
}

pub fn emit_assembly_file_from_ir(
    llvm_ir: &str,
    target_triple: &str,
    output_path: &Path,
    opt_level: LlvmOptLevel,
) -> Result<(), LlvmBackendError> {
    emit_codegen_artifact_from_ir(llvm_ir, target_triple, output_path, "asm", opt_level)
}

fn emit_codegen_artifact_from_ir(
    llvm_ir: &str,
    target_triple: &str,
    output_path: &Path,
    filetype: &str,
    opt_level: LlvmOptLevel,
) -> Result<(), LlvmBackendError> {
    let temp_dir = create_temp_dir()?;
    let input_path = temp_dir.join("rune.ll");
    let optimized_path = temp_dir.join("rune.opt.ll");
    fs::write(&input_path, llvm_ir).map_err(|error| LlvmBackendError {
        message: format!("failed to write temporary LLVM IR file: {error}"),
    })?;

    if let Some(parent) = output_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|error| LlvmBackendError {
            message: format!("failed to create `{}`: {error}", parent.display()),
        })?;
    }

    let input_arg = input_path.to_string_lossy().into_owned();
    let optimized_arg = optimized_path.to_string_lossy().into_owned();
    let output_arg = output_path.to_string_lossy().into_owned();
    run_llvm_tool(
        target_triple,
        "opt",
        llvm_opt_args(target_triple, opt_level, &input_arg, &optimized_arg),
    )?;
    run_llvm_tool(
        target_triple,
        "llc",
        llvm_codegen_args(target_triple, opt_level, filetype, &optimized_arg, &output_arg),
    )?;

    let _ = fs::remove_file(input_path);
    let _ = fs::remove_file(optimized_path);
    let _ = fs::remove_dir(temp_dir);
    Ok(())
}

fn run_llvm_tool<S, I>(target_triple: &str, tool_name: &str, args: I) -> Result<(), LlvmBackendError>
where
    S: AsRef<str>,
    I: IntoIterator<Item = S>,
{
    let tool = find_packaged_llvm_tool_for_target(tool_name, target_triple).ok_or_else(|| LlvmBackendError {
        message: format!("packaged LLVM tool not found: {tool_name}"),
    })?;
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect::<Vec<_>>();
    let output = Command::new(&tool)
        .args(&args)
        .output()
        .map_err(|error| LlvmBackendError {
            message: format!("failed to run `{}`: {error}", tool.display()),
        })?;

    if output.status.success() {
        return Ok(());
    }

    Err(LlvmBackendError {
        message: format!(
            "{} failed with exit code {}{}",
            tool.display(),
            output.status.code().unwrap_or(-1),
            if output.stderr.is_empty() {
                String::new()
            } else {
                format!("\n\n{}", String::from_utf8_lossy(&output.stderr))
            }
        ),
    })
}

fn llvm_codegen_args(
    target_triple: &str,
    opt_level: LlvmOptLevel,
    filetype: &str,
    input_arg: &str,
    output_arg: &str,
) -> Vec<String> {
    let mut args = match target_triple {
        "avr-atmega328p-arduino-uno" => {
            vec!["-mtriple=avr".to_string(), "-mcpu=atmega328p".to_string()]
        }
        _ => vec![format!("-mtriple={target_triple}")],
    };
    args.push(format!("-filetype={filetype}"));
    // AVR: keep -O2 (opt already ran Oz; llc at O2 gives better AVR scheduling
    // than Oz which can pessimise instruction selection on this tiny ISA).
    // Others: match the opt pipeline level so llc does not undo the trade-off
    // chosen above.
    args.push(match target_triple {
        "avr-atmega328p-arduino-uno" => "-O2".to_string(),
        _ => match opt_level {
            LlvmOptLevel::Full => "-O3".to_string(),
            LlvmOptLevel::MinSize => "-Oz".to_string(),
        },
    });
    args.push(input_arg.to_string());
    args.push("-o".to_string());
    args.push(output_arg.to_string());
    args
}

fn llvm_opt_args(
    target_triple: &str,
    opt_level: LlvmOptLevel,
    input_arg: &str,
    output_arg: &str,
) -> Vec<String> {
    // AVR always uses Oz regardless of opt_level: the ATmega328P has 32 KiB
    // of flash so code size always takes priority over speed.
    // All other targets run the full optimization pipeline first.
    // Previously this passed only "verify" for non-AVR targets, which meant
    // zero optimization before llc. Now we run the chosen pipeline so that
    // dead-code elimination, inlining, constant propagation, and all other
    // passes fire before instruction selection.
    let pipeline = match target_triple {
        "avr-atmega328p-arduino-uno" => "default<Oz>,verify".to_string(),
        _ => match opt_level {
            LlvmOptLevel::Full => "default<O3>,verify".to_string(),
            LlvmOptLevel::MinSize => "default<Oz>,verify".to_string(),
        },
    };
    vec![
        "-S".to_string(),
        format!("-passes={pipeline}"),
        input_arg.to_string(),
        "-o".to_string(),
        output_arg.to_string(),
    ]
}

fn create_temp_dir() -> Result<PathBuf, LlvmBackendError> {
    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let unique = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = env::temp_dir().join(format!("rune-llvm-{}-{stamp}-{unique}", std::process::id()));
    fs::create_dir_all(&dir).map_err(|error| LlvmBackendError {
        message: format!(
            "failed to create temporary LLVM directory `{}`: {error}",
            dir.display()
        ),
    })?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::{LlvmOptLevel, llvm_codegen_args, llvm_opt_args};

    #[test]
    fn avr_codegen_args_use_exact_cpu_and_size_optimization() {
        let args = llvm_codegen_args(
            "avr-atmega328p-arduino-uno",
            LlvmOptLevel::Full,
            "obj",
            "input.ll",
            "output.o",
        );
        assert!(args.contains(&"-mtriple=avr".to_string()));
        assert!(args.contains(&"-mcpu=atmega328p".to_string()));
        assert!(args.contains(&"-O2".to_string()));
    }

    #[test]
    fn avr_opt_args_use_size_pipeline_regardless_of_opt_level() {
        let args_full = llvm_opt_args("avr-atmega328p-arduino-uno", LlvmOptLevel::Full, "input.ll", "output.ll");
        assert!(args_full.contains(&"-passes=default<Oz>,verify".to_string()));
        let args_min = llvm_opt_args("avr-atmega328p-arduino-uno", LlvmOptLevel::MinSize, "input.ll", "output.ll");
        assert!(args_min.contains(&"-passes=default<Oz>,verify".to_string()));
    }

    #[test]
    fn non_avr_full_opt_runs_real_optimization_pipeline() {
        let args = llvm_opt_args("x86_64-unknown-linux-gnu", LlvmOptLevel::Full, "input.ll", "output.ll");
        assert!(args.contains(&"-passes=default<O3>,verify".to_string()));
        let codegen_args = llvm_codegen_args(
            "x86_64-unknown-linux-gnu",
            LlvmOptLevel::Full,
            "obj",
            "input.ll",
            "output.o",
        );
        assert!(codegen_args.contains(&"-O3".to_string()));
    }

    #[test]
    fn non_avr_minsize_uses_oz_pipeline() {
        let args = llvm_opt_args("x86_64-unknown-linux-gnu", LlvmOptLevel::MinSize, "input.ll", "output.ll");
        assert!(args.contains(&"-passes=default<Oz>,verify".to_string()));
        let codegen_args = llvm_codegen_args(
            "x86_64-unknown-linux-gnu",
            LlvmOptLevel::MinSize,
            "obj",
            "input.ll",
            "output.o",
        );
        assert!(codegen_args.contains(&"-Oz".to_string()));
    }
}

// === avr_cbe_opt (merged from avr_cbe_opt.rs) ===

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArduinoUnoEntrypointKind {
    Main,
    SetupLoop,
}

pub fn rewrite_arduino_uno_cbe_llvm_ir(
    llvm_ir: &str,
    entrypoint: ArduinoUnoEntrypointKind,
) -> String {
    let mut rename_map = HashMap::new();
    for line in llvm_ir.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("define ") {
            continue;
        }
        let Some(at_index) = trimmed.find('@') else {
            continue;
        };
        let name_start = at_index + 1;
        let name_end = trimmed[name_start..]
            .find('(')
            .map(|index| name_start + index)
            .unwrap_or(trimmed.len());
        let name = &trimmed[name_start..name_end];
        if name.starts_with("rune_rt_") {
            continue;
        }
        if matches!(
            (entrypoint, name),
            (ArduinoUnoEntrypointKind::Main, "main")
                | (ArduinoUnoEntrypointKind::SetupLoop, "setup")
                | (ArduinoUnoEntrypointKind::SetupLoop, "loop")
        ) {
            continue;
        }
        rename_map.insert(name.to_string(), format!("rune_cbe_{name}"));
    }

    rewrite_llvm_global_identifiers(llvm_ir, &rename_map)
}

pub fn rewrite_arduino_uno_cbe_source(
    c_source: &str,
    entrypoint: ArduinoUnoEntrypointKind,
) -> String {
    let renamed = match entrypoint {
        ArduinoUnoEntrypointKind::Main => {
            c_source.replace("int main(void)", "int rune_entry_main(void)")
        }
        ArduinoUnoEntrypointKind::SetupLoop => c_source
            .replace("void setup(void)", "void rune_entry_setup(void)")
            .replace("void loop(void)", "void rune_entry_loop(void)"),
    };
    internalize_rune_cbe_c_functions(&renamed)
}

fn rewrite_llvm_global_identifiers(source: &str, rename_map: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(source.len());
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'@' {
            let start = index + 1;
            let mut end = start;
            while end < bytes.len() {
                let ch = bytes[end];
                if ch.is_ascii_alphanumeric() || ch == b'_' || ch == b'.' {
                    end += 1;
                } else {
                    break;
                }
            }
            if end > start {
                let name = &source[start..end];
                if let Some(replacement) = rename_map.get(name) {
                    out.push('@');
                    out.push_str(replacement);
                    index = end;
                    continue;
                }
            }
        }
        out.push(bytes[index] as char);
        index += 1;
    }
    out
}

fn internalize_rune_cbe_c_functions(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    for line in source.lines() {
        let trimmed = line.trim_start();
        let function_name_start = trimmed.find(" rune_cbe_");
        let Some(function_name_start) = function_name_start else {
            out.push_str(line);
            out.push('\n');
            continue;
        };
        let function_name_start = function_name_start + 1;
        let Some(paren_index) = trimmed[function_name_start..]
            .find('(')
            .map(|index| function_name_start + index)
        else {
            out.push_str(line);
            out.push('\n');
            continue;
        };
        let is_function_decl_or_def = trimmed.ends_with('{')
            || trimmed.ends_with(';')
            || trimmed.ends_with(" ;")
            || trimmed.contains("__FUNCTIONALIGN__");
        if trimmed.starts_with("static ")
            || trimmed.starts_with("/*")
            || trimmed.contains('=')
            || !is_function_decl_or_def
            || !trimmed[function_name_start..paren_index].starts_with("rune_cbe_")
        {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let indent_len = line.len() - trimmed.len();
        out.push_str(&line[..indent_len]);
        out.push_str("static ");
        out.push_str(trimmed);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests_avr {
    use super::{
        rewrite_arduino_uno_cbe_llvm_ir, rewrite_arduino_uno_cbe_source,
        ArduinoUnoEntrypointKind,
    };

    #[test]
    fn rewrites_non_runtime_functions_for_main_entry() {
        let llvm_ir = "\
define i64 @main() {\n\
  ret i64 0\n\
}\n\
define i64 @helper() {\n\
  ret i64 1\n\
}\n\
define void @rune_rt_fail(i32 %code) {\n\
  ret void\n\
}\n";
        let rewritten = rewrite_arduino_uno_cbe_llvm_ir(llvm_ir, ArduinoUnoEntrypointKind::Main);
        assert!(rewritten.contains("@main()"));
        assert!(rewritten.contains("@rune_cbe_helper()"));
        assert!(rewritten.contains("@rune_rt_fail(i32 %code)"));
    }

    #[test]
    fn preserves_setup_loop_entrypoints() {
        let llvm_ir = "\
define void @setup() {\n\
  ret void\n\
}\n\
define void @loop() {\n\
  ret void\n\
}\n\
define i64 @helper() {\n\
  ret i64 1\n\
}\n";
        let rewritten =
            rewrite_arduino_uno_cbe_llvm_ir(llvm_ir, ArduinoUnoEntrypointKind::SetupLoop);
        assert!(rewritten.contains("@setup()"));
        assert!(rewritten.contains("@loop()"));
        assert!(rewritten.contains("@rune_cbe_helper()"));
    }

    #[test]
    fn internalizes_rune_cbe_c_helpers() {
        let c_source = "\
void rune_cbe_helper(void) __FUNCTIONALIGN__(1) ;\n\
\n\
void rune_cbe_helper(void) {\n\
}\n\
\n\
int main(void) {\n\
  return 0;\n\
}\n";
        let rewritten = rewrite_arduino_uno_cbe_source(c_source, ArduinoUnoEntrypointKind::Main);
        assert!(rewritten.contains("static void rune_cbe_helper(void) __FUNCTIONALIGN__(1) ;"));
        assert!(rewritten.contains("static void rune_cbe_helper(void) {"));
        assert!(rewritten.contains("int rune_entry_main(void) {"));
    }
}
