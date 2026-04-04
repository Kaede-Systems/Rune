use rune::optimize::{optimize_program, prune_program_for_executable};
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

#[test]
fn prunes_unreachable_functions_for_executable_roots() {
    let mut program = parse_source(
        "def live() -> i32:\n    return 1\n\n\
         def dead_helper() -> i32:\n    return 7\n\n\
         def main() -> i32:\n    return live()\n",
    )
    .unwrap();

    optimize_program(&mut program);
    prune_program_for_executable(&mut program);

    let function_names = program
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Function(function) => Some(function.name.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert!(function_names.contains(&"main"));
    assert!(function_names.contains(&"live"));
    assert!(!function_names.contains(&"dead_helper"));
}

#[test]
fn prunes_unreachable_exceptions_for_executable_roots() {
    let mut program = parse_source(
        r#"exception UsedError
exception DeadError

def helper() -> unit raises UsedError:
    raise UsedError("bad")

def dead_helper() -> unit raises DeadError:
    raise DeadError("dead")

def main() -> i32:
    helper()
    return 0
"#,
    )
    .unwrap();

    optimize_program(&mut program);
    prune_program_for_executable(&mut program);

    let exception_names = program
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Exception(exception) => Some(exception.name.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert!(exception_names.contains(&"UsedError"));
    assert!(!exception_names.contains(&"DeadError"));
}
