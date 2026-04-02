use crate::parser::{
    BinaryOp, Block, CallArg, ElifBlock, Expr, ExprKind, Function, IfStmt, Item, LetStmt, Program,
    ReturnStmt, Stmt, UnaryOp, WhileStmt,
};

pub fn optimize_program(program: &mut Program) {
    for item in &mut program.items {
        let Item::Function(function) = item else {
            continue;
        };
        optimize_function(function);
    }
}

fn optimize_function(function: &mut Function) {
    if function.is_extern {
        return;
    }
    optimize_block(&mut function.body);
}

fn optimize_block(block: &mut Block) {
    let mut optimized = Vec::new();
    for mut stmt in std::mem::take(&mut block.statements) {
        optimize_stmt(&mut stmt);
        match fold_control_flow(stmt) {
            ControlFlowFold::Keep(stmt) => optimized.push(stmt),
            ControlFlowFold::Inline(stmts) => optimized.extend(stmts),
            ControlFlowFold::Remove => {}
        }
    }
    block.statements = optimized;
}

fn optimize_stmt(stmt: &mut Stmt) {
    match stmt {
        Stmt::Block(stmt) => optimize_block(&mut stmt.block),
        Stmt::Let(LetStmt { value, .. }) => optimize_expr(value),
        Stmt::Assign(stmt) => optimize_expr(&mut stmt.value),
        Stmt::Return(ReturnStmt { value, .. }) => {
            if let Some(expr) = value {
                optimize_expr(expr);
            }
        }
        Stmt::If(IfStmt {
            condition,
            then_block,
            elif_blocks,
            else_block,
            ..
        }) => {
            optimize_expr(condition);
            optimize_block(then_block);
            for ElifBlock {
                condition, block, ..
            } in elif_blocks
            {
                optimize_expr(condition);
                optimize_block(block);
            }
            if let Some(block) = else_block {
                optimize_block(block);
            }
        }
        Stmt::While(WhileStmt {
            condition, body, ..
        }) => {
            optimize_expr(condition);
            optimize_block(body);
        }
        Stmt::Raise(stmt) => optimize_expr(&mut stmt.value),
        Stmt::Panic(stmt) => optimize_expr(&mut stmt.value),
        Stmt::Expr(stmt) => optimize_expr(&mut stmt.expr),
    }
}

fn optimize_expr(expr: &mut Expr) {
    match &mut expr.kind {
        ExprKind::Unary { expr: inner, .. } => optimize_expr(inner),
        ExprKind::Binary { left, right, .. } => {
            optimize_expr(left);
            optimize_expr(right);
        }
        ExprKind::Call { args, .. } => {
            for arg in args {
                match arg {
                    CallArg::Positional(expr) => optimize_expr(expr),
                    CallArg::Keyword { value, .. } => optimize_expr(value),
                }
            }
        }
        ExprKind::Await { expr } => optimize_expr(expr),
        ExprKind::Field { base, .. } => optimize_expr(base),
        ExprKind::Identifier(_)
        | ExprKind::Integer(_)
        | ExprKind::String(_)
        | ExprKind::Bool(_) => {}
    }

    fold_expr(expr);
}

