use rune::llvm_ir::emit_llvm_ir_source;

#[test]
fn emits_llvm_ir_for_simple_function_calls() {
    let ir = emit_llvm_ir_source(
        "def add(a: i64, b: i64) -> i64:\n    return a + b\n\n\
         def main() -> i64:\n    return add(20, 22)\n",
    )
    .expect("llvm ir should generate");

    assert!(ir.contains("define i64 @add(i64 %a, i64 %b)"));
    assert!(ir.contains("define i64 @main()"));
    assert!(ir.contains("call i64 @add(i64 20, i64 22)"));
}

#[test]
fn emits_llvm_ir_for_control_flow() {
    let ir = emit_llvm_ir_source(
        "def main() -> i64:\n    let x: i64 = 0\n    while x < 3:\n        x = x + 1\n    return x\n",
    )
    .expect("llvm ir should generate");

    assert!(ir.contains("while_loop_0:"));
    assert!(ir.contains("br i1"));
    assert!(ir.contains("icmp slt i64"));
}

#[test]
fn emits_runtime_print_family_calls() {
    let ir = emit_llvm_ir_source(
        "def main() -> i64:\n    print(\"sum=\")\n    eprintln(42)\n    flush()\n    return 0\n",
    )
    .expect("llvm ir should generate");

    assert!(ir.contains("declare void @rune_rt_print_str(ptr, i64)"));
    assert!(ir.contains("declare void @rune_rt_eprint_i64(i64)"));
    assert!(ir.contains("declare void @rune_rt_eprint_newline()"));
    assert!(ir.contains("declare void @rune_rt_flush_stdout()"));
}

#[test]
fn emits_stdlib_builtin_runtime_calls() {
    let ir = emit_llvm_ir_source(
        "def main() -> i32:\n    let pid: i32 = __rune_builtin_system_pid()\n    let cpus: i32 = __rune_builtin_system_cpu_count()\n    let argc: i32 = __rune_builtin_env_arg_count()\n    let ok: bool = __rune_builtin_network_tcp_connect_timeout(\"127.0.0.1\", 65535, 10)\n    __rune_builtin_time_sleep_ms(__rune_builtin_time_monotonic_ms())\n    if __rune_builtin_env_exists(\"PATH\"):\n        println(pid)\n    println(cpus)\n    println(argc)\n    println(ok)\n    return 0\n",
    )
    .expect("llvm ir should generate");

    assert!(ir.contains("declare i64 @rune_rt_time_monotonic_ms()"));
    assert!(ir.contains("declare void @rune_rt_time_sleep_ms(i64)"));
    assert!(ir.contains("declare i32 @rune_rt_system_pid()"));
    assert!(ir.contains("declare i32 @rune_rt_system_cpu_count()"));
    assert!(ir.contains("declare i32 @rune_rt_env_arg_count()"));
    assert!(ir.contains("declare i1 @rune_rt_env_exists(ptr, i64)"));
    assert!(ir.contains("declare i1 @rune_rt_network_tcp_connect_timeout(ptr, i64, i32, i32)"));
}

#[test]
fn emits_fs_terminal_and_audio_runtime_decls() {
    let ir = emit_llvm_ir_source(
        "def main() -> bool:\n    let before: bool = __rune_builtin_fs_exists(\"note.txt\")\n    let wrote: bool = __rune_builtin_fs_write_string(\"note.txt\", \"hello\")\n    let text: String = __rune_builtin_fs_read_string(\"note.txt\")\n    __rune_builtin_terminal_clear()\n    __rune_builtin_terminal_move_to(2, 4)\n    __rune_builtin_terminal_hide_cursor()\n    __rune_builtin_terminal_set_title(text)\n    __rune_builtin_terminal_show_cursor()\n    return before or wrote or __rune_builtin_audio_bell()\n",
    )
    .expect("llvm ir should generate");

    assert!(ir.contains("declare i1 @rune_rt_fs_exists(ptr, i64)"));
    assert!(ir.contains("declare ptr @rune_rt_fs_read_string(ptr, i64)"));
    assert!(ir.contains("declare i1 @rune_rt_fs_write_string(ptr, i64, ptr, i64)"));
    assert!(ir.contains("declare void @rune_rt_terminal_clear()"));
    assert!(ir.contains("declare void @rune_rt_terminal_move_to(i32, i32)"));
    assert!(ir.contains("declare void @rune_rt_terminal_hide_cursor()"));
    assert!(ir.contains("declare void @rune_rt_terminal_set_title(ptr, i64)"));
    assert!(ir.contains("declare void @rune_rt_terminal_show_cursor()"));
    assert!(ir.contains("declare i1 @rune_rt_audio_bell()"));
}

