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
