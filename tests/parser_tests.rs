use rune::parser::{CallArg, ExprKind, Item, Stmt, parse_source};

#[test]
fn parses_async_function_with_raises() {
    let program = parse_source(
        "async def main(name) -> i32 raises IoError:\n    println(name)\n    return 0\n",
    )
    .expect("program should parse");

    let Item::Function(function) = &program.items[0] else {
        panic!("expected function item");
    };
    assert!(function.is_async);
    assert_eq!(function.name, "main");
    assert_eq!(function.params[0].ty.name, "dynamic");
    assert_eq!(
        function.return_type.as_ref().map(|ty| ty.name.as_str()),
        Some("i32")
    );
    assert_eq!(
        function.raises.as_ref().map(|ty| ty.name.as_str()),
        Some("IoError")
    );
    assert_eq!(function.body.statements.len(), 2);
}

#[test]
fn parses_if_elif_else() {
    let program = parse_source(
        "def sign(x: i64) -> i64:\n    if x > 0:\n        return 1\n    elif x < 0:\n        return -1\n    else:\n        return 0\n",
    )
    .expect("program should parse");

    let Item::Function(function) = &program.items[0] else {
        panic!("expected function item");
    };
    let Stmt::If(if_stmt) = &function.body.statements[0] else {
        panic!("expected if statement");
    };
    assert_eq!(if_stmt.elif_blocks.len(), 1);
    assert!(if_stmt.else_block.is_some());
}

#[test]
fn parses_calls_and_fields() {
    let program =
        parse_source("def run() -> i32:\n    client.connect(server.host)\n    return 0\n")
            .expect("program should parse");

    let Item::Function(function) = &program.items[0] else {
        panic!("expected function item");
    };
    let Stmt::Expr(expr_stmt) = &function.body.statements[0] else {
        panic!("expected expression statement");
    };

    match &expr_stmt.expr.kind {
        ExprKind::Call { callee, args } => {
            assert_eq!(args.len(), 1);
            match &args[0] {
                CallArg::Positional(expr) => match &expr.kind {
                    ExprKind::Field { name, .. } => assert_eq!(name, "host"),
                    other => panic!("expected field access argument, found {other:?}"),
                },
                other => panic!("expected positional argument, found {other:?}"),
            }
            match &callee.kind {
                ExprKind::Field { name, .. } => assert_eq!(name, "connect"),
                other => panic!("expected field access callee, found {other:?}"),
            }
        }
        other => panic!("expected call expression, found {other:?}"),
    }
}

#[test]
fn rejects_missing_colon_after_function_signature() {
    let error = parse_source("def main() -> i32\n    return 0\n")
        .expect_err("parser should reject missing colon");
    assert!(error.message.contains("expected `:`"));
}

#[test]
fn rejects_bad_block_structure() {
    let error = parse_source("def main() -> i32:\nreturn 0\n")
        .expect_err("parser should reject missing indent");
    assert!(error.message.contains("expected indented block"));
}

#[test]
fn parses_keyword_arguments() {
    let program =
        parse_source("def run() -> i32:\n    connect(host=server, port=8080)\n    return 0\n")
            .expect("program should parse");

    let Item::Function(function) = &program.items[0] else {
        panic!("expected function item");
    };
    let Stmt::Expr(expr_stmt) = &function.body.statements[0] else {
        panic!("expected expression statement");
    };

    match &expr_stmt.expr.kind {
        ExprKind::Call { args, .. } => {
            assert_eq!(args.len(), 2);
            assert!(matches!(args[0], CallArg::Keyword { .. }));
            assert!(matches!(args[1], CallArg::Keyword { .. }));
        }
        other => panic!("expected call expression, found {other:?}"),
    }
}

