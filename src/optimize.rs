use std::collections::{HashMap, HashSet, VecDeque};

use crate::parser::{
    BinaryOp, Block, CallArg, ElifBlock, Expr, ExprKind, Function, IfStmt, Item, LetStmt, Program,
    ReturnStmt, Stmt, StructDecl, TypeRef, UnaryOp, WhileStmt,
};

pub fn optimize_program(program: &mut Program) {
    for item in &mut program.items {
        let Item::Function(function) = item else {
            continue;
        };
        optimize_function(function);
    }
}

pub fn prune_program_for_executable(program: &mut Program) {
    prune_program_to_entry_roots(program, &["main", "setup", "loop"]);
}

pub fn prune_program_to_entry_roots(program: &mut Program, entry_roots: &[&str]) {
    let mut function_map = HashMap::new();
    let mut struct_map = HashMap::new();
    let mut exception_names = HashSet::new();
    for item in &program.items {
        match item {
            Item::Function(function) => {
                function_map.insert(function.name.clone(), function);
            }
            Item::Struct(decl) => {
                struct_map.insert(decl.name.clone(), decl);
                for method in &decl.methods {
                    function_map.insert(struct_method_symbol(&decl.name, &method.name), method);
                }
            }
            Item::Import(_) => {}
            Item::Exception(exception) => {
                exception_names.insert(exception.name.clone());
            }
        }
    }

    let mut reachable_functions = HashSet::new();
    let mut reachable_structs = HashSet::new();
    let mut reachable_exceptions = HashSet::new();
    let mut queue = VecDeque::new();

    for root in entry_roots {
        if function_map.contains_key(*root) && reachable_functions.insert((*root).to_string()) {
            queue.push_back(ReachableItem::Function((*root).to_string()));
        }
    }

    while let Some(item) = queue.pop_front() {
        match item {
            ReachableItem::Function(name) => {
                if let Some(function) = function_map.get(&name) {
                    collect_function_type_deps(
                        function,
                        &struct_map,
                        &exception_names,
                        &mut reachable_structs,
                        &mut reachable_exceptions,
                        &mut queue,
                    );
                    collect_block_deps(
                        &function.body,
                        &function_map,
                        &struct_map,
                        &exception_names,
                        &mut reachable_functions,
                        &mut reachable_structs,
                        &mut reachable_exceptions,
                        &mut queue,
                    );
                }
            }
            ReachableItem::Struct(name) => {
                if let Some(decl) = struct_map.get(&name) {
                    for field in &decl.fields {
                        collect_type_ref_deps(&field.ty, &struct_map, &mut reachable_structs, &mut queue);
                    }
                    for method in &decl.methods {
                        let symbol = struct_method_symbol(&decl.name, &method.name);
                        if reachable_functions.insert(symbol.clone()) {
                            queue.push_back(ReachableItem::Function(symbol));
                        }
                    }
                }
            }
        }
    }

    for item in &mut program.items {
        if let Item::Struct(decl) = item {
            decl.methods.retain(|method| {
                reachable_functions.contains(&struct_method_symbol(&decl.name, &method.name))
            });
        }
    }

    program.items.retain(|item| match item {
        Item::Import(_) => true,
        Item::Exception(exception) => reachable_exceptions.contains(&exception.name),
        Item::Function(function) => reachable_functions.contains(&function.name),
        Item::Struct(decl) => reachable_structs.contains(&decl.name),
    });
}

#[derive(Debug, Clone)]
enum ReachableItem {
    Function(String),
    Struct(String),
}

fn struct_method_symbol(struct_name: &str, method_name: &str) -> String {
    format!("{struct_name}.{method_name}")
}

