use std::collections::BTreeSet;

use crate::lexer::Span;
use crate::parser::{CallArg, Expr, ExprKind, Item, Program, Stmt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub message: String,
    pub span: Span,
}

pub fn collect_warnings(program: &Program) -> Vec<Warning> {
    let mut called = BTreeSet::new();
    for item in &program.items {
        let Item::Function(function) = item else {
            continue;
        };
        if function.is_extern {
            continue;
        }
        collect_calls_in_block(&function.body, &mut called);
    }

    let mut warnings = Vec::new();
    for item in &program.items {
        let Item::Function(function) = item else {
            continue;
        };
        if function.is_extern {
            continue;
        }
        if function.name != "main" && !called.contains(&function.name) {
            warnings.push(Warning {
                message: format!("function `{}` is never used", function.name),
                span: function.span,
            });
        }

        let mut declared = Vec::<(String, Span)>::new();
        let mut used = BTreeSet::<String>::new();
        for param in &function.params {
            declared.push((param.name.clone(), param.span));
        }
        collect_locals_and_uses_in_block(&function.body, &mut declared, &mut used);
        for (name, span) in declared {
            if !used.contains(&name) {
                warnings.push(Warning {
                    message: format!("variable `{name}` is never used"),
                    span,
                });
            }
        }
    }
    warnings
}

fn collect_locals_and_uses_in_block(
    block: &crate::parser::Block,
    declared: &mut Vec<(String, Span)>,
    used: &mut BTreeSet<String>,
) {
    for stmt in &block.statements {
        match stmt {
            Stmt::Block(stmt) => collect_locals_and_uses_in_block(&stmt.block, declared, used),
            Stmt::Let(stmt) => {
                declared.push((stmt.name.clone(), stmt.span));
                collect_used_identifiers_in_expr(&stmt.value, used);
            }
            Stmt::Assign(stmt) => {
                used.insert(stmt.name.clone());
                collect_used_identifiers_in_expr(&stmt.value, used);
            }
            Stmt::Return(stmt) => {
                if let Some(expr) = &stmt.value {
                    collect_used_identifiers_in_expr(expr, used);
                }
            }
            Stmt::If(stmt) => {
                collect_used_identifiers_in_expr(&stmt.condition, used);
                collect_locals_and_uses_in_block(&stmt.then_block, declared, used);
                for elif in &stmt.elif_blocks {
                    collect_used_identifiers_in_expr(&elif.condition, used);
                    collect_locals_and_uses_in_block(&elif.block, declared, used);
                }
                if let Some(block) = &stmt.else_block {
                    collect_locals_and_uses_in_block(block, declared, used);
                }
            }
            Stmt::While(stmt) => {
                collect_used_identifiers_in_expr(&stmt.condition, used);
                collect_locals_and_uses_in_block(&stmt.body, declared, used);
            }
            Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Raise(stmt) => collect_used_identifiers_in_expr(&stmt.value, used),
            Stmt::Panic(stmt) => collect_used_identifiers_in_expr(&stmt.value, used),
            Stmt::Expr(stmt) => collect_used_identifiers_in_expr(&stmt.expr, used),
        }
    }
}

fn collect_calls_in_block(block: &crate::parser::Block, called: &mut BTreeSet<String>) {
    for stmt in &block.statements {
        match stmt {
            Stmt::Block(stmt) => collect_calls_in_block(&stmt.block, called),
            Stmt::Let(stmt) => collect_calls_in_expr(&stmt.value, called),
            Stmt::Assign(stmt) => collect_calls_in_expr(&stmt.value, called),
            Stmt::Return(stmt) => {
                if let Some(expr) = &stmt.value {
                    collect_calls_in_expr(expr, called);
                }
            }
            Stmt::If(stmt) => {
                collect_calls_in_expr(&stmt.condition, called);
                collect_calls_in_block(&stmt.then_block, called);
                for elif in &stmt.elif_blocks {
                    collect_calls_in_expr(&elif.condition, called);
                    collect_calls_in_block(&elif.block, called);
                }
                if let Some(block) = &stmt.else_block {
                    collect_calls_in_block(block, called);
                }
            }
            Stmt::While(stmt) => {
                collect_calls_in_expr(&stmt.condition, called);
                collect_calls_in_block(&stmt.body, called);
            }
            Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Raise(stmt) => collect_calls_in_expr(&stmt.value, called),
            Stmt::Panic(stmt) => collect_calls_in_expr(&stmt.value, called),
            Stmt::Expr(stmt) => collect_calls_in_expr(&stmt.expr, called),
        }
    }
}

fn collect_calls_in_expr(expr: &Expr, called: &mut BTreeSet<String>) {
    match &expr.kind {
        ExprKind::Unary { expr, .. } => collect_calls_in_expr(expr, called),
        ExprKind::Binary { left, right, .. } => {
            collect_calls_in_expr(left, called);
            collect_calls_in_expr(right, called);
        }
        ExprKind::Call { callee, args } => {
            if let ExprKind::Identifier(name) = &callee.kind {
                called.insert(name.clone());
            }
            for arg in args {
                match arg {
                    CallArg::Positional(expr) => collect_calls_in_expr(expr, called),
                    CallArg::Keyword { value, .. } => collect_calls_in_expr(value, called),
                }
            }
        }
        ExprKind::Await { expr } => collect_calls_in_expr(expr, called),
        ExprKind::Field { base, .. } => collect_calls_in_expr(base, called),
        ExprKind::Identifier(_)
        | ExprKind::Integer(_)
        | ExprKind::String(_)
        | ExprKind::Bool(_) => {}
    }
}

fn collect_used_identifiers_in_expr(expr: &Expr, used: &mut BTreeSet<String>) {
    match &expr.kind {
        ExprKind::Identifier(name) => {
            used.insert(name.clone());
        }
        ExprKind::Unary { expr, .. } => collect_used_identifiers_in_expr(expr, used),
        ExprKind::Binary { left, right, .. } => {
            collect_used_identifiers_in_expr(left, used);
            collect_used_identifiers_in_expr(right, used);
        }
        ExprKind::Call { callee, args } => {
            collect_used_identifiers_in_expr(callee, used);
            for arg in args {
                match arg {
                    CallArg::Positional(expr) => collect_used_identifiers_in_expr(expr, used),
                    CallArg::Keyword { value, .. } => collect_used_identifiers_in_expr(value, used),
                }
            }
        }
        ExprKind::Await { expr } => collect_used_identifiers_in_expr(expr, used),
        ExprKind::Field { base, .. } => collect_used_identifiers_in_expr(base, used),
        ExprKind::Integer(_) | ExprKind::String(_) | ExprKind::Bool(_) => {}
    }
}