#[test]
fn emits_dynamic_runtime_calls() {
    let ir = emit_llvm_ir_source(
        "def echo(value: dynamic) -> dynamic:\n    return value\n\n\
         def main() -> i32:\n    let value = 1\n    value = true\n    if value:\n        println(str(value))\n    println(echo(value) == true)\n    return 0\n",
    )
    .expect("dynamic llvm ir should generate");

    assert!(ir.contains("define { i64, i64, i64 } @echo(i64 %value.in.tag, i64 %value.in.payload, i64 %value.in.extra)"));
    assert!(ir.contains("declare i1 @rune_rt_dynamic_truthy(i64, i64, i64)"));
    assert!(ir.contains("declare ptr @rune_rt_dynamic_to_string(i64, i64, i64)"));
    assert!(ir.contains("declare i1 @rune_rt_dynamic_compare(ptr, ptr, i64)"));
}

#[test]
fn emits_external_function_declarations() {
    let ir = emit_llvm_ir_source(
        "extern def add_from_c(a: i32, b: i32) -> i32\n\n\
         def main() -> i32:\n    return add_from_c(20, 22)\n",
    )
    .expect("llvm ir should generate for externs");

    assert!(ir.contains("declare i32 @add_from_c(i32, i32)"));
    assert!(ir.contains("call i32 @add_from_c(i32 20, i32 22)"));
}

#[test]
fn emits_external_string_function_declarations() {
    let ir = emit_llvm_ir_source(
        "extern def greet_from_c(name: String) -> String\n\n\
         def main() -> i32:\n    println(greet_from_c(\"Rune\"))\n    return 0\n",
    )
    .expect("llvm ir should generate for extern string calls");

    assert!(ir.contains("declare ptr @greet_from_c(ptr)"));
    assert!(ir.contains("declare ptr @rune_rt_to_c_string(ptr, i64)"));
    assert!(ir.contains("declare ptr @rune_rt_from_c_string(ptr)"));
}

#[test]
fn emits_internal_string_function_abi() {
    let ir = emit_llvm_ir_source(
        "def exists(path: String) -> bool:\n    return __rune_builtin_fs_exists(path)\n\n\
         def read_text(path: String) -> String:\n    return __rune_builtin_fs_read_string(path)\n\n\
         def main() -> i32:\n    if exists(\"note.txt\"):\n        println(read_text(\"note.txt\"))\n    return 0\n",
    )
    .expect("llvm ir should generate for internal string functions");

    assert!(ir.contains("define i1 @exists(ptr %path.in.ptr, i64 %path.in.len)"));
    assert!(ir.contains("call i1 @exists(ptr "));
    assert!(ir.contains("define { ptr, i64 } @read_text(ptr %path.in.ptr, i64 %path.in.len)"));
    assert!(ir.contains("call { ptr, i64 } @read_text(ptr "));
    assert!(ir.contains("extractvalue { ptr, i64 }"));
}

#[test]
fn emits_str_builtin_runtime_calls() {
    let ir = emit_llvm_ir_source(
        "def main() -> i32:\n    println(str(42))\n    println(str(true))\n    return 0\n",
    )
    .expect("llvm ir should generate for str conversions");

    assert!(ir.contains("declare ptr @rune_rt_string_from_i64(i64)"));
    assert!(ir.contains("declare ptr @rune_rt_string_from_bool(i1)"));
    assert!(ir.contains("declare i64 @rune_rt_last_string_len()"));
}

#[test]
fn emits_int_string_runtime_call() {
    let ir = emit_llvm_ir_source(
        "def main() -> i32:\n    println(int(\"123\"))\n    return 0\n",
    )
    .expect("llvm ir should generate for string int conversion");

    assert!(ir.contains("declare i64 @rune_rt_string_to_i64(ptr, i64)"));
}

#[test]
fn emits_string_compare_runtime_call() {
    let ir = emit_llvm_ir_source(
        "def main(op: String) -> bool:\n    return op == \"+\"\n",
    )
    .expect("llvm ir should generate for string comparisons");

    assert!(ir.contains("declare i32 @rune_rt_string_compare(ptr, i64, ptr, i64)"));
    assert!(ir.contains("call i32 @rune_rt_string_compare"));
}