fn collect_function_type_deps(
    function: &Function,
    struct_map: &HashMap<String, &StructDecl>,
    exception_names: &HashSet<String>,
    reachable_structs: &mut HashSet<String>,
    reachable_exceptions: &mut HashSet<String>,
    queue: &mut VecDeque<ReachableItem>,
) {
    for param in &function.params {
        collect_type_ref_deps(&param.ty, struct_map, reachable_structs, queue);
    }
    if let Some(return_type) = &function.return_type {
        collect_type_ref_deps(return_type, struct_map, reachable_structs, queue);
    }
    if let Some(raises) = &function.raises {
        if exception_names.contains(&raises.name) {
            reachable_exceptions.insert(raises.name.clone());
        } else {
            collect_type_ref_deps(raises, struct_map, reachable_structs, queue);
        }
    }
}

fn collect_type_ref_deps(
    ty: &TypeRef,
    struct_map: &HashMap<String, &StructDecl>,
    reachable_structs: &mut HashSet<String>,
    queue: &mut VecDeque<ReachableItem>,
) {
    if struct_map.contains_key(&ty.name) && reachable_structs.insert(ty.name.clone()) {
        queue.push_back(ReachableItem::Struct(ty.name.clone()));
    }
}

fn collect_block_deps(
    block: &Block,
    function_map: &HashMap<String, &Function>,
    struct_map: &HashMap<String, &StructDecl>,
    exception_names: &HashSet<String>,
    reachable_functions: &mut HashSet<String>,
    reachable_structs: &mut HashSet<String>,
    reachable_exceptions: &mut HashSet<String>,
    queue: &mut VecDeque<ReachableItem>,
) {
    for stmt in &block.statements {
        collect_stmt_deps(
            stmt,
            function_map,
            struct_map,
            exception_names,
            reachable_functions,
            reachable_structs,
            reachable_exceptions,
            queue,
        );
    }
}

fn collect_stmt_deps(
    stmt: &Stmt,
    function_map: &HashMap<String, &Function>,
    struct_map: &HashMap<String, &StructDecl>,
    exception_names: &HashSet<String>,
    reachable_functions: &mut HashSet<String>,
    reachable_structs: &mut HashSet<String>,
    reachable_exceptions: &mut HashSet<String>,
    queue: &mut VecDeque<ReachableItem>,
) {
    match stmt {
        Stmt::Block(stmt) => collect_block_deps(
            &stmt.block,
            function_map,
            struct_map,
            exception_names,
            reachable_functions,
            reachable_structs,
            reachable_exceptions,
            queue,
        ),
        Stmt::Let(stmt) => {
            if let Some(ty) = &stmt.ty {
                collect_type_ref_deps(ty, struct_map, reachable_structs, queue);
            }
            collect_expr_deps(
                &stmt.value,
                function_map,
                struct_map,
                exception_names,
                reachable_functions,
                reachable_structs,
                reachable_exceptions,
                queue,
            );
        }
        Stmt::Assign(stmt) => collect_expr_deps(
            &stmt.value,
            function_map,
            struct_map,
            exception_names,
            reachable_functions,
            reachable_structs,
            reachable_exceptions,
            queue,
        ),
        Stmt::Return(stmt) => {
            if let Some(value) = &stmt.value {
                collect_expr_deps(
                    value,
                    function_map,
                    struct_map,
                    exception_names,
                    reachable_functions,
                    reachable_structs,
                    reachable_exceptions,
                    queue,
                );
            }
        }
        Stmt::If(stmt) => {
            collect_expr_deps(
                &stmt.condition,
                function_map,
                struct_map,
                exception_names,
                reachable_functions,
                reachable_structs,
                reachable_exceptions,
                queue,
            );
            collect_block_deps(
                &stmt.then_block,
                function_map,
                struct_map,
                exception_names,
                reachable_functions,
                reachable_structs,
                reachable_exceptions,
                queue,
            );
            for elif in &stmt.elif_blocks {
                collect_expr_deps(
                    &elif.condition,
                    function_map,
                    struct_map,
                    exception_names,
                    reachable_functions,
                    reachable_structs,
                    reachable_exceptions,
                    queue,
                );
                collect_block_deps(
                    &elif.block,
                    function_map,
                    struct_map,
                    exception_names,
                    reachable_functions,
                    reachable_structs,
                    reachable_exceptions,
                    queue,
                );
            }
            if let Some(block) = &stmt.else_block {
                collect_block_deps(
                    block,
                    function_map,
                    struct_map,
                    exception_names,
                    reachable_functions,
                    reachable_structs,
                    reachable_exceptions,
                    queue,
                );
            }
        }
        Stmt::While(stmt) => {
            collect_expr_deps(
                &stmt.condition,
                function_map,
                struct_map,
                exception_names,
                reachable_functions,
                reachable_structs,
                reachable_exceptions,
                queue,
            );
            collect_block_deps(
                &stmt.body,
                function_map,
                struct_map,
                exception_names,
                reachable_functions,
                reachable_structs,
                reachable_exceptions,
                queue,
            );
        }
        Stmt::Raise(stmt) => collect_expr_deps(
            &stmt.value,
            function_map,
            struct_map,
            exception_names,
            reachable_functions,
            reachable_structs,
            reachable_exceptions,
            queue,
        ),
        Stmt::Panic(stmt) => collect_expr_deps(
            &stmt.value,
            function_map,
            struct_map,
            exception_names,
            reachable_functions,
            reachable_structs,
            reachable_exceptions,
            queue,
        ),
        Stmt::Expr(stmt) => collect_expr_deps(
            &stmt.expr,
            function_map,
            struct_map,
            exception_names,
            reachable_functions,
            reachable_structs,
            reachable_exceptions,
            queue,
        ),
        Stmt::FieldAssign(stmt) => collect_expr_deps(
            &stmt.value,
            function_map,
            struct_map,
            exception_names,
            reachable_functions,
            reachable_structs,
            reachable_exceptions,
            queue,
        ),
        Stmt::Break(_) | Stmt::Continue(_) => {}
    }
}

