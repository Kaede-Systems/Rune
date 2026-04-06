use rune::codegen::emit_asm_source;

#[test]
fn emits_simple_function_and_main() {
    let asm = emit_asm_source(
        "def add(a: i64, b: i64) -> i64:\n    return a + b\n\ndef main() -> i64:\n    return add(20, 22)\n",
    )
    .expect("assembly should generate");

    assert!(asm.contains(".globl add"));
    assert!(asm.contains(".globl main"));
    assert!(asm.contains("call add"));
    assert!(asm.contains("mov rcx, 20"));
    assert!(asm.contains("mov rdx, 22"));
    assert!(!asm.contains("push rax"));
}

#[test]
fn emits_control_flow_labels() {
    let asm = emit_asm_source(
        "def main() -> i64:\n    let x: i64 = 0\n    while x < 3:\n        let y: i64 = x + 1\n        return y\n    return 0\n",
    )
    .expect("assembly should generate");

    assert!(asm.contains(".L.main.while."));
    assert!(asm.contains(".L.main.whileend."));
}

#[test]
fn rejects_async_functions() {
    let error = emit_asm_source("async def main() -> i64:\n    return 0\n")
        .expect_err("async codegen should fail");
    assert!(error.message.contains("async functions"));
}

#[test]
fn emits_runtime_input_calls_in_string_contexts() {
    let asm = emit_asm_source("def main() -> i32:\n    println(input())\n    return 0\n")
        .expect("input in string contexts should generate");

    assert!(asm.contains("call rune_rt_input_line"));
    assert!(asm.contains("call rune_rt_print_str"));
}

#[test]
fn emits_runtime_print_calls() {
    let asm = emit_asm_source(
        "def main() -> i64:\n    print(\"sum=\")\n    println(40 + 2)\n    return 0\n",
    )
    .expect("assembly should generate");

    assert!(asm.contains("call rune_rt_print_str"));
    assert!(asm.contains("call rune_rt_print_i64"));
    assert!(asm.contains("call rune_rt_print_newline"));
    assert!(asm.contains(".L.rune.str.0:"));
}

#[test]
fn emits_runtime_stderr_calls() {
    let asm = emit_asm_source(
        "def main() -> i64:\n    eprint(\"sum=\")\n    eprintln(40 + 2)\n    return 0\n",
    )
    .expect("assembly should generate");

    assert!(asm.contains("call rune_rt_eprint_str"));
    assert!(asm.contains("call rune_rt_eprint_i64"));
    assert!(asm.contains("call rune_rt_eprint_newline"));
}

#[test]
fn emits_runtime_flush_calls() {
    let asm = emit_asm_source("def main() -> i64:\n    flush()\n    eflush()\n    return 0\n")
        .expect("assembly should generate");

    assert!(asm.contains("call rune_rt_flush_stdout"));
    assert!(asm.contains("call rune_rt_flush_stderr"));
}

#[test]
fn reorders_keyword_arguments_for_calls() {
    let asm = emit_asm_source(
        "def add(lhs: i64, rhs: i64) -> i64:\n    return lhs + rhs\n\ndef main() -> i64:\n    return add(rhs=22, lhs=20)\n",
    )
    .expect("assembly should generate");

    assert!(asm.contains("mov rcx, 20"));
    assert!(asm.contains("mov rdx, 22"));
    assert!(!asm.contains("push rax"));
}

#[test]
fn preserves_argument_order_for_complex_calls() {
    let asm = emit_asm_source(
        "def div_pair(a: i32, b: i32) -> i32:\n    return a / b\n\n\
         def main() -> i32:\n    let x: i32 = 1\n    return div_pair(x + 8, 3)\n",
    )
    .expect("assembly should generate");

    assert!(asm.contains("add rax, 8"));
    assert!(asm.contains("mov rcx, rax"));
    assert!(asm.contains("pop rdx"));
    assert!(asm.contains("call div_pair"));
}

#[test]
fn auto_optimizes_constant_expressions() {
    let asm = emit_asm_source("def main() -> i64:\n    return 40 + 2\n")
        .expect("assembly should generate");

    assert!(asm.contains("mov rax, 42"));
    assert!(!asm.contains("add rax, rcx"));
}

#[test]
fn uses_tighter_stack_frames_for_simple_functions() {
    let asm = emit_asm_source("def add(a: i32, b: i32) -> i32:\n    return a + b\n")
        .expect("assembly should generate");

    assert!(asm.contains("sub rsp, 64"));
    assert!(!asm.contains("sub rsp, 128"));
}

