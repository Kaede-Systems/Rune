use std::path::{Path, PathBuf};

use crate::lexer::Span;
use crate::parser::{
    BinaryOp, Block, CallArg, ElifBlock, Expr, ExprKind, Function, IfStmt, ImportDecl, Item,
    Param, Program, ReturnStmt, Stmt, StructDecl, StructField, TypeRef,
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

fn import_item(module: &[&str], names: &[&str]) -> Item {
    Item::Import(ImportDecl {
        level: 0,
        module: module.iter().map(|segment| (*segment).to_string()).collect(),
        names: Some(names.iter().map(|name| (*name).to_string()).collect()),
        span: s(),
    })
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
            "recv_line",
            vec![param("self", "dynamic")],
            "String",
            vec![return_stmt(call_name("recv_line", vec![]))],
        ),
        function(
            "recv_nonempty",
            vec![param("self", "dynamic")],
            "String",
            vec![return_stmt(call_name("recv_nonempty", vec![]))],
        ),
        function(
            "send",
            vec![param("self", "dynamic"), param("value", "dynamic")],
            "bool",
            vec![return_stmt(call_name("send", vec![pos(ident("value"))]))],
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
                    return_stmt(int_lit(0)),
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
                    return_stmt(int_lit(-1)),
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
                "pin_mode",
                vec![
                    pos(field(ident("self"), "pin")),
                    pos(call_name("mode_output", vec![])),
                ],
            ))],
        ),
        function(
            "input",
            vec![param("self", "dynamic")],
            "unit",
            vec![expr_stmt(call_name(
                "pin_mode",
                vec![
                    pos(field(ident("self"), "pin")),
                    pos(call_name("mode_input", vec![])),
                ],
            ))],
        ),
        function(
            "input_pullup",
            vec![param("self", "dynamic")],
            "unit",
            vec![expr_stmt(call_name(
                "pin_mode",
                vec![
                    pos(field(ident("self"), "pin")),
                    pos(call_name("mode_input_pullup", vec![])),
                ],
            ))],
        ),
        function(
            "write",
            vec![param("self", "dynamic"), param("value", "bool")],
            "unit",
            vec![expr_stmt(call_name(
                "digital_out",
                vec![pos(field(ident("self"), "pin")), pos(ident("value"))],
            ))],
        ),
        function(
            "high",
            vec![param("self", "dynamic")],
            "unit",
            vec![expr_stmt(call_name(
                "digital_out",
                vec![pos(field(ident("self"), "pin")), pos(bool_lit(true))],
            ))],
        ),
        function(
            "low",
            vec![param("self", "dynamic")],
            "unit",
            vec![expr_stmt(call_name(
                "digital_out",
                vec![pos(field(ident("self"), "pin")), pos(bool_lit(false))],
            ))],
        ),
        function(
            "toggle",
            vec![param("self", "dynamic")],
            "unit",
            vec![expr_stmt(call_name(
                "digital_toggle",
                vec![pos(field(ident("self"), "pin"))],
            ))],
        ),
        function(
            "read",
            vec![param("self", "dynamic")],
            "bool",
            vec![return_stmt(call_name(
                "digital_in",
                vec![pos(field(ident("self"), "pin"))],
            ))],
        ),
        function(
            "read_pullup",
            vec![param("self", "dynamic")],
            "bool",
            vec![return_stmt(call_name(
                "digital_in_pullup",
                vec![pos(field(ident("self"), "pin"))],
            ))],
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
                            expr_stmt(call_name("delay_ms", vec![pos(ident("on_ms"))])),
                            expr_stmt(call_expr(field(ident("self"), "low"), vec![])),
                            expr_stmt(call_name("delay_ms", vec![pos(ident("off_ms"))])),
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
                "pin_mode",
                vec![
                    pos(field(ident("self"), "pin")),
                    pos(call_name("mode_output", vec![])),
                ],
            ))],
        ),
        function(
            "write",
            vec![param("self", "dynamic"), param("duty", "i64")],
            "unit",
            vec![expr_stmt(call_name(
                "pwm_write",
                vec![pos(field(ident("self"), "pin")), pos(ident("duty"))],
            ))],
        ),
        function(
            "max_duty",
            vec![param("self", "dynamic")],
            "i64",
            vec![return_stmt(call_name("pwm_duty_max", vec![]))],
        ),
        function(
            "off",
            vec![param("self", "dynamic")],
            "unit",
            vec![expr_stmt(call_name(
                "pwm_write",
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
                "analog_read",
                vec![pos(field(ident("self"), "pin"))],
            ))],
        ),
        function(
            "read_percent",
            vec![param("self", "dynamic")],
            "i64",
            vec![return_stmt(call_name(
                "analog_read_percent",
                vec![pos(field(ident("self"), "pin"))],
            ))],
        ),
        function(
            "read_voltage_mv",
            vec![param("self", "dynamic"), param("reference_mv", "i64")],
            "i64",
            vec![return_stmt(call_name(
                "analog_read_voltage_mv",
                vec![pos(field(ident("self"), "pin")), pos(ident("reference_mv"))],
            ))],
        ),
    ];

    Program {
        items: vec![
            import_item(
                &["arduino"],
                &[
                    "analog_read",
                    "analog_read_percent",
                    "analog_read_voltage_mv",
                    "delay_ms",
                    "digital_in",
                    "digital_in_pullup",
                    "digital_out",
                    "digital_toggle",
                    "mode_input",
                    "mode_input_pullup",
                    "mode_output",
                    "pin_mode",
                    "pwm_duty_max",
                    "pwm_write",
                ],
            ),
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
        _ => None,
    }
}

pub fn builtin_module_for_path(path: &Path) -> Option<BuiltinModule> {
    let path_text = path.to_str()?;
    let module_name = path_text.strip_prefix("<builtin>/")?;
    builtin_module(&[module_name.to_string()])
}