fn collect_expr_deps(
    expr: &Expr,
    function_map: &HashMap<String, &Function>,
    struct_map: &HashMap<String, &StructDecl>,
    exception_names: &HashSet<String>,
    reachable_functions: &mut HashSet<String>,
    reachable_structs: &mut HashSet<String>,
    reachable_exceptions: &mut HashSet<String>,
    queue: &mut VecDeque<ReachableItem>,
) {
    match &expr.kind {
        ExprKind::Identifier(_) | ExprKind::Integer(_) | ExprKind::String(_) | ExprKind::Bool(_) => {}
        ExprKind::Unary { expr, .. } | ExprKind::Await { expr } => collect_expr_deps(
            expr,
            function_map,
            struct_map,
            exception_names,
            reachable_functions,
            reachable_structs,
            reachable_exceptions,
            queue,
        ),
        ExprKind::Binary { left, right, .. } => {
            collect_expr_deps(
                left,
                function_map,
                struct_map,
                exception_names,
                reachable_functions,
                reachable_structs,
                reachable_exceptions,
                queue,
            );
            collect_expr_deps(
                right,
                function_map,
                struct_map,
                exception_names,
                reachable_functions,
                reachable_structs,
                reachable_exceptions,
                queue,
            );
        }
        ExprKind::Field { base, .. } => collect_expr_deps(
            base,
            function_map,
            struct_map,
            exception_names,
            reachable_functions,
            reachable_structs,
            reachable_exceptions,
            queue,
        ),
        ExprKind::Call { callee, args } => {
            for arg in args {
                match arg {
                    CallArg::Positional(expr) => collect_expr_deps(
                        expr,
                        function_map,
                        struct_map,
                        exception_names,
                        reachable_functions,
                        reachable_structs,
                        reachable_exceptions,
                        queue,
                    ),
                    CallArg::Keyword { value, .. } => collect_expr_deps(
                        value,
                        function_map,
                        struct_map,
                        exception_names,
                        reachable_functions,
                        reachable_structs,
                        reachable_exceptions,
                        queue,
                    ),
                }
            }

            match &callee.kind {
                ExprKind::Identifier(name) => {
                    if let Some(function) = function_map.get(name) {
                        if reachable_functions.insert(name.clone()) {
                            queue.push_back(ReachableItem::Function(name.clone()));
                        }
                        collect_function_type_deps(
                            function,
                            struct_map,
                            exception_names,
                            reachable_structs,
                            reachable_exceptions,
                            queue,
                        );
                    } else if exception_names.contains(name) {
                        reachable_exceptions.insert(name.clone());
                    } else if struct_map.contains_key(name) && reachable_structs.insert(name.clone()) {
                        queue.push_back(ReachableItem::Struct(name.clone()));
                    }
                }
                ExprKind::Field { base, name } => {
                    collect_expr_deps(
                        base,
                        function_map,
                        struct_map,
                        exception_names,
                        reachable_functions,
                        reachable_structs,
                        reachable_exceptions,
                        queue,
                    );
                    if let ExprKind::Identifier(struct_name) = &base.kind
                        && struct_map.contains_key(struct_name)
                    {
                        let symbol = struct_method_symbol(struct_name, name);
                        if reachable_functions.insert(symbol.clone()) {
                            queue.push_back(ReachableItem::Function(symbol));
                        }
                        if reachable_structs.insert(struct_name.clone()) {
                            queue.push_back(ReachableItem::Struct(struct_name.clone()));
                        }
                    }
                }
                _ => collect_expr_deps(
                    callee,
                    function_map,
                    struct_map,
                    exception_names,
                    reachable_functions,
                    reachable_structs,
                    reachable_exceptions,
                    queue,
                ),
            }
        }
    }
}