#[test]
fn direct_lowers_simple_leaf_arithmetic() {
    let asm = emit_asm_source("def add(a: i32, b: i32) -> i32:\n    return a + b\n")
        .expect("assembly should generate");

    assert!(asm.contains("add rax, QWORD PTR [rbp-16]"));
    assert!(!asm.contains("push rax"));
    assert!(!asm.contains("mov rcx, rax"));
}

#[test]
fn removes_dead_code_after_unconditional_jump() {
    let asm = emit_asm_source("def main() -> i32:\n    if true:\n        return 7\n    return 0\n")
        .expect("assembly should generate");

    assert!(asm.contains("mov rax, 7"));
    assert!(!asm.contains("mov rax, 0"));
}

#[test]
fn emits_time_builtin_runtime_call() {
    let asm = emit_asm_source("def main() -> i64:\n    return __rune_builtin_time_now_unix()\n")
        .expect("assembly should generate");

    assert!(asm.contains("call rune_rt_time_now_unix"));
}

#[test]
fn emits_extended_stdlib_runtime_calls() {
    let asm = emit_asm_source(
        "def main() -> bool:\n    __rune_builtin_time_sleep_ms(1)\n    __rune_builtin_time_sleep_us(__rune_builtin_time_monotonic_us())\n    let enabled: bool = __rune_builtin_env_get_bool(\"ENABLED\", false)\n    let cpus: i32 = __rune_builtin_system_cpu_count()\n    return __rune_builtin_network_tcp_connect_timeout(\"127.0.0.1\", cpus, 250)\n",
    )
    .expect("extended runtime calls should generate");

    assert!(asm.contains("call rune_rt_time_sleep_ms"));
    assert!(asm.contains("call rune_rt_time_monotonic_us"));
    assert!(asm.contains("call rune_rt_time_sleep_us"));
    assert!(asm.contains("call rune_rt_env_get_bool"));
    assert!(asm.contains("call rune_rt_system_cpu_count"));
    assert!(asm.contains("call rune_rt_network_tcp_connect_timeout"));
}

#[test]
fn emits_string_equal_runtime_call_for_equality() {
    let asm = emit_asm_source("def main(op: String) -> bool:\n    return op == \"+\"\n")
        .expect("string equality should generate");

    assert!(asm.contains("call rune_rt_string_equal"));
}

#[test]
fn emits_env_and_network_runtime_calls() {
    let asm = emit_asm_source(
        "def main() -> bool:\n    let host: String = __rune_builtin_env_get_string(\"HOST\", \"127.0.0.1\")\n    let port: i32 = __rune_builtin_env_get_i32(\"PORT\", 8080)\n    println(host)\n    return __rune_builtin_network_tcp_connect(host, port)\n",
    )
    .expect("assembly should generate");

    assert!(asm.contains("call rune_rt_env_get_string"));
    assert!(asm.contains("call rune_rt_env_get_i32"));
    assert!(asm.contains("call rune_rt_network_tcp_connect"));
}

#[test]
fn emits_fs_terminal_and_audio_runtime_calls() {
    let asm = emit_asm_source(
        "def main(path: String) -> bool:\n    let before: bool = __rune_builtin_fs_exists(path)\n    let wrote: bool = __rune_builtin_fs_write_string(path, \"hello\")\n    let text: String = __rune_builtin_fs_read_string(path)\n    __rune_builtin_terminal_clear()\n    __rune_builtin_terminal_move_to(2, 4)\n    __rune_builtin_terminal_hide_cursor()\n    __rune_builtin_terminal_set_title(text)\n    __rune_builtin_terminal_show_cursor()\n    return before or wrote or __rune_builtin_audio_bell()\n",
    )
    .expect("fs, terminal, and audio calls should lower");

    assert!(asm.contains("call rune_rt_fs_exists"));
    assert!(asm.contains("call rune_rt_fs_write_string"));
    assert!(asm.contains("call rune_rt_fs_read_string"));
    assert!(asm.contains("call rune_rt_terminal_clear"));
    assert!(asm.contains("call rune_rt_terminal_move_to"));
    assert!(asm.contains("call rune_rt_terminal_hide_cursor"));
    assert!(asm.contains("call rune_rt_terminal_set_title"));
    assert!(asm.contains("call rune_rt_terminal_show_cursor"));
    assert!(asm.contains("call rune_rt_audio_bell"));
    assert!(asm.contains("call rune_rt_last_string_len"));
}

