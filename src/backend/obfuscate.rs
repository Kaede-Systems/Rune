//! LLVM IR obfuscation pipeline.
//!
//! Transforms are applied cumulatively: level N applies all transforms for
//! levels 1 through N.
//!
//! Level 1 – mark user functions private (removes exported symbols)
//! Level 2 – rename user-defined symbols to random hex names
//! Level 3 – XOR-encrypt string literals; inject global-ctor decrypt function
//! Level 4 – inject fake global byte arrays (junk data)
//! Level 5 – inject unreachable dead functions (confuse disassembly)
//! Level 6 – inject more dead functions (denser noise)
//! Level 7 – inject fake global-ctor init (extra startup obfuscation)
//! Level 8 – insert volatile junk loads into live function entries
//! Level 9 – control-flow flattening (switch-based dispatch)
//! Level 10 – second rename pass + denser dead-function injection

use std::collections::HashMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// Deterministic PRNG (Xorshift64)
// ---------------------------------------------------------------------------

struct Xorshift64 {
    state: u64,
}

impl Xorshift64 {
    fn new(seed: u64) -> Self {
        Xorshift64 {
            state: if seed == 0 { 0xdeadbeef_cafebabe } else { seed },
        }
    }

    fn next(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }

    fn next_byte(&mut self) -> u8 {
        (self.next() & 0xFF) as u8
    }

    fn next_nonzero_byte(&mut self) -> u8 {
        loop {
            let b = self.next_byte();
            if b != 0 {
                return b;
            }
        }
    }

    fn next_hex(&mut self, nibbles: usize) -> String {
        (0..nibbles / 2 + 1)
            .map(|_| format!("{:02x}", self.next_byte()))
            .collect::<String>()
            .chars()
            .take(nibbles)
            .collect()
    }

    fn next_i64(&mut self) -> i64 {
        self.next() as i64
    }
}

// ---------------------------------------------------------------------------
// Context that threads state through the pipeline
// ---------------------------------------------------------------------------

struct ObfCtx {
    rng: Xorshift64,
    /// Symbols that must survive DCE (added to @llvm.used at the end).
    used_symbols: Vec<String>,
}

impl ObfCtx {
    fn new(seed: u64) -> Self {
        ObfCtx {
            rng: Xorshift64::new(seed),
            used_symbols: Vec::new(),
        }
    }

