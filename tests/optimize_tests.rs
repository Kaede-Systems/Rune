use rune::optimize::optimize_program;
use rune::parser::{ExprKind, Item, Stmt, parse_source};

#[test]
fn folds_constant_arithmetic() {
    let mut program = parse_source("def main() -> i64:\n    return 40 + 2\n").unwrap();
    optimize_program(&mut program);

    let Item::Function(function) = &program.items[0] else {
        panic!("expected function item");
    };
    let Stmt::Return(stmt) = &function.body.statements[0] else {
        panic!("expected return");
    };
    match &stmt.value.as_ref().unwrap().kind {
        ExprKind::Integer(value) => assert_eq!(value, "42"),
        other => panic!("expected folded integer, found {other:?}"),
    }
}

#[test]
fn folds_constant_if_statements() {
    let mut program =
        parse_source("def main() -> i64:\n    if true:\n        return 7\n    return 0\n").unwrap();
    optimize_program(&mut program);

    let Item::Function(function) = &program.items[0] else {
        panic!("expected function item");
    };
    assert_eq!(function.body.statements.len(), 2);
    let Stmt::Return(stmt) = &function.body.statements[0] else {
        panic!("expected return");
    };
    match &stmt.value.as_ref().unwrap().kind {
        ExprKind::Integer(value) => assert_eq!(value, "7"),
        other => panic!("expected folded return, found {other:?}"),
    }
}