#[test]
fn forwards_string_params_to_runtime_builtins() {
    let asm = emit_asm_source(
        "def exists(name: String) -> bool:\n    return __rune_builtin_env_exists(name)\n",
    )
    .expect("string parameters should lower");

    assert!(asm.contains("mov rcx, QWORD PTR [rbp-8]"));
    assert!(asm.contains("mov rdx, QWORD PTR [rbp-16]"));
    assert!(asm.contains("call rune_rt_env_exists"));
}

#[test]
fn lowers_string_arguments_for_user_functions() {
    let asm = emit_asm_source(
        "def ping(host: String, port: i32) -> bool:\n    return __rune_builtin_network_tcp_connect(host, port)\n\n\
         def main() -> bool:\n    return ping(\"127.0.0.1\", 80)\n",
    )
    .expect("string arguments should lower for user calls");

    assert!(asm.contains("lea rcx, .L.rune.str.0[rip]"));
    assert!(asm.contains("mov rdx, 9"));
    assert!(asm.contains("mov r8d, 80"));
    assert!(asm.contains("call ping"));
}

#[test]
fn emits_reassignment_stores() {
    let asm = emit_asm_source(
        "def main() -> i32:\n    let value: i32 = 1\n    value = 2\n    return value\n",
    )
    .expect("assignment codegen should work");

    assert!(asm.contains("mov QWORD PTR [rbp-8], rax"));
}

#[test]
fn emits_dynamic_local_storage_and_print_calls() {
    let asm = emit_asm_source(
        "def main() -> i64:\n    let value = 1\n    value = true\n    value = \"hi\"\n    println(value)\n    return 0\n",
    )
    .expect("dynamic locals should lower");

    assert!(asm.contains("mov QWORD PTR [rbp-8], rax"));
    assert!(asm.contains("mov QWORD PTR [rbp-16], rcx"));
    assert!(asm.contains("mov QWORD PTR [rbp-24], rdx"));
    assert!(asm.contains("call rune_rt_print_dynamic"));
}

#[test]
fn lowers_dynamic_parameters_for_user_functions() {
    let asm = emit_asm_source(
        "def echo(value) -> unit:\n    println(value)\n    return\n\n\
         def main() -> i32:\n    echo(\"hi\")\n    return 0\n",
    )
    .expect("dynamic params should lower");

    assert!(asm.contains("mov QWORD PTR [rbp-8], rcx"));
    assert!(asm.contains("mov QWORD PTR [rbp-16], rdx"));
    assert!(asm.contains("mov QWORD PTR [rbp-24], r8"));
    assert!(asm.contains("call rune_rt_print_dynamic"));
}

#[test]
fn lowers_dynamic_returning_helper_functions() {
    let asm = emit_asm_source(
        "def make_value(flag: bool) -> dynamic:\n    if flag:\n        return \"yes\"\n    return 42\n\n\
         def main() -> i32:\n    println(make_value(true))\n    println(make_value(false))\n    return 0\n",
    )
    .expect("dynamic returns should lower for helper functions");

    assert!(asm.contains("mov rax, 4"));
    assert!(asm.contains("mov rax, 2"));
    assert!(asm.contains("call make_value"));
    assert!(asm.contains("call rune_rt_print_dynamic"));
}

#[test]
fn still_rejects_dynamic_main_returns_in_native_backend() {
    let error = emit_asm_source("def main() -> dynamic:\n    return 1\n")
        .expect_err("dynamic main return should fail for now");
    assert!(
        error
            .message
            .contains("dynamic return values are not yet supported for `main`")
    );
}

#[test]
fn lowers_string_conversions_and_concat() {
    let asm = emit_asm_source(
        "def main() -> i32:\n    let prefix: String = str(42)\n    let joined: String = prefix + \"!\"\n    println(joined)\n    println(int(joined))\n    return 0\n",
    )
    .expect("string conversions and concat should lower");

    assert!(asm.contains("call rune_rt_string_from_i64"));
    assert!(asm.contains("call rune_rt_string_concat"));
    assert!(asm.contains("call rune_rt_last_string_len"));
    assert!(asm.contains("call rune_rt_string_to_i64"));
}

