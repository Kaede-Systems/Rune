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
