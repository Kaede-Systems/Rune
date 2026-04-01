use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::diagnostics::render_file_diagnostic;
use crate::lexer::Span;
use crate::parser::{ExceptionDecl, Function, ImportDecl, Item, Program, StructDecl, parse_source};

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
    load_module_recursive(
        &canonical,
        &mut visited,
        &mut exceptions,
        &mut structs,
        &mut functions,
        &mut function_origins,
        &mut import_sites,
        &mut sources,
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
    visited: &mut BTreeSet<PathBuf>,
    out_exceptions: &mut Vec<ExceptionDecl>,
    out_structs: &mut Vec<StructDecl>,
    out_functions: &mut Vec<Function>,
    function_origins: &mut HashMap<String, PathBuf>,
    import_sites: &mut HashMap<PathBuf, ImportSite>,
    sources: &mut HashMap<PathBuf, String>,
) -> Result<(), ModuleLoadError> {
    if !visited.insert(path.to_path_buf()) {
        return Ok(());
    }

    let source = fs::read_to_string(path).map_err(|source| ModuleLoadError::Io {
        context: format!("failed to read `{}`", path.display()),
        source,
        trace: Vec::new(),
    })?;
    sources.insert(path.to_path_buf(), source.clone());
    let program = parse_source(&source).map_err(|error| ModuleLoadError::Parse {
        path: path.to_path_buf(),
        source: source.clone(),
        message: error.to_string(),
        span: error.span,
        trace: Vec::new(),
    })?;

    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    for item in &program.items {
        if let Item::Import(import) = item {
            let module_path = resolve_module_path(base_dir, import);
            if !module_path.is_file() {
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

            let nested_source =
                fs::read_to_string(&module_path).map_err(|source| ModuleLoadError::Io {
                    context: format!("failed to read `{}`", module_path.display()),
                    source,
                    trace: vec![ImportSite {
                        importer_path: path.to_path_buf(),
                        importer_span: import.span,
                        module_name: import.module.join("."),
                    }],
                })?;
            let nested_program =
                parse_source(&nested_source).map_err(|error| ModuleLoadError::Parse {
                    path: module_path.clone(),
                    source: nested_source.clone(),
                    message: error.to_string(),
                    span: error.span,
                    trace: vec![ImportSite {
                        importer_path: path.to_path_buf(),
                        importer_span: import.span,
                        module_name: import.module.join("."),
                    }],
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

            load_module_recursive(
                &module_path,
                visited,
                out_exceptions,
                out_structs,
                out_functions,
                function_origins,
                import_sites,
                sources,
            )
            .map_err(|mut error| {
                error.push_trace(ImportSite {
                    importer_path: path.to_path_buf(),
                    importer_span: import.span,
                    module_name: import.module.join("."),
                });
                error
            })?;
        }
    }

    for item in program.items {
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

    Ok(())
}

fn resolve_module_path(base_dir: &Path, import: &ImportDecl) -> PathBuf {
    if import.level == 0 {
        let roots = [
            "system", "time", "network", "env", "fs", "terminal", "audio", "io",
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