#[test]
fn lowers_dynamic_add_through_runtime_helper() {
    let asm = emit_asm_source(
        "def main() -> i32:\n    let value = 40\n    value = value + 2\n    value = value + \"!\"\n    println(value)\n    return 0\n",
    )
    .expect("dynamic + should lower");

    assert!(asm.contains("call rune_rt_dynamic_binary"));
    assert!(asm.contains("sub rsp, 80"));
    assert!(asm.contains("lea rcx, [rsp]"));
}

#[test]
fn lowers_dynamic_numeric_arithmetic_through_runtime_helper() {
    // Use a function returning `dynamic` to ensure the value is genuinely
    // dynamic at IR level and cannot be folded to a static integer.
    let asm = emit_asm_source(
        "def get_val() -> dynamic:\n    return 10\ndef main() -> i32:\n    let value = get_val()\n    let a = value - 3\n    let b = value * 5\n    let c = value / 7\n    println(a)\n    return 0\n",
    )
    .expect("dynamic numeric arithmetic should lower");

    assert!(asm.contains("call rune_rt_dynamic_binary"));
}

#[test]
fn lowers_fstrings_through_string_helpers() {
    let asm = emit_asm_source(
        "def main() -> i32:\n    let value: String = f\"sum={40 + 2} ok={true}\"\n    println(value)\n    return 0\n",
    )
    .expect("f-strings should lower");

    assert!(asm.contains("call rune_rt_string_from_i64"));
    assert!(asm.contains("call rune_rt_string_from_bool"));
    assert!(asm.contains("call rune_rt_string_concat"));
}

#[test]
fn lowers_boolean_operators_and_dynamic_truthiness() {
    let asm = emit_asm_source(
        "def main() -> i32:\n    let value = 1\n    value = true\n    if value and not false:\n        println(\"yes\")\n    return 0\n",
    )
    .expect("boolean operators should lower");

    assert!(asm.contains("call rune_rt_dynamic_truthy"));
    assert!(asm.contains(".L.main.logicshort."));
    assert!(asm.contains("setne al"));
}

#[test]
fn lowers_modulo_for_static_and_dynamic_values() {
    let asm = emit_asm_source(
        "def rem(a: i32, b: i32) -> i32:\n    return a % b\n\n\
         def main() -> i32:\n    let a: i32 = rem(10, 3)\n    let value = 10\n    value = true\n    value = 10\n    value = value % 4\n    println(a)\n    println(value)\n    return 0\n",
    )
    .expect("modulo should lower");

    assert!(asm.contains("idiv rcx"));
    assert!(asm.contains("mov rax, rdx"));
    assert!(asm.contains("mov r9, 4"));
    assert!(asm.contains("call rune_rt_dynamic_binary"));
}

#[test]
fn lowers_panic_to_runtime_call() {
    let asm =
        emit_asm_source("def main() -> i32:\n    panic(\"boom\")\n").expect("panic should lower");

    assert!(asm.contains("call rune_rt_panic"));
    assert!(asm.contains("Rune panic") || asm.contains(".L.rune.str."));
}

#[test]
fn lowers_zero_division_to_runtime_error_code() {
    let asm = emit_asm_source(
        "def main() -> i32:\n    let value: i64 = 10\n    let zero: i64 = 0\n    println(value / zero)\n    return 0\n",
    )
    .expect("zero division should lower");

    assert!(asm.contains("call rune_rt_fail"));
    assert!(asm.contains("mov ecx, 1001"));
}

#[test]
fn lowers_dynamic_comparisons_through_runtime_helper() {
    let asm = emit_asm_source(
        "def main() -> i32:\n    let value = 40\n    value = \"40\"\n    if value == 40:\n        println(\"eq\")\n    if value != 99:\n        println(\"ne\")\n    return 0\n",
    )
    .expect("dynamic comparisons should lower");

    assert!(asm.contains("call rune_rt_dynamic_compare"));
    assert!(asm.contains("mov r8, 0"));
    assert!(asm.contains("mov r8, 1"));
}

#[test]
fn lowers_struct_locals_and_field_reads() {
    let asm = emit_asm_source(
        "struct Point:\n    x: i32\n    y: i32\n\n\
         def main() -> i32:\n    let point: Point = Point(x=20, y=22)\n    return point.x + point.y\n",
    )
    .expect("struct locals should lower");

    assert!(asm.contains("mov QWORD PTR [rbp-"));
    assert!(asm.contains("add rax, QWORD PTR [rbp-"));
}