fn optimize_function(function: &mut Function) {
    if function.is_extern {
        return;
    }
    optimize_block(&mut function.body);
    eliminate_dead_pure_lets(&mut function.body);
}

// ---------------------------------------------------------------------------
// Dead pure-let elimination
//
// Removes `let x = <pure-expr>` statements where `x` is never read anywhere
// in the function body.  A pure expression is one with no function calls or
// await expressions — all side effects come from calls, so if there are no
// calls it is safe to drop an unused binding entirely.
//
// If the init expression contains a call (and therefore has observable side
// effects) the binding is kept even when the name is never read, because the
// call must still execute.
// ---------------------------------------------------------------------------

fn eliminate_dead_pure_lets(block: &mut Block) {
    // Collect every name that is READ (appears in an expression context that
    // is not a let-binding target) anywhere within this block tree.
    let mut read_names: HashSet<String> = HashSet::new();
    for stmt in &block.statements {
        collect_reads_stmt(stmt, &mut read_names);
    }

    block.statements.retain(|stmt| {
        if let Stmt::Let(let_stmt) = stmt {
            if !read_names.contains(&let_stmt.name) && is_pure_expr(&let_stmt.value) {
                return false;
            }
        }
        true
    });

    // Recurse so that inner blocks also have their dead lets removed.
    for stmt in &mut block.statements {
        match stmt {
            Stmt::Block(s) => eliminate_dead_pure_lets(&mut s.block),
            Stmt::If(s) => {
                eliminate_dead_pure_lets(&mut s.then_block);
                for elif in &mut s.elif_blocks {
                    eliminate_dead_pure_lets(&mut elif.block);
                }
                if let Some(b) = &mut s.else_block {
                    eliminate_dead_pure_lets(b);
                }
            }
            Stmt::While(s) => eliminate_dead_pure_lets(&mut s.body),
            _ => {}
        }
    }
}

