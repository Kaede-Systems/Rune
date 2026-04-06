use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::render_file_diagnostic;
use crate::frontend::lexer::Span;
use crate::frontend::parser::{
    AssignStmt, BinaryOp, Block, CallArg, ElifBlock, ExceptionDecl, Expr, ExprKind, ExprStmt,
    FieldAssignStmt, Function, IfStmt, ImportDecl, Item, LetStmt, PanicStmt, Param, Program,
    RaiseStmt, ReturnStmt, Stmt, StructDecl, StructField, TypeRef, WhileStmt, parse_source,
};

pub enum BuiltinModuleBody {
    Program(Program),
}

pub struct BuiltinModule {
    pub virtual_path: PathBuf,
    pub body: BuiltinModuleBody,
}

fn s() -> Span {
    Span { line: 1, column: 1 }
}

fn ty(name: &str) -> TypeRef {
    TypeRef {
        name: name.to_string(),
        span: s(),
    }
}

fn param(name: &str, ty_name: &str) -> Param {
    Param {
        name: name.to_string(),
        ty: ty(ty_name),
        span: s(),
    }
}

fn ident(name: &str) -> Expr {
    Expr {
        kind: ExprKind::Identifier(name.to_string()),
        span: s(),
    }
}

fn string_lit(value: &str) -> Expr {
    Expr {
        kind: ExprKind::String(value.to_string()),
        span: s(),
    }
}

fn int_lit(value: i64) -> Expr {
    Expr {
        kind: ExprKind::Integer(value.to_string()),
        span: s(),
    }
}

fn bool_lit(value: bool) -> Expr {
    Expr {
        kind: ExprKind::Bool(value),
        span: s(),
    }
}

fn field(base: Expr, name: &str) -> Expr {
    Expr {
        kind: ExprKind::Field {
            base: Box::new(base),
            name: name.to_string(),
        },
        span: s(),
    }
}

fn call_name(name: &str, args: Vec<CallArg>) -> Expr {
    Expr {
        kind: ExprKind::Call {
            callee: Box::new(ident(name)),
            args,
        },
        span: s(),
    }
}

fn call_expr(callee: Expr, args: Vec<CallArg>) -> Expr {
    Expr {
        kind: ExprKind::Call {
            callee: Box::new(callee),
            args,
        },
        span: s(),
    }
}

fn pos(expr: Expr) -> CallArg {
    CallArg::Positional(expr)
}

fn kw(name: &str, value: Expr) -> CallArg {
    CallArg::Keyword {
        name: name.to_string(),
        value,
        span: s(),
    }
}

fn binary(left: Expr, op: BinaryOp, right: Expr) -> Expr {
    Expr {
        kind: ExprKind::Binary {
            left: Box::new(left),
            op,
            right: Box::new(right),
        },
        span: s(),
    }
}

fn return_stmt(expr: Expr) -> Stmt {
    Stmt::Return(ReturnStmt {
        value: Some(expr),
        span: s(),
    })
}

fn return_unit_stmt() -> Stmt {
    Stmt::Return(ReturnStmt {
        value: None,
        span: s(),
    })
}

fn expr_stmt(expr: Expr) -> Stmt {
    Stmt::Expr(crate::parser::ExprStmt { expr })
}

fn if_stmt(condition: Expr, then_block: Vec<Stmt>, else_block: Option<Vec<Stmt>>) -> Stmt {
    Stmt::If(IfStmt {
        condition,
        then_block: Block {
            statements: then_block,
        },
        elif_blocks: Vec::<ElifBlock>::new(),
        else_block: else_block.map(|statements| Block { statements }),
        span: s(),
    })
}

fn function(name: &str, params: Vec<Param>, return_type: &str, body: Vec<Stmt>) -> Function {
    Function {
        is_extern: false,
        is_async: false,
        name: name.to_string(),
        params,
        return_type: Some(ty(return_type)),
        raises: None,
        body: Block { statements: body },
        span: s(),
    }
}

fn env_program() -> Program {
    Program {
        items: vec![
            Item::Function(function(
                "has",
                vec![param("name", "String")],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_env_exists",
                    vec![pos(ident("name"))],
                ))],
            )),
            Item::Function(function(
                "exists",
                vec![param("name", "String")],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_env_exists",
                    vec![pos(ident("name"))],
                ))],
            )),
            Item::Function(function(
                "get_i32",
                vec![param("name", "String"), param("default", "i32")],
                "i32",
                vec![return_stmt(call_name(
                    "__rune_builtin_env_get_i32",
                    vec![pos(ident("name")), pos(ident("default"))],
                ))],
            )),
            Item::Function(function(
                "get_bool",
                vec![param("name", "String"), param("default", "bool")],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_env_get_bool",
                    vec![pos(ident("name")), pos(ident("default"))],
                ))],
            )),
            Item::Function(function(
                "get",
                vec![param("name", "String"), param("default", "String")],
                "String",
                vec![return_stmt(call_name(
                    "__rune_builtin_env_get_string",
                    vec![pos(ident("name")), pos(ident("default"))],
                ))],
            )),
            Item::Function(function(
                "arg_count",
                vec![],
                "i32",
                vec![return_stmt(call_name("__rune_builtin_env_arg_count", vec![]))],
            )),
            Item::Function(function(
                "arg",
                vec![param("index", "i32")],
                "String",
                vec![return_stmt(call_name(
                    "__rune_builtin_env_arg",
                    vec![pos(ident("index"))],
                ))],
            )),
            Item::Function(function(
                "arg_or",
                vec![param("index", "i32"), param("default", "String")],
                "String",
                vec![
                    if_stmt(
                        binary(ident("index"), BinaryOp::Less, int_lit(0)),
                        vec![return_stmt(ident("default"))],
                        None,
                    ),
                    if_stmt(
                        binary(
                            ident("index"),
                            BinaryOp::Less,
                            call_name("arg_count", vec![]),
                        ),
                        vec![return_stmt(call_name("arg", vec![pos(ident("index"))]))],
                        None,
                    ),
                    return_stmt(ident("default")),
                ],
            )),
            Item::Function(function(
                "get_i32_or_zero",
                vec![param("name", "String")],
                "i32",
                vec![return_stmt(call_name(
                    "get_i32",
                    vec![pos(ident("name")), pos(int_lit(0))],
                ))],
            )),
            Item::Function(function(
                "get_bool_or_false",
                vec![param("name", "String")],
                "bool",
                vec![return_stmt(call_name(
                    "get_bool",
                    vec![pos(ident("name")), pos(bool_lit(false))],
                ))],
            )),
            Item::Function(function(
                "get_bool_or_true",
                vec![param("name", "String")],
                "bool",
                vec![return_stmt(call_name(
                    "get_bool",
                    vec![pos(ident("name")), pos(bool_lit(true))],
                ))],
            )),
            Item::Function(function(
                "get_or_empty",
                vec![param("name", "String")],
                "String",
                vec![return_stmt(call_name(
                    "get",
                    vec![pos(ident("name")), pos(string_lit(""))],
                ))],
            )),
        ],
    }
}

fn time_program() -> Program {
    Program {
        items: vec![
            Item::Function(function(
                "unix_now",
                vec![],
                "i64",
                vec![return_stmt(call_name("__rune_builtin_time_now_unix", vec![]))],
            )),
            Item::Function(function(
                "has_wall_clock",
                vec![],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_time_has_wall_clock",
                    vec![],
                ))],
            )),
            Item::Function(function(
                "monotonic_ms",
                vec![],
                "i64",
                vec![return_stmt(call_name(
                    "__rune_builtin_time_monotonic_ms",
                    vec![],
                ))],
            )),
            Item::Function(function(
                "monotonic_us",
                vec![],
                "i64",
                vec![return_stmt(call_name(
                    "__rune_builtin_time_monotonic_us",
                    vec![],
                ))],
            )),
            Item::Function(function(
                "sleep_ms",
                vec![param("ms", "i64")],
                "unit",
                vec![expr_stmt(call_name(
                    "__rune_builtin_time_sleep_ms",
                    vec![pos(ident("ms"))],
                ))],
            )),
            Item::Function(function(
                "sleep_us",
                vec![param("us", "i64")],
                "unit",
                vec![expr_stmt(call_name(
                    "__rune_builtin_time_sleep_us",
                    vec![pos(ident("us"))],
                ))],
            )),
            Item::Function(function(
                "sleep",
                vec![param("seconds", "i64")],
                "unit",
                vec![expr_stmt(call_name(
                    "sleep_ms",
                    vec![pos(binary(ident("seconds"), BinaryOp::Multiply, int_lit(1000)))],
                ))],
            )),
            Item::Function(function(
                "sleep_until",
                vec![param("deadline_ms", "i64")],
                "unit",
                vec![
                    Stmt::Let(crate::parser::LetStmt {
                        name: "now".to_string(),
                        ty: Some(ty("i64")),
                        value: call_name("monotonic_ms", vec![]),
                        span: s(),
                    }),
                    if_stmt(
                        binary(ident("deadline_ms"), BinaryOp::Greater, ident("now")),
                        vec![expr_stmt(call_name(
                            "sleep_ms",
                            vec![pos(binary(
                                ident("deadline_ms"),
                                BinaryOp::Subtract,
                                ident("now"),
                            ))],
                        ))],
                        None,
                    ),
                ],
            )),
            Item::Function(function(
                "sleep_until_us",
                vec![param("deadline_us", "i64")],
                "unit",
                vec![
                    Stmt::Let(crate::parser::LetStmt {
                        name: "now".to_string(),
                        ty: Some(ty("i64")),
                        value: call_name("monotonic_us", vec![]),
                        span: s(),
                    }),
                    if_stmt(
                        binary(ident("deadline_us"), BinaryOp::Greater, ident("now")),
                        vec![expr_stmt(call_name(
                            "sleep_us",
                            vec![pos(binary(
                                ident("deadline_us"),
                                BinaryOp::Subtract,
                                ident("now"),
                            ))],
                        ))],
                        None,
                    ),
                ],
            )),
        ],
    }
}

fn clock_program() -> Program {
    Program {
        items: vec![
            Item::Function(function(
                "ticks_ms",
                vec![],
                "i64",
                vec![return_stmt(call_name("monotonic_ms", vec![]))],
            )),
            Item::Function(function(
                "has_wall_clock",
                vec![],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_time_has_wall_clock",
                    vec![],
                ))],
            )),
            Item::Function(function(
                "ticks_us",
                vec![],
                "i64",
                vec![return_stmt(call_name("monotonic_us", vec![]))],
            )),
            Item::Function(function(
                "monotonic_ms",
                vec![],
                "i64",
                vec![return_stmt(call_name(
                    "__rune_builtin_time_monotonic_ms",
                    vec![],
                ))],
            )),
            Item::Function(function(
                "monotonic_us",
                vec![],
                "i64",
                vec![return_stmt(call_name(
                    "__rune_builtin_time_monotonic_us",
                    vec![],
                ))],
            )),
            Item::Function(function(
                "sleep_ms",
                vec![param("ms", "i64")],
                "unit",
                vec![expr_stmt(call_name(
                    "__rune_builtin_time_sleep_ms",
                    vec![pos(ident("ms"))],
                ))],
            )),
            Item::Function(function(
                "sleep_us",
                vec![param("us", "i64")],
                "unit",
                vec![expr_stmt(call_name(
                    "__rune_builtin_time_sleep_us",
                    vec![pos(ident("us"))],
                ))],
            )),
            Item::Function(function(
                "sleep",
                vec![param("seconds", "i64")],
                "unit",
                vec![expr_stmt(call_name(
                    "sleep_ms",
                    vec![pos(binary(ident("seconds"), BinaryOp::Multiply, int_lit(1000)))],
                ))],
            )),
            Item::Function(function(
                "elapsed_ms",
                vec![param("start_ms", "i64")],
                "i64",
                vec![return_stmt(binary(
                    call_name("ticks_ms", vec![]),
                    BinaryOp::Subtract,
                    ident("start_ms"),
                ))],
            )),
            Item::Function(function(
                "elapsed_us",
                vec![param("start_us", "i64")],
                "i64",
                vec![return_stmt(binary(
                    call_name("ticks_us", vec![]),
                    BinaryOp::Subtract,
                    ident("start_us"),
                ))],
            )),
            Item::Function(function(
                "wait_until_ms",
                vec![param("deadline_ms", "i64")],
                "unit",
                vec![
                    Stmt::Let(crate::parser::LetStmt {
                        name: "now".to_string(),
                        ty: Some(ty("i64")),
                        value: call_name("ticks_ms", vec![]),
                        span: s(),
                    }),
                    if_stmt(
                        binary(ident("deadline_ms"), BinaryOp::Greater, ident("now")),
                        vec![expr_stmt(call_name(
                            "sleep_ms",
                            vec![pos(binary(
                                ident("deadline_ms"),
                                BinaryOp::Subtract,
                                ident("now"),
                            ))],
                        ))],
                        None,
                    ),
                ],
            )),
            Item::Function(function(
                "wait_until_us",
                vec![param("deadline_us", "i64")],
                "unit",
                vec![
                    Stmt::Let(crate::parser::LetStmt {
                        name: "now".to_string(),
                        ty: Some(ty("i64")),
                        value: call_name("ticks_us", vec![]),
                        span: s(),
                    }),
                    if_stmt(
                        binary(ident("deadline_us"), BinaryOp::Greater, ident("now")),
                        vec![expr_stmt(call_name(
                            "sleep_us",
                            vec![pos(binary(
                                ident("deadline_us"),
                                BinaryOp::Subtract,
                                ident("now"),
                            ))],
                        ))],
                        None,
                    ),
                ],
            )),
        ],
    }
}

fn sys_program() -> Program {
    Program {
        items: vec![
            Item::Function(function(
                "pid",
                vec![],
                "i32",
                vec![return_stmt(call_name("__rune_builtin_system_pid", vec![]))],
            )),
            Item::Function(function(
                "cpu_count",
                vec![],
                "i32",
                vec![return_stmt(call_name(
                    "__rune_builtin_system_cpu_count",
                    vec![],
                ))],
            )),
            Item::Function(function(
                "platform",
                vec![],
                "String",
                vec![return_stmt(call_name(
                    "__rune_builtin_system_platform",
                    vec![],
                ))],
            )),
            Item::Function(function(
                "arch",
                vec![],
                "String",
                vec![return_stmt(call_name("__rune_builtin_system_arch", vec![]))],
            )),
            Item::Function(function(
                "target",
                vec![],
                "String",
                vec![return_stmt(call_name("__rune_builtin_system_target", vec![]))],
            )),
            Item::Function(function(
                "board",
                vec![],
                "String",
                vec![return_stmt(call_name("__rune_builtin_system_board", vec![]))],
            )),
            Item::Function(function(
                "is_embedded",
                vec![],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_system_is_embedded",
                    vec![],
                ))],
            )),
            Item::Function(function(
                "is_wasm",
                vec![],
                "bool",
                vec![return_stmt(call_name("__rune_builtin_system_is_wasm", vec![]))],
            )),
            Item::Function(function(
                "is_host",
                vec![],
                "bool",
                vec![return_stmt(Expr {
                    kind: ExprKind::Unary {
                        op: crate::parser::UnaryOp::Not,
                        expr: Box::new(binary(
                            call_name("is_embedded", vec![]),
                            BinaryOp::Or,
                            call_name("is_wasm", vec![]),
                        )),
                    },
                    span: s(),
                })],
            )),
            Item::Function(function(
                "is_desktop",
                vec![],
                "bool",
                vec![return_stmt(call_name("is_host", vec![]))],
            )),
            Item::Function(function(
                "is_windows",
                vec![],
                "bool",
                vec![return_stmt(binary(
                    call_name("platform", vec![]),
                    BinaryOp::EqualEqual,
                    string_lit("windows"),
                ))],
            )),
            Item::Function(function(
                "is_linux",
                vec![],
                "bool",
                vec![return_stmt(binary(
                    call_name("platform", vec![]),
                    BinaryOp::EqualEqual,
                    string_lit("linux"),
                ))],
            )),
            Item::Function(function(
                "is_macos",
                vec![],
                "bool",
                vec![return_stmt(binary(
                    call_name("platform", vec![]),
                    BinaryOp::EqualEqual,
                    string_lit("macos"),
                ))],
            )),
            Item::Function(function(
                "exit",
                vec![param("code", "i32")],
                "unit",
                vec![expr_stmt(call_name(
                    "__rune_builtin_system_exit",
                    vec![pos(ident("code"))],
                ))],
            )),
            Item::Function(function(
                "quit",
                vec![param("code", "i32")],
                "unit",
                vec![expr_stmt(call_name("exit", vec![pos(ident("code"))]))],
            )),
            Item::Function(function(
                "exit_success",
                vec![],
                "unit",
                vec![expr_stmt(call_name("exit", vec![pos(int_lit(0))]))],
            )),
            Item::Function(function(
                "exit_failure",
                vec![],
                "unit",
                vec![expr_stmt(call_name("exit", vec![pos(int_lit(1))]))],
            )),
        ],
    }
}