#[test]
fn lowers_class_locals_and_field_reads() {
    let asm = emit_asm_source(
        "class Point:\n    x: i32\n    y: i32\n\n\
         def main() -> i32:\n    let point: Point = Point(x=20, y=22)\n    return point.x + point.y\n",
    )
    .expect("class locals should lower");

    assert!(asm.contains("mov QWORD PTR [rbp-"));
    assert!(asm.contains("add rax, QWORD PTR [rbp-"));
}

#[test]
fn lowers_class_methods_and_method_calls() {
    let asm = emit_asm_source(
        "class Point:\n    x: i32\n    y: i32\n    def sum(self) -> i32:\n        return self.x + self.y\n\n\
         def main() -> i32:\n    let point: Point = Point(x=20, y=22)\n    return point.sum()\n",
    )
    .expect("class method call should lower");

    assert!(asm.contains(".globl Point__sum"));
    assert!(asm.contains("call Point__sum"));
}

#[test]
fn lowers_struct_parameters_for_user_functions() {
    let asm = emit_asm_source(
        "struct Point:\n    x: i32\n    y: i32\n\n\
         def sum_point(point: Point) -> i32:\n    return point.x + point.y\n\n\
         def main() -> i32:\n    let point: Point = Point(x=20, y=22)\n    return sum_point(point)\n",
    )
    .expect("struct params should lower");

    assert!(asm.contains("lea rcx, [rbp-"));
    assert!(asm.contains("mov rax, QWORD PTR [rcx+0]"));
    assert!(asm.contains("mov rax, QWORD PTR [rcx+8]"));
    assert!(asm.contains("call sum_point"));
}

#[test]
fn lowers_extern_function_calls() {
    let asm = emit_asm_source(
        "extern def add_from_c(a: i32, b: i32) -> i32\n\n\
         def main() -> i32:\n    return add_from_c(20, 22)\n",
    )
    .expect("extern call should lower");

    assert!(asm.contains("call add_from_c"));
    assert!(!asm.contains(".globl add_from_c\nadd_from_c:"));
}

#[test]
fn lowers_extern_string_function_calls() {
    let asm = emit_asm_source(
        "extern def greet_from_c(name: String) -> String\n\n\
         def main() -> i32:\n    println(greet_from_c(\"Rune\"))\n    return 0\n",
    )
    .expect("extern string call should lower");

    assert!(asm.contains("call rune_rt_to_c_string"));
    assert!(asm.contains("call greet_from_c"));
    assert!(asm.contains("call rune_rt_from_c_string"));
}

#[test]
fn lowers_internal_string_returning_functions() {
    let asm = emit_asm_source(
        "def read_line() -> String:\n    return input()\n\n\
         def main() -> i32:\n    println(read_line())\n    return 0\n",
    )
    .expect("internal string return should lower");

    assert!(asm.contains("call rune_rt_input_line"));
    assert!(asm.contains("call read_line"));
    assert!(asm.contains("call rune_rt_print_str"));
}

#[test]
fn emits_bitwise_and_or_xor() {
    let asm = emit_asm_source(
        "def f(a: i64, b: i64) -> i64:\n    let x: i64 = a & b\n    let y: i64 = x | b\n    let z: i64 = y ^ a\n    return z\n",
    )
    .expect("bitwise ops should generate");

    assert!(asm.contains("and ") || asm.contains("and\t"));
    assert!(asm.contains("or ") || asm.contains("or\t"));
    assert!(asm.contains("xor ") || asm.contains("xor\t"));
}

#[test]
fn emits_shift_operations() {
    let asm = emit_asm_source(
        "def f(a: i64) -> i64:\n    let x: i64 = a << 2\n    let y: i64 = x >> 1\n    return y\n",
    )
    .expect("shift ops should generate");

    assert!(asm.contains("shl") || asm.contains("sal"));
    assert!(asm.contains("sar") || asm.contains("shr"));
}

#[test]
fn emits_bitwise_not() {
    let asm = emit_asm_source(
        "def f(a: i64) -> i64:\n    let x: i64 = ~a\n    return x\n",
    )
    .expect("bitwise not should generate");

    assert!(asm.contains("not ") || asm.contains("not\t"));
}

#[test]
fn emits_hex_integer_literal() {
    let asm = emit_asm_source(
        "def f() -> i64:\n    return 0xFF\n",
    )
    .expect("hex literal should generate");

    assert!(asm.contains("255") || asm.contains("0xff") || asm.contains("0xFF"));
}