/// Returns `true` when the expression has no observable side effects (no
/// `Call` or `Await` nodes anywhere in its tree).
fn is_pure_expr(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Integer(_) | ExprKind::Bool(_) | ExprKind::String(_) => true,
        ExprKind::Identifier(_) => true,
        ExprKind::Unary { expr: inner, .. } => is_pure_expr(inner),
        ExprKind::Binary { left, right, .. } => is_pure_expr(left) && is_pure_expr(right),
        ExprKind::Field { base, .. } => is_pure_expr(base),
        // Calls have observable side effects; treat them as impure.
        ExprKind::Call { .. } | ExprKind::Await { .. } => false,
    }
}

/// Collect all variable names that are *read* (appear in expression position,
/// not as the target of a `let` or `assign`) within `stmt` and its children.
fn collect_reads_stmt(stmt: &Stmt, reads: &mut HashSet<String>) {
    match stmt {
        Stmt::Block(s) => {
            for inner in &s.block.statements {
                collect_reads_stmt(inner, reads);
            }
        }
        Stmt::Let(s) => collect_reads_expr(&s.value, reads),
        Stmt::Assign(s) => collect_reads_expr(&s.value, reads),
        Stmt::FieldAssign(s) => {
            reads.insert(s.base.clone());
            collect_reads_expr(&s.value, reads);
        }
        Stmt::Return(s) => {
            if let Some(v) = &s.value {
                collect_reads_expr(v, reads);
            }
        }
        Stmt::If(s) => {
            collect_reads_expr(&s.condition, reads);
            for inner in &s.then_block.statements {
                collect_reads_stmt(inner, reads);
            }
            for elif in &s.elif_blocks {
                collect_reads_expr(&elif.condition, reads);
                for inner in &elif.block.statements {
                    collect_reads_stmt(inner, reads);
                }
            }
            if let Some(b) = &s.else_block {
                for inner in &b.statements {
                    collect_reads_stmt(inner, reads);
                }
            }
        }
        Stmt::While(s) => {
            collect_reads_expr(&s.condition, reads);
            for inner in &s.body.statements {
                collect_reads_stmt(inner, reads);
            }
        }
        Stmt::Raise(s) => collect_reads_expr(&s.value, reads),
        Stmt::Panic(s) => collect_reads_expr(&s.value, reads),
        Stmt::Expr(s) => collect_reads_expr(&s.expr, reads),
        Stmt::Break(_) | Stmt::Continue(_) => {}
    }
}