#[test]
fn parses_untyped_parameters_as_dynamic() {
    let program = parse_source("def greet(name, times: i32) -> i32:\n    return times\n")
        .expect("program should parse");

    let Item::Function(function) = &program.items[0] else {
        panic!("expected function item");
    };
    assert_eq!(function.params[0].ty.name, "dynamic");
    assert_eq!(function.params[1].ty.name, "i32");
}

#[test]
fn parses_mixed_arguments() {
    let program = parse_source("def run() -> i32:\n    add(10, rhs=32)\n    return 0\n")
        .expect("program should parse");

    let Item::Function(function) = &program.items[0] else {
        panic!("expected function item");
    };
    let Stmt::Expr(expr_stmt) = &function.body.statements[0] else {
        panic!("expected expression statement");
    };

    match &expr_stmt.expr.kind {
        ExprKind::Call { args, .. } => {
            assert_eq!(args.len(), 2);
            assert!(matches!(args[0], CallArg::Positional(_)));
            assert!(matches!(args[1], CallArg::Keyword { .. }));
        }
        other => panic!("expected call expression, found {other:?}"),
    }
}

#[test]
fn parses_assignment_statements() {
    let program =
        parse_source("def run() -> unit:\n    let value: i32 = 1\n    value = 2\n    return\n")
            .expect("program should parse");

    let Item::Function(function) = &program.items[0] else {
        panic!("expected function item");
    };
    assert!(matches!(function.body.statements[1], Stmt::Assign(_)));
}

#[test]
fn parses_break_and_continue_statements() {
    let program = parse_source(
        "def run() -> unit:\n    while true:\n        break\n    while true:\n        continue\n",
    )
    .expect("program should parse");

    let Item::Function(function) = &program.items[0] else {
        panic!("expected function item");
    };
    let Stmt::While(first_loop) = &function.body.statements[0] else {
        panic!("expected while statement");
    };
    assert!(matches!(first_loop.body.statements[0], Stmt::Break(_)));
    let Stmt::While(second_loop) = &function.body.statements[1] else {
        panic!("expected while statement");
    };
    assert!(matches!(second_loop.body.statements[0], Stmt::Continue(_)));
}

#[test]
fn parses_import_items() {
    let program = parse_source(
        "import math.core\nfrom net.http import get, post\n\ndef main() -> i32:\n    return 0\n",
    )
    .expect("program should parse");

    match &program.items[0] {
        Item::Import(import) => {
            assert_eq!(import.level, 0);
            assert_eq!(import.module, vec!["math", "core"]);
            assert!(import.names.is_none());
        }
        other => panic!("expected import item, found {other:?}"),
    }

    match &program.items[1] {
        Item::Import(import) => {
            assert_eq!(import.level, 0);
            assert_eq!(import.module, vec!["net", "http"]);
            assert_eq!(
                import.names.as_ref().expect("expected imported names"),
                &vec!["get".to_string(), "post".to_string()]
            );
        }
        other => panic!("expected import item, found {other:?}"),
    }
}

#[test]
fn parses_relative_import_items() {
    let program = parse_source(
        "from .math import add\nfrom ..shared.util import helper\n\ndef main() -> i32:\n    return 0\n",
    )
    .expect("program should parse");

    match &program.items[0] {
        Item::Import(import) => {
            assert_eq!(import.level, 1);
            assert_eq!(import.module, vec!["math"]);
        }
        other => panic!("expected import item, found {other:?}"),
    }

    match &program.items[1] {
        Item::Import(import) => {
            assert_eq!(import.level, 2);
            assert_eq!(import.module, vec!["shared", "util"]);
        }
        other => panic!("expected import item, found {other:?}"),
    }
}

#[test]
fn parses_parenthesized_multiline_import_items() {
    let program = parse_source(
        "from arduino import (\n    uart_begin,\n    uart_write,\n    uart_read_byte,\n)\n\ndef main() -> i32:\n    return 0\n",
    )
    .expect("program should parse");

    match &program.items[0] {
        Item::Import(import) => {
            assert_eq!(import.level, 0);
            assert_eq!(import.module, vec!["arduino"]);
            assert_eq!(
                import.names.as_ref().expect("expected imported names"),
                &vec![
                    "uart_begin".to_string(),
                    "uart_write".to_string(),
                    "uart_read_byte".to_string()
                ]
            );
        }
        other => panic!("expected import item, found {other:?}"),
    }
}