fn io_program() -> Program {
    Program {
        items: vec![
            Item::Function(function(
                "write",
                vec![param("value", "dynamic")],
                "unit",
                vec![expr_stmt(call_name("print", vec![pos(call_name("str", vec![pos(ident("value"))]))]))],
            )),
            Item::Function(function(
                "stdout_write",
                vec![param("value", "dynamic")],
                "unit",
                vec![expr_stmt(call_name("write", vec![pos(ident("value"))]))],
            )),
            Item::Function(function(
                "writeln",
                vec![param("value", "dynamic")],
                "unit",
                vec![expr_stmt(call_name(
                    "println",
                    vec![pos(call_name("str", vec![pos(ident("value"))]))],
                ))],
            )),
            Item::Function(function(
                "stdout_writeln",
                vec![param("value", "dynamic")],
                "unit",
                vec![expr_stmt(call_name("writeln", vec![pos(ident("value"))]))],
            )),
            Item::Function(function(
                "error",
                vec![param("value", "dynamic")],
                "unit",
                vec![expr_stmt(call_name(
                    "eprint",
                    vec![pos(call_name("str", vec![pos(ident("value"))]))],
                ))],
            )),
            Item::Function(function(
                "stderr_write",
                vec![param("value", "dynamic")],
                "unit",
                vec![expr_stmt(call_name("error", vec![pos(ident("value"))]))],
            )),
            Item::Function(function(
                "errorln",
                vec![param("value", "dynamic")],
                "unit",
                vec![expr_stmt(call_name(
                    "eprintln",
                    vec![pos(call_name("str", vec![pos(ident("value"))]))],
                ))],
            )),
            Item::Function(function(
                "stderr_writeln",
                vec![param("value", "dynamic")],
                "unit",
                vec![expr_stmt(call_name("errorln", vec![pos(ident("value"))]))],
            )),
            Item::Function(function(
                "flush_out",
                vec![],
                "unit",
                vec![expr_stmt(call_name("flush", vec![]))],
            )),
            Item::Function(function(
                "flush_stdout",
                vec![],
                "unit",
                vec![expr_stmt(call_name("flush_out", vec![]))],
            )),
            Item::Function(function(
                "flush_err",
                vec![],
                "unit",
                vec![expr_stmt(call_name("eflush", vec![]))],
            )),
            Item::Function(function(
                "flush_stderr",
                vec![],
                "unit",
                vec![expr_stmt(call_name("flush_err", vec![]))],
            )),
            Item::Function(function(
                "read_line",
                vec![],
                "String",
                vec![return_stmt(call_name("input", vec![]))],
            )),
            Item::Function(function(
                "prompt",
                vec![param("message", "String")],
                "String",
                vec![
                    expr_stmt(call_name("write", vec![pos(ident("message"))])),
                    expr_stmt(call_name("flush_out", vec![])),
                    return_stmt(call_name("read_line", vec![])),
                ],
            )),
            Item::Function(function(
                "error_prompt",
                vec![param("message", "String")],
                "String",
                vec![
                    expr_stmt(call_name("error", vec![pos(ident("message"))])),
                    expr_stmt(call_name("flush_err", vec![])),
                    return_stmt(call_name("read_line", vec![])),
                ],
            )),
        ],
    }
}

fn terminal_program() -> Program {
    Program {
        items: vec![
            Item::Function(function(
                "clear",
                vec![],
                "unit",
                vec![expr_stmt(call_name("__rune_builtin_terminal_clear", vec![]))],
            )),
            Item::Function(function(
                "clear_screen",
                vec![],
                "unit",
                vec![expr_stmt(call_name("clear", vec![]))],
            )),
            Item::Function(function(
                "move_to",
                vec![param("row", "i32"), param("col", "i32")],
                "unit",
                vec![expr_stmt(call_name(
                    "__rune_builtin_terminal_move_to",
                    vec![pos(ident("row")), pos(ident("col"))],
                ))],
            )),
            Item::Function(function(
                "move_cursor",
                vec![param("row", "i32"), param("col", "i32")],
                "unit",
                vec![expr_stmt(call_name(
                    "move_to",
                    vec![pos(ident("row")), pos(ident("col"))],
                ))],
            )),
            Item::Function(function(
                "hide_cursor",
                vec![],
                "unit",
                vec![expr_stmt(call_name(
                    "__rune_builtin_terminal_hide_cursor",
                    vec![],
                ))],
            )),
            Item::Function(function(
                "cursor_hide",
                vec![],
                "unit",
                vec![expr_stmt(call_name("hide_cursor", vec![]))],
            )),
            Item::Function(function(
                "show_cursor",
                vec![],
                "unit",
                vec![expr_stmt(call_name(
                    "__rune_builtin_terminal_show_cursor",
                    vec![],
                ))],
            )),
            Item::Function(function(
                "cursor_show",
                vec![],
                "unit",
                vec![expr_stmt(call_name("show_cursor", vec![]))],
            )),
            Item::Function(function(
                "set_title",
                vec![param("title", "String")],
                "unit",
                vec![expr_stmt(call_name(
                    "__rune_builtin_terminal_set_title",
                    vec![pos(ident("title"))],
                ))],
            )),
            Item::Function(function(
                "title",
                vec![param("text", "String")],
                "unit",
                vec![expr_stmt(call_name("set_title", vec![pos(ident("text"))]))],
            )),
            Item::Function(function(
                "home",
                vec![],
                "unit",
                vec![expr_stmt(call_name(
                    "move_to",
                    vec![pos(int_lit(1)), pos(int_lit(1))],
                ))],
            )),
            Item::Function(function(
                "clear_and_home",
                vec![],
                "unit",
                vec![
                    expr_stmt(call_name("clear", vec![])),
                    expr_stmt(call_name("move_to", vec![pos(int_lit(1)), pos(int_lit(1))])),
                ],
            )),
            Item::Function(function(
                "hide",
                vec![],
                "unit",
                vec![expr_stmt(call_name("hide_cursor", vec![]))],
            )),
            Item::Function(function(
                "show",
                vec![],
                "unit",
                vec![expr_stmt(call_name("show_cursor", vec![]))],
            )),
        ],
    }
}

fn fs_program() -> Program {
    Program {
        items: vec![
            Item::Function(function(
                "current_dir",
                vec![],
                "String",
                vec![return_stmt(call_name("__rune_builtin_fs_current_dir", vec![]))],
            )),
            Item::Function(function(
                "cwd",
                vec![],
                "String",
                vec![return_stmt(call_name("current_dir", vec![]))],
            )),
            Item::Function(function(
                "set_current_dir",
                vec![param("path", "String")],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_fs_set_current_dir",
                    vec![pos(ident("path"))],
                ))],
            )),
            Item::Function(function(
                "chdir",
                vec![param("path", "String")],
                "bool",
                vec![return_stmt(call_name(
                    "set_current_dir",
                    vec![pos(ident("path"))],
                ))],
            )),
            Item::Function(function(
                "exists",
                vec![param("path", "String")],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_fs_exists",
                    vec![pos(ident("path"))],
                ))],
            )),
            Item::Function(function(
                "read",
                vec![param("path", "String")],
                "String",
                vec![return_stmt(call_name("read_string", vec![pos(ident("path"))]))],
            )),
            Item::Function(function(
                "read_string",
                vec![param("path", "String")],
                "String",
                vec![return_stmt(call_name(
                    "__rune_builtin_fs_read_string",
                    vec![pos(ident("path"))],
                ))],
            )),
            Item::Function(function(
                "read_text",
                vec![param("path", "String")],
                "String",
                vec![return_stmt(call_name("read_string", vec![pos(ident("path"))]))],
            )),
            Item::Function(function(
                "write_string",
                vec![param("path", "String"), param("content", "String")],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_fs_write_string",
                    vec![pos(ident("path")), pos(ident("content"))],
                ))],
            )),
            Item::Function(function(
                "write",
                vec![param("path", "String"), param("content", "String")],
                "bool",
                vec![return_stmt(call_name(
                    "write_string",
                    vec![pos(ident("path")), pos(ident("content"))],
                ))],
            )),
            Item::Function(function(
                "write_text",
                vec![param("path", "String"), param("content", "String")],
                "bool",
                vec![return_stmt(call_name(
                    "write_string",
                    vec![pos(ident("path")), pos(ident("content"))],
                ))],
            )),
            Item::Function(function(
                "append_string",
                vec![param("path", "String"), param("content", "String")],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_fs_append_string",
                    vec![pos(ident("path")), pos(ident("content"))],
                ))],
            )),
            Item::Function(function(
                "append_text",
                vec![param("path", "String"), param("content", "String")],
                "bool",
                vec![return_stmt(call_name(
                    "append_string",
                    vec![pos(ident("path")), pos(ident("content"))],
                ))],
            )),
            Item::Function(function(
                "append",
                vec![param("path", "String"), param("content", "String")],
                "bool",
                vec![return_stmt(call_name(
                    "append_string",
                    vec![pos(ident("path")), pos(ident("content"))],
                ))],
            )),
            Item::Function(function(
                "remove",
                vec![param("path", "String")],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_fs_remove",
                    vec![pos(ident("path"))],
                ))],
            )),
            Item::Function(function(
                "delete",
                vec![param("path", "String")],
                "bool",
                vec![return_stmt(call_name("remove", vec![pos(ident("path"))]))],
            )),
            Item::Function(function(
                "remove_file",
                vec![param("path", "String")],
                "bool",
                vec![return_stmt(call_name("remove", vec![pos(ident("path"))]))],
            )),
            Item::Function(function(
                "rename",
                vec![param("from_path", "String"), param("to_path", "String")],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_fs_rename",
                    vec![pos(ident("from_path")), pos(ident("to_path"))],
                ))],
            )),
            Item::Function(function(
                "move",
                vec![param("from_path", "String"), param("to_path", "String")],
                "bool",
                vec![return_stmt(call_name(
                    "rename",
                    vec![pos(ident("from_path")), pos(ident("to_path"))],
                ))],
            )),
            Item::Function(function(
                "copy",
                vec![param("from_path", "String"), param("to_path", "String")],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_fs_copy",
                    vec![pos(ident("from_path")), pos(ident("to_path"))],
                ))],
            )),
            Item::Function(function(
                "is_file",
                vec![param("path", "String")],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_fs_is_file",
                    vec![pos(ident("path"))],
                ))],
            )),
            Item::Function(function(
                "is_dir",
                vec![param("path", "String")],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_fs_is_dir",
                    vec![pos(ident("path"))],
                ))],
            )),
            Item::Function(function(
                "canonicalize",
                vec![param("path", "String")],
                "String",
                vec![return_stmt(call_name(
                    "__rune_builtin_fs_canonicalize",
                    vec![pos(ident("path"))],
                ))],
            )),
            Item::Function(function(
                "absolute",
                vec![param("path", "String")],
                "String",
                vec![return_stmt(call_name(
                    "canonicalize",
                    vec![pos(ident("path"))],
                ))],
            )),
            Item::Function(function(
                "file_size",
                vec![param("path", "String")],
                "i64",
                vec![return_stmt(call_name(
                    "__rune_builtin_fs_file_size",
                    vec![pos(ident("path"))],
                ))],
            )),
            Item::Function(function(
                "size",
                vec![param("path", "String")],
                "i64",
                vec![return_stmt(call_name(
                    "file_size",
                    vec![pos(ident("path"))],
                ))],
            )),
            Item::Function(function(
                "create_dir",
                vec![param("path", "String")],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_fs_create_dir",
                    vec![pos(ident("path"))],
                ))],
            )),
            Item::Function(function(
                "mkdir",
                vec![param("path", "String")],
                "bool",
                vec![return_stmt(call_name("create_dir", vec![pos(ident("path"))]))],
            )),
            Item::Function(function(
                "create_dir_all",
                vec![param("path", "String")],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_fs_create_dir_all",
                    vec![pos(ident("path"))],
                ))],
            )),
            Item::Function(function(
                "mkdir_p",
                vec![param("path", "String")],
                "bool",
                vec![return_stmt(call_name(
                    "create_dir_all",
                    vec![pos(ident("path"))],
                ))],
            )),
            Item::Function(function(
                "mkdirs",
                vec![param("path", "String")],
                "bool",
                vec![return_stmt(call_name(
                    "create_dir_all",
                    vec![pos(ident("path"))],
                ))],
            )),
        ],
    }
}

fn json_program() -> Program {
    Program {
        items: vec![
            Item::Function(function(
                "parse",
                vec![param("text", "String")],
                "Json",
                vec![return_stmt(call_name(
                    "__rune_builtin_json_parse",
                    vec![pos(ident("text"))],
                ))],
            )),
            Item::Function(function(
                "stringify",
                vec![param("value", "Json")],
                "String",
                vec![return_stmt(call_name(
                    "__rune_builtin_json_stringify",
                    vec![pos(ident("value"))],
                ))],
            )),
            Item::Function(function(
                "as_string",
                vec![param("value", "Json")],
                "String",
                vec![return_stmt(call_name("to_string", vec![pos(ident("value"))]))],
            )),
            Item::Function(function(
                "kind",
                vec![param("value", "Json")],
                "String",
                vec![return_stmt(call_name(
                    "__rune_builtin_json_kind",
                    vec![pos(ident("value"))],
                ))],
            )),
            Item::Function(function(
                "value_kind",
                vec![param("value", "Json")],
                "String",
                vec![return_stmt(call_name("kind", vec![pos(ident("value"))]))],
            )),
            Item::Function(function(
                "is_null",
                vec![param("value", "Json")],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_json_is_null",
                    vec![pos(ident("value"))],
                ))],
            )),
            Item::Function(function(
                "len",
                vec![param("value", "Json")],
                "i64",
                vec![return_stmt(call_name(
                    "__rune_builtin_json_len",
                    vec![pos(ident("value"))],
                ))],
            )),
            Item::Function(function(
                "get",
                vec![param("value", "Json"), param("key", "String")],
                "Json",
                vec![return_stmt(call_name(
                    "__rune_builtin_json_get",
                    vec![pos(ident("value")), pos(ident("key"))],
                ))],
            )),
            Item::Function(function(
                "index",
                vec![param("value", "Json"), param("at", "i64")],
                "Json",
                vec![return_stmt(call_name(
                    "__rune_builtin_json_index",
                    vec![pos(ident("value")), pos(ident("at"))],
                ))],
            )),
            Item::Function(function(
                "to_string",
                vec![param("value", "Json")],
                "String",
                vec![return_stmt(call_name(
                    "__rune_builtin_json_to_string",
                    vec![pos(ident("value"))],
                ))],
            )),
            Item::Function(function(
                "as_text",
                vec![param("value", "Json")],
                "String",
                vec![return_stmt(call_name("to_string", vec![pos(ident("value"))]))],
            )),
            Item::Function(function(
                "to_i64",
                vec![param("value", "Json")],
                "i64",
                vec![return_stmt(call_name(
                    "__rune_builtin_json_to_i64",
                    vec![pos(ident("value"))],
                ))],
            )),
            Item::Function(function(
                "as_i64",
                vec![param("value", "Json")],
                "i64",
                vec![return_stmt(call_name("to_i64", vec![pos(ident("value"))]))],
            )),
            Item::Function(function(
                "to_bool",
                vec![param("value", "Json")],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_json_to_bool",
                    vec![pos(ident("value"))],
                ))],
            )),
            Item::Function(function(
                "as_bool",
                vec![param("value", "Json")],
                "bool",
                vec![return_stmt(call_name("to_bool", vec![pos(ident("value"))]))],
            )),
        ],
    }
}

fn audio_program() -> Program {
    Program {
        items: vec![
            Item::Function(function(
                "bell",
                vec![],
                "bool",
                vec![return_stmt(call_name("__rune_builtin_audio_bell", vec![]))],
            )),
            Item::Function(function(
                "beep",
                vec![],
                "bool",
                vec![return_stmt(call_name("bell", vec![]))],
            )),
        ],
    }
}