    fn fresh_name(&mut self, prefix: &str) -> String {
        let hex = self.rng.next_hex(12);
        format!("{prefix}{hex}")
    }
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Apply all obfuscation transforms for the given level (1–10) to `ir`.
/// `seed` drives the PRNG; use a time-based seed for non-determinism.
pub fn obfuscate_llvm_ir(ir: String, level: u8, seed: u64) -> String {
    assert!((1..=10).contains(&level), "obfuscate level must be 1–10");
    let mut ctx = ObfCtx::new(seed);
    let mut ir = ir;

    if level >= 1 {
        ir = mark_functions_private(ir);
    }
    if level >= 2 {
        ir = rename_user_symbols(ir, &mut ctx);
    }
    if level >= 3 {
        ir = encrypt_strings(ir, &mut ctx);
    }
    if level >= 4 {
        ir = inject_junk_globals(ir, &mut ctx, 8);
    }
    if level >= 5 {
        ir = inject_dead_functions(ir, &mut ctx, 5);
    }
    if level >= 6 {
        ir = inject_dead_functions(ir, &mut ctx, 12);
    }
    if level >= 7 {
        ir = inject_fake_ctor(ir, &mut ctx);
    }
    if level >= 8 {
        ir = inject_volatile_junk_loads(ir, &mut ctx);
    }
    if level >= 9 {
        ir = flatten_control_flow(ir, &mut ctx);
    }
    if level >= 10 {
        // Second rename pass covers injected symbols; add another layer of dead code.
        ir = rename_user_symbols(ir, &mut ctx);
        ir = inject_dead_functions(ir, &mut ctx, 20);
    }

    // Append @llvm.used so injected dead code survives DCE.
    if !ctx.used_symbols.is_empty() {
        ir = append_llvm_used(ir, &ctx.used_symbols);
    }

    ir
}

/// Run `llvm-strip --strip-all` on the final linked binary.
/// Removes all symbol table entries and section names.
pub fn strip_binary(path: &Path) -> Result<(), String> {
    let strip = crate::driver::toolchain::find_packaged_llvm_tool("llvm-strip")
        .ok_or_else(|| "packaged llvm-strip not found; binary will not be stripped".to_string())?;

    let output = std::process::Command::new(&strip)
        .arg("--strip-all")
        .arg(path)
        .output()
        .map_err(|e| format!("failed to run llvm-strip: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "llvm-strip failed ({}): {stderr}",
            output.status.code().unwrap_or(-1)
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Level 1 – Mark user functions private
// ---------------------------------------------------------------------------

fn mark_functions_private(ir: String) -> String {
    let mut out = String::with_capacity(ir.len() + 64);
    for line in ir.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("define ")
            && !trimmed.contains(" @main(")
            && !trimmed.contains(" @rune_entry_main(")
            && !trimmed.contains(" private ")
            && !trimmed.contains(" internal ")
        {
            out.push_str(&line.replacen("define ", "define private ", 1));
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

// ---------------------------------------------------------------------------
// Level 2 – Rename user-defined symbols
// ---------------------------------------------------------------------------

fn rename_user_symbols(ir: String, ctx: &mut ObfCtx) -> String {
    // Collect all defined (not declared) function names.
    let mut name_map: HashMap<String, String> = HashMap::new();
    for line in ir.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("define ") {
            if let Some(at) = trimmed.find('@') {
                let rest = &trimmed[at + 1..];
                let end = rest.find('(').unwrap_or(rest.len());
                let name = rest[..end].trim().to_string();
                if should_rename_symbol(&name) {
                    name_map
                        .entry(name)
                        .or_insert_with(|| format!("_F{}", ctx.rng.next_hex(12)));
                }
            }
        }
    }

    // Replace every @old_name with @new_name, respecting word boundaries.
    let mut result = ir;
    for (old, new) in &name_map {
        result = replace_symbol(&result, old, new);
    }
    result
}

fn should_rename_symbol(name: &str) -> bool {
    name != "main"
        && !name.starts_with("rune_")
        && !name.starts_with("__rune_")
        && !name.starts_with("llvm.")
        && !name.starts_with("__ob_")
        && !name.starts_with("_F")
}

/// Replace `@old` with `@new` only when followed by a non-ident character.
fn replace_symbol(ir: &str, old: &str, new: &str) -> String {
    let search = format!("@{old}");
    let replacement = format!("@{new}");
    let mut result = String::with_capacity(ir.len());
    let mut pos = 0;
    while pos < ir.len() {
        if let Some(rel) = ir[pos..].find(&search) {
            let abs = pos + rel;
            let after_end = abs + search.len();
            let next_char = ir[after_end..].chars().next();
            let is_boundary = next_char.map_or(true, |c| !c.is_alphanumeric() && c != '_');
            result.push_str(&ir[pos..abs]);
            if is_boundary {
                result.push_str(&replacement);
            } else {
                result.push_str(&search);
            }
            pos = after_end;
        } else {
            result.push_str(&ir[pos..]);
            break;
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Level 3 – XOR string encryption
// ---------------------------------------------------------------------------

fn encrypt_strings(ir: String, ctx: &mut ObfCtx) -> String {
    // Collect all string globals.
    struct StrEntry {
        global_name: String,
        len: usize,
        key: u8,
    }

    let mut entries: Vec<StrEntry> = Vec::new();
    let mut modified_ir = String::with_capacity(ir.len() * 2);

    for line in ir.lines() {
        let trimmed = line.trim_start();
        // Pattern: @.str.N = private unnamed_addr constant [M x i8] c"..."
        if let Some(entry) = try_parse_string_global(trimmed) {
            let key = ctx.rng.next_nonzero_byte();
            let encrypted: Vec<u8> = entry.bytes.iter().map(|&b| b ^ key).collect();
            let enc_lit = format_i8_array(&encrypted);
            // Change constant → global (must be writable for in-place decryption).
            modified_ir.push_str(&format!(
                "@{} = private unnamed_addr global [{} x i8] {}\n",
                entry.name, entry.len, enc_lit
            ));
            entries.push(StrEntry {
                global_name: entry.name,
                len: entry.len,
                key,
            });
        } else {
            modified_ir.push_str(line);
            modified_ir.push('\n');
        }
    }

    if entries.is_empty() {
        return modified_ir;
    }

    // Emit one decrypt-loop function per string.
    let mut decrypt_fn_names: Vec<String> = Vec::new();
    for entry in &entries {
        let fn_name = format!("__ob_dec_{}", ctx.rng.next_hex(10));
        let loop_label = format!("__ob_lp_{}", ctx.rng.next_hex(6));
        let exit_label = format!("__ob_ex_{}", ctx.rng.next_hex(6));
        let len = entry.len as u64;
        let key = entry.key;
        let global = &entry.global_name;
        modified_ir.push_str(&format!(
            "\ndefine private void @{fn_name}() {{\n\
             entry:\n\
               br label %{loop_label}\n\
             {loop_label}:\n\
               %_ob_i_{fn_name} = phi i64 [ 0, %entry ], [ %_ob_in_{fn_name}, %{loop_label} ]\n\
               %_ob_ptr_{fn_name} = getelementptr [{len} x i8], ptr @{global}, i64 0, i64 %_ob_i_{fn_name}\n\
               %_ob_b_{fn_name} = load i8, ptr %_ob_ptr_{fn_name}\n\
               %_ob_d_{fn_name} = xor i8 %_ob_b_{fn_name}, {key}\n\
               store i8 %_ob_d_{fn_name}, ptr %_ob_ptr_{fn_name}\n\
               %_ob_in_{fn_name} = add i64 %_ob_i_{fn_name}, 1\n\
               %_ob_done_{fn_name} = icmp eq i64 %_ob_in_{fn_name}, {len}\n\
               br i1 %_ob_done_{fn_name}, label %{exit_label}, label %{loop_label}\n\
             {exit_label}:\n\
               ret void\n\
             }}\n"
        ));
        decrypt_fn_names.push(fn_name);
    }

    // Emit master decrypt-all function.
    let master_name = format!("__ob_decrypt_all_{}", ctx.rng.next_hex(8));
    modified_ir.push_str(&format!("\ndefine private void @{master_name}() {{\nentry:\n"));
    for fn_name in &decrypt_fn_names {
        modified_ir.push_str(&format!("  call void @{fn_name}()\n"));
    }
    modified_ir.push_str("  ret void\n}\n");

    // Register with @llvm.global_ctors (priority 65535 = late startup).
    // If one already exists, we'll append ours by using appending linkage.
    let existing_ctors = if modified_ir.contains("@llvm.global_ctors") {
        // Extend existing: the appending global allows duplicates in LTO.
        // Here we just add a second definition – LLVM merges appending globals.
        true
    } else {
        false
    };
    if existing_ctors {
        modified_ir.push_str(&format!(
            "\n@llvm.global_ctors = appending global \
             [1 x {{ i32, ptr, ptr }}] \
             [{{ i32, ptr, ptr }} {{ i32 65534, ptr @{master_name}, ptr null }}]\n"
        ));
    } else {
        modified_ir.push_str(&format!(
            "\n@llvm.global_ctors = appending global \
             [1 x {{ i32, ptr, ptr }}] \
             [{{ i32, ptr, ptr }} {{ i32 65535, ptr @{master_name}, ptr null }}]\n"
        ));
    }

    modified_ir
}

struct ParsedStringGlobal {
    name: String,
    len: usize,
    bytes: Vec<u8>,
}

fn try_parse_string_global(line: &str) -> Option<ParsedStringGlobal> {
    // Expected (trimmed): @.str.N = private unnamed_addr constant [M x i8] c"..."
    if !line.starts_with('@') {
        return None;
    }
    let rest = &line[1..];
    let eq = rest.find(" = ")?;
    let name = rest[..eq].to_string();

    let after_eq = &rest[eq + 3..];
    // Must be a constant string global.
    if !after_eq.contains("constant [") || !after_eq.contains("x i8]") {
        return None;
    }
    // Parse [M x i8]
    let bracket = after_eq.find("[")?;
    let inner = &after_eq[bracket + 1..];
    let x_pos = inner.find(" x i8]")?;
    let len: usize = inner[..x_pos].trim().parse().ok()?;

    // Parse c"..."
    let c_quote = after_eq.find("c\"")?;
    let content_start = c_quote + 2;
    let content = &after_eq[content_start..];
    let close_quote = content.rfind('"')?;
    let raw = &content[..close_quote];

    let bytes = parse_llvm_string_literal(raw);
    if bytes.len() != len {
        return None;
    }

    Some(ParsedStringGlobal { name, len, bytes })
}

/// Parse LLVM `c"..."` content (without surrounding quotes) into raw bytes.
fn parse_llvm_string_literal(s: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 2 < chars.len() {
            let hex = format!("{}{}", chars[i + 1], chars[i + 2]);
            if let Ok(b) = u8::from_str_radix(&hex, 16) {
                bytes.push(b);
                i += 3;
                continue;
            }
        }
        bytes.push(chars[i] as u8);
        i += 1;
    }
    bytes
}

/// Format a byte slice as an LLVM i8 array literal: `[i8 X, i8 Y, ...]`
fn format_i8_array(bytes: &[u8]) -> String {
    let elements: Vec<String> = bytes.iter().map(|&b| format!("i8 {b}")).collect();
    format!("[{}]", elements.join(", "))
}

// ---------------------------------------------------------------------------
// Level 4 – Inject fake global byte arrays
// ---------------------------------------------------------------------------

fn inject_junk_globals(ir: String, ctx: &mut ObfCtx, count: usize) -> String {
    let mut out = ir;
    let mut injected: Vec<String> = Vec::new();

    for _ in 0..count {
        let name = ctx.fresh_name("__ob_g_");
        let size = 16 + (ctx.rng.next() % 48) as usize;
        let bytes: Vec<u8> = (0..size).map(|_| ctx.rng.next_byte()).collect();
        let arr = format_i8_array(&bytes);
        out.push_str(&format!(
            "\n@{name} = private global [{size} x i8] {arr}\n"
        ));
        injected.push(name);
    }

    ctx.used_symbols.extend(injected);
    out
}

// ---------------------------------------------------------------------------
// Level 5-6 – Inject unreachable dead functions
// ---------------------------------------------------------------------------

fn inject_dead_functions(ir: String, ctx: &mut ObfCtx, count: usize) -> String {
    let mut out = ir;

    for _ in 0..count {
        let name = ctx.fresh_name("__ob_fn_");
        // Build a function body with several fake arithmetic operations.
        let a = ctx.rng.next_i64();
        let b = ctx.rng.next_i64();
        let c = ctx.rng.next_i64();
        let d = ctx.rng.next_i64();
        out.push_str(&format!(
            "\ndefine private i64 @{name}(i64 %_a, i64 %_b) {{\n\
             entry:\n\
               %_r0 = xor i64 %_a, {a}\n\
               %_r1 = add i64 %_r0, %_b\n\
               %_r2 = mul i64 %_r1, {b}\n\
               %_r3 = xor i64 %_r2, {c}\n\
               %_r4 = add i64 %_r3, {d}\n\
               %_r5 = and i64 %_r4, 9223372036854775807\n\
               ret i64 %_r5\n\
             }}\n"
        ));
        ctx.used_symbols.push(name);
    }

    out
}

// ---------------------------------------------------------------------------
// Level 7 – Inject fake global-constructor init function
// ---------------------------------------------------------------------------

fn inject_fake_ctor(ir: String, ctx: &mut ObfCtx) -> String {
    let mut out = ir;
    let flag_name = ctx.fresh_name("__ob_flag_");
    let fn_name = ctx.fresh_name("__ob_init_");

    let v0 = ctx.rng.next_byte();
    let v1 = ctx.rng.next_byte();
    let v2 = ctx.rng.next_byte();
    let result = v0 ^ v1 ^ v2;

    out.push_str(&format!(
        "\n@{flag_name} = private global i8 0\n\
         \ndefine private void @{fn_name}() {{\n\
         entry:\n\
           %_ob_v0 = xor i8 {v0}, {v1}\n\
           %_ob_v1 = xor i8 %_ob_v0, {v2}\n\
           store i8 %_ob_v1, ptr @{flag_name}\n\
           ret void\n\
         }}\n\
         \n@llvm.global_ctors = appending global \
         [1 x {{ i32, ptr, ptr }}] \
         [{{ i32, ptr, ptr }} {{ i32 65533, ptr @{fn_name}, ptr null }}]\n"
    ));
    // The stored value is `result` (always the same), making this a no-op semantically,
    // but the code looks like important initialization to a decompiler.
    let _ = result;
    ctx.used_symbols.push(flag_name);

    out
}

// ---------------------------------------------------------------------------
// Level 8 – Insert volatile junk loads at function entries
// ---------------------------------------------------------------------------

fn inject_volatile_junk_loads(ir: String, ctx: &mut ObfCtx) -> String {
    if ctx.used_symbols.is_empty() {
        // No junk globals to load from; nothing to do.
        return ir;
    }

    // Pick a random junk global to load from.
    let global_idx = (ctx.rng.next() as usize) % ctx.used_symbols.len();
    let global = ctx.used_symbols[global_idx].clone();

    let mut out = String::with_capacity(ir.len() + 512);
    let mut in_function = false;
    let mut entry_injected = false;
    let mut fn_counter: u64 = 0;

    for line in ir.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("define ") && trimmed.ends_with('{') {
            in_function = true;
            entry_injected = false;
            fn_counter += 1;
            out.push_str(line);
            out.push('\n');
            continue;
        }

        if in_function && !entry_injected && trimmed == "entry:" {
            out.push_str(line);
            out.push('\n');
            // Inject volatile load from junk global.
            let uid = fn_counter;
            out.push_str(&format!(
                "  %_ob_jl_{uid} = load volatile i8, ptr @{global}\n"
            ));
            entry_injected = true;
            continue;
        }

        if in_function && trimmed == "}" {
            in_function = false;
        }

        out.push_str(line);
        out.push('\n');
    }

    out
}

// ---------------------------------------------------------------------------
// Level 9 – Control-flow flattening
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum BBTerm {
    Ret { line: String },
    Br { target: String },
    CondBr { cond: String, true_tgt: String, false_tgt: String },
    Other(String),
}

#[derive(Debug)]
struct BasicBlock {
    label: String,
    body: Vec<String>, // non-terminator instructions
    term: BBTerm,
}

fn flatten_control_flow(ir: String, ctx: &mut ObfCtx) -> String {
    let mut out = String::with_capacity(ir.len() * 2);
    let mut i = 0;
    let lines: Vec<&str> = ir.lines().collect();

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        // Detect function definitions.
        if trimmed.starts_with("define ") && trimmed.ends_with('{') {
            // Parse signature line and function body.
            let sig_line = line.to_string();
            let mut body_lines: Vec<String> = Vec::new();
            i += 1;
            let mut depth = 1usize;
            while i < lines.len() {
                let l = lines[i];
                if l.trim() == "{" {
                    depth += 1;
                }
                if l.trim() == "}" {
                    depth -= 1;
                    if depth == 0 {
                        i += 1;
                        break;
                    }
                }
                body_lines.push(l.to_string());
                i += 1;
            }

            let flattened = try_flatten_function(&sig_line, &body_lines, ctx);
            out.push_str(&flattened);
            continue;
        }

        out.push_str(line);
        out.push('\n');
        i += 1;
    }

    out
}

/// Attempt to flatten a function. Returns original if not eligible.
fn try_flatten_function(sig: &str, body: &[String], ctx: &mut ObfCtx) -> String {
    // Only flatten functions with no switch instructions (our match expressions).
    let has_switch = body.iter().any(|l| l.trim().starts_with("switch "));
    if has_switch {
        return emit_function_verbatim(sig, body);
    }

    let blocks = parse_basic_blocks(body);
    if blocks.len() < 2 {
        // Single-block functions: not worth flattening.
        return emit_function_verbatim(sig, body);
    }

    // Ensure all terminators are types we can rewrite.
    let all_rewritable = blocks.iter().all(|bb| {
        matches!(
            &bb.term,
            BBTerm::Ret { .. } | BBTerm::Br { .. } | BBTerm::CondBr { .. }
        )
    });
    if !all_rewritable {
        return emit_function_verbatim(sig, body);
    }

    // Extract return type from signature to know if we need a return slot.
    let ret_type = extract_return_type(sig);

    let prefix = format!("_ob_{:x}_", ctx.rng.next() & 0xFFFF);
    let state_ptr = format!("%{prefix}state");
    let dispatch_label = format!("{prefix}dispatch");
    let exit_label = format!("{prefix}exit");
    let ret_slot = format!("%{prefix}ret");

    // Map label → block index.
    let mut label_to_idx: HashMap<String, u32> = HashMap::new();
    for (idx, bb) in blocks.iter().enumerate() {
        label_to_idx.insert(bb.label.clone(), idx as u32);
    }

    let needs_ret_slot = ret_type != "void";
    let entry_idx: u32 = 0;
    let exit_sentinel: u32 = blocks.len() as u32; // one past last valid block

    let mut out = String::with_capacity(body.iter().map(|l| l.len()).sum::<usize>() * 3);
    // Signature with noinline optnone to prevent LLVM from undoing the flattening.
    let sig_with_attrs = sig.replacen("define ", "define noinline ", 1);
    out.push_str(&sig_with_attrs);
    out.push('\n');

    // New entry block: allocate state + optional ret slot, then jump to dispatch.
    out.push_str("entry:\n");
    out.push_str(&format!("  {state_ptr} = alloca i32\n"));
    if needs_ret_slot {
        out.push_str(&format!("  {ret_slot} = alloca {ret_type}\n"));
    }
    // Copy alloca instructions from original entry block.
    let entry_block = &blocks[0];
    for instr in &entry_block.body {
        let t = instr.trim();
        if t.starts_with('%') && t.contains(" = alloca ") {
            out.push_str(instr);
            out.push('\n');
        }
    }
    out.push_str(&format!("  store i32 {entry_idx}, ptr {state_ptr}\n"));
    out.push_str(&format!("  br label %{dispatch_label}\n"));

    // Dispatch block: load state, switch to blocks.
    let state_val = format!("%{prefix}sv");
    out.push_str(&format!("{dispatch_label}:\n"));
    out.push_str(&format!(
        "  {state_val} = load i32, ptr {state_ptr}\n"
    ));
    // Build switch table.
    let cases: String = blocks
        .iter()
        .enumerate()
        .map(|(idx, _bb)| format!("i32 {idx}, label %{prefix}bb_{idx}", idx = idx, prefix = prefix))
        .collect::<Vec<_>>()
        .join(" ");
    out.push_str(&format!(
        "  switch i32 {state_val}, label %{exit_label} [{cases}]\n"
    ));

    // Emit each original block under a new label.
    for (idx, bb) in blocks.iter().enumerate() {
        out.push_str(&format!("{prefix}bb_{idx}:\n"));
        // Body instructions (skip allocas from entry since they're hoisted).
        for instr in &bb.body {
            let t = instr.trim();
            if idx == 0 && t.starts_with('%') && t.contains(" = alloca ") {
                continue; // already emitted in entry
            }
            out.push_str(instr);
            out.push('\n');
        }
        // Rewrite terminator.
        let next_idx = format!("%{prefix}nx_{idx}");
        match &bb.term {
            BBTerm::Ret { .. } => {
                if needs_ret_slot {
                    // Extract ret value from terminator line.
                    if let BBTerm::Ret { line } = &bb.term {
                        let val = extract_ret_value(line, &ret_type);
                        if let Some(v) = val {
                            out.push_str(&format!("  store {ret_type} {v}, ptr {ret_slot}\n"));
                        }
                    }
                }
                out.push_str(&format!(
                    "  store i32 {exit_sentinel}, ptr {state_ptr}\n\
                       br label %{exit_label}\n"
                ));
            }
            BBTerm::Br { target } => {
                let tgt_idx = label_to_idx.get(target).copied().unwrap_or(exit_sentinel);
                out.push_str(&format!(
                    "  store i32 {tgt_idx}, ptr {state_ptr}\n\
                       br label %{dispatch_label}\n"
                ));
            }
            BBTerm::CondBr { cond, true_tgt, false_tgt } => {
                let t_idx = label_to_idx.get(true_tgt).copied().unwrap_or(exit_sentinel);
                let f_idx = label_to_idx.get(false_tgt).copied().unwrap_or(exit_sentinel);
                out.push_str(&format!(
                    "  {next_idx} = select i1 {cond}, i32 {t_idx}, i32 {f_idx}\n\
                       store i32 {next_idx}, ptr {state_ptr}\n\
                       br label %{dispatch_label}\n"
                ));
            }
            BBTerm::Other(line) => {
                // Should not happen (we checked above), but emit verbatim if so.
                out.push_str(line);
                out.push('\n');
            }
        }
    }

    // Exit block.
    out.push_str(&format!("{exit_label}:\n"));
    if needs_ret_slot {
        out.push_str(&format!(
            "  %{prefix}rv = load {ret_type}, ptr {ret_slot}\n\
               ret {ret_type} %{prefix}rv\n"
        ));
    } else {
        out.push_str("  ret void\n");
    }

    out.push_str("}\n");
    out
}

fn emit_function_verbatim(sig: &str, body: &[String]) -> String {
    let mut out = String::new();
    out.push_str(sig);
    out.push('\n');
    for line in body {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("}\n");
    out
}

/// Parse LLVM IR function body into basic blocks.
fn parse_basic_blocks(body: &[String]) -> Vec<BasicBlock> {
    let mut blocks: Vec<BasicBlock> = Vec::new();
    let mut current_label = "entry".to_string();
    let mut current_body: Vec<String> = Vec::new();

    for line in body {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // A basic block label: line like "label_name:" with nothing after the colon.
        if let Some(label) = trimmed.strip_suffix(':') {
            if !label.contains(' ') && !label.starts_with('%') {
                // Finalize previous block if any instructions exist.
                if let Some(term_line) = current_body.pop() {
                    let term = parse_terminator(&term_line);
                    blocks.push(BasicBlock {
                        label: current_label.clone(),
                        body: current_body.clone(),
                        term,
                    });
                }
                current_label = label.to_string();
                current_body.clear();
                continue;
            }
        }

        current_body.push(line.to_string());
    }

    // Finalize last block.
    if let Some(term_line) = current_body.pop() {
        let term = parse_terminator(&term_line);
        blocks.push(BasicBlock {
            label: current_label,
            body: current_body,
            term,
        });
    }

    blocks
}

fn parse_terminator(line: &str) -> BBTerm {
    let t = line.trim();

    // ret void | ret T %val
    if t.starts_with("ret ") {
        return BBTerm::Ret {
            line: line.to_string(),
        };
    }

    // br label %X
    if t.starts_with("br label %") {
        let target = t["br label %".len()..].trim().to_string();
        return BBTerm::Br { target };
    }

    // br i1 %cond, label %A, label %B
    if t.starts_with("br i1 ") {
        if let Some(parsed) = parse_cond_br(t) {
            return parsed;
        }
    }

    BBTerm::Other(line.to_string())
}

fn parse_cond_br(t: &str) -> Option<BBTerm> {
    // "br i1 %cond, label %A, label %B"
    let rest = t.strip_prefix("br i1 ")?;
    let comma1 = rest.find(", label %")?;
    let cond = rest[..comma1].trim().to_string();
    let after1 = &rest[comma1 + 2..]; // ", label %A, label %B"
    let after1 = after1.trim_start_matches(", label %");
    // Now: "A, label %B"
    let comma2 = after1.find(", label %")?;
    let true_tgt = after1[..comma2].trim().to_string();
    let false_tgt = after1[comma2..].trim_start_matches(", label %").trim().to_string();

    Some(BBTerm::CondBr {
        cond,
        true_tgt,
        false_tgt,
    })
}

/// Extract the return type from a function signature like
/// `define private i32 @foo(...)`.
fn extract_return_type(sig: &str) -> String {
    let t = sig.trim();
    // Strip "define " prefix and any modifiers before the type.
    let mut rest = t;
    for prefix in &[
        "define noinline private ",
        "define noinline internal ",
        "define noinline ",
        "define private ",
        "define internal ",
        "define ",
    ] {
        if let Some(r) = rest.strip_prefix(prefix) {
            rest = r;
            break;
        }
    }
    // rest is now "RetType @name(...)"
    let at = rest.find(" @").unwrap_or(rest.len());
    rest[..at].trim().to_string()
}

/// Extract the return value from a `ret T %val` or `ret T N` line.
fn extract_ret_value(line: &str, ret_type: &str) -> Option<String> {
    let t = line.trim();
    let prefix = format!("ret {ret_type} ");
    t.strip_prefix(&prefix).map(|v| v.trim().to_string())
}

// ---------------------------------------------------------------------------
// Utility: append @llvm.used
// ---------------------------------------------------------------------------

fn append_llvm_used(mut ir: String, symbols: &[String]) -> String {
    let count = symbols.len();
    let entries: String = symbols
        .iter()
        .map(|s| format!("ptr @{s}"))
        .collect::<Vec<_>>()
        .join(", ");
    ir.push_str(&format!(
        "\n@llvm.used = appending global [{count} x ptr] [{entries}], section \"llvm.metadata\"\n"
    ));
    ir
}
