use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::builtin_modules::{BuiltinModuleBody, builtin_module, builtin_module_for_path};
use crate::diagnostics::render_file_diagnostic;
use crate::lexer::Span;
use crate::parser::{
    AssignStmt, Block, CallArg, ElifBlock, ExceptionDecl, Expr, ExprKind, ExprStmt, Function,
    IfStmt, ImportDecl, Item, LetStmt, PanicStmt, Param, Program, RaiseStmt, ReturnStmt, Stmt,
    StructDecl, StructField, TypeRef, WhileStmt, parse_source,
};

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
}

impl fmt::Display for ModuleLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModuleLoadError::Io {
                context,
                source,
                trace: _,
            } => write!(f, "{context}: {source}"),
            ModuleLoadError::Parse {
                path,
                source: _,
                message,
                span: _,
                trace: _,
            } => {
                write!(f, "failed to parse `{}`: {message}", path.display())
            }
            ModuleLoadError::MissingModule {
                module,
                path,
                importer_path: _,
                importer_source: _,
                importer_span: _,
                trace: _,
            } => {
                write!(f, "module `{module}` was not found at `{}`", path.display())
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
                "module `{module}` does not export `{name}` in `{}`",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ModuleLoadError {}

impl ModuleLoadError {
    fn push_trace(&mut self, site: ImportSite) {
        match self {
            ModuleLoadError::Io { trace, .. }
            | ModuleLoadError::Parse { trace, .. }
            | ModuleLoadError::MissingModule { trace, .. }
            | ModuleLoadError::MissingImport { trace, .. } => trace.push(site),
        }
    }

    pub fn render(&self) -> String {
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
                rendered.push_str(&format!("{context}: {source}"));
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
                rendered.push_str(&render_file_diagnostic(path, source, message, *span));
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
                    &format!("module `{module}` was not found at `{}`", path.display()),
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
                        "module `{module}` does not export `{name}` in `{}`",
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
    load_module_recursive(
        &canonical,
        true,
        &mut visited,
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
    out_exceptions: &mut Vec<ExceptionDecl>,
    out_structs: &mut Vec<StructDecl>,
    out_functions: &mut Vec<Function>,
    function_origins: &mut HashMap<String, PathBuf>,
    import_sites: &mut HashMap<PathBuf, ImportSite>,
    sources: &mut HashMap<PathBuf, String>,
    export_maps: &mut HashMap<PathBuf, ExportMap>,
) -> Result<ExportMap, ModuleLoadError> {
    if !visited.insert(path.to_path_buf()) {
        return Ok(export_maps.get(path).cloned().unwrap_or_default());
    }

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