fn serial_program() -> Program {
    let serial_port_methods = vec![
        function(
            "connect",
            vec![param("self", "dynamic")],
            "bool",
            vec![return_stmt(call_name(
                "open",
                vec![pos(field(ident("self"), "port")), pos(field(ident("self"), "baud"))],
            ))],
        ),
        function(
            "is_open",
            vec![param("self", "dynamic")],
            "bool",
            vec![return_stmt(call_name("is_open", vec![]))],
        ),
        function(
            "close",
            vec![param("self", "dynamic")],
            "unit",
            vec![expr_stmt(call_name("close", vec![]))],
        ),
        function(
            "flush",
            vec![param("self", "dynamic")],
            "unit",
            vec![expr_stmt(call_name("flush", vec![]))],
        ),
        function(
            "available",
            vec![param("self", "dynamic")],
            "i64",
            vec![return_stmt(call_name("available", vec![]))],
        ),
        function(
            "read_byte",
            vec![param("self", "dynamic")],
            "i64",
            vec![return_stmt(call_name("read_byte", vec![]))],
        ),
        function(
            "read_byte_timeout",
            vec![param("self", "dynamic"), param("timeout_ms", "i64")],
            "i64",
            vec![return_stmt(call_name(
                "read_byte_timeout",
                vec![pos(ident("timeout_ms"))],
            ))],
        ),
        function(
            "peek_byte",
            vec![param("self", "dynamic")],
            "i64",
            vec![return_stmt(call_name("peek_byte", vec![]))],
        ),
        function(
            "recv_line",
            vec![param("self", "dynamic")],
            "String",
            vec![return_stmt(call_name("recv_line", vec![]))],
        ),
        function(
            "recv_line_timeout",
            vec![param("self", "dynamic"), param("timeout_ms", "i64")],
            "String",
            vec![return_stmt(call_name(
                "recv_line_timeout",
                vec![pos(ident("timeout_ms"))],
            ))],
        ),
        function(
            "recv_nonempty",
            vec![param("self", "dynamic")],
            "String",
            vec![return_stmt(call_name("recv_nonempty", vec![]))],
        ),
        function(
            "recv_nonempty_timeout",
            vec![param("self", "dynamic"), param("timeout_ms", "i64")],
            "String",
            vec![return_stmt(call_name(
                "recv_nonempty_timeout",
                vec![pos(ident("timeout_ms"))],
            ))],
        ),
        function(
            "send",
            vec![param("self", "dynamic"), param("value", "dynamic")],
            "bool",
            vec![return_stmt(call_name("send", vec![pos(ident("value"))]))],
        ),
        function(
            "write_byte",
            vec![param("self", "dynamic"), param("value", "i64")],
            "bool",
            vec![return_stmt(call_name("write_byte", vec![pos(ident("value"))]))],
        ),
        function(
            "send_i64",
            vec![param("self", "dynamic"), param("value", "i64")],
            "bool",
            vec![return_stmt(call_name("send_i64", vec![pos(ident("value"))]))],
        ),
        function(
            "send_bool",
            vec![param("self", "dynamic"), param("value", "bool")],
            "bool",
            vec![return_stmt(call_name("send_bool", vec![pos(ident("value"))]))],
        ),
        function(
            "send_line",
            vec![param("self", "dynamic"), param("value", "dynamic")],
            "bool",
            vec![return_stmt(call_name("send_line", vec![pos(ident("value"))]))],
        ),
        function(
            "send_line_i64",
            vec![param("self", "dynamic"), param("value", "i64")],
            "bool",
            vec![return_stmt(call_name("send_line_i64", vec![pos(ident("value"))]))],
        ),
        function(
            "send_line_bool",
            vec![param("self", "dynamic"), param("value", "bool")],
            "bool",
            vec![return_stmt(call_name("send_line_bool", vec![pos(ident("value"))]))],
        ),
    ];

    Program {
        items: vec![
            Item::Function(function(
                "begin",
                vec![param("baud", "i64")],
                "unit",
                vec![if_stmt(
                    call_name("__rune_builtin_system_is_embedded", vec![]),
                    vec![expr_stmt(call_name(
                        "__rune_builtin_arduino_uart_begin",
                        vec![pos(ident("baud"))],
                    ))],
                    None,
                )],
            )),
            Item::Function(function(
                "open",
                vec![param("port", "String"), param("baud", "i64")],
                "bool",
                vec![
                    if_stmt(
                        call_name("__rune_builtin_system_is_embedded", vec![]),
                        vec![
                            expr_stmt(call_name(
                                "__rune_builtin_arduino_uart_begin",
                                vec![pos(ident("baud"))],
                            )),
                            return_stmt(bool_lit(true)),
                        ],
                        None,
                    ),
                    return_stmt(call_name(
                        "__rune_builtin_serial_open",
                        vec![pos(ident("port")), pos(ident("baud"))],
                    )),
                ],
            )),
            Item::Function(function(
                "is_open",
                vec![],
                "bool",
                vec![
                    if_stmt(
                        call_name("__rune_builtin_system_is_embedded", vec![]),
                        vec![return_stmt(bool_lit(true))],
                        None,
                    ),
                    return_stmt(call_name("__rune_builtin_serial_is_open", vec![])),
                ],
            )),
            Item::Function(function(
                "close",
                vec![],
                "unit",
                vec![
                    if_stmt(
                        call_name("__rune_builtin_system_is_embedded", vec![]),
                        vec![return_unit_stmt()],
                        None,
                    ),
                    expr_stmt(call_name("__rune_builtin_serial_close", vec![])),
                ],
            )),
            Item::Function(function(
                "flush",
                vec![],
                "unit",
                vec![
                    if_stmt(
                        call_name("__rune_builtin_system_is_embedded", vec![]),
                        vec![
                            expr_stmt(call_name("__rune_builtin_serial_flush", vec![])),
                            return_unit_stmt(),
                        ],
                        None,
                    ),
                    expr_stmt(call_name("__rune_builtin_serial_flush", vec![])),
                ],
            )),
            Item::Function(function(
                "available",
                vec![],
                "i64",
                vec![
                    if_stmt(
                        call_name("__rune_builtin_system_is_embedded", vec![]),
                        vec![return_stmt(call_name(
                            "__rune_builtin_arduino_uart_available",
                            vec![],
                        ))],
                        None,
                    ),
                    return_stmt(call_name("__rune_builtin_serial_available", vec![])),
                ],
            )),
            Item::Function(function(
                "read_byte",
                vec![],
                "i64",
                vec![
                    if_stmt(
                        call_name("__rune_builtin_system_is_embedded", vec![]),
                        vec![return_stmt(call_name(
                            "__rune_builtin_arduino_uart_read_byte",
                            vec![],
                        ))],
                        None,
                    ),
                    return_stmt(call_name("__rune_builtin_serial_read_byte", vec![])),
                ],
            )),
            Item::Function(function(
                "read_byte_timeout",
                vec![param("timeout_ms", "i64")],
                "i64",
                vec![
                    if_stmt(
                        call_name("__rune_builtin_system_is_embedded", vec![]),
                        vec![
                            if_stmt(
                                binary(ident("timeout_ms"), BinaryOp::LessEqual, int_lit(0)),
                                vec![return_stmt(call_name("read_byte", vec![]))],
                                None,
                            ),
                            Stmt::Let(crate::parser::LetStmt {
                                name: "deadline".to_string(),
                                ty: Some(ty("i64")),
                                value: binary(
                                    call_name("__rune_builtin_arduino_millis", vec![]),
                                    BinaryOp::Add,
                                    ident("timeout_ms"),
                                ),
                                span: s(),
                            }),
                            Stmt::While(crate::parser::WhileStmt {
                                condition: binary(
                                    call_name("__rune_builtin_arduino_millis", vec![]),
                                    BinaryOp::Less,
                                    ident("deadline"),
                                ),
                                body: Block {
                                    statements: vec![
                                        if_stmt(
                                            binary(call_name("available", vec![]), BinaryOp::Greater, int_lit(0)),
                                            vec![return_stmt(call_name("read_byte", vec![]))],
                                            None,
                                        ),
                                        expr_stmt(call_name(
                                            "__rune_builtin_arduino_delay_ms",
                                            vec![pos(int_lit(1))],
                                        )),
                                    ],
                                },
                                span: s(),
                            }),
                            return_stmt(int_lit(-1)),
                        ],
                        None,
                    ),
                    return_stmt(call_name(
                        "__rune_builtin_serial_read_byte_timeout",
                        vec![pos(ident("timeout_ms"))],
                    )),
                ],
            )),
            Item::Function(function(
                "peek_byte",
                vec![],
                "i64",
                vec![
                    if_stmt(
                        call_name("__rune_builtin_system_is_embedded", vec![]),
                        vec![return_stmt(call_name(
                            "__rune_builtin_arduino_uart_peek_byte",
                            vec![],
                        ))],
                        None,
                    ),
                    return_stmt(call_name("__rune_builtin_serial_peek_byte", vec![])),
                ],
            )),
            Item::Function(function(
                "recv_line",
                vec![],
                "String",
                vec![
                    if_stmt(
                        call_name("__rune_builtin_system_is_embedded", vec![]),
                        vec![return_stmt(call_name("input", vec![]))],
                        None,
                    ),
                    return_stmt(call_name("__rune_builtin_serial_read_line", vec![])),
                ],
            )),
            Item::Function(function(
                "recv_line_timeout",
                vec![param("timeout_ms", "i64")],
                "String",
                vec![
                    if_stmt(
                        call_name("__rune_builtin_system_is_embedded", vec![]),
                        vec![return_stmt(call_name("recv_line", vec![]))],
                        None,
                    ),
                    return_stmt(call_name(
                        "__rune_builtin_serial_read_line_timeout",
                        vec![pos(ident("timeout_ms"))],
                    )),
                ],
            )),
            Item::Function(function(
                "recv_nonempty",
                vec![],
                "String",
                vec![
                    Stmt::Let(crate::parser::LetStmt {
                        name: "line".to_string(),
                        ty: Some(ty("String")),
                        value: call_name("recv_line", vec![]),
                        span: s(),
                    }),
                    Stmt::While(crate::parser::WhileStmt {
                        condition: binary(ident("line"), BinaryOp::EqualEqual, string_lit("")),
                        body: Block {
                            statements: vec![Stmt::Assign(crate::parser::AssignStmt {
                                name: "line".to_string(),
                                value: call_name("recv_line", vec![]),
                                span: s(),
                            })],
                        },
                        span: s(),
                    }),
                    return_stmt(ident("line")),
                ],
            )),
            Item::Function(function(
                "recv_nonempty_timeout",
                vec![param("timeout_ms", "i64")],
                "String",
                vec![
                    Stmt::Let(crate::parser::LetStmt {
                        name: "line".to_string(),
                        ty: Some(ty("String")),
                        value: call_name("recv_line_timeout", vec![pos(ident("timeout_ms"))]),
                        span: s(),
                    }),
                    if_stmt(
                        binary(ident("line"), BinaryOp::EqualEqual, string_lit("")),
                        vec![return_stmt(string_lit(""))],
                        None,
                    ),
                    return_stmt(ident("line")),
                ],
            )),
            Item::Function(function(
                "write",
                vec![param("text", "String")],
                "unit",
                vec![if_stmt(
                    call_name("__rune_builtin_system_is_embedded", vec![]),
                    vec![
                        expr_stmt(call_name(
                            "__rune_builtin_arduino_uart_write",
                            vec![pos(ident("text"))],
                        )),
                        return_unit_stmt(),
                    ],
                    Some(vec![expr_stmt(call_name("print", vec![pos(ident("text"))]))]),
                )],
            )),
            Item::Function(function(
                "write_line",
                vec![param("text", "String")],
                "unit",
                vec![if_stmt(
                    call_name("__rune_builtin_system_is_embedded", vec![]),
                    vec![
                        expr_stmt(call_name(
                            "__rune_builtin_arduino_uart_write",
                            vec![pos(ident("text"))],
                        )),
                        expr_stmt(call_name(
                            "__rune_builtin_arduino_uart_write_byte",
                            vec![pos(int_lit(10))],
                        )),
                        return_unit_stmt(),
                    ],
                    Some(vec![expr_stmt(call_name("println", vec![pos(ident("text"))]))]),
                )],
            )),
            Item::Function(function(
                "send",
                vec![param("value", "dynamic")],
                "bool",
                vec![
                    Stmt::Let(crate::parser::LetStmt {
                        name: "text".to_string(),
                        ty: Some(ty("String")),
                        value: call_name("str", vec![pos(ident("value"))]),
                        span: s(),
                    }),
                    if_stmt(
                        call_name("__rune_builtin_system_is_embedded", vec![]),
                        vec![
                            expr_stmt(call_name(
                                "__rune_builtin_arduino_uart_write",
                                vec![pos(ident("text"))],
                            )),
                            return_stmt(bool_lit(true)),
                        ],
                        None,
                    ),
                    return_stmt(call_name(
                        "__rune_builtin_serial_write",
                        vec![pos(ident("text"))],
                    )),
                ],
            )),
            Item::Function(function(
                "write_byte",
                vec![param("value", "i64")],
                "bool",
                vec![
                    if_stmt(
                        call_name("__rune_builtin_system_is_embedded", vec![]),
                        vec![
                            expr_stmt(call_name(
                                "__rune_builtin_arduino_uart_write_byte",
                                vec![pos(ident("value"))],
                            )),
                            return_stmt(bool_lit(true)),
                        ],
                        None,
                    ),
                    return_stmt(call_name(
                        "__rune_builtin_serial_write_byte",
                        vec![pos(ident("value"))],
                    )),
                ],
            )),
            Item::Function(function(
                "send_i64",
                vec![param("value", "i64")],
                "bool",
                vec![return_stmt(call_name("send", vec![pos(ident("value"))]))],
            )),
            Item::Function(function(
                "send_bool",
                vec![param("value", "bool")],
                "bool",
                vec![return_stmt(call_name("send", vec![pos(ident("value"))]))],
            )),
            Item::Function(function(
                "send_line",
                vec![param("value", "dynamic")],
                "bool",
                vec![
                    Stmt::Let(crate::parser::LetStmt {
                        name: "text".to_string(),
                        ty: Some(ty("String")),
                        value: call_name("str", vec![pos(ident("value"))]),
                        span: s(),
                    }),
                    if_stmt(
                        call_name("__rune_builtin_system_is_embedded", vec![]),
                        vec![
                            expr_stmt(call_name(
                                "__rune_builtin_arduino_uart_write",
                                vec![pos(ident("text"))],
                            )),
                            expr_stmt(call_name(
                                "__rune_builtin_arduino_uart_write_byte",
                                vec![pos(int_lit(10))],
                            )),
                            return_stmt(bool_lit(true)),
                        ],
                        None,
                    ),
                    return_stmt(call_name(
                        "__rune_builtin_serial_write_line",
                        vec![pos(ident("text"))],
                    )),
                ],
            )),
            Item::Function(function(
                "send_line_i64",
                vec![param("value", "i64")],
                "bool",
                vec![return_stmt(call_name("send_line", vec![pos(ident("value"))]))],
            )),
            Item::Function(function(
                "send_line_bool",
                vec![param("value", "bool")],
                "bool",
                vec![return_stmt(call_name("send_line", vec![pos(ident("value"))]))],
            )),
            Item::Struct(StructDecl {
                name: "SerialPort".to_string(),
                fields: vec![
                    StructField {
                        name: "port".to_string(),
                        ty: ty("String"),
                        span: s(),
                    },
                    StructField {
                        name: "baud".to_string(),
                        ty: ty("i64"),
                        span: s(),
                    },
                ],
                methods: serial_port_methods,
                span: s(),
            }),
            Item::Function(function(
                "serial_port",
                vec![param("port", "String"), param("baud", "i64")],
                "SerialPort",
                vec![return_stmt(call_expr(
                    ident("SerialPort"),
                    vec![kw("port", ident("port")), kw("baud", ident("baud"))],
                ))],
            )),
        ],
    }
}