#[test]
fn parses_boolean_operators() {
    let program = parse_source(
        "def main() -> unit:\n    let value = not false or true and false\n    return\n",
    )
    .expect("program should parse");

    let Item::Function(function) = &program.items[0] else {
        panic!("expected function item");
    };
    let Stmt::Let(let_stmt) = &function.body.statements[0] else {
        panic!("expected let statement");
    };
    assert!(matches!(let_stmt.value.kind, ExprKind::Binary { .. }));
}

#[test]
fn parses_modulo_operator() {
    let program = parse_source("def main() -> unit:\n    let value = 10 % 3\n    return\n")
        .expect("program should parse");

    let Item::Function(function) = &program.items[0] else {
        panic!("expected function item");
    };
    let Stmt::Let(let_stmt) = &function.body.statements[0] else {
        panic!("expected let statement");
    };
    assert!(matches!(let_stmt.value.kind, ExprKind::Binary { .. }));
}

#[test]
fn parses_struct_declaration_and_constructor() {
    let program = parse_source(
        "struct Point:\n    x: i32\n    y: i32\n\n\
         def main() -> i32:\n    let point: Point = Point(x=20, y=22)\n    return point.x\n",
    )
    .expect("program should parse");

    match &program.items[0] {
        Item::Struct(decl) => {
            assert_eq!(decl.name, "Point");
            assert_eq!(decl.fields.len(), 2);
            assert_eq!(decl.fields[0].name, "x");
            assert_eq!(decl.fields[1].name, "y");
        }
        other => panic!("expected struct item, found {other:?}"),
    }

    let Item::Function(function) = &program.items[1] else {
        panic!("expected function item");
    };
    let Stmt::Let(let_stmt) = &function.body.statements[0] else {
        panic!("expected let statement");
    };
    match &let_stmt.value.kind {
        ExprKind::Call { callee, args } => {
            assert!(matches!(&callee.kind, ExprKind::Identifier(name) if name == "Point"));
            assert_eq!(args.len(), 2);
            assert!(matches!(args[0], CallArg::Keyword { .. }));
        }
        other => panic!("expected struct constructor call, found {other:?}"),
    }
}

#[test]
fn parses_class_declaration_and_constructor() {
    let program = parse_source(
        "class Point:\n    x: i32\n    y: i32\n\n\
         def main() -> i32:\n    let point: Point = Point(x=20, y=22)\n    return point.x\n",
    )
    .expect("program should parse");

    match &program.items[0] {
        Item::Struct(decl) => {
            assert_eq!(decl.name, "Point");
            assert_eq!(decl.fields.len(), 2);
            assert_eq!(decl.fields[0].name, "x");
            assert_eq!(decl.fields[1].name, "y");
        }
        other => panic!("expected class item lowered as struct, found {other:?}"),
    }
}

#[test]
fn parses_class_method_declaration() {
    let program = parse_source(
        "class Point:\n    x: i32\n    y: i32\n    def sum(self) -> i32:\n        return self.x + self.y\n",
    )
    .expect("class method should parse");

    match &program.items[0] {
        Item::Struct(decl) => {
            assert_eq!(decl.name, "Point");
            assert_eq!(decl.methods.len(), 1);
            assert_eq!(decl.methods[0].name, "sum");
            assert_eq!(decl.methods[0].params[0].name, "self");
        }
        other => panic!("expected class item lowered as struct, found {other:?}"),
    }
}