fn collect_reads_expr(expr: &Expr, reads: &mut HashSet<String>) {
    match &expr.kind {
        ExprKind::Identifier(name) => {
            reads.insert(name.clone());
        }
        ExprKind::Integer(_) | ExprKind::Bool(_) | ExprKind::String(_) => {}
        ExprKind::Unary { expr: inner, .. } | ExprKind::Await { expr: inner } => {
            collect_reads_expr(inner, reads);
        }
        ExprKind::Binary { left, right, .. } => {
            collect_reads_expr(left, reads);
            collect_reads_expr(right, reads);
        }
        ExprKind::Field { base, .. } => collect_reads_expr(base, reads),
        ExprKind::Call { callee, args } => {
            collect_reads_expr(callee, reads);
            for arg in args {
                match arg {
                    CallArg::Positional(e) => collect_reads_expr(e, reads),
                    CallArg::Keyword { value, .. } => collect_reads_expr(value, reads),
                }
            }
        }
    }
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
        Stmt::FieldAssign(stmt) => optimize_expr(&mut stmt.value),
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
        Stmt::Break(_) | Stmt::Continue(_) => {}
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
                    BinaryOp::BitwiseAnd => Some(ExprKind::Integer((lhs & rhs).to_string())),
                    BinaryOp::BitwiseOr => Some(ExprKind::Integer((lhs | rhs).to_string())),
                    BinaryOp::BitwiseXor => Some(ExprKind::Integer((lhs ^ rhs).to_string())),
                    BinaryOp::ShiftLeft => {
                        if rhs >= 0 && rhs < 64 {
                            Some(ExprKind::Integer((lhs << rhs).to_string()))
                        } else {
                            None
                        }
                    }
                    BinaryOp::ShiftRight => {
                        if rhs >= 0 && rhs < 64 {
                            Some(ExprKind::Integer((lhs >> rhs).to_string()))
                        } else {
                            None
                        }
                    }
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
                BinaryOp::Divide => {
                    if int_value(right) == Some(1) {
                        expr.kind = left.kind.clone();
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

#[cfg(test)]
mod tests {
    use super::{eliminate_dead_pure_lets, optimize_program, prune_program_to_entry_roots};
    use crate::parser::{Item, Stmt, parse_source};

    #[test]
    fn prunes_unreachable_functions_and_structs_from_entry_roots() {
        let mut program = parse_source(
            r"class Used:
    value: i64
    def read(self) -> i64:
        return self.value

class Unused:
    value: i64
    def read(self) -> i64:
        return self.value

def helper() -> i64:
    let item: Used = Used(value=7)
    return item.read()

def dead_helper() -> i64:
    let item: Unused = Unused(value=9)
    return item.read()

def main() -> i32:
    println(helper())
    return 0
",
        )
        .expect("program should parse");

        optimize_program(&mut program);
        prune_program_to_entry_roots(&mut program, &["main", "setup", "loop"]);

        let function_names = program
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Function(function) => Some(function.name.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let struct_names = program
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Struct(decl) => Some(decl.name.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert!(function_names.contains(&"main"));
        assert!(function_names.contains(&"helper"));
        assert!(!function_names.contains(&"dead_helper"));
        assert!(struct_names.contains(&"Used"));
        assert!(!struct_names.contains(&"Unused"));
    }

    #[test]
    fn divide_by_one_folds_to_operand() {
        let mut program = parse_source(
            r"def main() -> i32:
    return 42 / 1
",
        )
        .expect("program should parse");
        optimize_program(&mut program);
        let Item::Function(main_fn) = &program.items[0] else {
            panic!("expected function");
        };
        let Stmt::Return(ret) = &main_fn.body.statements[0] else {
            panic!("expected return");
        };
        let value = ret.value.as_ref().expect("return should have a value");
        assert_eq!(
            format!("{:?}", value.kind),
            format!("{:?}", crate::parser::ExprKind::Integer("42".to_string())),
            "42 / 1 should fold to 42"
        );
    }

    #[test]
    fn dead_pure_let_is_eliminated() {
        let mut program = parse_source(
            r"def main() -> i32:
    let dead = 5 + 3
    return 0
",
        )
        .expect("program should parse");
        optimize_program(&mut program);
        let Item::Function(main_fn) = &program.items[0] else {
            panic!("expected function");
        };
        // After elimination the body should contain only the return statement.
        assert_eq!(
            main_fn.body.statements.len(),
            1,
            "dead pure let should have been removed"
        );
        assert!(matches!(main_fn.body.statements[0], Stmt::Return(_)));
    }

    #[test]
    fn dead_let_with_call_is_kept() {
        let mut program = parse_source(
            r"def main() -> i32:
    let _x = some_side_effect()
    return 0
",
        )
        .expect("program should parse");
        optimize_program(&mut program);
        let Item::Function(main_fn) = &program.items[0] else {
            panic!("expected function");
        };
        // The let must be preserved because the call may have side effects.
        assert_eq!(
            main_fn.body.statements.len(),
            2,
            "let with a call init must not be removed"
        );
    }

    #[test]
    fn used_let_is_not_eliminated() {
        let mut program = parse_source(
            r"def main() -> i32:
    let x = 10
    return x
",
        )
        .expect("program should parse");
        optimize_program(&mut program);
        let Item::Function(main_fn) = &program.items[0] else {
            panic!("expected function");
        };
        // `x` is read by the return, so the let must stay.
        assert_eq!(main_fn.body.statements.len(), 2, "used let must not be removed");
    }
}