fn gpio_program() -> Program {
    let gpio_pin_methods = vec![
        function(
            "output",
            vec![param("self", "dynamic")],
            "unit",
            vec![expr_stmt(call_name(
                "__rune_builtin_gpio_pin_mode",
                vec![
                    pos(field(ident("self"), "pin")),
                    pos(call_name("__rune_builtin_gpio_mode_output", vec![])),
                ],
            ))],
        ),
        function(
            "input",
            vec![param("self", "dynamic")],
            "unit",
            vec![expr_stmt(call_name(
                "__rune_builtin_gpio_pin_mode",
                vec![
                    pos(field(ident("self"), "pin")),
                    pos(call_name("__rune_builtin_gpio_mode_input", vec![])),
                ],
            ))],
        ),
        function(
            "input_pullup",
            vec![param("self", "dynamic")],
            "unit",
            vec![expr_stmt(call_name(
                "__rune_builtin_gpio_pin_mode",
                vec![
                    pos(field(ident("self"), "pin")),
                    pos(call_name("__rune_builtin_gpio_mode_input_pullup", vec![])),
                ],
            ))],
        ),
        function(
            "write",
            vec![param("self", "dynamic"), param("value", "bool")],
            "unit",
            vec![
                expr_stmt(call_name(
                    "__rune_builtin_gpio_pin_mode",
                    vec![
                        pos(field(ident("self"), "pin")),
                        pos(call_name("__rune_builtin_gpio_mode_output", vec![])),
                    ],
                )),
                expr_stmt(call_name(
                    "__rune_builtin_gpio_digital_write",
                    vec![pos(field(ident("self"), "pin")), pos(ident("value"))],
                )),
            ],
        ),
        function(
            "high",
            vec![param("self", "dynamic")],
            "unit",
            vec![
                expr_stmt(call_name(
                    "__rune_builtin_gpio_pin_mode",
                    vec![
                        pos(field(ident("self"), "pin")),
                        pos(call_name("__rune_builtin_gpio_mode_output", vec![])),
                    ],
                )),
                expr_stmt(call_name(
                    "__rune_builtin_gpio_digital_write",
                    vec![pos(field(ident("self"), "pin")), pos(bool_lit(true))],
                )),
            ],
        ),
        function(
            "low",
            vec![param("self", "dynamic")],
            "unit",
            vec![
                expr_stmt(call_name(
                    "__rune_builtin_gpio_pin_mode",
                    vec![
                        pos(field(ident("self"), "pin")),
                        pos(call_name("__rune_builtin_gpio_mode_output", vec![])),
                    ],
                )),
                expr_stmt(call_name(
                    "__rune_builtin_gpio_digital_write",
                    vec![pos(field(ident("self"), "pin")), pos(bool_lit(false))],
                )),
            ],
        ),
        function(
            "toggle",
            vec![param("self", "dynamic")],
            "unit",
            vec![Stmt::If(IfStmt {
                condition: call_name(
                    "__rune_builtin_gpio_digital_read",
                    vec![pos(field(ident("self"), "pin"))],
                ),
                then_block: Block {
                    statements: vec![expr_stmt(call_name(
                        "__rune_builtin_gpio_digital_write",
                        vec![pos(field(ident("self"), "pin")), pos(bool_lit(false))],
                    ))],
                },
                elif_blocks: vec![],
                else_block: Some(Block {
                    statements: vec![expr_stmt(call_name(
                        "__rune_builtin_gpio_digital_write",
                        vec![pos(field(ident("self"), "pin")), pos(bool_lit(true))],
                    ))],
                }),
                span: s(),
            })],
        ),
        function(
            "read",
            vec![param("self", "dynamic")],
            "bool",
            vec![return_stmt(call_name(
                "__rune_builtin_gpio_digital_read",
                vec![pos(field(ident("self"), "pin"))],
            ))],
        ),
        function(
            "read_pullup",
            vec![param("self", "dynamic")],
            "bool",
            vec![
                expr_stmt(call_name(
                    "__rune_builtin_gpio_pin_mode",
                    vec![
                        pos(field(ident("self"), "pin")),
                        pos(call_name("__rune_builtin_gpio_mode_input_pullup", vec![])),
                    ],
                )),
                return_stmt(call_name(
                    "__rune_builtin_gpio_digital_read",
                    vec![pos(field(ident("self"), "pin"))],
                )),
            ],
        ),
        function(
            "blink",
            vec![
                param("self", "dynamic"),
                param("times", "i64"),
                param("on_ms", "i64"),
                param("off_ms", "i64"),
            ],
            "unit",
            vec![
                Stmt::Let(crate::parser::LetStmt {
                    name: "count".to_string(),
                    ty: Some(ty("i64")),
                    value: int_lit(0),
                    span: s(),
                }),
                Stmt::While(crate::parser::WhileStmt {
                    condition: binary(ident("count"), BinaryOp::Less, ident("times")),
                    body: Block {
                        statements: vec![
                            expr_stmt(call_expr(field(ident("self"), "high"), vec![])),
                            expr_stmt(call_name("__rune_builtin_time_sleep_ms", vec![pos(ident("on_ms"))])),
                            expr_stmt(call_expr(field(ident("self"), "low"), vec![])),
                            expr_stmt(call_name("__rune_builtin_time_sleep_ms", vec![pos(ident("off_ms"))])),
                            Stmt::Assign(crate::parser::AssignStmt {
                                name: "count".to_string(),
                                value: binary(ident("count"), BinaryOp::Add, int_lit(1)),
                                span: s(),
                            }),
                        ],
                    },
                    span: s(),
                }),
            ],
        ),
    ];

    let gpio_pwm_methods = vec![
        function(
            "output",
            vec![param("self", "dynamic")],
            "unit",
            vec![expr_stmt(call_name(
                "__rune_builtin_gpio_pin_mode",
                vec![
                    pos(field(ident("self"), "pin")),
                    pos(call_name("__rune_builtin_gpio_mode_output", vec![])),
                ],
            ))],
        ),
        function(
            "write",
            vec![param("self", "dynamic"), param("duty", "i64")],
            "unit",
            vec![expr_stmt(call_name(
                "__rune_builtin_gpio_pwm_write",
                vec![pos(field(ident("self"), "pin")), pos(ident("duty"))],
            ))],
        ),
        function(
            "max_duty",
            vec![param("self", "dynamic")],
            "i64",
            vec![return_stmt(call_name("__rune_builtin_gpio_pwm_duty_max", vec![]))],
        ),
        function(
            "off",
            vec![param("self", "dynamic")],
            "unit",
            vec![expr_stmt(call_name(
                "__rune_builtin_gpio_pwm_write",
                vec![pos(field(ident("self"), "pin")), pos(int_lit(0))],
            ))],
        ),
    ];

    let gpio_analog_methods = vec![
        function(
            "read",
            vec![param("self", "dynamic")],
            "i64",
            vec![return_stmt(call_name(
                "__rune_builtin_gpio_analog_read",
                vec![pos(field(ident("self"), "pin"))],
            ))],
        ),
        function(
            "read_percent",
            vec![param("self", "dynamic")],
            "i64",
            vec![return_stmt(binary(
                binary(
                    call_name(
                        "__rune_builtin_gpio_analog_read",
                        vec![pos(field(ident("self"), "pin"))],
                    ),
                    BinaryOp::Multiply,
                    int_lit(100),
                ),
                BinaryOp::Divide,
                call_name("__rune_builtin_gpio_analog_max", vec![]),
            ))],
        ),
        function(
            "read_voltage_mv",
            vec![param("self", "dynamic"), param("reference_mv", "i64")],
            "i64",
            vec![return_stmt(binary(
                binary(
                    call_name(
                        "__rune_builtin_gpio_analog_read",
                        vec![pos(field(ident("self"), "pin"))],
                    ),
                    BinaryOp::Multiply,
                    ident("reference_mv"),
                ),
                BinaryOp::Divide,
                call_name("__rune_builtin_gpio_analog_max", vec![]),
            ))],
        ),
    ];

    Program {
        items: vec![
            Item::Struct(StructDecl {
                name: "GpioPin".to_string(),
                fields: vec![StructField {
                    name: "pin".to_string(),
                    ty: ty("i64"),
                    span: s(),
                }],
                methods: gpio_pin_methods,
                span: s(),
            }),
            Item::Struct(StructDecl {
                name: "GpioPwm".to_string(),
                fields: vec![StructField {
                    name: "pin".to_string(),
                    ty: ty("i64"),
                    span: s(),
                }],
                methods: gpio_pwm_methods,
                span: s(),
            }),
            Item::Struct(StructDecl {
                name: "GpioAnalogIn".to_string(),
                fields: vec![StructField {
                    name: "pin".to_string(),
                    ty: ty("i64"),
                    span: s(),
                }],
                methods: gpio_analog_methods,
                span: s(),
            }),
            Item::Function(function(
                "pin_mode",
                vec![param("pin", "i64"), param("mode", "i64")],
                "unit",
                vec![expr_stmt(call_name(
                    "__rune_builtin_gpio_pin_mode",
                    vec![pos(ident("pin")), pos(ident("mode"))],
                ))],
            )),
            Item::Function(function(
                "mode_input",
                vec![],
                "i64",
                vec![return_stmt(call_name("__rune_builtin_gpio_mode_input", vec![]))],
            )),
            Item::Function(function(
                "mode_output",
                vec![],
                "i64",
                vec![return_stmt(call_name("__rune_builtin_gpio_mode_output", vec![]))],
            )),
            Item::Function(function(
                "mode_input_pullup",
                vec![],
                "i64",
                vec![return_stmt(call_name(
                    "__rune_builtin_gpio_mode_input_pullup",
                    vec![],
                ))],
            )),
            Item::Function(function(
                "digital_write",
                vec![param("pin", "i64"), param("value", "bool")],
                "unit",
                vec![expr_stmt(call_name(
                    "__rune_builtin_gpio_digital_write",
                    vec![pos(ident("pin")), pos(ident("value"))],
                ))],
            )),
            Item::Function(function(
                "digital_read",
                vec![param("pin", "i64")],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_gpio_digital_read",
                    vec![pos(ident("pin"))],
                ))],
            )),
            Item::Function(function(
                "digital_out",
                vec![param("pin", "i64"), param("value", "bool")],
                "unit",
                vec![
                    expr_stmt(call_name(
                        "__rune_builtin_gpio_pin_mode",
                        vec![
                            pos(ident("pin")),
                            pos(call_name("__rune_builtin_gpio_mode_output", vec![])),
                        ],
                    )),
                    expr_stmt(call_name(
                        "__rune_builtin_gpio_digital_write",
                        vec![pos(ident("pin")), pos(ident("value"))],
                    )),
                ],
            )),
            Item::Function(function(
                "digital_in",
                vec![param("pin", "i64")],
                "bool",
                vec![
                    expr_stmt(call_name(
                        "__rune_builtin_gpio_pin_mode",
                        vec![
                            pos(ident("pin")),
                            pos(call_name("__rune_builtin_gpio_mode_input", vec![])),
                        ],
                    )),
                    return_stmt(call_name(
                        "__rune_builtin_gpio_digital_read",
                        vec![pos(ident("pin"))],
                    )),
                ],
            )),
            Item::Function(function(
                "digital_in_pullup",
                vec![param("pin", "i64")],
                "bool",
                vec![
                    expr_stmt(call_name(
                        "__rune_builtin_gpio_pin_mode",
                        vec![
                            pos(ident("pin")),
                            pos(call_name("__rune_builtin_gpio_mode_input_pullup", vec![])),
                        ],
                    )),
                    return_stmt(call_name(
                        "__rune_builtin_gpio_digital_read",
                        vec![pos(ident("pin"))],
                    )),
                ],
            )),
            Item::Function(function(
                "pwm_write",
                vec![param("pin", "i64"), param("duty", "i64")],
                "unit",
                vec![expr_stmt(call_name(
                    "__rune_builtin_gpio_pwm_write",
                    vec![pos(ident("pin")), pos(ident("duty"))],
                ))],
            )),
            Item::Function(function(
                "pwm_duty_max",
                vec![],
                "i64",
                vec![return_stmt(call_name("__rune_builtin_gpio_pwm_duty_max", vec![]))],
            )),
            Item::Function(function(
                "analog_read",
                vec![param("pin", "i64")],
                "i64",
                vec![return_stmt(call_name(
                    "__rune_builtin_gpio_analog_read",
                    vec![pos(ident("pin"))],
                ))],
            )),
            Item::Function(function(
                "analog_in",
                vec![param("pin", "i64")],
                "i64",
                vec![return_stmt(call_name("analog_read", vec![pos(ident("pin"))]))],
            )),
            Item::Function(function(
                "analog_max",
                vec![],
                "i64",
                vec![return_stmt(call_name("__rune_builtin_gpio_analog_max", vec![]))],
            )),
            Item::Function(function(
                "analog_read_percent",
                vec![param("pin", "i64")],
                "i64",
                vec![return_stmt(binary(
                    binary(call_name("analog_read", vec![pos(ident("pin"))]), BinaryOp::Multiply, int_lit(100)),
                    BinaryOp::Divide,
                    call_name("analog_max", vec![]),
                ))],
            )),
            Item::Function(function(
                "analog_in_percent",
                vec![param("pin", "i64")],
                "i64",
                vec![return_stmt(call_name(
                    "analog_read_percent",
                    vec![pos(ident("pin"))],
                ))],
            )),
            Item::Function(function(
                "analog_read_voltage_mv",
                vec![param("pin", "i64"), param("reference_mv", "i64")],
                "i64",
                vec![return_stmt(binary(
                    binary(
                        call_name("analog_read", vec![pos(ident("pin"))]),
                        BinaryOp::Multiply,
                        ident("reference_mv"),
                    ),
                    BinaryOp::Divide,
                    call_name("analog_max", vec![]),
                ))],
            )),
            Item::Function(function(
                "analog_in_voltage_mv",
                vec![param("pin", "i64"), param("reference_mv", "i64")],
                "i64",
                vec![return_stmt(call_name(
                    "analog_read_voltage_mv",
                    vec![pos(ident("pin")), pos(ident("reference_mv"))],
                ))],
            )),
            Item::Function(function(
                "gpio_pin",
                vec![param("pin", "i64")],
                "GpioPin",
                vec![return_stmt(call_expr(
                    ident("GpioPin"),
                    vec![kw("pin", ident("pin"))],
                ))],
            )),
            Item::Function(function(
                "pin",
                vec![param("pin", "i64")],
                "GpioPin",
                vec![return_stmt(call_name("gpio_pin", vec![pos(ident("pin"))]))],
            )),
            Item::Function(function(
                "pwm_pin",
                vec![param("pin", "i64")],
                "GpioPwm",
                vec![return_stmt(call_expr(
                    ident("GpioPwm"),
                    vec![kw("pin", ident("pin"))],
                ))],
            )),
            Item::Function(function(
                "pwm",
                vec![param("pin", "i64")],
                "GpioPwm",
                vec![return_stmt(call_name("pwm_pin", vec![pos(ident("pin"))]))],
            )),
            Item::Function(function(
                "analog_pin",
                vec![param("pin", "i64")],
                "GpioAnalogIn",
                vec![return_stmt(call_expr(
                    ident("GpioAnalogIn"),
                    vec![kw("pin", ident("pin"))],
                ))],
            )),
            Item::Function(function(
                "analog",
                vec![param("pin", "i64")],
                "GpioAnalogIn",
                vec![return_stmt(call_name("analog_pin", vec![pos(ident("pin"))]))],
            )),
        ],
    }
}

fn pwm_program() -> Program {
    let pwm_methods = vec![
        function(
            "output",
            vec![param("self", "dynamic")],
            "unit",
            vec![expr_stmt(call_name("pin_mode", vec![
                pos(field(ident("self"), "pin")),
                pos(call_name("mode_output", vec![])),
            ]))],
        ),
        function(
            "write",
            vec![param("self", "dynamic"), param("duty", "i64")],
            "unit",
            vec![expr_stmt(call_name(
                "__rune_builtin_gpio_pwm_write",
                vec![pos(field(ident("self"), "pin")), pos(ident("duty"))],
            ))],
        ),
        function(
            "max_duty",
            vec![param("self", "dynamic")],
            "i64",
            vec![return_stmt(call_name(
                "__rune_builtin_gpio_pwm_duty_max",
                vec![],
            ))],
        ),
        function(
            "off",
            vec![param("self", "dynamic")],
            "unit",
            vec![expr_stmt(call_name(
                "__rune_builtin_gpio_pwm_write",
                vec![pos(field(ident("self"), "pin")), pos(int_lit(0))],
            ))],
        ),
    ];

    Program {
        items: vec![
            Item::Struct(StructDecl {
                name: "PwmPin".to_string(),
                fields: vec![StructField {
                    name: "pin".to_string(),
                    ty: ty("i64"),
                    span: s(),
                }],
                methods: pwm_methods,
                span: s(),
            }),
            Item::Function(function(
                "pin_mode",
                vec![param("pin", "i64"), param("mode", "i64")],
                "unit",
                vec![expr_stmt(call_name(
                    "__rune_builtin_gpio_pin_mode",
                    vec![pos(ident("pin")), pos(ident("mode"))],
                ))],
            )),
            Item::Function(function(
                "mode_output",
                vec![],
                "i64",
                vec![return_stmt(call_name("__rune_builtin_gpio_mode_output", vec![]))],
            )),
            Item::Function(function(
                "write",
                vec![param("pin", "i64"), param("duty", "i64")],
                "unit",
                vec![expr_stmt(call_name(
                    "__rune_builtin_gpio_pwm_write",
                    vec![pos(ident("pin")), pos(ident("duty"))],
                ))],
            )),
            Item::Function(function(
                "max_duty",
                vec![],
                "i64",
                vec![return_stmt(call_name(
                    "__rune_builtin_gpio_pwm_duty_max",
                    vec![],
                ))],
            )),
            Item::Function(function(
                "pwm_pin",
                vec![param("pin", "i64")],
                "PwmPin",
                vec![return_stmt(call_expr(
                    ident("PwmPin"),
                    vec![kw("pin", ident("pin"))],
                ))],
            )),
            Item::Function(function(
                "pin",
                vec![param("pin", "i64")],
                "PwmPin",
                vec![return_stmt(call_name("pwm_pin", vec![pos(ident("pin"))]))],
            )),
        ],
    }
}

