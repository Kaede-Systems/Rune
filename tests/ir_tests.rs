use rune::ir::lower_program;
use rune::parser::parse_source;

#[test]
fn lowers_assignments_into_ir() {
    let program =
        parse_source("def main() -> unit:\n    let value: i32 = 1\n    value = 2\n    return\n")
            .expect("program should parse");
    let ir = lower_program(&program).to_string();

    assert!(ir.contains("value = copy %t0"));
    assert!(ir.contains("value = copy %t1"));
}

#[test]
fn lowers_control_flow_into_ir_labels() {
    let program = parse_source(
        "def main() -> i32:\n    let x: i32 = 0\n    while x < 3:\n        x = x + 1\n    return x\n",
    )
    .expect("program should parse");
    let ir = lower_program(&program).to_string();

    assert!(ir.contains("while_loop_"));
    assert!(ir.contains("branch"));
    assert!(ir.contains("jump while_loop_"));
}

#[test]
fn specializes_dynamic_locals_when_not_reassigned() {
    let program =
        parse_source("def main() -> unit:\n    let value = 42\n    println(value)\n    return\n")
            .expect("program should parse");
    let ir = lower_program(&program).to_string();

    assert!(ir.contains("local value: i32"));
}

#[test]
fn keeps_dynamic_locals_dynamic_when_reassigned() {
    let program =
        parse_source("def main() -> unit:\n    let value = 42\n    value = true\n    return\n")
            .expect("program should parse");
    let ir = lower_program(&program).to_string();

    assert!(ir.contains("local value: dynamic"));
}

#[test]
fn lowers_boolean_operators_into_ir() {
    let program = parse_source(
        "def main() -> unit:\n    let value = not false or true and false\n    return\n",
    )
    .expect("program should parse");
    let ir = lower_program(&program).to_string();

    assert!(ir.contains("not"));
    assert!(ir.contains("Or"));
    assert!(ir.contains("And"));
}

#[test]
fn lowers_modulo_into_ir() {
    let program = parse_source("def main() -> unit:\n    let value = 10 % 3\n    return\n")
        .expect("program should parse");
    let ir = lower_program(&program).to_string();

    assert!(ir.contains("Modulo"));
}

#[test]
fn lowers_bitwise_ops_into_ir() {
    let program = parse_source(
        "def f(a: i64, b: i64) -> i64:\n    let x: i64 = a & b\n    let y: i64 = a | b\n    let z: i64 = a ^ b\n    return x\n",
    )
    .expect("program should parse");
    let ir = lower_program(&program).to_string();

    assert!(ir.contains("BitwiseAnd"), "missing BitwiseAnd in IR: {ir}");
    assert!(ir.contains("BitwiseOr"), "missing BitwiseOr in IR");
    assert!(ir.contains("BitwiseXor"), "missing BitwiseXor in IR");
}

#[test]
fn lowers_shift_ops_into_ir() {
    let program = parse_source(
        "def f(a: i64) -> i64:\n    let x: i64 = a << 2\n    let y: i64 = a >> 1\n    return x\n",
    )
    .expect("program should parse");
    let ir = lower_program(&program).to_string();

    assert!(ir.contains("ShiftLeft"), "missing ShiftLeft in IR: {ir}");
    assert!(ir.contains("ShiftRight"), "missing ShiftRight in IR");
}

#[test]
fn lowers_bitwise_not_into_ir() {
    let program = parse_source(
        "def f(a: i64) -> i64:\n    let x: i64 = ~a\n    return x\n",
    )
    .expect("program should parse");
    let ir = lower_program(&program).to_string();

    assert!(ir.contains("bnot"), "missing bnot in IR: {ir}");
}

#[test]
fn lowers_for_range_into_while_loop_ir() {
    let program = parse_source(
        "def main() -> unit:\n    for i in range(10):\n        println(i)\n    return\n",
    )
    .expect("for range should parse");
    let ir = lower_program(&program).to_string();

    // The for loop desugars to a while loop; verify control-flow labels appear
    assert!(ir.contains("while_loop_"), "expected while_loop_ label in IR: {ir}");
    assert!(ir.contains("branch"), "expected branch in IR: {ir}");
}

#[test]
fn lowers_for_range_two_args_into_while_loop_ir() {
    let program = parse_source(
        "def main() -> unit:\n    for i in range(2, 8):\n        println(i)\n    return\n",
    )
    .expect("for range(start, stop) should parse");
    let ir = lower_program(&program).to_string();

    assert!(ir.contains("while_loop_"), "expected while_loop_ label in IR: {ir}");
}

#[test]
fn lowers_for_range_three_args_into_while_loop_ir() {
    let program = parse_source(
        "def main() -> unit:\n    for i in range(0, 20, 2):\n        println(i)\n    return\n",
    )
    .expect("for range(start, stop, step) should parse");
    let ir = lower_program(&program).to_string();

    assert!(ir.contains("while_loop_"), "expected while_loop_ label in IR: {ir}");
}