fn fold_expr(expr: &mut Expr) {
    match &expr.kind {
        ExprKind::Unary {
            op: UnaryOp::Negate,
            expr: inner,
        } => {
            if let ExprKind::Integer(value) = &inner.kind
                && let Ok(number) = value.parse::<i64>()
            {
                expr.kind = ExprKind::Integer((-number).to_string());
            }
        }
        ExprKind::Unary {
            op: UnaryOp::Not,
            expr: inner,
        } => {
            if let ExprKind::Bool(value) = &inner.kind {
                expr.kind = ExprKind::Bool(!value);
            }
        }
        ExprKind::Binary { left, op, right } => {
            if let (Some(lhs), Some(rhs)) = (bool_value(left), bool_value(right)) {
                let folded = match op {
                    BinaryOp::And => Some(ExprKind::Bool(lhs && rhs)),
                    BinaryOp::Or => Some(ExprKind::Bool(lhs || rhs)),
                    _ => None,
                };

                if let Some(kind) = folded {
                    expr.kind = kind;
                    return;
                }
            }
            if let (Some(lhs), Some(rhs)) = (int_value(left), int_value(right)) {
                let folded = match op {
                    BinaryOp::And | BinaryOp::Or => None,
                    BinaryOp::Add => Some(ExprKind::Integer((lhs + rhs).to_string())),
                    BinaryOp::Subtract => Some(ExprKind::Integer((lhs - rhs).to_string())),
                    BinaryOp::Multiply => Some(ExprKind::Integer((lhs * rhs).to_string())),
                    BinaryOp::Divide => {
                        if rhs != 0 {
                            Some(ExprKind::Integer((lhs / rhs).to_string()))
                        } else {
                            None
                        }
                    }
                    BinaryOp::Modulo => {
                        if rhs != 0 {
                            Some(ExprKind::Integer((lhs % rhs).to_string()))
                        } else {
                            None
                        }
                    }
                    BinaryOp::EqualEqual => Some(ExprKind::Bool(lhs == rhs)),
                    BinaryOp::NotEqual => Some(ExprKind::Bool(lhs != rhs)),
                    BinaryOp::Greater => Some(ExprKind::Bool(lhs > rhs)),
                    BinaryOp::GreaterEqual => Some(ExprKind::Bool(lhs >= rhs)),
                    BinaryOp::Less => Some(ExprKind::Bool(lhs < rhs)),
                    BinaryOp::LessEqual => Some(ExprKind::Bool(lhs <= rhs)),
                };

                if let Some(kind) = folded {
                    expr.kind = kind;
                    return;
                }
            }

            match op {
                BinaryOp::And => {
                    if bool_value(left) == Some(true) {
                        expr.kind = right.kind.clone();
                    } else if bool_value(left) == Some(false) || bool_value(right) == Some(false) {
                        expr.kind = ExprKind::Bool(false);
                    }
                }
                BinaryOp::Or => {
                    if bool_value(left) == Some(false) {
                        expr.kind = right.kind.clone();
                    } else if bool_value(left) == Some(true) || bool_value(right) == Some(true) {
                        expr.kind = ExprKind::Bool(true);
                    }
                }
                BinaryOp::Add => {
                    if int_value(right) == Some(0) {
                        expr.kind = left.kind.clone();
                    } else if int_value(left) == Some(0) {
                        expr.kind = right.kind.clone();
                    }
                }
                BinaryOp::Subtract => {
                    if int_value(right) == Some(0) {
                        expr.kind = left.kind.clone();
                    }
                }
                BinaryOp::Multiply => {
                    if int_value(right) == Some(1) {
                        expr.kind = left.kind.clone();
                    } else if int_value(left) == Some(1) {
                        expr.kind = right.kind.clone();
                    } else if int_value(right) == Some(0) || int_value(left) == Some(0) {
                        expr.kind = ExprKind::Integer("0".to_string());
                    }
                }
                BinaryOp::Modulo => {
                    if int_value(right) == Some(1) {
                        expr.kind = ExprKind::Integer("0".to_string());
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}

fn int_value(expr: &Expr) -> Option<i64> {
    match &expr.kind {
        ExprKind::Integer(value) => value.parse::<i64>().ok(),
        _ => None,
    }
}

fn bool_value(expr: &Expr) -> Option<bool> {
    match &expr.kind {
        ExprKind::Bool(value) => Some(*value),
        _ => None,
    }
}

enum ControlFlowFold {
    Keep(Stmt),
    Inline(Vec<Stmt>),
    Remove,
}

fn fold_control_flow(stmt: Stmt) -> ControlFlowFold {
    match stmt {
        Stmt::Block(stmt) => ControlFlowFold::Inline(stmt.block.statements),
        Stmt::If(if_stmt) => {
            if let Some(value) = bool_value(&if_stmt.condition) {
                if value {
                    return ControlFlowFold::Inline(if_stmt.then_block.statements);
                }
                for elif in if_stmt.elif_blocks {
                    if let Some(elif_value) = bool_value(&elif.condition) {
                        if elif_value {
                            return ControlFlowFold::Inline(elif.block.statements);
                        }
                    } else {
                        let rebuilt = Stmt::If(crate::parser::IfStmt {
                            condition: elif.condition,
                            then_block: elif.block,
                            elif_blocks: Vec::new(),
                            else_block: if_stmt.else_block,
                            span: if_stmt.span,
                        });
                        return ControlFlowFold::Keep(rebuilt);
                    }
                }
                return if let Some(block) = if_stmt.else_block {
                    ControlFlowFold::Inline(block.statements)
                } else {
                    ControlFlowFold::Remove
                };
            }
            ControlFlowFold::Keep(Stmt::If(if_stmt))
        }
        Stmt::While(while_stmt) => {
            if bool_value(&while_stmt.condition) == Some(false) {
                ControlFlowFold::Remove
            } else {
                ControlFlowFold::Keep(Stmt::While(while_stmt))
            }
        }
        other => ControlFlowFold::Keep(other),
    }
}