fn adc_program() -> Program {
    let analog_methods = vec![
        function(
            "read",
            vec![param("self", "dynamic")],
            "i64",
            vec![return_stmt(call_name(
                "__rune_builtin_gpio_analog_read",
                vec![pos(field(ident("self"), "pin"))],
            ))],
        ),
        function(
            "max",
            vec![param("self", "dynamic")],
            "i64",
            vec![return_stmt(call_name("__rune_builtin_gpio_analog_max", vec![]))],
        ),
        function(
            "read_percent",
            vec![param("self", "dynamic")],
            "i64",
            vec![return_stmt(binary(
                binary(
                    call_name(
                        "__rune_builtin_gpio_analog_read",
                        vec![pos(field(ident("self"), "pin"))],
                    ),
                    BinaryOp::Multiply,
                    int_lit(100),
                ),
                BinaryOp::Divide,
                call_name("__rune_builtin_gpio_analog_max", vec![]),
            ))],
        ),
        function(
            "read_voltage_mv",
            vec![param("self", "dynamic"), param("reference_mv", "i64")],
            "i64",
            vec![return_stmt(binary(
                binary(
                    call_name(
                        "__rune_builtin_gpio_analog_read",
                        vec![pos(field(ident("self"), "pin"))],
                    ),
                    BinaryOp::Multiply,
                    ident("reference_mv"),
                ),
                BinaryOp::Divide,
                call_name("__rune_builtin_gpio_analog_max", vec![]),
            ))],
        ),
    ];

    Program {
        items: vec![
            Item::Struct(StructDecl {
                name: "AdcPin".to_string(),
                fields: vec![StructField {
                    name: "pin".to_string(),
                    ty: ty("i64"),
                    span: s(),
                }],
                methods: analog_methods,
                span: s(),
            }),
            Item::Function(function(
                "read",
                vec![param("pin", "i64")],
                "i64",
                vec![return_stmt(call_name(
                    "__rune_builtin_gpio_analog_read",
                    vec![pos(ident("pin"))],
                ))],
            )),
            Item::Function(function(
                "max",
                vec![],
                "i64",
                vec![return_stmt(call_name("__rune_builtin_gpio_analog_max", vec![]))],
            )),
            Item::Function(function(
                "read_percent",
                vec![param("pin", "i64")],
                "i64",
                vec![return_stmt(binary(
                    binary(call_name("read", vec![pos(ident("pin"))]), BinaryOp::Multiply, int_lit(100)),
                    BinaryOp::Divide,
                    call_name("max", vec![]),
                ))],
            )),
            Item::Function(function(
                "read_voltage_mv",
                vec![param("pin", "i64"), param("reference_mv", "i64")],
                "i64",
                vec![return_stmt(binary(
                    binary(
                        call_name("read", vec![pos(ident("pin"))]),
                        BinaryOp::Multiply,
                        ident("reference_mv"),
                    ),
                    BinaryOp::Divide,
                    call_name("max", vec![]),
                ))],
            )),
            Item::Function(function(
                "adc_pin",
                vec![param("pin", "i64")],
                "AdcPin",
                vec![return_stmt(call_expr(
                    ident("AdcPin"),
                    vec![kw("pin", ident("pin"))],
                ))],
            )),
            Item::Function(function(
                "pin",
                vec![param("pin", "i64")],
                "AdcPin",
                vec![return_stmt(call_name("adc_pin", vec![pos(ident("pin"))]))],
            )),
        ],
    }
}