#[test]
fn emits_binary_integer_literal() {
    let asm = emit_asm_source(
        "def f() -> i64:\n    return 0b1010\n",
    )
    .expect("binary literal should generate");

    assert!(asm.contains("10") || asm.contains("0b1010"));
}

#[test]
fn emits_augmented_assignment() {
    let asm = emit_asm_source(
        "def f(x: i64) -> i64:\n    x += 5\n    x -= 2\n    return x\n",
    )
    .expect("augmented assignment should generate");

    assert!(asm.contains("add ") || asm.contains("add\t"));
    assert!(asm.contains("sub ") || asm.contains("sub\t"));
}

#[test]
fn emits_for_range_loop_as_while_labels() {
    let asm = emit_asm_source(
        "def main() -> i64:\n    for i in range(10):\n        println(i)\n    return 0\n",
    )
    .expect("for range loop should generate assembly");

    // for range desugars to while; verify the while control-flow labels are present
    assert!(
        asm.contains(".L.main.while.") || asm.contains(".L.main.whileend."),
        "expected while loop labels in asm: {}",
        &asm[..asm.len().min(500)]
    );
}

#[test]
fn emits_for_range_two_args_loop() {
    let asm = emit_asm_source(
        "def main() -> i64:\n    for i in range(2, 8):\n        println(i)\n    return 0\n",
    )
    .expect("for range(start, stop) loop should generate assembly");

    assert!(
        asm.contains(".L.main.while.") || asm.contains(".L.main.whileend."),
        "expected while loop labels in asm"
    );
}

#[test]
fn emits_for_range_three_args_loop() {
    let asm = emit_asm_source(
        "def main() -> i64:\n    for i in range(0, 20, 2):\n        println(i)\n    return 0\n",
    )
    .expect("for range(start, stop, step) loop should generate assembly");

    assert!(
        asm.contains(".L.main.while.") || asm.contains(".L.main.whileend."),
        "expected while loop labels in asm"
    );
}

#[test]
fn emits_string_len_call() {
    let asm = emit_asm_source(
        "def f(s: String) -> i64:\n    return s.len()\n\ndef main() -> i64:\n    return f(\"hello\")\n",
    )
    .expect("String.len() should generate assembly");

    assert!(asm.contains("rune_rt_string_len"), "expected call to rune_rt_string_len");
}

#[test]
fn emits_string_upper_call() {
    let asm = emit_asm_source(
        "def main() -> i64:\n    let s: String = \"hello\"\n    println(s.upper())\n    return 0\n",
    )
    .expect("String.upper() should generate assembly");

    assert!(asm.contains("rune_rt_string_upper"), "expected call to rune_rt_string_upper");
}

#[test]
fn emits_string_lower_call() {
    let asm = emit_asm_source(
        "def main() -> i64:\n    let s: String = \"HELLO\"\n    println(s.lower())\n    return 0\n",
    )
    .expect("String.lower() should generate assembly");

    assert!(asm.contains("rune_rt_string_lower"), "expected call to rune_rt_string_lower");
}

#[test]
fn emits_string_contains_call() {
    let asm = emit_asm_source(
        "def main() -> i64:\n    let s: String = \"hello world\"\n    let has: bool = s.contains(\"world\")\n    return 0\n",
    )
    .expect("String.contains() should generate assembly");

    assert!(asm.contains("rune_rt_string_contains"), "expected call to rune_rt_string_contains");
}

#[test]
fn accepts_same_name_let_in_different_branches() {
    // `let result` in both branches is valid: exclusive branches share a stack slot.
    let asm = emit_asm_source(
        "def main() -> i64:\n    let x: i64 = 1\n    if x > 0:\n        let result: i64 = x + 10\n        return result\n    else:\n        let result: i64 = x - 10\n        return result\n    return 0\n",
    )
    .expect("same-name locals in different branches should codegen");

    assert!(asm.contains(".globl main"));
}

#[test]
fn accepts_same_name_let_in_loop_body() {
    // `let acc` declared each iteration via a loop body is valid.
    let asm = emit_asm_source(
        "def main() -> i64:\n    let i: i64 = 0\n    while i < 3:\n        let acc: i64 = i * 2\n        i = i + 1\n    return 0\n",
    )
    .expect("let inside while body should codegen");

    assert!(asm.contains(".globl main"));
}