#[test]
fn parses_top_level_script_statements_as_synthetic_main() {
    let program = parse_source("println(\"Hello World boi\")\n").expect("script should parse");

    let Item::Function(function) = &program.items[0] else {
        panic!("expected synthetic main function");
    };
    assert_eq!(function.name, "main");
    assert_eq!(
        function.return_type.as_ref().map(|ty| ty.name.as_str()),
        Some("i32")
    );
    assert_eq!(function.body.statements.len(), 2);
    assert!(matches!(function.body.statements[0], Stmt::Expr(_)));
    assert!(matches!(function.body.statements[1], Stmt::Return(_)));
}

#[test]
fn rejects_top_level_statements_mixed_with_explicit_main() {
    let error = parse_source("println(\"hi\")\n\ndef main() -> i32:\n    return 0\n")
        .expect_err("top-level statements mixed with main should fail");
    assert!(error
        .message
        .contains("top-level statements cannot be combined with an explicit `main()`"));
}

#[test]
fn parses_extern_function_declaration() {
    let program = parse_source("extern def add_from_c(a: i32, b: i32) -> i32\n")
        .expect("extern function should parse");

    let Item::Function(function) = &program.items[0] else {
        panic!("expected function item");
    };
    assert!(function.is_extern);
    assert_eq!(function.name, "add_from_c");
    assert_eq!(function.params.len(), 2);
    assert_eq!(
        function.return_type.as_ref().map(|ty| ty.name.as_str()),
        Some("i32")
    );
    assert!(function.body.statements.is_empty());
}

#[test]
fn parses_fstrings_as_string_expressions() {
    let program =
        parse_source("def main() -> unit:\n    println(f\"hello {40 + 2}\")\n    return\n")
            .expect("f-string program should parse");

    let Item::Function(function) = &program.items[0] else {
        panic!("expected function item");
    };
    let Stmt::Expr(expr_stmt) = &function.body.statements[0] else {
        panic!("expected expression statement");
    };

    match &expr_stmt.expr.kind {
        ExprKind::Call { args, .. } => match &args[0] {
            CallArg::Positional(expr) => assert!(matches!(expr.kind, ExprKind::Binary { .. })),
            other => panic!("expected positional argument, found {other:?}"),
        },
        other => panic!("expected call expression, found {other:?}"),
    }
}

#[test]
fn parses_augmented_assignment() {
    let program = parse_source(
        "def f() -> unit:\n    let x: i64 = 0\n    x += 5\n    x -= 2\n    x *= 3\n",
    )
    .expect("augmented assignment should parse");

    let Item::Function(func) = &program.items[0] else { panic!() };
    // augmented assignments desugar to plain assigns at parse time
    // x += 5 becomes x = x + 5
    let Stmt::Assign(a) = &func.body.statements[1] else {
        panic!("expected assign statement for x += 5, got {:?}", func.body.statements[1]);
    };
    assert_eq!(a.name, "x");
    // value should be a binary add
    match &a.value.kind {
        ExprKind::Binary { op, .. } => {
            assert_eq!(format!("{op:?}"), "Add");
        }
        other => panic!("expected Binary(Add), got {other:?}"),
    }
}

#[test]
fn parses_field_assignment() {
    let program = parse_source(
        "def f() -> unit:\n    obj.x = 10\n",
    )
    .expect("field assignment should parse");

    let Item::Function(func) = &program.items[0] else { panic!() };
    let Stmt::FieldAssign(fa) = &func.body.statements[0] else {
        panic!("expected FieldAssign, got {:?}", func.body.statements[0]);
    };
    assert_eq!(fa.base, "obj");
    assert_eq!(fa.fields, vec!["x"]);
}

#[test]
fn parses_assert_desugars_to_if() {
    let program = parse_source(
        "def f() -> unit:\n    assert x > 0\n",
    )
    .expect("assert should parse");

    let Item::Function(func) = &program.items[0] else { panic!() };
    // assert desugars to if not (cond): panic(...)
    let Stmt::If(_) = &func.body.statements[0] else {
        panic!("assert should desugar to if, got {:?}", func.body.statements[0]);
    };
}