fn network_program() -> Program {
    let tcp_client_methods = vec![
        function(
            "connect",
            vec![param("self", "dynamic")],
            "bool",
            vec![return_stmt(call_name(
                "tcp_connect",
                vec![pos(field(ident("self"), "host")), pos(field(ident("self"), "port"))],
            ))],
        ),
        function(
            "bind",
            vec![param("self", "dynamic")],
            "bool",
            vec![return_stmt(call_name(
                "tcp_bind",
                vec![pos(field(ident("self"), "host")), pos(field(ident("self"), "port"))],
            ))],
        ),
        function(
            "connect_timeout",
            vec![param("self", "dynamic"), param("timeout_ms", "i32")],
            "bool",
            vec![return_stmt(call_name(
                "tcp_connect_timeout",
                vec![
                    pos(field(ident("self"), "host")),
                    pos(field(ident("self"), "port")),
                    pos(ident("timeout_ms")),
                ],
            ))],
        ),
        function(
            "probe",
            vec![param("self", "dynamic")],
            "bool",
            vec![return_stmt(call_name(
                "tcp_probe",
                vec![pos(field(ident("self"), "host")), pos(field(ident("self"), "port"))],
            ))],
        ),
        function(
            "probe_timeout",
            vec![param("self", "dynamic"), param("timeout_ms", "i32")],
            "bool",
            vec![return_stmt(call_name(
                "tcp_probe_timeout",
                vec![
                    pos(field(ident("self"), "host")),
                    pos(field(ident("self"), "port")),
                    pos(ident("timeout_ms")),
                ],
            ))],
        ),
        function(
            "listen",
            vec![param("self", "dynamic")],
            "bool",
            vec![return_stmt(call_name(
                "tcp_listen",
                vec![pos(field(ident("self"), "host")), pos(field(ident("self"), "port"))],
            ))],
        ),
        function(
            "send",
            vec![param("self", "dynamic"), param("value", "dynamic")],
            "bool",
            vec![return_stmt(call_name(
                "tcp_send",
                vec![
                    pos(field(ident("self"), "host")),
                    pos(field(ident("self"), "port")),
                    pos(call_name("str", vec![pos(ident("value"))])),
                ],
            ))],
        ),
        function(
            "send_line",
            vec![param("self", "dynamic"), param("value", "dynamic")],
            "bool",
            vec![return_stmt(call_name(
                "tcp_send_line",
                vec![
                    pos(field(ident("self"), "host")),
                    pos(field(ident("self"), "port")),
                    pos(ident("value")),
                ],
            ))],
        ),
        function(
            "recv",
            vec![param("self", "dynamic"), param("max_bytes", "i32")],
            "String",
            vec![return_stmt(call_name(
                "tcp_recv",
                vec![
                    pos(field(ident("self"), "host")),
                    pos(field(ident("self"), "port")),
                    pos(ident("max_bytes")),
                ],
            ))],
        ),
        function(
            "recv_timeout",
            vec![
                param("self", "dynamic"),
                param("max_bytes", "i32"),
                param("timeout_ms", "i32"),
            ],
            "String",
            vec![return_stmt(call_name(
                "tcp_recv_timeout",
                vec![
                    pos(field(ident("self"), "host")),
                    pos(field(ident("self"), "port")),
                    pos(ident("max_bytes")),
                    pos(ident("timeout_ms")),
                ],
            ))],
        ),
        function(
            "request",
            vec![
                param("self", "dynamic"),
                param("value", "dynamic"),
                param("max_bytes", "i32"),
                param("timeout_ms", "i32"),
            ],
            "String",
            vec![return_stmt(call_name(
                "tcp_request",
                vec![
                    pos(field(ident("self"), "host")),
                    pos(field(ident("self"), "port")),
                    pos(call_name("str", vec![pos(ident("value"))])),
                    pos(ident("max_bytes")),
                    pos(ident("timeout_ms")),
                ],
            ))],
        ),
        function(
            "request_line",
            vec![
                param("self", "dynamic"),
                param("value", "dynamic"),
                param("max_bytes", "i32"),
                param("timeout_ms", "i32"),
            ],
            "String",
            vec![return_stmt(call_name(
                "request_line",
                vec![
                    pos(field(ident("self"), "host")),
                    pos(field(ident("self"), "port")),
                    pos(ident("value")),
                    pos(ident("max_bytes")),
                    pos(ident("timeout_ms")),
                ],
            ))],
        ),
        function(
            "send_text",
            vec![param("self", "dynamic"), param("value", "String")],
            "bool",
            vec![return_stmt(call_name(
                "tcp_send",
                vec![
                    pos(field(ident("self"), "host")),
                    pos(field(ident("self"), "port")),
                    pos(ident("value")),
                ],
            ))],
        ),
        function(
            "send_line_text",
            vec![param("self", "dynamic"), param("value", "String")],
            "bool",
            vec![return_stmt(call_name(
                "tcp_send_line",
                vec![
                    pos(field(ident("self"), "host")),
                    pos(field(ident("self"), "port")),
                    pos(ident("value")),
                ],
            ))],
        ),
        function(
            "open_handle",
            vec![param("self", "dynamic"), param("timeout_ms", "i32")],
            "i32",
            vec![return_stmt(call_name(
                "tcp_client_open",
                vec![
                    pos(field(ident("self"), "host")),
                    pos(field(ident("self"), "port")),
                    pos(ident("timeout_ms")),
                ],
            ))],
        ),
        function(
            "send_handle",
            vec![
                param("self", "dynamic"),
                param("handle", "i32"),
                param("value", "dynamic"),
            ],
            "bool",
            vec![return_stmt(call_name(
                "tcp_client_send",
                vec![
                    pos(ident("handle")),
                    pos(call_name("str", vec![pos(ident("value"))])),
                ],
            ))],
        ),
        function(
            "send_line_handle",
            vec![
                param("self", "dynamic"),
                param("handle", "i32"),
                param("value", "dynamic"),
            ],
            "bool",
            vec![return_stmt(call_name(
                "tcp_client_send_line",
                vec![pos(ident("handle")), pos(ident("value"))],
            ))],
        ),
        function(
            "recv_handle",
            vec![
                param("self", "dynamic"),
                param("handle", "i32"),
                param("max_bytes", "i32"),
                param("timeout_ms", "i32"),
            ],
            "String",
            vec![return_stmt(call_name(
                "tcp_client_recv",
                vec![
                    pos(ident("handle")),
                    pos(ident("max_bytes")),
                    pos(ident("timeout_ms")),
                ],
            ))],
        ),
        function(
            "close_handle",
            vec![param("self", "dynamic"), param("handle", "i32")],
            "bool",
            vec![return_stmt(call_name(
                "tcp_client_close",
                vec![pos(ident("handle"))],
            ))],
        ),
    ];

    let tcp_server_methods = vec![
        function(
            "listen",
            vec![param("self", "dynamic")],
            "bool",
            vec![return_stmt(call_name(
                "tcp_listen",
                vec![pos(field(ident("self"), "host")), pos(field(ident("self"), "port"))],
            ))],
        ),
        function(
            "bind",
            vec![param("self", "dynamic")],
            "bool",
            vec![return_stmt(call_name(
                "tcp_bind",
                vec![pos(field(ident("self"), "host")), pos(field(ident("self"), "port"))],
            ))],
        ),
        function(
            "accept_once",
            vec![
                param("self", "dynamic"),
                param("max_bytes", "i32"),
                param("timeout_ms", "i32"),
            ],
            "String",
            vec![return_stmt(call_name(
                "tcp_accept_once",
                vec![
                    pos(field(ident("self"), "host")),
                    pos(field(ident("self"), "port")),
                    pos(ident("max_bytes")),
                    pos(ident("timeout_ms")),
                ],
            ))],
        ),
        function(
            "reply_once",
            vec![
                param("self", "dynamic"),
                param("value", "dynamic"),
                param("max_bytes", "i32"),
                param("timeout_ms", "i32"),
            ],
            "String",
            vec![return_stmt(call_name(
                "tcp_reply_once",
                vec![
                    pos(field(ident("self"), "host")),
                    pos(field(ident("self"), "port")),
                    pos(call_name("str", vec![pos(ident("value"))])),
                    pos(ident("max_bytes")),
                    pos(ident("timeout_ms")),
                ],
            ))],
        ),
        function(
            "reply_once_line",
            vec![
                param("self", "dynamic"),
                param("value", "dynamic"),
                param("max_bytes", "i32"),
                param("timeout_ms", "i32"),
            ],
            "String",
            vec![return_stmt(call_name(
                "reply_once_line",
                vec![
                    pos(field(ident("self"), "host")),
                    pos(field(ident("self"), "port")),
                    pos(ident("value")),
                    pos(ident("max_bytes")),
                    pos(ident("timeout_ms")),
                ],
            ))],
        ),
        function(
            "reply_once_text",
            vec![
                param("self", "dynamic"),
                param("value", "String"),
                param("max_bytes", "i32"),
                param("timeout_ms", "i32"),
            ],
            "String",
            vec![return_stmt(call_name(
                "tcp_reply_once",
                vec![
                    pos(field(ident("self"), "host")),
                    pos(field(ident("self"), "port")),
                    pos(ident("value")),
                    pos(ident("max_bytes")),
                    pos(ident("timeout_ms")),
                ],
            ))],
        ),
        function(
            "open_handle",
            vec![param("self", "dynamic")],
            "i32",
            vec![return_stmt(call_name(
                "tcp_server_open",
                vec![pos(field(ident("self"), "host")), pos(field(ident("self"), "port"))],
            ))],
        ),
        function(
            "accept",
            vec![
                param("self", "dynamic"),
                param("handle", "i32"),
                param("max_bytes", "i32"),
                param("timeout_ms", "i32"),
            ],
            "String",
            vec![return_stmt(call_name(
                "tcp_server_accept",
                vec![
                    pos(ident("handle")),
                    pos(ident("max_bytes")),
                    pos(ident("timeout_ms")),
                ],
            ))],
        ),
        function(
            "reply",
            vec![
                param("self", "dynamic"),
                param("handle", "i32"),
                param("value", "dynamic"),
                param("max_bytes", "i32"),
                param("timeout_ms", "i32"),
            ],
            "String",
            vec![return_stmt(call_name(
                "tcp_server_reply",
                vec![
                    pos(ident("handle")),
                    pos(call_name("str", vec![pos(ident("value"))])),
                    pos(ident("max_bytes")),
                    pos(ident("timeout_ms")),
                ],
            ))],
        ),
        function(
            "reply_line",
            vec![
                param("self", "dynamic"),
                param("handle", "i32"),
                param("value", "dynamic"),
                param("max_bytes", "i32"),
                param("timeout_ms", "i32"),
            ],
            "String",
            vec![return_stmt(call_name(
                "tcp_server_reply_line",
                vec![
                    pos(ident("handle")),
                    pos(ident("value")),
                    pos(ident("max_bytes")),
                    pos(ident("timeout_ms")),
                ],
            ))],
        ),
        function(
            "reply_text",
            vec![
                param("self", "dynamic"),
                param("handle", "i32"),
                param("value", "String"),
                param("max_bytes", "i32"),
                param("timeout_ms", "i32"),
            ],
            "String",
            vec![return_stmt(call_name(
                "tcp_server_reply",
                vec![
                    pos(ident("handle")),
                    pos(ident("value")),
                    pos(ident("max_bytes")),
                    pos(ident("timeout_ms")),
                ],
            ))],
        ),
        function(
            "close_handle",
            vec![param("self", "dynamic"), param("handle", "i32")],
            "bool",
            vec![return_stmt(call_name(
                "tcp_server_close",
                vec![pos(ident("handle"))],
            ))],
        ),
    ];

    let udp_endpoint_methods = vec![
        function(
            "bind",
            vec![param("self", "dynamic")],
            "bool",
            vec![return_stmt(call_name(
                "udp_bind",
                vec![pos(field(ident("self"), "host")), pos(field(ident("self"), "port"))],
            ))],
        ),
        function(
            "send",
            vec![param("self", "dynamic"), param("value", "dynamic")],
            "bool",
            vec![return_stmt(call_name(
                "udp_send",
                vec![
                    pos(field(ident("self"), "host")),
                    pos(field(ident("self"), "port")),
                    pos(call_name("str", vec![pos(ident("value"))])),
                ],
            ))],
        ),
        function(
            "send_line",
            vec![param("self", "dynamic"), param("value", "dynamic")],
            "bool",
            vec![return_stmt(call_name(
                "udp_send_line",
                vec![
                    pos(field(ident("self"), "host")),
                    pos(field(ident("self"), "port")),
                    pos(ident("value")),
                ],
            ))],
        ),
        function(
            "recv",
            vec![
                param("self", "dynamic"),
                param("max_bytes", "i32"),
                param("timeout_ms", "i32"),
            ],
            "String",
            vec![return_stmt(call_name(
                "udp_recv",
                vec![
                    pos(field(ident("self"), "host")),
                    pos(field(ident("self"), "port")),
                    pos(ident("max_bytes")),
                    pos(ident("timeout_ms")),
                ],
            ))],
        ),
        function(
            "send_text",
            vec![param("self", "dynamic"), param("value", "String")],
            "bool",
            vec![return_stmt(call_name(
                "udp_send",
                vec![
                    pos(field(ident("self"), "host")),
                    pos(field(ident("self"), "port")),
                    pos(ident("value")),
                ],
            ))],
        ),
        function(
            "send_line_text",
            vec![param("self", "dynamic"), param("value", "String")],
            "bool",
            vec![return_stmt(call_name(
                "udp_send_line",
                vec![
                    pos(field(ident("self"), "host")),
                    pos(field(ident("self"), "port")),
                    pos(ident("value")),
                ],
            ))],
        ),
    ];

    Program {
        items: vec![
            Item::Function(function(
                "tcp_connect",
                vec![param("host", "String"), param("port", "i32")],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_network_tcp_connect",
                    vec![pos(ident("host")), pos(ident("port"))],
                ))],
            )),
            Item::Function(function(
                "connect",
                vec![param("host", "String"), param("port", "i32")],
                "bool",
                vec![return_stmt(call_name(
                    "tcp_connect",
                    vec![pos(ident("host")), pos(ident("port"))],
                ))],
            )),
            Item::Function(function(
                "tcp_connect_timeout",
                vec![
                    param("host", "String"),
                    param("port", "i32"),
                    param("timeout_ms", "i32"),
                ],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_network_tcp_connect_timeout",
                    vec![pos(ident("host")), pos(ident("port")), pos(ident("timeout_ms"))],
                ))],
            )),
            Item::Function(function(
                "connect_timeout",
                vec![
                    param("host", "String"),
                    param("port", "i32"),
                    param("timeout_ms", "i32"),
                ],
                "bool",
                vec![return_stmt(call_name(
                    "tcp_connect_timeout",
                    vec![pos(ident("host")), pos(ident("port")), pos(ident("timeout_ms"))],
                ))],
            )),
            Item::Function(function(
                "tcp_probe",
                vec![param("host", "String"), param("port", "i32")],
                "bool",
                vec![return_stmt(call_name(
                    "tcp_connect",
                    vec![pos(ident("host")), pos(ident("port"))],
                ))],
            )),
            Item::Function(function(
                "probe",
                vec![param("host", "String"), param("port", "i32")],
                "bool",
                vec![return_stmt(call_name(
                    "tcp_probe",
                    vec![pos(ident("host")), pos(ident("port"))],
                ))],
            )),
            Item::Function(function(
                "tcp_probe_timeout",
                vec![
                    param("host", "String"),
                    param("port", "i32"),
                    param("timeout_ms", "i32"),
                ],
                "bool",
                vec![return_stmt(call_name(
                    "tcp_connect_timeout",
                    vec![pos(ident("host")), pos(ident("port")), pos(ident("timeout_ms"))],
                ))],
            )),
            Item::Function(function(
                "probe_timeout",
                vec![
                    param("host", "String"),
                    param("port", "i32"),
                    param("timeout_ms", "i32"),
                ],
                "bool",
                vec![return_stmt(call_name(
                    "tcp_probe_timeout",
                    vec![pos(ident("host")), pos(ident("port")), pos(ident("timeout_ms"))],
                ))],
            )),
            Item::Function(function(
                "tcp_listen",
                vec![param("host", "String"), param("port", "i32")],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_network_tcp_listen",
                    vec![pos(ident("host")), pos(ident("port"))],
                ))],
            )),
            Item::Function(function(
                "listen",
                vec![param("host", "String"), param("port", "i32")],
                "bool",
                vec![return_stmt(call_name(
                    "tcp_listen",
                    vec![pos(ident("host")), pos(ident("port"))],
                ))],
            )),
            Item::Function(function(
                "tcp_bind",
                vec![param("host", "String"), param("port", "i32")],
                "bool",
                vec![return_stmt(call_name(
                    "tcp_listen",
                    vec![pos(ident("host")), pos(ident("port"))],
                ))],
            )),
            Item::Function(function(
                "bind",
                vec![param("host", "String"), param("port", "i32")],
                "bool",
                vec![return_stmt(call_name(
                    "tcp_bind",
                    vec![pos(ident("host")), pos(ident("port"))],
                ))],
            )),
            Item::Function(function(
                "udp_bind",
                vec![param("host", "String"), param("port", "i32")],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_network_udp_bind",
                    vec![pos(ident("host")), pos(ident("port"))],
                ))],
            )),
            Item::Function(function(
                "tcp_send",
                vec![
                    param("host", "String"),
                    param("port", "i32"),
                    param("data", "String"),
                ],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_network_tcp_send",
                    vec![pos(ident("host")), pos(ident("port")), pos(ident("data"))],
                ))],
            )),
            Item::Function(function(
                "send",
                vec![
                    param("host", "String"),
                    param("port", "i32"),
                    param("data", "String"),
                ],
                "bool",
                vec![return_stmt(call_name(
                    "tcp_send",
                    vec![pos(ident("host")), pos(ident("port")), pos(ident("data"))],
                ))],
            )),
            Item::Function(function(
                "udp_send",
                vec![
                    param("host", "String"),
                    param("port", "i32"),
                    param("data", "String"),
                ],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_network_udp_send",
                    vec![pos(ident("host")), pos(ident("port")), pos(ident("data"))],
                ))],
            )),
            Item::Function(function(
                "send_udp",
                vec![
                    param("host", "String"),
                    param("port", "i32"),
                    param("data", "String"),
                ],
                "bool",
                vec![return_stmt(call_name(
                    "udp_send",
                    vec![pos(ident("host")), pos(ident("port")), pos(ident("data"))],
                ))],
            )),
            Item::Function(function(
                "tcp_recv",
                vec![
                    param("host", "String"),
                    param("port", "i32"),
                    param("max_bytes", "i32"),
                ],
                "String",
                vec![return_stmt(call_name(
                    "__rune_builtin_network_tcp_recv",
                    vec![pos(ident("host")), pos(ident("port")), pos(ident("max_bytes"))],
                ))],
            )),
            Item::Function(function(
                "recv",
                vec![
                    param("host", "String"),
                    param("port", "i32"),
                    param("max_bytes", "i32"),
                ],
                "String",
                vec![return_stmt(call_name(
                    "tcp_recv",
                    vec![pos(ident("host")), pos(ident("port")), pos(ident("max_bytes"))],
                ))],
            )),
            Item::Function(function(
                "tcp_recv_timeout",
                vec![
                    param("host", "String"),
                    param("port", "i32"),
                    param("max_bytes", "i32"),
                    param("timeout_ms", "i32"),
                ],
                "String",
                vec![return_stmt(call_name(
                    "__rune_builtin_network_tcp_recv_timeout",
                    vec![
                        pos(ident("host")),
                        pos(ident("port")),
                        pos(ident("max_bytes")),
                        pos(ident("timeout_ms")),
                    ],
                ))],
            )),
            Item::Function(function(
                "recv_timeout",
                vec![
                    param("host", "String"),
                    param("port", "i32"),
                    param("max_bytes", "i32"),
                    param("timeout_ms", "i32"),
                ],
                "String",
                vec![return_stmt(call_name(
                    "tcp_recv_timeout",
                    vec![
                        pos(ident("host")),
                        pos(ident("port")),
                        pos(ident("max_bytes")),
                        pos(ident("timeout_ms")),
                    ],
                ))],
            )),
            Item::Function(function(
                "udp_recv",
                vec![
                    param("host", "String"),
                    param("port", "i32"),
                    param("max_bytes", "i32"),
                    param("timeout_ms", "i32"),
                ],
                "String",
                vec![return_stmt(call_name(
                    "__rune_builtin_network_udp_recv",
                    vec![
                        pos(ident("host")),
                        pos(ident("port")),
                        pos(ident("max_bytes")),
                        pos(ident("timeout_ms")),
                    ],
                ))],
            )),
            Item::Function(function(
                "recv_udp",
                vec![
                    param("host", "String"),
                    param("port", "i32"),
                    param("max_bytes", "i32"),
                    param("timeout_ms", "i32"),
                ],
                "String",
                vec![return_stmt(call_name(
                    "udp_recv",
                    vec![
                        pos(ident("host")),
                        pos(ident("port")),
                        pos(ident("max_bytes")),
                        pos(ident("timeout_ms")),
                    ],
                ))],
            )),
            Item::Function(function(
                "tcp_request",
                vec![
                    param("host", "String"),
                    param("port", "i32"),
                    param("data", "String"),
                    param("max_bytes", "i32"),
                    param("timeout_ms", "i32"),
                ],
                "String",
                vec![return_stmt(call_name(
                    "__rune_builtin_network_tcp_request",
                    vec![
                        pos(ident("host")),
                        pos(ident("port")),
                        pos(ident("data")),
                        pos(ident("max_bytes")),
                        pos(ident("timeout_ms")),
                    ],
                ))],
            )),
            Item::Function(function(
                "tcp_accept_once",
                vec![
                    param("host", "String"),
                    param("port", "i32"),
                    param("max_bytes", "i32"),
                    param("timeout_ms", "i32"),
                ],
                "String",
                vec![return_stmt(call_name(
                    "__rune_builtin_network_tcp_accept_once",
                    vec![
                        pos(ident("host")),
                        pos(ident("port")),
                        pos(ident("max_bytes")),
                        pos(ident("timeout_ms")),
                    ],
                ))],
            )),
            Item::Function(function(
                "tcp_server_open",
                vec![param("host", "String"), param("port", "i32")],
                "i32",
                vec![return_stmt(call_name(
                    "__rune_builtin_network_tcp_server_open",
                    vec![pos(ident("host")), pos(ident("port"))],
                ))],
            )),
            Item::Function(function(
                "tcp_client_open",
                vec![
                    param("host", "String"),
                    param("port", "i32"),
                    param("timeout_ms", "i32"),
                ],
                "i32",
                vec![return_stmt(call_name(
                    "__rune_builtin_network_tcp_client_open",
                    vec![
                        pos(ident("host")),
                        pos(ident("port")),
                        pos(ident("timeout_ms")),
                    ],
                ))],
            )),
            Item::Function(function(
                "tcp_server_accept",
                vec![
                    param("handle", "i32"),
                    param("max_bytes", "i32"),
                    param("timeout_ms", "i32"),
                ],
                "String",
                vec![return_stmt(call_name(
                    "__rune_builtin_network_tcp_server_accept",
                    vec![
                        pos(ident("handle")),
                        pos(ident("max_bytes")),
                        pos(ident("timeout_ms")),
                    ],
                ))],
            )),
            Item::Function(function(
                "tcp_server_reply",
                vec![
                    param("handle", "i32"),
                    param("data", "String"),
                    param("max_bytes", "i32"),
                    param("timeout_ms", "i32"),
                ],
                "String",
                vec![return_stmt(call_name(
                    "__rune_builtin_network_tcp_server_reply",
                    vec![
                        pos(ident("handle")),
                        pos(ident("data")),
                        pos(ident("max_bytes")),
                        pos(ident("timeout_ms")),
                    ],
                ))],
            )),
            Item::Function(function(
                "tcp_client_send",
                vec![param("handle", "i32"), param("data", "String")],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_network_tcp_client_send",
                    vec![pos(ident("handle")), pos(ident("data"))],
                ))],
            )),
            Item::Function(function(
                "tcp_client_send_line",
                vec![param("handle", "i32"), param("value", "dynamic")],
                "bool",
                vec![return_stmt(call_name(
                    "tcp_client_send",
                    vec![
                        pos(ident("handle")),
                        pos(binary(
                            call_name("str", vec![pos(ident("value"))]),
                            BinaryOp::Add,
                            string_lit("\n"),
                        )),
                    ],
                ))],
            )),
            Item::Function(function(
                "tcp_client_recv",
                vec![
                    param("handle", "i32"),
                    param("max_bytes", "i32"),
                    param("timeout_ms", "i32"),
                ],
                "String",
                vec![return_stmt(call_name(
                    "__rune_builtin_network_tcp_client_recv",
                    vec![
                        pos(ident("handle")),
                        pos(ident("max_bytes")),
                        pos(ident("timeout_ms")),
                    ],
                ))],
            )),
            Item::Function(function(
                "tcp_server_reply_line",
                vec![
                    param("handle", "i32"),
                    param("value", "dynamic"),
                    param("max_bytes", "i32"),
                    param("timeout_ms", "i32"),
                ],
                "String",
                vec![return_stmt(call_name(
                    "tcp_server_reply",
                    vec![
                        pos(ident("handle")),
                        pos(binary(
                            call_name("str", vec![pos(ident("value"))]),
                            BinaryOp::Add,
                            string_lit("\n"),
                        )),
                        pos(ident("max_bytes")),
                        pos(ident("timeout_ms")),
                    ],
                ))],
            )),
            Item::Function(function(
                "tcp_client_close",
                vec![param("handle", "i32")],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_network_tcp_client_close",
                    vec![pos(ident("handle"))],
                ))],
            )),
            Item::Function(function(
                "tcp_server_close",
                vec![param("handle", "i32")],
                "bool",
                vec![return_stmt(call_name(
                    "__rune_builtin_network_tcp_server_close",
                    vec![pos(ident("handle"))],
                ))],
            )),
            Item::Function(function(
                "accept_once",
                vec![
                    param("host", "String"),
                    param("port", "i32"),
                    param("max_bytes", "i32"),
                    param("timeout_ms", "i32"),
                ],
                "String",
                vec![return_stmt(call_name(
                    "tcp_accept_once",
                    vec![
                        pos(ident("host")),
                        pos(ident("port")),
                        pos(ident("max_bytes")),
                        pos(ident("timeout_ms")),
                    ],
                ))],
            )),
            Item::Function(function(
                "tcp_reply_once",
                vec![
                    param("host", "String"),
                    param("port", "i32"),
                    param("data", "String"),
                    param("max_bytes", "i32"),
                    param("timeout_ms", "i32"),
                ],
                "String",
                vec![return_stmt(call_name(
                    "__rune_builtin_network_tcp_reply_once",
                    vec![
                        pos(ident("host")),
                        pos(ident("port")),
                        pos(ident("data")),
                        pos(ident("max_bytes")),
                        pos(ident("timeout_ms")),
                    ],
                ))],
            )),
            Item::Function(function(
                "reply_once",
                vec![
                    param("host", "String"),
                    param("port", "i32"),
                    param("data", "String"),
                    param("max_bytes", "i32"),
                    param("timeout_ms", "i32"),
                ],
                "String",
                vec![return_stmt(call_name(
                    "tcp_reply_once",
                    vec![
                        pos(ident("host")),
                        pos(ident("port")),
                        pos(ident("data")),
                        pos(ident("max_bytes")),
                        pos(ident("timeout_ms")),
                    ],
                ))],
            )),
            Item::Function(function(
                "reply_once_line",
                vec![
                    param("host", "String"),
                    param("port", "i32"),
                    param("value", "dynamic"),
                    param("max_bytes", "i32"),
                    param("timeout_ms", "i32"),
                ],
                "String",
                vec![return_stmt(call_name(
                    "tcp_reply_once",
                    vec![
                        pos(ident("host")),
                        pos(ident("port")),
                        pos(binary(
                            call_name("str", vec![pos(ident("value"))]),
                            BinaryOp::Add,
                            string_lit("\n"),
                        )),
                        pos(ident("max_bytes")),
                        pos(ident("timeout_ms")),
                    ],
                ))],
            )),
            Item::Function(function(
                "request",
                vec![
                    param("host", "String"),
                    param("port", "i32"),
                    param("data", "String"),
                    param("max_bytes", "i32"),
                    param("timeout_ms", "i32"),
                ],
                "String",
                vec![return_stmt(call_name(
                    "tcp_request",
                    vec![
                        pos(ident("host")),
                        pos(ident("port")),
                        pos(ident("data")),
                        pos(ident("max_bytes")),
                        pos(ident("timeout_ms")),
                    ],
                ))],
            )),
            Item::Function(function(
                "last_error_code",
                vec![],
                "i32",
                vec![return_stmt(call_name(
                    "__rune_builtin_network_last_error_code",
                    vec![],
                ))],
            )),
            Item::Function(function(
                "last_error",
                vec![],
                "String",
                vec![return_stmt(call_name(
                    "__rune_builtin_network_last_error_message",
                    vec![],
                ))],
            )),
            Item::Function(function(
                "clear_error",
                vec![],
                "unit",
                vec![expr_stmt(call_name(
                    "__rune_builtin_network_clear_error",
                    vec![],
                ))],
            )),
            Item::Function(function(
                "request_line",
                vec![
                    param("host", "String"),
                    param("port", "i32"),
                    param("value", "dynamic"),
                    param("max_bytes", "i32"),
                    param("timeout_ms", "i32"),
                ],
                "String",
                vec![return_stmt(call_name(
                    "tcp_request",
                    vec![
                        pos(ident("host")),
                        pos(ident("port")),
                        pos(binary(
                            call_name("str", vec![pos(ident("value"))]),
                            BinaryOp::Add,
                            string_lit("\n"),
                        )),
                        pos(ident("max_bytes")),
                        pos(ident("timeout_ms")),
                    ],
                ))],
            )),
            Item::Function(function(
                "tcp_send_line",
                vec![
                    param("host", "String"),
                    param("port", "i32"),
                    param("value", "dynamic"),
                ],
                "bool",
                vec![return_stmt(call_name(
                    "tcp_send",
                    vec![
                        pos(ident("host")),
                        pos(ident("port")),
                        pos(binary(
                            call_name("str", vec![pos(ident("value"))]),
                            BinaryOp::Add,
                            string_lit("\n"),
                        )),
                    ],
                ))],
            )),
            Item::Function(function(
                "send_line",
                vec![
                    param("host", "String"),
                    param("port", "i32"),
                    param("value", "dynamic"),
                ],
                "bool",
                vec![return_stmt(call_name(
                    "tcp_send_line",
                    vec![pos(ident("host")), pos(ident("port")), pos(ident("value"))],
                ))],
            )),
            Item::Function(function(
                "udp_send_line",
                vec![
                    param("host", "String"),
                    param("port", "i32"),
                    param("value", "dynamic"),
                ],
                "bool",
                vec![return_stmt(call_name(
                    "udp_send",
                    vec![
                        pos(ident("host")),
                        pos(ident("port")),
                        pos(binary(
                            call_name("str", vec![pos(ident("value"))]),
                            BinaryOp::Add,
                            string_lit("\n"),
                        )),
                    ],
                ))],
            )),
            Item::Function(function(
                "send_line_udp",
                vec![
                    param("host", "String"),
                    param("port", "i32"),
                    param("value", "dynamic"),
                ],
                "bool",
                vec![return_stmt(call_name(
                    "udp_send_line",
                    vec![pos(ident("host")), pos(ident("port")), pos(ident("value"))],
                ))],
            )),
            Item::Struct(StructDecl {
                name: "TcpClient".to_string(),
                fields: vec![
                    StructField {
                        name: "host".to_string(),
                        ty: ty("String"),
                        span: s(),
                    },
                    StructField {
                        name: "port".to_string(),
                        ty: ty("i32"),
                        span: s(),
                    },
                ],
                methods: tcp_client_methods,
                span: s(),
            }),
            Item::Struct(StructDecl {
                name: "UdpEndpoint".to_string(),
                fields: vec![
                    StructField {
                        name: "host".to_string(),
                        ty: ty("String"),
                        span: s(),
                    },
                    StructField {
                        name: "port".to_string(),
                        ty: ty("i32"),
                        span: s(),
                    },
                ],
                methods: udp_endpoint_methods,
                span: s(),
            }),
            Item::Struct(StructDecl {
                name: "TcpServer".to_string(),
                fields: vec![
                    StructField {
                        name: "host".to_string(),
                        ty: ty("String"),
                        span: s(),
                    },
                    StructField {
                        name: "port".to_string(),
                        ty: ty("i32"),
                        span: s(),
                    },
                ],
                methods: tcp_server_methods,
                span: s(),
            }),
            Item::Function(function(
                "tcp_client",
                vec![param("host", "String"), param("port", "i32")],
                "TcpClient",
                vec![return_stmt(call_expr(
                    ident("TcpClient"),
                    vec![kw("host", ident("host")), kw("port", ident("port"))],
                ))],
            )),
            Item::Function(function(
                "tcp_server",
                vec![param("host", "String"), param("port", "i32")],
                "TcpServer",
                vec![return_stmt(call_expr(
                    ident("TcpServer"),
                    vec![kw("host", ident("host")), kw("port", ident("port"))],
                ))],
            )),
            Item::Function(function(
                "udp_endpoint",
                vec![param("host", "String"), param("port", "i32")],
                "UdpEndpoint",
                vec![return_stmt(call_expr(
                    ident("UdpEndpoint"),
                    vec![kw("host", ident("host")), kw("port", ident("port"))],
                ))],
            )),
        ],
    }
}

pub fn builtin_module(module: &[String]) -> Option<BuiltinModule> {
    match module {
        [name] if name == "env" => Some(BuiltinModule {
            virtual_path: PathBuf::from("<builtin>/env"),
            body: BuiltinModuleBody::Program(env_program()),
        }),
        [name] if name == "time" => Some(BuiltinModule {
            virtual_path: PathBuf::from("<builtin>/time"),
            body: BuiltinModuleBody::Program(time_program()),
        }),
        [name] if name == "clock" => Some(BuiltinModule {
            virtual_path: PathBuf::from("<builtin>/clock"),
            body: BuiltinModuleBody::Program(clock_program()),
        }),
        [name] if name == "sys" || name == "system" => Some(BuiltinModule {
            virtual_path: PathBuf::from(format!("<builtin>/{name}")),
            body: BuiltinModuleBody::Program(sys_program()),
        }),
        [name] if name == "io" => Some(BuiltinModule {
            virtual_path: PathBuf::from("<builtin>/io"),
            body: BuiltinModuleBody::Program(io_program()),
        }),
        [name] if name == "terminal" => Some(BuiltinModule {
            virtual_path: PathBuf::from("<builtin>/terminal"),
            body: BuiltinModuleBody::Program(terminal_program()),
        }),
        [name] if name == "fs" => Some(BuiltinModule {
            virtual_path: PathBuf::from("<builtin>/fs"),
            body: BuiltinModuleBody::Program(fs_program()),
        }),
        [name] if name == "json" => Some(BuiltinModule {
            virtual_path: PathBuf::from("<builtin>/json"),
            body: BuiltinModuleBody::Program(json_program()),
        }),
        [name] if name == "audio" => Some(BuiltinModule {
            virtual_path: PathBuf::from("<builtin>/audio"),
            body: BuiltinModuleBody::Program(audio_program()),
        }),
        [name] if name == "network" => Some(BuiltinModule {
            virtual_path: PathBuf::from("<builtin>/network"),
            body: BuiltinModuleBody::Program(network_program()),
        }),
        [name] if name == "serial" => Some(BuiltinModule {
            virtual_path: PathBuf::from("<builtin>/serial"),
            body: BuiltinModuleBody::Program(serial_program()),
        }),
        [name] if name == "gpio" => Some(BuiltinModule {
            virtual_path: PathBuf::from("<builtin>/gpio"),
            body: BuiltinModuleBody::Program(gpio_program()),
        }),
        [name] if name == "pwm" => Some(BuiltinModule {
            virtual_path: PathBuf::from("<builtin>/pwm"),
            body: BuiltinModuleBody::Program(pwm_program()),
        }),
        [name] if name == "adc" => Some(BuiltinModule {
            virtual_path: PathBuf::from("<builtin>/adc"),
            body: BuiltinModuleBody::Program(adc_program()),
        }),
        _ => None,
    }
}

pub fn builtin_module_for_path(path: &Path) -> Option<BuiltinModule> {
    let path_text = path.to_str()?;
    let module_name = path_text.strip_prefix("<builtin>/")?;
    builtin_module(&[module_name.to_string()])
}

// === module_loader (merged from module_loader.rs) ===

#[derive(Debug)]
pub enum ModuleLoadError {
    Io {
        context: String,
        source: std::io::Error,
        trace: Vec<ImportSite>,
    },
    Parse {
        path: PathBuf,
        source: String,
        message: String,
        span: Span,
        trace: Vec<ImportSite>,
    },
    MissingModule {
        module: String,
        path: PathBuf,
        importer_path: PathBuf,
        importer_source: String,
        importer_span: Span,
        trace: Vec<ImportSite>,
    },
    MissingImport {
        module: String,
        name: String,
        path: PathBuf,
        importer_path: PathBuf,
        importer_source: String,
        importer_span: Span,
        trace: Vec<ImportSite>,
    },
    ImportCycle {
        module: String,
        path: PathBuf,
        importer_path: PathBuf,
        importer_source: String,
        importer_span: Span,
        trace: Vec<ImportSite>,
    },
}