#[test]
fn parses_bitwise_binary_ops() {
    let program = parse_source(
        "def f() -> unit:\n    let a = x & y\n    let b = x | y\n    let c = x ^ y\n    let d = x << 2\n    let e = x >> 1\n",
    )
    .expect("bitwise ops should parse");

    let Item::Function(func) = &program.items[0] else { panic!() };

    let ops: Vec<String> = func.body.statements.iter().filter_map(|s| {
        if let Stmt::Let(ls) = s {
            if let ExprKind::Binary { op, .. } = &ls.value.kind {
                return Some(format!("{op:?}"));
            }
        }
        None
    }).collect();

    assert!(ops.contains(&"BitwiseAnd".to_string()), "missing BitwiseAnd, got {ops:?}");
    assert!(ops.contains(&"BitwiseOr".to_string()), "missing BitwiseOr");
    assert!(ops.contains(&"BitwiseXor".to_string()), "missing BitwiseXor");
    assert!(ops.contains(&"ShiftLeft".to_string()), "missing ShiftLeft");
    assert!(ops.contains(&"ShiftRight".to_string()), "missing ShiftRight");
}

#[test]
fn parses_match_with_int_cases() {
    // match with two integer arms and no wildcard — desugars to if/elif with no else.
    let src = concat!(
        "def check(x: i64) -> i64:\n",
        "    match x:\n",
        "        case 1:\n",
        "            return 10\n",
        "        case 2:\n",
        "            return 20\n",
        "    return 0\n",
    );

    let program = parse_source(src).expect("program should parse");

    let Item::Function(function) = &program.items[0] else {
        panic!("expected function item");
    };

    // First statement is the desugared if/elif
    let Stmt::If(if_stmt) = &function.body.statements[0] else {
        panic!("expected if statement from match desugar");
    };

    // First arm desugars to `if x == 1`
    match &if_stmt.condition.kind {
        ExprKind::Binary { op, right, .. } => {
            assert_eq!(format!("{op:?}"), "EqualEqual");
            assert_eq!(format!("{:?}", right.kind), r#"Integer("1")"#);
        }
        other => panic!("unexpected condition kind: {other:?}"),
    }

    // Second arm desugars to elif `x == 2`
    assert_eq!(if_stmt.elif_blocks.len(), 1);
    match &if_stmt.elif_blocks[0].condition.kind {
        ExprKind::Binary { op, right, .. } => {
            assert_eq!(format!("{op:?}"), "EqualEqual");
            assert_eq!(format!("{:?}", right.kind), r#"Integer("2")"#);
        }
        other => panic!("unexpected elif condition: {other:?}"),
    }

    // No wildcard → no else block
    assert!(if_stmt.else_block.is_none());
}

#[test]
fn parses_match_with_string_cases() {
    // match with string literal arms — desugars to if/elif.
    let src = concat!(
        "def greet(name: str) -> i64:\n",
        "    match name:\n",
        "        case \"alice\":\n",
        "            return 1\n",
        "        case \"bob\":\n",
        "            return 2\n",
        "    return 0\n",
    );

    let program = parse_source(src).expect("program should parse");

    let Item::Function(function) = &program.items[0] else {
        panic!("expected function item");
    };

    let Stmt::If(if_stmt) = &function.body.statements[0] else {
        panic!("expected if statement from match desugar");
    };

    match &if_stmt.condition.kind {
        ExprKind::Binary { op, right, .. } => {
            assert_eq!(format!("{op:?}"), "EqualEqual");
            assert_eq!(format!("{:?}", right.kind), r#"String("alice")"#);
        }
        other => panic!("unexpected condition kind: {other:?}"),
    }

    assert_eq!(if_stmt.elif_blocks.len(), 1);
    match &if_stmt.elif_blocks[0].condition.kind {
        ExprKind::Binary { op, right, .. } => {
            assert_eq!(format!("{op:?}"), "EqualEqual");
            assert_eq!(format!("{:?}", right.kind), r#"String("bob")"#);
        }
        other => panic!("unexpected elif condition: {other:?}"),
    }

    assert!(if_stmt.else_block.is_none());
}

#[test]
fn parses_match_with_wildcard_default() {
    // match with a wildcard arm — desugars to if/elif/else.
    let src = concat!(
        "def label(x: i64) -> i64:\n",
        "    match x:\n",
        "        case 1:\n",
        "            return 100\n",
        "        case 2:\n",
        "            return 200\n",
        "        case _:\n",
        "            return 0\n",
        "    return -1\n",
    );

    let program = parse_source(src).expect("program should parse");

    let Item::Function(function) = &program.items[0] else {
        panic!("expected function item");
    };

    let Stmt::If(if_stmt) = &function.body.statements[0] else {
        panic!("expected if statement from match desugar");
    };

    // First arm: if x == 1
    match &if_stmt.condition.kind {
        ExprKind::Binary { op, right, .. } => {
            assert_eq!(format!("{op:?}"), "EqualEqual");
            assert_eq!(format!("{:?}", right.kind), r#"Integer("1")"#);
        }
        other => panic!("unexpected condition: {other:?}"),
    }

    // Second arm: elif x == 2
    assert_eq!(if_stmt.elif_blocks.len(), 1);

    // Wildcard arm becomes else block
    assert!(if_stmt.else_block.is_some());
    let else_block = if_stmt.else_block.as_ref().unwrap();
    assert_eq!(else_block.statements.len(), 1);
}

#[test]
fn parses_bitwise_not_unary() {
    let program = parse_source(
        "def f() -> unit:\n    let a = ~x\n",
    )
    .expect("bitwise not should parse");

    let Item::Function(func) = &program.items[0] else { panic!() };
    let Stmt::Let(ls) = &func.body.statements[0] else { panic!() };
    match &ls.value.kind {
        ExprKind::Unary { op, .. } => assert_eq!(format!("{op:?}"), "BitwiseNot"),
        other => panic!("expected Unary(BitwiseNot), got {other:?}"),
    }
}

#[test]
fn parses_for_range_one_arg_desugars_to_block() {
    // for i in range(10): ... desugars to a Block containing let bindings + while
    let program = parse_source(
        "def f() -> unit:\n    for i in range(10):\n        println(i)\n",
    )
    .expect("for range(stop) should parse");

    let Item::Function(func) = &program.items[0] else { panic!() };
    // The for loop desugars to a Block at the top level
    let Stmt::Block(_block) = &func.body.statements[0] else {
        panic!("for loop should desugar to a Block, got {:?}", func.body.statements[0]);
    };
}

#[test]
fn parses_for_range_two_args() {
    let program = parse_source(
        "def f() -> unit:\n    for i in range(2, 8):\n        println(i)\n",
    )
    .expect("for range(start, stop) should parse");

    let Item::Function(func) = &program.items[0] else { panic!() };
    let Stmt::Block(_) = &func.body.statements[0] else {
        panic!("for range(start, stop) should desugar to Block");
    };
}

#[test]
fn parses_for_range_three_args() {
    let program = parse_source(
        "def f() -> unit:\n    for i in range(0, 20, 2):\n        println(i)\n",
    )
    .expect("for range(start, stop, step) should parse");

    let Item::Function(func) = &program.items[0] else { panic!() };
    let Stmt::Block(_) = &func.body.statements[0] else {
        panic!("for range(start, stop, step) should desugar to Block");
    };
}

#[test]
fn rejects_for_with_non_range_iterable() {
    let err = parse_source("def f() -> unit:\n    for i in items:\n        println(i)\n")
        .expect_err("for with non-range iterable should fail");
    assert!(
        err.message.contains("range"),
        "error should mention range, got: {}",
        err.message
    );
}