impl fmt::Display for ModuleLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            ModuleLoadError::Io {
                context,
                source,
                trace: _,
            } => write!(f, "{code}: {context}: {source}"),
            ModuleLoadError::Parse {
                path,
                source: _,
                message,
                span: _,
                trace: _,
            } => {
                write!(f, "{code}: failed to parse `{}`: {message}", path.display())
            }
            ModuleLoadError::MissingModule {
                module,
                path,
                importer_path: _,
                importer_source: _,
                importer_span: _,
                trace: _,
            } => {
                write!(
                    f,
                    "{code}: module `{module}` was not found at `{}`",
                    path.display()
                )
            }
            ModuleLoadError::MissingImport {
                module,
                name,
                path,
                importer_path: _,
                importer_source: _,
                importer_span: _,
                trace: _,
            } => write!(
                f,
                "{code}: module `{module}` does not export `{name}` in `{}`",
                path.display()
            ),
            ModuleLoadError::ImportCycle {
                module,
                path,
                importer_path: _,
                importer_source: _,
                importer_span: _,
                trace: _,
            } => write!(
                f,
                "{code}: import cycle detected for module `{module}` at `{}`",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ModuleLoadError {}

impl ModuleLoadError {
    pub fn code(&self) -> &'static str {
        match self {
            ModuleLoadError::Io { .. } => "E2000",
            ModuleLoadError::Parse { .. } => "E2001",
            ModuleLoadError::MissingModule { .. } => "E2002",
            ModuleLoadError::MissingImport { .. } => "E2003",
            ModuleLoadError::ImportCycle { .. } => "E2004",
        }
    }

    fn push_trace(&mut self, site: ImportSite) {
        match self {
            ModuleLoadError::Io { trace, .. }
            | ModuleLoadError::Parse { trace, .. }
            | ModuleLoadError::MissingModule { trace, .. }
            | ModuleLoadError::MissingImport { trace, .. }
            | ModuleLoadError::ImportCycle { trace, .. } => trace.push(site),
        }
    }

    pub fn render(&self) -> String {
        let code = self.code();
        match self {
            ModuleLoadError::Io {
                context,
                source,
                trace,
            } => {
                let mut rendered = String::new();
                if !trace.is_empty() {
                    rendered.push_str(&render_import_trace(trace));
                    rendered.push('\n');
                }
                rendered.push_str(&format!("{code}: {context}: {source}"));
                rendered
            }
            ModuleLoadError::Parse {
                path,
                source,
                message,
                span,
                trace,
            } => {
                let mut rendered = String::new();
                if !trace.is_empty() {
                    rendered.push_str(&render_import_trace(trace));
                    rendered.push('\n');
                }
                rendered.push_str(&render_file_diagnostic(
                    path,
                    source,
                    &format!("{code}: {message}"),
                    *span,
                ));
                rendered
            }
            ModuleLoadError::MissingModule {
                module,
                path,
                importer_path,
                importer_source,
                importer_span,
                trace,
            } => {
                let mut rendered = String::new();
                if !trace.is_empty() {
                    rendered.push_str(&render_import_trace(trace));
                    rendered.push('\n');
                }
                rendered.push_str(&render_file_diagnostic(
                    importer_path,
                    importer_source,
                    &format!(
                        "{code}: module `{module}` was not found at `{}`",
                        path.display()
                    ),
                    *importer_span,
                ));
                rendered
            }
            ModuleLoadError::MissingImport {
                module,
                name,
                path,
                importer_path,
                importer_source,
                importer_span,
                trace,
            } => {
                let mut rendered = String::new();
                if !trace.is_empty() {
                    rendered.push_str(&render_import_trace(trace));
                    rendered.push('\n');
                }
                rendered.push_str(&render_file_diagnostic(
                    importer_path,
                    importer_source,
                    &format!(
                        "{code}: module `{module}` does not export `{name}` in `{}`",
                        path.display()
                    ),
                    *importer_span,
                ));
                rendered
            }
            ModuleLoadError::ImportCycle {
                module,
                path,
                importer_path,
                importer_source,
                importer_span,
                trace,
            } => {
                let mut rendered = String::new();
                if !trace.is_empty() {
                    rendered.push_str(&render_import_trace(trace));
                    rendered.push('\n');
                }
                rendered.push_str(&render_file_diagnostic(
                    importer_path,
                    importer_source,
                    &format!(
                        "{code}: import cycle detected for module `{module}` at `{}`",
                        path.display()
                    ),
                    *importer_span,
                ));
                rendered
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoadedProgram {
    pub program: Program,
    pub entry_path: PathBuf,
    pub function_origins: HashMap<String, PathBuf>,
    pub import_sites: HashMap<PathBuf, ImportSite>,
    pub sources: HashMap<PathBuf, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportKind {
    Function,
    Struct,
    Exception,
}

#[derive(Debug, Clone)]
struct ModuleExport {
    internal_name: String,
    kind: ExportKind,
}

type ExportMap = HashMap<String, ModuleExport>;

#[derive(Debug, Clone)]
pub struct ImportSite {
    pub importer_path: PathBuf,
    pub importer_span: crate::lexer::Span,
    pub module_name: String,
}

fn render_import_trace(trace: &[ImportSite]) -> String {
    let mut lines = Vec::with_capacity(trace.len() + 1);
    lines.push("Traceback (most recent import last):".to_string());
    for site in trace.iter().rev() {
        lines.push(format!(
            "  {}:{}:{} imported `{}`",
            site.importer_path.display(),
            site.importer_span.line,
            site.importer_span.column,
            site.module_name
        ));
    }
    lines.join("\n")
}

pub fn load_program_from_path(path: &Path) -> Result<Program, ModuleLoadError> {
    Ok(load_program_bundle_from_path(path)?.program)
}

pub fn load_program_bundle_from_path(path: &Path) -> Result<LoadedProgram, ModuleLoadError> {
    let canonical = fs::canonicalize(path).map_err(|source| ModuleLoadError::Io {
        context: format!("failed to resolve `{}`", path.display()),
        source,
        trace: Vec::new(),
    })?;

    let mut visited = BTreeSet::new();
    let mut exceptions = Vec::new();
    let mut structs = Vec::new();
    let mut functions = Vec::new();
    let mut function_origins = HashMap::new();
    let mut import_sites = HashMap::new();
    let mut sources = HashMap::new();
    let mut export_maps = HashMap::new();
    let mut active_stack = Vec::new();
    load_module_recursive(
        &canonical,
        true,
        &mut visited,
        &mut active_stack,
        &mut exceptions,
        &mut structs,
        &mut functions,
        &mut function_origins,
        &mut import_sites,
        &mut sources,
        &mut export_maps,
    )?;
    Ok(LoadedProgram {
        program: Program {
            items: exceptions
                .into_iter()
                .map(Item::Exception)
                .chain(structs.into_iter().map(Item::Struct))
                .chain(functions.into_iter().map(Item::Function))
                .collect(),
        },
        entry_path: canonical,
        function_origins,
        import_sites,
        sources,
    })
}

fn load_module_recursive(
    path: &Path,
    is_entry: bool,
    visited: &mut BTreeSet<PathBuf>,
    active_stack: &mut Vec<PathBuf>,
    out_exceptions: &mut Vec<ExceptionDecl>,
    out_structs: &mut Vec<StructDecl>,
    out_functions: &mut Vec<Function>,
    function_origins: &mut HashMap<String, PathBuf>,
    import_sites: &mut HashMap<PathBuf, ImportSite>,
    sources: &mut HashMap<PathBuf, String>,
    export_maps: &mut HashMap<PathBuf, ExportMap>,
) -> Result<ExportMap, ModuleLoadError> {
    if active_stack.contains(&path.to_path_buf()) {
        return Err(ModuleLoadError::ImportCycle {
            module: path
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("<module>")
                .to_string(),
            path: path.to_path_buf(),
            importer_path: path.to_path_buf(),
            importer_source: sources.get(path).cloned().unwrap_or_default(),
            importer_span: Span { line: 1, column: 1 },
            trace: Vec::new(),
        });
    }
    if !visited.insert(path.to_path_buf()) {
        return Ok(export_maps.get(path).cloned().unwrap_or_default());
    }

    active_stack.push(path.to_path_buf());
    let (program, source) = load_module_program(path)?;
    sources.insert(path.to_path_buf(), source.clone());

    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut direct_imports: ExportMap = HashMap::new();
    let mut namespace_imports: HashMap<String, ExportMap> = HashMap::new();
    for item in &program.items {
        if let Item::Import(import) = item {
            let module_path = resolve_module_path(base_dir, import);
            let is_builtin = module_path
                .to_str()
                .is_some_and(|path_text| path_text.starts_with("<builtin>/"));
            if !is_builtin && !module_path.is_file() {
                return Err(ModuleLoadError::MissingModule {
                    module: import.module.join("."),
                    path: module_path,
                    importer_path: path.to_path_buf(),
                    importer_source: source.clone(),
                    importer_span: import.span,
                    trace: Vec::new(),
                });
            }

            import_sites
                .entry(module_path.clone())
                .or_insert_with(|| ImportSite {
                    importer_path: path.to_path_buf(),
                    importer_span: import.span,
                    module_name: import.module.join("."),
                });

            let (nested_program, _) = load_module_program(&module_path).map_err(|mut error| {
                error.push_trace(ImportSite {
                    importer_path: path.to_path_buf(),
                    importer_span: import.span,
                    module_name: import.module.join("."),
                });
                error
            })?;

            if let Some(names) = &import.names {
                for name in names {
                    let exists = nested_program.items.iter().any(|item| {
                        matches!(item, Item::Function(function) if function.name == *name)
                            || matches!(item, Item::Exception(exception) if exception.name == *name)
                            || matches!(item, Item::Struct(decl) if decl.name == *name)
                    });
                    if !exists {
                        return Err(ModuleLoadError::MissingImport {
                            module: import.module.join("."),
                            name: name.clone(),
                            path: module_path.clone(),
                            importer_path: path.to_path_buf(),
                            importer_source: source.clone(),
                            importer_span: import.span,
                            trace: Vec::new(),
                        });
                    }
                }
            }

            let nested_exports = load_module_recursive(
                &module_path,
                false,
                visited,
                active_stack,
                out_exceptions,
                out_structs,
                out_functions,
                function_origins,
                import_sites,
                sources,
                export_maps,
            )
            .map_err(|mut error| {
                error.push_trace(ImportSite {
                    importer_path: path.to_path_buf(),
                    importer_span: import.span,
                    module_name: import.module.join("."),
                });
                error
            })?;

            if let Some(names) = &import.names {
                for name in names {
                    if let Some(export) = nested_exports.get(name) {
                        direct_imports.insert(name.clone(), export.clone());
                    }
                }
            } else if let Some(alias) = import.module.last() {
                namespace_imports.insert(alias.clone(), nested_exports);
            }
        }
    }
    active_stack.pop();

    let own_exports = collect_module_exports(path, &program, is_entry);
    export_maps.insert(path.to_path_buf(), own_exports.clone());
    let rewritten = rewrite_program_for_namespace(
        &program,
        &own_exports,
        &direct_imports,
        &namespace_imports,
    );

    for item in rewritten.items {
        match item {
            Item::Exception(exception) => out_exceptions.push(exception),
            Item::Struct(decl) => out_structs.push(decl),
            Item::Function(function) => {
                function_origins.insert(function.name.clone(), path.to_path_buf());
                out_functions.push(function);
            }
            Item::Import(_) => {}
        }
    }

    Ok(own_exports)
}

fn collect_module_exports(path: &Path, program: &Program, is_entry: bool) -> ExportMap {
    let mut exports = HashMap::new();
    for item in &program.items {
        match item {
            Item::Function(function) => {
                exports.insert(
                    function.name.clone(),
                    ModuleExport {
                        internal_name: module_internal_symbol(path, &function.name, is_entry),
                        kind: ExportKind::Function,
                    },
                );
            }
            Item::Struct(decl) => {
                exports.insert(
                    decl.name.clone(),
                    ModuleExport {
                        internal_name: module_internal_symbol(path, &decl.name, is_entry),
                        kind: ExportKind::Struct,
                    },
                );
            }
            Item::Exception(exception) => {
                exports.insert(
                    exception.name.clone(),
                    ModuleExport {
                        internal_name: module_internal_symbol(path, &exception.name, is_entry),
                        kind: ExportKind::Exception,
                    },
                );
            }
            Item::Import(_) => {}
        }
    }
    exports
}

fn module_internal_symbol(path: &Path, name: &str, is_entry: bool) -> String {
    if is_entry {
        return name.to_string();
    }
    let raw = path.display().to_string();
    let mut prefix = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            prefix.push(ch);
        } else {
            prefix.push('_');
        }
    }
    format!("__mod_{prefix}__{name}")
}

fn rewrite_program_for_namespace(
    program: &Program,
    own_exports: &ExportMap,
    direct_imports: &ExportMap,
    namespace_imports: &HashMap<String, ExportMap>,
) -> Program {
    Program {
        items: program
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Import(_) => None,
                Item::Exception(exception) => Some(Item::Exception(ExceptionDecl {
                    name: own_exports
                        .get(&exception.name)
                        .map(|export| export.internal_name.clone())
                        .unwrap_or_else(|| exception.name.clone()),
                    span: exception.span,
                })),
                Item::Struct(decl) => Some(Item::Struct(rewrite_struct_decl(
                    decl,
                    own_exports,
                    direct_imports,
                    namespace_imports,
                ))),
                Item::Function(function) => Some(Item::Function(rewrite_function(
                    function,
                    own_exports,
                    direct_imports,
                    namespace_imports,
                ))),
            })
            .collect(),
    }
}

fn rewrite_struct_decl(
    decl: &StructDecl,
    own_exports: &ExportMap,
    direct_imports: &ExportMap,
    namespace_imports: &HashMap<String, ExportMap>,
) -> StructDecl {
    StructDecl {
        name: own_exports
            .get(&decl.name)
            .map(|export| export.internal_name.clone())
            .unwrap_or_else(|| decl.name.clone()),
        fields: decl
            .fields
            .iter()
            .map(|field| StructField {
                name: field.name.clone(),
                ty: rewrite_type_ref(&field.ty, own_exports, direct_imports),
                span: field.span,
            })
            .collect(),
        methods: decl
            .methods
            .iter()
            .map(|method| rewrite_method(method, own_exports, direct_imports, namespace_imports))
            .collect(),
        span: decl.span,
    }
}

fn rewrite_method(
    function: &Function,
    own_exports: &ExportMap,
    direct_imports: &ExportMap,
    namespace_imports: &HashMap<String, ExportMap>,
) -> Function {
    let mut rewritten = rewrite_function(function, own_exports, direct_imports, namespace_imports);
    rewritten.name = function.name.clone();
    rewritten
}

fn rewrite_function(
    function: &Function,
    own_exports: &ExportMap,
    direct_imports: &ExportMap,
    namespace_imports: &HashMap<String, ExportMap>,
) -> Function {
    let mut locals = HashMap::new();
    for param in &function.params {
        locals.insert(param.name.clone(), ());
    }
    Function {
        is_extern: function.is_extern,
        is_async: function.is_async,
        name: own_exports
            .get(&function.name)
            .map(|export| export.internal_name.clone())
            .unwrap_or_else(|| function.name.clone()),
        params: function
            .params
            .iter()
            .map(|param| Param {
                name: param.name.clone(),
                ty: rewrite_type_ref(&param.ty, own_exports, direct_imports),
                span: param.span,
            })
            .collect(),
        return_type: function
            .return_type
            .as_ref()
            .map(|ty| rewrite_type_ref(ty, own_exports, direct_imports)),
        raises: function
            .raises
            .as_ref()
            .map(|ty| rewrite_type_ref(ty, own_exports, direct_imports)),
        body: rewrite_block(
            &function.body,
            &mut locals,
            own_exports,
            direct_imports,
            namespace_imports,
        ),
        span: function.span,
    }
}

fn rewrite_block(
    block: &Block,
    locals: &mut HashMap<String, ()>,
    own_exports: &ExportMap,
    direct_imports: &ExportMap,
    namespace_imports: &HashMap<String, ExportMap>,
) -> Block {
    let mut scoped = locals.clone();
    Block {
        statements: block
            .statements
            .iter()
            .map(|stmt| {
                rewrite_stmt(
                    stmt,
                    &mut scoped,
                    own_exports,
                    direct_imports,
                    namespace_imports,
                )
            })
            .collect(),
    }
}

fn rewrite_stmt(
    stmt: &Stmt,
    locals: &mut HashMap<String, ()>,
    own_exports: &ExportMap,
    direct_imports: &ExportMap,
    namespace_imports: &HashMap<String, ExportMap>,
) -> Stmt {
    match stmt {
        Stmt::Block(block) => Stmt::Block(crate::parser::BlockStmt {
            block: rewrite_block(
                &block.block,
                locals,
                own_exports,
                direct_imports,
                namespace_imports,
            ),
            span: block.span,
        }),
        Stmt::Let(let_stmt) => {
            let value = rewrite_expr(
                &let_stmt.value,
                locals,
                own_exports,
                direct_imports,
                namespace_imports,
            );
            let ty = let_stmt
                .ty
                .as_ref()
                .map(|ty| rewrite_type_ref(ty, own_exports, direct_imports));
            locals.insert(let_stmt.name.clone(), ());
            Stmt::Let(LetStmt {
                name: let_stmt.name.clone(),
                ty,
                value,
                span: let_stmt.span,
            })
        }
        Stmt::Assign(assign) => Stmt::Assign(AssignStmt {
            name: assign.name.clone(),
            value: rewrite_expr(
                &assign.value,
                locals,
                own_exports,
                direct_imports,
                namespace_imports,
            ),
            span: assign.span,
        }),
        Stmt::FieldAssign(assign) => Stmt::FieldAssign(FieldAssignStmt {
            base: assign.base.clone(),
            fields: assign.fields.clone(),
            value: rewrite_expr(
                &assign.value,
                locals,
                own_exports,
                direct_imports,
                namespace_imports,
            ),
            span: assign.span,
        }),
        Stmt::Return(ret) => Stmt::Return(ReturnStmt {
            value: ret.value.as_ref().map(|expr| {
                rewrite_expr(expr, locals, own_exports, direct_imports, namespace_imports)
            }),
            span: ret.span,
        }),
        Stmt::If(if_stmt) => Stmt::If(IfStmt {
            condition: rewrite_expr(
                &if_stmt.condition,
                locals,
                own_exports,
                direct_imports,
                namespace_imports,
            ),
            then_block: rewrite_block(
                &if_stmt.then_block,
                locals,
                own_exports,
                direct_imports,
                namespace_imports,
            ),
            elif_blocks: if_stmt
                .elif_blocks
                .iter()
                .map(|elif| ElifBlock {
                    condition: rewrite_expr(
                        &elif.condition,
                        locals,
                        own_exports,
                        direct_imports,
                        namespace_imports,
                    ),
                    block: rewrite_block(
                        &elif.block,
                        locals,
                        own_exports,
                        direct_imports,
                        namespace_imports,
                    ),
                    span: elif.span,
                })
                .collect(),
            else_block: if_stmt.else_block.as_ref().map(|block| {
                rewrite_block(
                    block,
                    locals,
                    own_exports,
                    direct_imports,
                    namespace_imports,
                )
            }),
            span: if_stmt.span,
        }),
        Stmt::While(while_stmt) => Stmt::While(WhileStmt {
            condition: rewrite_expr(
                &while_stmt.condition,
                locals,
                own_exports,
                direct_imports,
                namespace_imports,
            ),
            body: rewrite_block(
                &while_stmt.body,
                locals,
                own_exports,
                direct_imports,
                namespace_imports,
            ),
            span: while_stmt.span,
        }),
        Stmt::Break(stmt) => Stmt::Break(stmt.clone()),
        Stmt::Continue(stmt) => Stmt::Continue(stmt.clone()),
        Stmt::Raise(stmt) => Stmt::Raise(RaiseStmt {
            value: rewrite_expr(
                &stmt.value,
                locals,
                own_exports,
                direct_imports,
                namespace_imports,
            ),
            span: stmt.span,
        }),
        Stmt::Panic(stmt) => Stmt::Panic(PanicStmt {
            value: rewrite_expr(
                &stmt.value,
                locals,
                own_exports,
                direct_imports,
                namespace_imports,
            ),
            span: stmt.span,
        }),
        Stmt::Expr(stmt) => Stmt::Expr(ExprStmt {
            expr: rewrite_expr(
                &stmt.expr,
                locals,
                own_exports,
                direct_imports,
                namespace_imports,
            ),
        }),
    }
}

fn rewrite_expr(
    expr: &Expr,
    locals: &HashMap<String, ()>,
    own_exports: &ExportMap,
    direct_imports: &ExportMap,
    namespace_imports: &HashMap<String, ExportMap>,
) -> Expr {
    match &expr.kind {
        ExprKind::Identifier(name) => Expr {
            kind: ExprKind::Identifier(resolve_identifier(
                name,
                locals,
                own_exports,
                direct_imports,
            )),
            span: expr.span,
        },
        ExprKind::Integer(_) | ExprKind::String(_) | ExprKind::Bool(_) => expr.clone(),
        ExprKind::Unary { op, expr: inner } => Expr {
            kind: ExprKind::Unary {
                op: *op,
                expr: Box::new(rewrite_expr(
                    inner,
                    locals,
                    own_exports,
                    direct_imports,
                    namespace_imports,
                )),
            },
            span: expr.span,
        },
        ExprKind::Binary { left, op, right } => Expr {
            kind: ExprKind::Binary {
                left: Box::new(rewrite_expr(
                    left,
                    locals,
                    own_exports,
                    direct_imports,
                    namespace_imports,
                )),
                op: *op,
                right: Box::new(rewrite_expr(
                    right,
                    locals,
                    own_exports,
                    direct_imports,
                    namespace_imports,
                )),
            },
            span: expr.span,
        },
        ExprKind::Call { callee, args } => Expr {
            kind: ExprKind::Call {
                callee: Box::new(rewrite_expr(
                    callee,
                    locals,
                    own_exports,
                    direct_imports,
                    namespace_imports,
                )),
                args: args
                    .iter()
                    .map(|arg| match arg {
                        CallArg::Positional(value) => CallArg::Positional(rewrite_expr(
                            value,
                            locals,
                            own_exports,
                            direct_imports,
                            namespace_imports,
                        )),
                        CallArg::Keyword { name, value, span } => CallArg::Keyword {
                            name: name.clone(),
                            value: rewrite_expr(
                                value,
                                locals,
                                own_exports,
                                direct_imports,
                                namespace_imports,
                            ),
                            span: *span,
                        },
                    })
                    .collect(),
            },
            span: expr.span,
        },
        ExprKind::Await { expr: inner } => Expr {
            kind: ExprKind::Await {
                expr: Box::new(rewrite_expr(
                    inner,
                    locals,
                    own_exports,
                    direct_imports,
                    namespace_imports,
                )),
            },
            span: expr.span,
        },
        ExprKind::Field { base, name } => {
            if let ExprKind::Identifier(module_name) = &base.kind
                && !locals.contains_key(module_name)
                && let Some(exports) = namespace_imports.get(module_name)
                && let Some(export) = exports.get(name)
            {
                return Expr {
                    kind: ExprKind::Identifier(export.internal_name.clone()),
                    span: expr.span,
                };
            }
            Expr {
                kind: ExprKind::Field {
                    base: Box::new(rewrite_expr(
                        base,
                        locals,
                        own_exports,
                        direct_imports,
                        namespace_imports,
                    )),
                    name: name.clone(),
                },
                span: expr.span,
            }
        }
    }
}

fn resolve_identifier(
    name: &str,
    locals: &HashMap<String, ()>,
    own_exports: &ExportMap,
    direct_imports: &ExportMap,
) -> String {
    if locals.contains_key(name) {
        return name.to_string();
    }
    if let Some(export) = direct_imports.get(name) {
        return export.internal_name.clone();
    }
    if let Some(export) = own_exports.get(name) {
        return export.internal_name.clone();
    }
    name.to_string()
}

fn rewrite_type_ref(ty: &TypeRef, own_exports: &ExportMap, direct_imports: &ExportMap) -> TypeRef {
    let rewritten = direct_imports
        .get(&ty.name)
        .filter(|export| matches!(export.kind, ExportKind::Struct | ExportKind::Exception))
        .or_else(|| {
            own_exports
                .get(&ty.name)
                .filter(|export| matches!(export.kind, ExportKind::Struct | ExportKind::Exception))
        })
        .map(|export| export.internal_name.clone())
        .unwrap_or_else(|| ty.name.clone());
    TypeRef {
        name: rewritten,
        span: ty.span,
    }
}

fn resolve_module_path(base_dir: &Path, import: &ImportDecl) -> PathBuf {
    if import.level == 0 && let Some(module) = builtin_module(&import.module) {
        return module.virtual_path;
    }

    if import.level == 0 {
        let roots = [
            "system", "sys", "time", "network", "env", "fs", "terminal", "audio", "io",
            "json", "arduino", "gpio", "serial", "pwm", "adc",
        ];
        if import
            .module
            .first()
            .is_some_and(|segment| roots.contains(&segment.as_str()))
        {
            let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("stdlib");
            for segment in &import.module {
                path.push(segment);
            }
            path.set_extension("rn");
            return path;
        }
    }

    if import.level == 0
        && import
            .module
            .first()
            .is_some_and(|segment| segment == "std")
    {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("stdlib");
        for segment in &import.module {
            path.push(segment);
        }
        path.set_extension("rn");
        return path;
    }

    let mut path = base_dir.to_path_buf();
    if import.level > 1 {
        for _ in 1..import.level {
            if let Some(parent) = path.parent() {
                path = parent.to_path_buf();
            }
        }
    }
    for segment in &import.module {
        path.push(segment);
    }
    path.set_extension("rn");
    path
}

fn load_module_program(path: &Path) -> Result<(Program, String), ModuleLoadError> {
    if let Some(module) = builtin_module_for_path(path) {
        return match module.body {
            BuiltinModuleBody::Program(program) => {
                Ok((program, format!("<builtin module {}>", path.display())))
            }
        };
    }

    let source = fs::read_to_string(path).map_err(|source| ModuleLoadError::Io {
        context: format!("failed to read `{}`", path.display()),
        source,
        trace: Vec::new(),
    })?;
    let program = parse_source(&source).map_err(|error| ModuleLoadError::Parse {
        path: path.to_path_buf(),
        source: source.clone(),
        message: error.to_string(),
        span: error.span,
        trace: Vec::new(),
    })?;
    Ok((program, source))
}
