use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::codegen::CodegenError;
use crate::llvm_backend::{emit_object_file, emit_object_file_from_ir};
use crate::llvm_ir::emit_llvm_ir;
use crate::module_loader::load_program_from_path;
use crate::optimize::optimize_program;
use crate::parser::{Item, Program, TypeRef};
use crate::semantic::check_program;
use crate::toolchain::{find_packaged_llvm_tool, find_packaged_wasm_ld};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildOptions {
    pub link_search_paths: Vec<PathBuf>,
    pub link_libs: Vec<String>,
    pub link_args: Vec<String>,
    pub link_c_sources: Vec<PathBuf>,
}

#[derive(Debug)]
pub enum BuildError {
    Io {
        context: String,
        source: std::io::Error,
    },
    Codegen(CodegenError),
    ModuleLoad(String),
    UnsupportedFfiSignature(String),
    RustcNotFound,
    UnsupportedTarget(String),
    UnsupportedBackendForTarget(String),
    ToolNotFound(String),
    ToolFailed {
        tool: String,
        status: i32,
        stderr: String,
    },
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuildError::Io { context, source } => write!(f, "{context}: {source}"),
            BuildError::Codegen(error) => write!(f, "{error}"),
            BuildError::ModuleLoad(message) => write!(f, "{message}"),
            BuildError::UnsupportedFfiSignature(message) => write!(f, "{message}"),
            BuildError::RustcNotFound => {
                write!(f, "failed to locate `rustc` in the Rust installation")
            }
            BuildError::UnsupportedTarget(target) => {
                write!(f, "unsupported target triple `{target}`")
            }
            BuildError::UnsupportedBackendForTarget(target) => {
                write!(
                    f,
                    "the current native assembly backend only supports Windows targets; `{target}` requires a target-aware backend"
                )
            }
            BuildError::ToolNotFound(tool) => write!(f, "required build tool not found: {tool}"),
            BuildError::ToolFailed {
                tool,
                status,
                stderr,
            } => write!(
                f,
                "{tool} failed with exit code {status}{}",
                if stderr.trim().is_empty() {
                    String::new()
                } else {
                    format!("\n\n{stderr}")
                }
            ),
        }
    }
}

impl std::error::Error for BuildError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetPlatform {
    Windows,
    Linux,
    MacOS,
    Wasm,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetSpec {
    pub triple: &'static str,
    pub platform: TargetPlatform,
    pub exe_extension: &'static str,
    pub library_extension: &'static str,
    pub static_library_extension: &'static str,
    pub needs_macos_sdk: bool,
}

const KNOWN_TARGETS: &[TargetSpec] = &[
    TargetSpec {
        triple: "x86_64-pc-windows-gnu",
        platform: TargetPlatform::Windows,
        exe_extension: "exe",
        library_extension: "dll",
        static_library_extension: "lib",
        needs_macos_sdk: false,
    },
    TargetSpec {
        triple: "x86_64-pc-windows-msvc",
        platform: TargetPlatform::Windows,
        exe_extension: "exe",
        library_extension: "dll",
        static_library_extension: "lib",
        needs_macos_sdk: false,
    },
    TargetSpec {
        triple: "aarch64-pc-windows-gnu",
        platform: TargetPlatform::Windows,
        exe_extension: "exe",
        library_extension: "dll",
        static_library_extension: "lib",
        needs_macos_sdk: false,
    },
    TargetSpec {
        triple: "x86_64-unknown-linux-gnu",
        platform: TargetPlatform::Linux,
        exe_extension: "",
        library_extension: "so",
        static_library_extension: "a",
        needs_macos_sdk: false,
    },
    TargetSpec {
        triple: "aarch64-unknown-linux-gnu",
        platform: TargetPlatform::Linux,
        exe_extension: "",
        library_extension: "so",
        static_library_extension: "a",
        needs_macos_sdk: false,
    },
    TargetSpec {
        triple: "x86_64-apple-darwin",
        platform: TargetPlatform::MacOS,
        exe_extension: "",
        library_extension: "dylib",
        static_library_extension: "a",
        needs_macos_sdk: true,
    },
    TargetSpec {
        triple: "aarch64-apple-darwin",
        platform: TargetPlatform::MacOS,
        exe_extension: "",
        library_extension: "dylib",
        static_library_extension: "a",
        needs_macos_sdk: true,
    },
    TargetSpec {
        triple: "wasm32-unknown-unknown",
        platform: TargetPlatform::Wasm,
        exe_extension: "wasm",
        library_extension: "wasm",
        static_library_extension: "a",
        needs_macos_sdk: false,
    },
    TargetSpec {
        triple: "wasm32-wasip1",
        platform: TargetPlatform::Wasm,
        exe_extension: "wasm",
        library_extension: "wasm",
        static_library_extension: "a",
        needs_macos_sdk: false,
    },
];

pub fn supported_targets() -> &'static [TargetSpec] {
    KNOWN_TARGETS
}

pub fn target_spec(target: Option<&str>) -> Result<TargetSpec, BuildError> {
    match target {
        Some(target) => KNOWN_TARGETS
            .iter()
            .find(|spec| spec.triple == target)
            .cloned()
            .ok_or_else(|| BuildError::UnsupportedTarget(target.to_string())),
        None => Ok(host_target_spec()),
    }
}

fn host_target_spec() -> TargetSpec {
    if cfg!(target_os = "windows") {
        KNOWN_TARGETS
            .iter()
            .find(|spec| spec.triple == "x86_64-pc-windows-gnu")
            .expect("known windows host target should exist")
            .clone()
    } else if cfg!(target_os = "macos") {
        KNOWN_TARGETS
            .iter()
            .find(|spec| spec.triple == "x86_64-apple-darwin")
            .expect("known macOS host target should exist")
            .clone()
    } else {
        KNOWN_TARGETS
            .iter()
            .find(|spec| spec.triple == "x86_64-unknown-linux-gnu")
            .expect("known linux host target should exist")
            .clone()
    }
}

pub fn build_executable(
    source_path: &Path,
    output_path: &Path,
    target: Option<&str>,
) -> Result<(), BuildError> {
    build_executable_with_options(source_path, output_path, target, &BuildOptions::default())
}

pub fn build_executable_llvm(
    source_path: &Path,
    output_path: &Path,
    target: Option<&str>,
) -> Result<(), BuildError> {
    build_executable_llvm_with_options(source_path, output_path, target, &BuildOptions::default())
}

pub fn emit_c_header(source_path: &Path, output_path: &Path) -> Result<(), BuildError> {
    let mut program = load_program_from_path(source_path)
        .map_err(|error| BuildError::ModuleLoad(error.to_string()))?;
    check_program(&program).map_err(|error| {
        BuildError::Codegen(CodegenError {
            message: error.message,
            span: error.span,
        })
    })?;
    optimize_program(&mut program);
    validate_ffi_signatures(&program)?;
    write_c_header(&program, output_path)
}

pub fn build_executable_with_options(
    source_path: &Path,
    output_path: &Path,
    target: Option<&str>,
    options: &BuildOptions,
) -> Result<(), BuildError> {
    build_executable_with_backend(source_path, output_path, target, false, options)
}

pub fn build_executable_llvm_with_options(
    source_path: &Path,
    output_path: &Path,
    target: Option<&str>,
    options: &BuildOptions,
) -> Result<(), BuildError> {
    build_executable_with_backend(source_path, output_path, target, true, options)
}

fn build_executable_with_backend(
    source_path: &Path,
    output_path: &Path,
    target: Option<&str>,
    force_llvm: bool,
    options: &BuildOptions,
) -> Result<(), BuildError> {
    let target_spec = target_spec(target)?;
    let mut program = load_program_from_path(source_path)
        .map_err(|error| BuildError::ModuleLoad(error.to_string()))?;
    check_program(&program).map_err(|error| {
        BuildError::Codegen(CodegenError {
            message: error.message,
            span: error.span,
        })
    })?;
    optimize_program(&mut program);
    let should_try_llvm = true;
    if should_try_llvm {
        match build_executable_via_llvm(&program, output_path, &target_spec, source_path, options) {
            Ok(()) => return Ok(()),
            Err(error)
                if !force_llvm
                    && target_spec.platform == TargetPlatform::Windows
                    && matches!(error, BuildError::Codegen(_)) => {}
            Err(error) => return Err(error),
        }
    }

    build_executable_via_native_asm(&program, output_path, &target_spec, options)
}

fn build_executable_via_native_asm(
    program: &Program,
    output_path: &Path,
    target_spec: &TargetSpec,
    options: &BuildOptions,
) -> Result<(), BuildError> {
    let asm = crate::codegen::emit_program(&program).map_err(BuildError::Codegen)?;
    let asm = rename_entry_symbol(&asm);

    let rustc = find_rustc().ok_or(BuildError::RustcNotFound)?;
    let temp_dir = create_temp_dir()?;
    let wrapper = rust_exe_wrapper_source();
    let wrapper_path = write_wrapper_files(&temp_dir, &asm, &wrapper)?;
    let compiled_c_objects = compile_c_sources(&temp_dir, target_spec, &options.link_c_sources)?;
    let mut effective_options = options.clone();
    effective_options.link_args.extend(
        compiled_c_objects
            .iter()
            .map(|path| path.display().to_string()),
    );

    if let Some(parent) = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|source| BuildError::Io {
            context: format!("failed to create `{}`", parent.display()),
            source,
        })?;
    }

    let mut command = rustc_command(
        &rustc,
        &wrapper_path,
        output_path,
        Some(&target_spec),
        false,
        &effective_options,
    );
    let status = command.output().map_err(|source| BuildError::Io {
        context: format!("failed to run `{}`", rustc.display()),
        source,
    })?;

    if !status.status.success() {
        return Err(BuildError::ToolFailed {
            tool: rustc.display().to_string(),
            status: status.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&status.stderr).into_owned(),
        });
    }

    let _ = fs::remove_file(temp_dir.join("out.s"));
    let _ = fs::remove_file(&wrapper_path);
    let _ = fs::remove_dir(&temp_dir);
    Ok(())
}

pub fn build_shared_library(
    source_path: &Path,
    output_path: &Path,
    target: Option<&str>,
) -> Result<(), BuildError> {
    let target_spec = target_spec(target)?;
    let mut program = load_program_from_path(source_path)
        .map_err(|error| BuildError::ModuleLoad(error.to_string()))?;
    check_program(&program).map_err(|error| {
        BuildError::Codegen(CodegenError {
            message: error.message,
            span: error.span,
        })
    })?;
    optimize_program(&mut program);

    validate_ffi_signatures(&program)?;
    if target_spec.platform == TargetPlatform::Wasm {
        return Err(BuildError::UnsupportedBackendForTarget(
            "shared-library wasm output is not yet supported; use `rune build <file.rn> --target wasm32-unknown-unknown` for standalone wasm modules".into(),
        ));
    }
    if matches!(
        target_spec.platform,
        TargetPlatform::Linux | TargetPlatform::MacOS
    ) {
        build_shared_library_via_llvm(&program, output_path, &target_spec, source_path)?;
        write_c_header_for_library(&program, output_path)?;
        return Ok(());
    }

    let asm = crate::codegen::emit_program(&program).map_err(BuildError::Codegen)?;
    let asm = rename_functions_for_library(&program, &asm);

    let rustc = find_rustc().ok_or(BuildError::RustcNotFound)?;
    let temp_dir = create_temp_dir()?;
    let wrapper = rust_cdylib_wrapper_source(&program);
    let wrapper_path = write_wrapper_files(&temp_dir, &asm, &wrapper)?;

    if let Some(parent) = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|source| BuildError::Io {
            context: format!("failed to create `{}`", parent.display()),
            source,
        })?;
    }

    let mut command = rustc_command(
        &rustc,
        &wrapper_path,
        output_path,
        Some(&target_spec),
        true,
        &BuildOptions::default(),
    );
    let status = command.output().map_err(|source| BuildError::Io {
        context: format!("failed to run `{}`", rustc.display()),
        source,
    })?;

    if !status.status.success() {
        return Err(BuildError::ToolFailed {
            tool: rustc.display().to_string(),
            status: status.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&status.stderr).into_owned(),
        });
    }

    let _ = fs::remove_file(temp_dir.join("out.s"));
    let _ = fs::remove_file(&wrapper_path);
    let _ = fs::remove_dir(&temp_dir);
    write_c_header_for_library(&program, output_path)?;
    Ok(())
}

pub fn build_static_library(
    source_path: &Path,
    output_path: &Path,
    target: Option<&str>,
) -> Result<(), BuildError> {
    let target_spec = target_spec(target)?;
    let mut program = load_program_from_path(source_path)
        .map_err(|error| BuildError::ModuleLoad(error.to_string()))?;
    check_program(&program).map_err(|error| {
        BuildError::Codegen(CodegenError {
            message: error.message,
            span: error.span,
        })
    })?;
    optimize_program(&mut program);
    validate_ffi_signatures(&program)?;

    let temp_dir = create_temp_dir()?;
    let obj_path = temp_dir.join(object_file_name(&target_spec));
    emit_object_file(&program, target_spec.triple, &obj_path).map_err(|error| {
        BuildError::Codegen(CodegenError {
            message: error.message,
            span: crate::lexer::Span { line: 1, column: 1 },
        })
    })?;

    if let Some(parent) = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|source| BuildError::Io {
            context: format!("failed to create `{}`", parent.display()),
            source,
        })?;
    }

    create_archive(output_path, &obj_path, &target_spec)?;

    let _ = fs::remove_file(obj_path);
    let _ = fs::remove_dir(temp_dir);
    write_c_header_for_library(&program, output_path)?;
    Ok(())
}

fn build_executable_via_llvm(
    program: &Program,
    output_path: &Path,
    target_spec: &TargetSpec,
    _source_path: &Path,
    options: &BuildOptions,
) -> Result<(), BuildError> {
    if target_spec.triple == "wasm32-wasip1" {
        return build_wasi_module_via_rust_runtime(program, output_path, target_spec, options);
    }
    if target_spec.platform == TargetPlatform::Wasm {
        return build_wasm_module_via_llvm(program, output_path, target_spec);
    }
    if target_spec.platform == TargetPlatform::Windows {
        return build_windows_executable_via_llvm_rust_wrapper(
            program,
            output_path,
            target_spec,
            options,
        );
    }
    if target_spec.platform == TargetPlatform::Linux
        && host_native_target_triple() == Some(target_spec.triple)
    {
        return build_unix_executable_via_packaged_clang(program, output_path, target_spec, options);
    }
    Err(BuildError::ToolNotFound(
        "direct LLVM/LLD native linking for non-Windows targets requires packaged target runtime/sysroot assets; Zig is no longer used".into(),
    ))
}

fn build_windows_executable_via_llvm_rust_wrapper(
    program: &Program,
    output_path: &Path,
    target_spec: &TargetSpec,
    options: &BuildOptions,
) -> Result<(), BuildError> {
    if target_spec.triple == "aarch64-pc-windows-gnu" {
        let assets = crate::toolchain::detect_windows_dev_assets().ok_or_else(|| {
            BuildError::ToolNotFound(
                "Windows ARM64 LLVM builds require packaged MSVC/Windows SDK assets".into(),
            )
        })?;
        if assets.msvc_lib_arm64.is_none()
            || assets.sdk_lib_ucrt_arm64.is_none()
            || assets.sdk_lib_um_arm64.is_none()
        {
            return Err(BuildError::ToolNotFound(
                "Windows ARM64 LLVM builds require packaged ARM64 MSVC/Windows SDK libraries".into(),
            ));
        }
    }
    let rustc = find_rustc().ok_or(BuildError::RustcNotFound)?;
    let temp_dir = create_temp_dir()?;
    let obj_path = temp_dir.join("out.obj");
    let wrapper_path = temp_dir.join("wrapper.rs");
    let llvm_ir = emit_llvm_ir(program).map_err(|error| {
        BuildError::Codegen(CodegenError {
            message: error.message,
            span: crate::lexer::Span { line: 1, column: 1 },
        })
    })?;
    let llvm_ir = rename_llvm_main_symbol_for_native_entry(&llvm_ir);
    emit_object_file_from_ir(&llvm_ir, target_spec.triple, &obj_path).map_err(|error| {
        BuildError::Codegen(CodegenError {
            message: error.message,
            span: crate::lexer::Span { line: 1, column: 1 },
        })
    })?;
    fs::write(&wrapper_path, rust_llvm_exe_wrapper_source()).map_err(|source| BuildError::Io {
        context: format!("failed to write `{}`", wrapper_path.display()),
        source,
    })?;

    let compiled_c_objects = compile_c_sources(&temp_dir, target_spec, &options.link_c_sources)?;
    let mut effective_options = options.clone();
    effective_options.link_args.push(obj_path.display().to_string());
    effective_options.link_args.extend(
        compiled_c_objects
            .iter()
            .map(|path| path.display().to_string()),
    );

    if let Some(parent) = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|source| BuildError::Io {
            context: format!("failed to create `{}`", parent.display()),
            source,
        })?;
    }

    let wrapper_target = windows_wrapper_target_spec(target_spec);
    let mut command = rustc_command(
        &rustc,
        &wrapper_path,
        output_path,
        Some(&wrapper_target),
        false,
        &effective_options,
    );
    let status = command.output().map_err(|source| BuildError::Io {
        context: format!("failed to run `{}`", rustc.display()),
        source,
    })?;

    if !status.status.success() {
        return Err(BuildError::ToolFailed {
            tool: rustc.display().to_string(),
            status: status.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&status.stderr).into_owned(),
        });
    }

    let _ = fs::remove_file(obj_path);
    let _ = fs::remove_file(wrapper_path);
    let _ = fs::remove_dir(temp_dir);
    Ok(())
}

fn build_unix_executable_via_packaged_clang(
    program: &Program,
    output_path: &Path,
    target_spec: &TargetSpec,
    options: &BuildOptions,
) -> Result<(), BuildError> {
    let temp_dir = create_temp_dir()?;
    let runtime_path = temp_dir.join("runtime.c");
    let wrapper_path = temp_dir.join("main_wrapper.c");
    let obj_path = temp_dir.join(object_file_name(target_spec));
    fs::write(&runtime_path, portable_runtime_source()).map_err(|source| BuildError::Io {
        context: format!("failed to write `{}`", runtime_path.display()),
        source,
    })?;
    fs::write(&wrapper_path, c_native_exe_wrapper_source()).map_err(|source| BuildError::Io {
        context: format!("failed to write `{}`", wrapper_path.display()),
        source,
    })?;

    emit_object_file(program, target_spec.triple, &obj_path).map_err(|error| {
        BuildError::Codegen(CodegenError {
            message: error.message,
            span: crate::lexer::Span { line: 1, column: 1 },
        })
    })?;

    let mut c_sources = vec![runtime_path.clone(), wrapper_path.clone()];
    c_sources.extend(options.link_c_sources.iter().cloned());
    let compiled_c_objects = compile_c_sources(&temp_dir, target_spec, &c_sources)?;

    let mut link_objects = Vec::with_capacity(1 + compiled_c_objects.len());
    link_objects.push(obj_path.clone());
    link_objects.extend(compiled_c_objects.iter().cloned());
    link_with_packaged_clang(target_spec, &link_objects, output_path, false, options)?;

    let _ = fs::remove_file(runtime_path);
    let _ = fs::remove_file(wrapper_path);
    let _ = fs::remove_file(obj_path);
    let _ = fs::remove_dir(temp_dir);
    Ok(())
}

fn windows_wrapper_target_spec(target_spec: &TargetSpec) -> TargetSpec {
    match target_spec.triple {
        "aarch64-pc-windows-gnu" => TargetSpec {
            triple: "aarch64-pc-windows-msvc",
            platform: TargetPlatform::Windows,
            exe_extension: target_spec.exe_extension,
            library_extension: target_spec.library_extension,
            static_library_extension: target_spec.static_library_extension,
            needs_macos_sdk: false,
        },
        _ => target_spec.clone(),
    }
}

fn build_unix_shared_library_via_packaged_clang(
    program: &Program,
    output_path: &Path,
    target_spec: &TargetSpec,
) -> Result<(), BuildError> {
    let temp_dir = create_temp_dir()?;
    let runtime_path = temp_dir.join("runtime.c");
    let obj_path = temp_dir.join(object_file_name(target_spec));
    fs::write(&runtime_path, portable_runtime_source()).map_err(|source| BuildError::Io {
        context: format!("failed to write `{}`", runtime_path.display()),
        source,
    })?;

    emit_object_file(program, target_spec.triple, &obj_path).map_err(|error| {
        BuildError::Codegen(CodegenError {
            message: error.message,
            span: crate::lexer::Span { line: 1, column: 1 },
        })
    })?;

    let runtime_objects =
        compile_c_sources(&temp_dir, target_spec, std::slice::from_ref(&runtime_path))?;
    let mut link_objects = Vec::with_capacity(1 + runtime_objects.len());
    link_objects.push(obj_path.clone());
    link_objects.extend(runtime_objects.iter().cloned());
    link_with_packaged_clang(
        target_spec,
        &link_objects,
        output_path,
        true,
        &BuildOptions::default(),
    )?;

    let _ = fs::remove_file(runtime_path);
    let _ = fs::remove_file(obj_path);
    let _ = fs::remove_dir(temp_dir);
    Ok(())
}

fn build_wasm_module_via_llvm(
    program: &Program,
    output_path: &Path,
    target_spec: &TargetSpec,
) -> Result<(), BuildError> {
    let temp_dir = create_temp_dir()?;
    let obj_path = temp_dir.join("out.o");
    emit_object_file(program, target_spec.triple, &obj_path).map_err(|error| {
        BuildError::Codegen(CodegenError {
            message: error.message,
            span: crate::lexer::Span { line: 1, column: 1 },
        })
    })?;

    if let Some(parent) = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|source| BuildError::Io {
            context: format!("failed to create `{}`", parent.display()),
            source,
        })?;
    }

    let wasm_ld = find_packaged_wasm_ld().ok_or_else(|| {
        BuildError::ToolNotFound("wasm-ld.exe (expected in packaged LLVM toolchain)".into())
    })?;
    let status = Command::new(&wasm_ld)
        .arg("--no-entry")
        .arg("--export-all")
        .arg("--export-memory")
        .arg("--allow-undefined")
        .arg("-o")
        .arg(output_path)
        .arg(&obj_path)
        .output()
        .map_err(|source| BuildError::Io {
            context: format!("failed to run `{}`", wasm_ld.display()),
            source,
        })?;
    if !status.status.success() {
        return Err(BuildError::ToolFailed {
            tool: wasm_ld.display().to_string(),
            status: status.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&status.stderr).into_owned(),
        });
    }

    let loader_path = wasm_loader_path(output_path);
    fs::write(&loader_path, wasm_loader_source(output_path)).map_err(|source| BuildError::Io {
        context: format!("failed to write `{}`", loader_path.display()),
        source,
    })?;
    let _ = fs::remove_file(obj_path);
    let _ = fs::remove_dir(temp_dir);
    Ok(())
}

fn build_wasi_module_via_rust_runtime(
    program: &Program,
    output_path: &Path,
    target_spec: &TargetSpec,
    options: &BuildOptions,
) -> Result<(), BuildError> {
    let temp_dir = create_temp_dir()?;
    let obj_path = temp_dir.join("out.o");
    let wrapper_path = temp_dir.join("wrapper.rs");
    let llvm_ir = emit_llvm_ir(program).map_err(|error| {
        BuildError::Codegen(CodegenError {
            message: error.message,
            span: crate::lexer::Span { line: 1, column: 1 },
        })
    })?;
    let llvm_ir = rename_llvm_main_symbol_for_wasi(&llvm_ir);
    emit_object_file_from_ir(&llvm_ir, target_spec.triple, &obj_path).map_err(|error| {
        BuildError::Codegen(CodegenError {
            message: error.message,
            span: crate::lexer::Span { line: 1, column: 1 },
        })
    })?;
    fs::write(&wrapper_path, rust_wasi_wrapper_source()).map_err(|source| BuildError::Io {
        context: format!("failed to write `{}`", wrapper_path.display()),
        source,
    })?;

    if let Some(parent) = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|source| BuildError::Io {
            context: format!("failed to create `{}`", parent.display()),
            source,
        })?;
    }

    let rustc = find_rustc().ok_or(BuildError::RustcNotFound)?;
    let status = Command::new(&rustc)
        .arg(&wrapper_path)
        .arg("-o")
        .arg(output_path)
        .arg("--edition=2024")
        .arg("--target")
        .arg(target_spec.triple)
        .arg("-C")
        .arg("opt-level=3")
        .arg("-C")
        .arg(format!("link-arg={}", obj_path.display()))
        .args(render_rustc_link_args(options))
        .output()
        .map_err(|source| BuildError::Io {
            context: format!("failed to run `{}`", rustc.display()),
            source,
        })?;

    if !status.status.success() {
        return Err(BuildError::ToolFailed {
            tool: rustc.display().to_string(),
            status: status.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&status.stderr).into_owned(),
        });
    }

    let _ = fs::remove_file(obj_path);
    let _ = fs::remove_file(wrapper_path);
    let _ = fs::remove_dir(temp_dir);
    Ok(())
}

fn rename_llvm_main_symbol_for_wasi(llvm_ir: &str) -> String {
    llvm_ir.replace("@main(", "@rune_wasi_main(")
}

fn rename_llvm_main_symbol_for_native_entry(llvm_ir: &str) -> String {
    llvm_ir.replace("@main(", "@rune_entry_main(")
}

fn wasm_loader_path(output_path: &Path) -> PathBuf {
    output_path.with_extension("js")
}

fn wasm_loader_source(output_path: &Path) -> String {
    let wasm_file_name = output_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("module.wasm");
    format!(
        r#""use strict";

const isNode = typeof process !== "undefined" && !!(process.versions && process.versions.node);
const fs = isNode ? require("fs") : null;
const path = isNode ? require("path") : null;
const os = isNode ? require("os") : null;
const childProcess = isNode ? require("child_process") : null;
const perf = typeof performance !== "undefined" ? performance : null;

async function loadBytes(source) {{
  if (source == null) {{
    if (isNode) {{
      return fs.readFileSync(path.join(__dirname, "{wasm_file_name}"));
    }}
    const response = await fetch("{wasm_file_name}");
    return new Uint8Array(await response.arrayBuffer());
  }}
  if (typeof source === "string") {{
    if (isNode && !/^https?:\/\//.test(source) && !source.startsWith("file:")) {{
      return fs.readFileSync(source);
    }}
    const response = await fetch(source);
    return new Uint8Array(await response.arrayBuffer());
  }}
  if (source instanceof ArrayBuffer) {{
    return new Uint8Array(source);
  }}
  if (ArrayBuffer.isView(source)) {{
    return new Uint8Array(source.buffer, source.byteOffset, source.byteLength);
  }}
  throw new TypeError("unsupported Rune wasm source input");
}}

function createHost(options = {{}}) {{
  let instance = null;
  let memory = null;
  let heapBase = 0;
  let heapCursor = 0;
  let lastStringLen = 0;
  let stdinLines = null;
  const hostArgv = Array.isArray(options.argv)
    ? options.argv.map((value) => String(value))
    : (isNode ? process.argv.slice(2) : []);
  const monotonicStart = (perf && typeof perf.now === "function") ? perf.now() : Date.now();
  const decoder = new TextDecoder("utf-8");
  const encoder = new TextEncoder();
  const browserBuffers = {{ stdout: "", stderr: "" }};

  function setInstance(nextInstance) {{
    instance = nextInstance;
    refreshMemory();
  }}

  function refreshMemory() {{
    memory = instance && instance.exports && instance.exports.memory ? instance.exports.memory : null;
    if (!memory) {{
      throw new Error("Rune wasm module did not export memory");
    }}
    if (!heapBase) {{
      const exported = instance.exports.__heap_base;
      if (typeof exported === "number") {{
        heapBase = exported;
      }} else if (typeof exported === "bigint") {{
        heapBase = Number(exported);
      }} else if (exported && typeof exported.value === "number") {{
        heapBase = exported.value;
      }} else if (exported && typeof exported.value === "bigint") {{
        heapBase = Number(exported.value);
      }} else {{
        heapBase = 65536;
      }}
      heapCursor = heapBase;
    }}
  }}

  function ensureCapacity(extraBytes) {{
    refreshMemory();
    const needed = heapCursor + extraBytes;
    while (needed > memory.buffer.byteLength) {{
      const growBy = Math.ceil((needed - memory.buffer.byteLength) / 65536);
      memory.grow(growBy);
      refreshMemory();
    }}
  }}

  function allocString(text) {{
    const bytes = encoder.encode(text);
    ensureCapacity(bytes.length + 1);
    const view = new Uint8Array(memory.buffer, heapCursor, bytes.length + 1);
    view.set(bytes);
    view[bytes.length] = 0;
    const ptr = heapCursor;
    heapCursor += bytes.length + 1;
    lastStringLen = bytes.length;
    return ptr;
  }}

  function readString(ptr, len) {{
    refreshMemory();
    return decoder.decode(new Uint8Array(memory.buffer, Number(ptr), Number(len)));
  }}

  function flushBrowserBuffer(stream) {{
    const value = browserBuffers[stream];
    if (!value) {{
      return;
    }}
    if (stream === "stderr") {{
      console.error(value.replace(/\n$/, ""));
    }} else {{
      console.log(value.replace(/\n$/, ""));
    }}
    browserBuffers[stream] = "";
  }}

  function writeText(stream, text) {{
    if (isNode) {{
      (stream === "stderr" ? process.stderr : process.stdout).write(text);
      return;
    }}
    browserBuffers[stream] += text;
    if (text.includes("\n")) {{
      flushBrowserBuffer(stream);
    }}
  }}

  function readInputLine() {{
    if (isNode) {{
      if (stdinLines === null) {{
        const raw = fs.readFileSync(0, "utf8");
        stdinLines = raw.replace(/\r\n/g, "\n").split("\n");
      }}
      return stdinLines.shift() ?? "";
    }}
    if (typeof prompt === "function") {{
      const value = prompt("") ?? "";
      return String(value);
    }}
    return "";
  }}

  function parseEnvBool(raw, fallback) {{
    switch (String(raw).trim().toLowerCase()) {{
      case "1":
      case "true":
      case "yes":
      case "on":
        return true;
      case "0":
      case "false":
      case "no":
      case "off":
        return false;
      default:
        return fallback;
    }}
  }}

  function sleepMs(ms) {{
    const duration = Number(ms);
    if (!(duration > 0)) {{
      return;
    }}
    const end = Date.now() + duration;
    while (Date.now() < end) {{
      // busy wait keeps the wasm import synchronous
    }}
  }}

  function tcpConnect(host, port, timeoutMs) {{
    if (!isNode) {{
      return false;
    }}
    const probe = [
      "const net = require('net');",
      "const host = process.argv[1];",
      "const port = Number(process.argv[2]);",
      "const timeout = Number(process.argv[3]);",
      "const socket = new net.Socket();",
      "let done = false;",
      "function finish(ok) {{ if (!done) {{ done = true; try {{ socket.destroy(); }} catch (_) {{}} process.exit(ok ? 0 : 1); }} }}",
      "socket.setTimeout(timeout);",
      "socket.once('connect', () => finish(true));",
      "socket.once('timeout', () => finish(false));",
      "socket.once('error', () => finish(false));",
      "socket.connect(port, host);"
    ].join("");
    const result = childProcess.spawnSync(process.execPath, ["-e", probe, host, String(port), String(timeoutMs)], {{
      stdio: "ignore"
    }});
    return result.status === 0;
  }}

  const imports = {{
    env: {{
      rune_rt_print_i64(value) {{ writeText("stdout", value.toString()); }},
      rune_rt_eprint_i64(value) {{ writeText("stderr", value.toString()); }},
      rune_rt_print_str(ptr, len) {{ writeText("stdout", readString(ptr, len)); }},
      rune_rt_eprint_str(ptr, len) {{ writeText("stderr", readString(ptr, len)); }},
      rune_rt_print_newline() {{ writeText("stdout", "\n"); }},
      rune_rt_eprint_newline() {{ writeText("stderr", "\n"); }},
      rune_rt_flush_stdout() {{ if (!isNode) flushBrowserBuffer("stdout"); }},
      rune_rt_flush_stderr() {{ if (!isNode) flushBrowserBuffer("stderr"); }},
      rune_rt_input_line() {{
        return allocString(readInputLine());
      }},
      rune_rt_last_string_len() {{
        return BigInt(lastStringLen);
      }},
      rune_rt_panic(msgPtr, msgLen, ctxPtr, ctxLen) {{
        const message = readString(msgPtr, msgLen);
        const context = readString(ctxPtr, ctxLen);
        writeText("stderr", `Rune panic: ${{message}}\n  ${{context}}\n`);
        throw new Error(`Rune panic: ${{message}} (${{context}})`);
      }},
      rune_rt_time_now_unix() {{
        return BigInt(Math.floor(Date.now() / 1000));
      }},
      rune_rt_time_monotonic_ms() {{
        const now = (perf && typeof perf.now === "function") ? perf.now() : Date.now();
        return BigInt(Math.floor(now - monotonicStart));
      }},
      rune_rt_time_sleep_ms(ms) {{
        sleepMs(ms);
      }},
      rune_rt_system_pid() {{
        return isNode ? (process.pid | 0) : 0;
      }},
      rune_rt_system_cpu_count() {{
        if (isNode && os) {{
          if (typeof os.availableParallelism === "function") {{
            return os.availableParallelism() | 0;
          }}
          return os.cpus().length | 0;
        }}
        return 1;
      }},
      rune_rt_system_exit(code) {{
        if (isNode) {{
          process.exit(Number(code));
        }}
        throw new Error(`Rune requested exit(${{Number(code)}}) in a non-Node host`);
      }},
      rune_rt_env_exists(ptr, len) {{
        if (!isNode) {{
          return false;
        }}
        const key = readString(ptr, len);
        return Object.prototype.hasOwnProperty.call(process.env, key);
      }},
      rune_rt_env_get_i32(ptr, len, defaultValue) {{
        if (!isNode) {{
          return Number(defaultValue) | 0;
        }}
        const key = readString(ptr, len);
        const raw = process.env[key];
        const parsed = Number.parseInt(raw ?? "", 10);
        return Number.isFinite(parsed) ? (parsed | 0) : (Number(defaultValue) | 0);
      }},
      rune_rt_env_get_bool(ptr, len, defaultValue) {{
        if (!isNode) {{
          return !!defaultValue;
        }}
        const key = readString(ptr, len);
        const raw = process.env[key];
        if (raw == null) {{
          return !!defaultValue;
        }}
        return parseEnvBool(raw, !!defaultValue);
      }},
      rune_rt_env_arg_count() {{
        return hostArgv.length | 0;
      }},
      rune_rt_network_tcp_connect(ptr, len, port) {{
        return tcpConnect(readString(ptr, len), Number(port), 250);
      }},
      rune_rt_network_tcp_connect_timeout(ptr, len, port, timeoutMs) {{
        return tcpConnect(readString(ptr, len), Number(port), Number(timeoutMs));
      }},
      rune_rt_fs_exists(ptr, len) {{
        if (!isNode || !fs) {{
          throw new Error("Rune fs builtins require a Node host for wasm32-unknown-unknown");
        }}
        return fs.existsSync(readString(ptr, len));
      }},
      rune_rt_fs_read_string(ptr, len) {{
        if (!isNode || !fs) {{
          throw new Error("Rune fs builtins require a Node host for wasm32-unknown-unknown");
        }}
        return allocString(fs.readFileSync(readString(ptr, len), "utf8"));
      }},
      rune_rt_fs_write_string(pathPtr, pathLen, contentPtr, contentLen) {{
        if (!isNode || !fs) {{
          throw new Error("Rune fs builtins require a Node host for wasm32-unknown-unknown");
        }}
        fs.writeFileSync(readString(pathPtr, pathLen), readString(contentPtr, contentLen), "utf8");
        return true;
      }},
      rune_rt_terminal_clear() {{
        if (!isNode) {{
          throw new Error("Rune terminal builtins require a Node host for wasm32-unknown-unknown");
        }}
        writeText("stdout", "\u001b[2J\u001b[H");
      }},
      rune_rt_terminal_move_to(row, col) {{
        if (!isNode) {{
          throw new Error("Rune terminal builtins require a Node host for wasm32-unknown-unknown");
        }}
        writeText("stdout", `\u001b[${{Number(row)}};${{Number(col)}}H`);
      }},
      rune_rt_terminal_hide_cursor() {{
        if (!isNode) {{
          throw new Error("Rune terminal builtins require a Node host for wasm32-unknown-unknown");
        }}
        writeText("stdout", "\u001b[?25l");
      }},
      rune_rt_terminal_show_cursor() {{
        if (!isNode) {{
          throw new Error("Rune terminal builtins require a Node host for wasm32-unknown-unknown");
        }}
        writeText("stdout", "\u001b[?25h");
      }},
      rune_rt_terminal_set_title(ptr, len) {{
        if (!isNode) {{
          throw new Error("Rune terminal builtins require a Node host for wasm32-unknown-unknown");
        }}
        writeText("stdout", `\u001b]0;${{readString(ptr, len)}}\u0007`);
      }},
      rune_rt_audio_bell() {{
        if (!isNode) {{
          return false;
        }}
        writeText("stdout", "\u0007");
        return true;
      }}
    }}
  }};

  return {{
    imports,
    setInstance,
    readString,
    allocString,
    argCount() {{
      return hostArgv.length | 0;
    }}
  }};
}}

async function instantiateRuneWasm(source, options = {{}}) {{
  const bytes = await loadBytes(source);
  const host = createHost(options);
  const result = await WebAssembly.instantiate(bytes, host.imports);
  host.setInstance(result.instance);
  return {{
    instance: result.instance,
    exports: result.instance.exports,
    readString: host.readString,
    allocString: host.allocString,
    runMain() {{
      if (typeof result.instance.exports.main !== "function") {{
        throw new Error("Rune wasm module does not export `main`");
      }}
      if (result.instance.exports.main.length >= 2) {{
        return result.instance.exports.main(host.argCount(), 0);
      }}
      return result.instance.exports.main();
    }}
  }};
}}

const api = {{ instantiateRuneWasm }};

if (typeof module !== "undefined" && module.exports) {{
  module.exports = api;
  if (isNode && require.main === module) {{
    (async () => {{
      const argv = process.argv.slice(2);
      const wasmSource = argv[0] && argv[0].endsWith(".wasm") ? argv[0] : undefined;
      const programArgs = wasmSource ? argv.slice(1) : argv;
      const runtime = await instantiateRuneWasm(wasmSource, {{ argv: programArgs }});
      process.exit(Number(runtime.runMain()));
    }})().catch((error) => {{
      console.error(error.stack || String(error));
      process.exit(1);
    }});
  }}
}} else {{
  globalThis.RuneWasm = api;
}}
"#
    )
}

fn build_shared_library_via_llvm(
    program: &Program,
    output_path: &Path,
    target_spec: &TargetSpec,
    _source_path: &Path,
) -> Result<(), BuildError> {
    if target_spec.platform == TargetPlatform::Linux
        && host_native_target_triple() == Some(target_spec.triple)
    {
        return build_unix_shared_library_via_packaged_clang(program, output_path, target_spec);
    }
    if target_spec.platform != TargetPlatform::Windows {
        return Err(BuildError::ToolNotFound(
            "direct LLVM/LLD shared-library linking for non-Windows targets requires packaged target runtime/sysroot assets; Zig is no longer used".into(),
        ));
    }
    let temp_dir = create_temp_dir()?;
    let runtime_path = temp_dir.join("runtime.c");
    let obj_path = temp_dir.join("out.o");
    fs::write(&runtime_path, portable_runtime_source()).map_err(|source| BuildError::Io {
        context: format!("failed to write `{}`", runtime_path.display()),
        source,
    })?;

    if let Some(parent) = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|source| BuildError::Io {
            context: format!("failed to create `{}`", parent.display()),
            source,
        })?;
    }

    emit_object_file(program, target_spec.triple, &obj_path).map_err(|error| {
        BuildError::Codegen(CodegenError {
            message: error.message,
            span: crate::lexer::Span { line: 1, column: 1 },
        })
    })?;

    let runtime_objects =
        compile_c_sources(&temp_dir, target_spec, std::slice::from_ref(&runtime_path))?;
    let mut link_args = Vec::with_capacity(1 + runtime_objects.len());
    link_args.push(obj_path.clone());
    link_args.extend(runtime_objects.iter().cloned());
    link_with_packaged_clang(
        target_spec,
        &link_args,
        output_path,
        true,
        &BuildOptions::default(),
    )?;

    let _ = fs::remove_file(runtime_path);
    let _ = fs::remove_file(obj_path);
    let _ = fs::remove_dir(temp_dir);
    Ok(())
}

fn create_archive(
    output_path: &Path,
    obj_path: &Path,
    target_spec: &TargetSpec,
) -> Result<(), BuildError> {
    match target_spec.platform {
        TargetPlatform::Windows => {
            let llvm_lib = find_packaged_llvm_tool("llvm-lib.exe").ok_or_else(|| {
                BuildError::ToolNotFound(
                    "llvm-lib.exe (expected in packaged LLVM toolchain)".into(),
                )
            })?;
            let status = Command::new(&llvm_lib)
                .arg(format!("/OUT:{}", output_path.display()))
                .arg(obj_path)
                .output()
                .map_err(|source| BuildError::Io {
                    context: format!("failed to run `{}`", llvm_lib.display()),
                    source,
                })?;
            if !status.status.success() {
                return Err(BuildError::ToolFailed {
                    tool: llvm_lib.display().to_string(),
                    status: status.status.code().unwrap_or(-1),
                    stderr: String::from_utf8_lossy(&status.stderr).into_owned(),
                });
            }
        }
        TargetPlatform::Linux | TargetPlatform::MacOS | TargetPlatform::Wasm => {
            let llvm_ar = find_packaged_llvm_tool("llvm-ar").ok_or_else(|| {
                BuildError::ToolNotFound("llvm-ar (expected in packaged LLVM toolchain)".into())
            })?;
            let status = Command::new(&llvm_ar)
                .arg("rcs")
                .arg(output_path)
                .arg(obj_path)
                .output()
                .map_err(|source| BuildError::Io {
                    context: format!("failed to run `{}`", llvm_ar.display()),
                    source,
                })?;
            if !status.status.success() {
                return Err(BuildError::ToolFailed {
                    tool: llvm_ar.display().to_string(),
                    status: status.status.code().unwrap_or(-1),
                    stderr: String::from_utf8_lossy(&status.stderr).into_owned(),
                });
            }
        }
    }

    Ok(())
}

fn object_file_name(target_spec: &TargetSpec) -> &'static str {
    match target_spec.platform {
        TargetPlatform::Windows => "out.obj",
        TargetPlatform::Linux | TargetPlatform::MacOS | TargetPlatform::Wasm => "out.o",
    }
}

fn find_rustc() -> Option<PathBuf> {
    let home = env::var_os("USERPROFILE")?;
    let candidate = PathBuf::from(home)
        .join(".rustup")
        .join("toolchains")
        .join("stable-x86_64-pc-windows-gnu")
        .join("bin")
        .join("rustc.exe");

    if candidate.is_file() {
        Some(candidate)
    } else {
        None
    }
}

fn portable_runtime_source() -> &'static str {
    r#"#include <stdio.h>
#include <stdlib.h>
#include <stdbool.h>
#include <stdint.h>
#include <inttypes.h>
#include <string.h>
#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#endif

static void rune_rt_init_io(void) {
#ifdef _WIN32
    static int initialized = 0;
    if (!initialized) {
        _setmode(_fileno(stdin), _O_BINARY);
        _setmode(_fileno(stdout), _O_BINARY);
        _setmode(_fileno(stderr), _O_BINARY);
        initialized = 1;
    }
#endif
}

void rune_rt_print_i64(int64_t value) { rune_rt_init_io(); printf("%" PRId64, value); fflush(stdout); }
void rune_rt_eprint_i64(int64_t value) { rune_rt_init_io(); fprintf(stderr, "%" PRId64, value); fflush(stderr); }
void rune_rt_print_newline(void) { rune_rt_init_io(); fwrite("\n", 1, 1, stdout); fflush(stdout); }
void rune_rt_eprint_newline(void) { rune_rt_init_io(); fwrite("\n", 1, 1, stderr); fflush(stderr); }
void rune_rt_flush_stdout(void) { rune_rt_init_io(); fflush(stdout); }
void rune_rt_flush_stderr(void) { rune_rt_init_io(); fflush(stderr); }
void rune_rt_print_str(const char* ptr, int64_t len) { rune_rt_init_io(); fwrite(ptr, 1, (size_t)len, stdout); fflush(stdout); }
void rune_rt_eprint_str(const char* ptr, int64_t len) { rune_rt_init_io(); fwrite(ptr, 1, (size_t)len, stderr); fflush(stderr); }
static int64_t rune_rt_last_len = 0;
char* rune_rt_input_line(void) {
    rune_rt_init_io();
    size_t cap = 128;
    size_t len = 0;
    char* buffer = (char*)malloc(cap);
    if (!buffer) {
        fprintf(stderr, "Rune runtime: failed to allocate input buffer\n");
        exit(111);
    }
    int ch = 0;
    while ((ch = getchar()) != EOF) {
        if (ch == '\n') break;
        if (ch == '\r') continue;
        if (len + 1 >= cap) {
            cap *= 2;
            char* grown = (char*)realloc(buffer, cap);
            if (!grown) {
                free(buffer);
                fprintf(stderr, "Rune runtime: failed to grow input buffer\n");
                exit(111);
            }
            buffer = grown;
        }
        buffer[len++] = (char)ch;
    }
    buffer[len] = '\0';
    rune_rt_last_len = (int64_t)len;
    return buffer;
}
int64_t rune_rt_last_string_len(void) { return rune_rt_last_len; }
static char* rune_rt_store_heap_string(char* buffer, size_t len) {
    if (!buffer) {
        fprintf(stderr, "Rune runtime: failed to allocate string buffer\n");
        exit(111);
    }
    buffer[len] = '\0';
    rune_rt_last_len = (int64_t)len;
    return buffer;
}
static char* rune_rt_store_copied_string(const char* text) {
    size_t len = strlen(text);
    char* out = (char*)malloc(len + 1);
    if (!out) {
        fprintf(stderr, "Rune runtime: failed to allocate string copy\n");
        exit(111);
    }
    memcpy(out, text, len);
    return rune_rt_store_heap_string(out, len);
}
char* rune_rt_string_concat(const char* left_ptr, int64_t left_len, const char* right_ptr, int64_t right_len) {
    size_t total = (size_t)left_len + (size_t)right_len;
    char* out = (char*)malloc(total + 1);
    if (!out) {
        fprintf(stderr, "Rune runtime: failed to allocate concatenated string\n");
        exit(111);
    }
    memcpy(out, left_ptr, (size_t)left_len);
    memcpy(out + left_len, right_ptr, (size_t)right_len);
    return rune_rt_store_heap_string(out, total);
}
char* rune_rt_string_from_i64(int64_t value) {
    char buffer[64];
    int written = snprintf(buffer, sizeof(buffer), "%" PRId64, value);
    if (written < 0) {
        fprintf(stderr, "Rune runtime: failed to format integer string\n");
        exit(111);
    }
    return rune_rt_store_copied_string(buffer);
}
char* rune_rt_string_from_bool(_Bool value) {
    return rune_rt_store_copied_string(value ? "true" : "false");
}
int64_t rune_rt_string_to_i64(const char* ptr, int64_t len) {
    char* text = (char*)malloc((size_t)len + 1);
    char* end = NULL;
    long long parsed = 0;
    if (!text) {
        fprintf(stderr, "Rune runtime: failed to allocate numeric conversion buffer\n");
        exit(111);
    }
    memcpy(text, ptr, (size_t)len);
    text[len] = '\0';
    parsed = strtoll(text, &end, 10);
    if (end == text) {
        fprintf(stderr, "Rune runtime: failed to convert string `%s` to i64\n", text);
        free(text);
        exit(111);
    }
    free(text);
    return (int64_t)parsed;
}
char* rune_rt_to_c_string(const char* ptr, int64_t len) {
    char* out = (char*)malloc((size_t)len + 1);
    if (!out) {
        fprintf(stderr, "Rune runtime: failed to allocate C string\n");
        exit(111);
    }
    memcpy(out, ptr, (size_t)len);
    out[len] = '\0';
    return out;
}
char* rune_rt_from_c_string(const char* ptr) {
    if (!ptr) {
        rune_rt_last_len = 0;
        char* out = (char*)malloc(1);
        if (!out) {
            fprintf(stderr, "Rune runtime: failed to allocate empty Rune string\n");
            exit(111);
        }
        out[0] = '\0';
        return out;
    }
    size_t len = strlen(ptr);
    char* out = (char*)malloc(len + 1);
    if (!out) {
        fprintf(stderr, "Rune runtime: failed to allocate Rune string from C string\n");
        exit(111);
    }
    memcpy(out, ptr, len + 1);
    rune_rt_last_len = (int64_t)len;
    return out;
}
void rune_rt_panic(const char* msg_ptr, int64_t msg_len, const char* ctx_ptr, int64_t ctx_len) {
    rune_rt_init_io();
    fprintf(stderr, "Rune panic: ");
    fwrite(msg_ptr, 1, (size_t)msg_len, stderr);
    fprintf(stderr, "\n  ");
    fwrite(ctx_ptr, 1, (size_t)ctx_len, stderr);
    fprintf(stderr, "\n");
    fflush(stderr);
    exit(101);
}
char* rune_rt_dynamic_to_string(int64_t tag, int64_t payload, int64_t extra) {
    switch (tag) {
        case 0:
            return rune_rt_store_copied_string("unit");
        case 1:
            return rune_rt_string_from_bool(payload != 0);
        case 2:
            return rune_rt_string_from_i64((int32_t)payload);
        case 3:
            return rune_rt_string_from_i64(payload);
        case 4: {
            char* out = (char*)malloc((size_t)extra + 1);
            if (!out) {
                fprintf(stderr, "Rune runtime: failed to allocate dynamic string copy\n");
                exit(111);
            }
            memcpy(out, (const char*)payload, (size_t)extra);
            return rune_rt_store_heap_string(out, (size_t)extra);
        }
        default:
            fprintf(stderr, "Rune runtime: unknown dynamic string tag %" PRId64 "\n", tag);
            exit(111);
    }
}
int64_t rune_rt_dynamic_to_i64(int64_t tag, int64_t payload, int64_t extra) {
    switch (tag) {
        case 0:
            return 0;
        case 1:
            return payload != 0 ? 1 : 0;
        case 2:
            return (int32_t)payload;
        case 3:
            return payload;
        case 4:
            return rune_rt_string_to_i64((const char*)payload, extra);
        default:
            fprintf(stderr, "Rune runtime: unknown dynamic numeric tag %" PRId64 "\n", tag);
            exit(111);
    }
}
static bool rune_rt_dynamic_to_i64_lossy(int64_t tag, int64_t payload, int64_t extra, int64_t* out_value) {
    (void)extra;
    switch (tag) {
        case 1:
            *out_value = payload != 0 ? 1 : 0;
            return true;
        case 2:
            *out_value = (int32_t)payload;
            return true;
        case 3:
            *out_value = payload;
            return true;
        default:
            return false;
    }
}
void rune_rt_print_dynamic(int64_t tag, int64_t payload, int64_t extra) {
    char* text = rune_rt_dynamic_to_string(tag, payload, extra);
    rune_rt_print_str(text, rune_rt_last_string_len());
}
void rune_rt_eprint_dynamic(int64_t tag, int64_t payload, int64_t extra) {
    char* text = rune_rt_dynamic_to_string(tag, payload, extra);
    rune_rt_eprint_str(text, rune_rt_last_string_len());
}
bool rune_rt_dynamic_truthy(int64_t tag, int64_t payload, int64_t extra) {
    switch (tag) {
        case 0:
            return false;
        case 1:
            return payload != 0;
        case 2:
            return (int32_t)payload != 0;
        case 3:
            return payload != 0;
        case 4:
            return extra != 0;
        default:
            fprintf(stderr, "Rune runtime: unknown dynamic truthiness tag %" PRId64 "\n", tag);
            exit(111);
    }
}
void rune_rt_dynamic_binary(const int64_t* left, const int64_t* right, int64_t* out, int64_t op) {
    int64_t left_tag = left[0];
    int64_t left_payload = left[1];
    int64_t left_extra = left[2];
    int64_t right_tag = right[0];
    int64_t right_payload = right[1];
    int64_t right_extra = right[2];
    if (op == 0 && (left_tag == 4 || right_tag == 4)) {
        char* left_text = rune_rt_dynamic_to_string(left_tag, left_payload, left_extra);
        int64_t left_len = rune_rt_last_string_len();
        char* right_text = rune_rt_dynamic_to_string(right_tag, right_payload, right_extra);
        int64_t right_len = rune_rt_last_string_len();
        char* joined = rune_rt_string_concat(left_text, left_len, right_text, right_len);
        out[0] = 4;
        out[1] = (int64_t)joined;
        out[2] = rune_rt_last_string_len();
        return;
    }
    int64_t left_number = 0;
    int64_t right_number = 0;
    if (!rune_rt_dynamic_to_i64_lossy(left_tag, left_payload, left_extra, &left_number)) {
        fprintf(stderr, "Rune runtime: dynamic binary op unsupported left tag %" PRId64 "\n", left_tag);
        exit(111);
    }
    if (!rune_rt_dynamic_to_i64_lossy(right_tag, right_payload, right_extra, &right_number)) {
        fprintf(stderr, "Rune runtime: dynamic binary op unsupported right tag %" PRId64 "\n", right_tag);
        exit(111);
    }
    out[0] = 3;
    switch (op) {
        case 0: out[1] = left_number + right_number; break;
        case 1: out[1] = left_number - right_number; break;
        case 2: out[1] = left_number * right_number; break;
        case 3:
            if (right_number == 0) {
                fprintf(stderr, "Rune runtime: dynamic division by zero\n");
                exit(111);
            }
            out[1] = left_number / right_number;
            break;
        case 4:
            if (right_number == 0) {
                fprintf(stderr, "Rune runtime: dynamic modulo by zero\n");
                exit(111);
            }
            out[1] = left_number % right_number;
            break;
        default:
            fprintf(stderr, "Rune runtime: unknown dynamic binary opcode %" PRId64 "\n", op);
            exit(111);
    }
    out[2] = 0;
}
bool rune_rt_dynamic_compare(const int64_t* left, const int64_t* right, int64_t op) {
    int64_t left_tag = left[0];
    int64_t left_payload = left[1];
    int64_t left_extra = left[2];
    int64_t right_tag = right[0];
    int64_t right_payload = right[1];
    int64_t right_extra = right[2];
    if (op == 0 || op == 1) {
        bool equal = false;
        if (left_tag == 4 || right_tag == 4) {
            char* left_text = rune_rt_dynamic_to_string(left_tag, left_payload, left_extra);
            int64_t left_len = rune_rt_last_string_len();
            char* right_text = rune_rt_dynamic_to_string(right_tag, right_payload, right_extra);
            int64_t right_len = rune_rt_last_string_len();
            equal = left_len == right_len
                && memcmp(left_text, right_text, (size_t)left_len) == 0;
        } else {
            int64_t left_number = 0;
            int64_t right_number = 0;
            if (rune_rt_dynamic_to_i64_lossy(left_tag, left_payload, left_extra, &left_number)
                && rune_rt_dynamic_to_i64_lossy(right_tag, right_payload, right_extra, &right_number)) {
                equal = left_number == right_number;
            } else {
                equal = left_tag == right_tag
                    && left_payload == right_payload
                    && left_extra == right_extra;
            }
        }
        return op == 0 ? equal : !equal;
    }
    int64_t left_number = 0;
    int64_t right_number = 0;
    if (!rune_rt_dynamic_to_i64_lossy(left_tag, left_payload, left_extra, &left_number)) {
        fprintf(stderr, "Rune runtime: dynamic comparison unsupported left tag %" PRId64 "\n", left_tag);
        exit(111);
    }
    if (!rune_rt_dynamic_to_i64_lossy(right_tag, right_payload, right_extra, &right_number)) {
        fprintf(stderr, "Rune runtime: dynamic comparison unsupported right tag %" PRId64 "\n", right_tag);
        exit(111);
    }
    switch (op) {
        case 2: return left_number > right_number;
        case 3: return left_number >= right_number;
        case 4: return left_number < right_number;
        case 5: return left_number <= right_number;
        default:
            fprintf(stderr, "Rune runtime: unknown dynamic comparison opcode %" PRId64 "\n", op);
            exit(111);
    }
}
bool rune_rt_fs_exists(const char* ptr, int64_t len) {
    char* path = (char*)malloc((size_t)len + 1);
    if (!path) {
        return false;
    }
    memcpy(path, ptr, (size_t)len);
    path[len] = '\0';
    FILE* file = fopen(path, "rb");
    free(path);
    if (!file) {
        return false;
    }
    fclose(file);
    return true;
}
char* rune_rt_fs_read_string(const char* ptr, int64_t len) {
    char* path = (char*)malloc((size_t)len + 1);
    if (!path) {
        fprintf(stderr, "Rune runtime: failed to allocate file path\n");
        exit(111);
    }
    memcpy(path, ptr, (size_t)len);
    path[len] = '\0';
    FILE* file = fopen(path, "rb");
    free(path);
    if (!file) {
        fprintf(stderr, "Rune runtime: failed to open file for reading\n");
        exit(111);
    }
    if (fseek(file, 0, SEEK_END) != 0) {
        fclose(file);
        fprintf(stderr, "Rune runtime: failed to seek file\n");
        exit(111);
    }
    long size = ftell(file);
    if (size < 0) {
        fclose(file);
        fprintf(stderr, "Rune runtime: failed to measure file size\n");
        exit(111);
    }
    rewind(file);
    char* buffer = (char*)malloc((size_t)size + 1);
    if (!buffer) {
        fclose(file);
        fprintf(stderr, "Rune runtime: failed to allocate file buffer\n");
        exit(111);
    }
    size_t read = fread(buffer, 1, (size_t)size, file);
    fclose(file);
    buffer[read] = '\0';
    rune_rt_last_len = (int64_t)read;
    return buffer;
}
bool rune_rt_fs_write_string(const char* path_ptr, int64_t path_len, const char* content_ptr, int64_t content_len) {
    char* path = (char*)malloc((size_t)path_len + 1);
    if (!path) {
        return false;
    }
    memcpy(path, path_ptr, (size_t)path_len);
    path[path_len] = '\0';
    FILE* file = fopen(path, "wb");
    free(path);
    if (!file) {
        return false;
    }
    size_t wrote = fwrite(content_ptr, 1, (size_t)content_len, file);
    fclose(file);
    return wrote == (size_t)content_len;
}
void rune_rt_terminal_clear(void) {
    rune_rt_init_io();
    fputs("\x1b[2J\x1b[H", stdout);
    fflush(stdout);
}
void rune_rt_terminal_move_to(int32_t row, int32_t col) {
    rune_rt_init_io();
    if (row < 1) row = 1;
    if (col < 1) col = 1;
    fprintf(stdout, "\x1b[%d;%dH", row, col);
    fflush(stdout);
}
void rune_rt_terminal_hide_cursor(void) {
    rune_rt_init_io();
    fputs("\x1b[?25l", stdout);
    fflush(stdout);
}
void rune_rt_terminal_show_cursor(void) {
    rune_rt_init_io();
    fputs("\x1b[?25h", stdout);
    fflush(stdout);
}
void rune_rt_terminal_set_title(const char* ptr, int64_t len) {
    rune_rt_init_io();
    fputs("\x1b]0;", stdout);
    fwrite(ptr, 1, (size_t)len, stdout);
    fputc('\x07', stdout);
    fflush(stdout);
}
bool rune_rt_audio_bell(void) {
    rune_rt_init_io();
    if (fputc('\x07', stdout) == EOF) {
        return false;
    }
    return fflush(stdout) == 0;
}
"#
}

fn c_native_exe_wrapper_source() -> &'static str {
    r#"#include <stdint.h>
extern int64_t rune_entry_main(void);
int main(void) {
    return (int)rune_entry_main();
}
"#
}

fn rustc_command(
    rustc: &Path,
    wrapper_path: &Path,
    output_path: &Path,
    target: Option<&TargetSpec>,
    shared_library: bool,
    options: &BuildOptions,
) -> Command {
    let mut command = Command::new(rustc);
    command
        .arg(wrapper_path)
        .arg("-o")
        .arg(output_path)
        .arg("--edition=2024")
        .arg("-C")
        .arg("opt-level=3")
        .arg("-C")
        .arg("codegen-units=1");
    if shared_library {
        command.arg("--crate-type=cdylib");
    }
    if let Some(target) = target {
        command.arg("--target").arg(target.triple);
        if let Some(linker) = target_linker_env(target) {
            command.arg("-C").arg(format!("linker={linker}"));
        }
        if let Some(sdk_root) = target_sdk_root_env(target) {
            command.arg("-C").arg("link-arg=-isysroot");
            command.arg("-C").arg(format!("link-arg={sdk_root}"));
        }
    }
    command.args(render_rustc_link_args(options));
    command
}

fn render_rustc_link_args(options: &BuildOptions) -> Vec<String> {
    let mut args = Vec::new();
    for path in &options.link_search_paths {
        args.push("-L".to_string());
        args.push(format!("native={}", path.display()));
    }
    for lib in &options.link_libs {
        args.push("-l".to_string());
        args.push(lib.clone());
    }
    for arg in &options.link_args {
        args.push("-C".to_string());
        args.push(format!("link-arg={arg}"));
    }
    args
}

fn compile_c_sources(
    temp_dir: &Path,
    target_spec: &TargetSpec,
    sources: &[PathBuf],
) -> Result<Vec<PathBuf>, BuildError> {
    if sources.is_empty() {
        return Ok(Vec::new());
    }
    let clang = find_packaged_llvm_tool("clang").ok_or_else(|| {
        BuildError::ToolNotFound("clang (expected in packaged LLVM toolchain)".into())
    })?;
    let mut objects = Vec::new();
    for (index, source) in sources.iter().enumerate() {
        let ext = if target_spec.platform == TargetPlatform::Windows {
            "obj"
        } else {
            "o"
        };
        let obj_path = temp_dir.join(format!("c_source_{index}.{ext}"));
        let mut command = Command::new(&clang);
        command
            .arg(format!("--target={}", target_spec.triple))
            .arg("-c")
            .arg(source)
            .arg("-o")
            .arg(&obj_path);
        if target_spec.platform == TargetPlatform::Windows
            && let Some(assets) = crate::toolchain::detect_windows_dev_assets()
        {
            command.arg(format!("-I{}", assets.msvc_include.display()));
            command.arg(format!("-I{}", assets.sdk_include_ucrt.display()));
            command.arg(format!("-I{}", assets.sdk_include_um.display()));
        }
        if target_spec.platform == TargetPlatform::MacOS
            && let Some(sdk_root) = target_sdk_root_env(target_spec)
        {
            command.arg("-isysroot").arg(sdk_root);
        }
        let status = command.output()
            .map_err(|source_error| BuildError::Io {
                context: format!("failed to run `{}`", clang.display()),
                source: source_error,
            })?;
        if !status.status.success() {
            return Err(BuildError::ToolFailed {
                tool: clang.display().to_string(),
                status: status.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&status.stderr).into_owned(),
            });
        }
        objects.push(obj_path);
    }
    Ok(objects)
}

fn link_with_packaged_clang(
    target_spec: &TargetSpec,
    objects: &[PathBuf],
    output_path: &Path,
    shared_library: bool,
    options: &BuildOptions,
) -> Result<(), BuildError> {
    let clang = find_packaged_llvm_tool("clang").ok_or_else(|| {
        BuildError::ToolNotFound("clang (expected in packaged LLVM toolchain)".into())
    })?;
    let mut command = Command::new(&clang);
    command
        .arg(format!("--target={}", target_spec.triple))
        .arg("-fuse-ld=lld")
        .arg("-O3");
    if shared_library {
        command.arg("-shared");
    }
    if target_spec.platform == TargetPlatform::MacOS
        && let Some(sdk_root) = target_sdk_root_env(target_spec)
    {
        command.arg("-isysroot").arg(sdk_root);
    }
    for object in objects {
        command.arg(object);
    }
    for path in &options.link_search_paths {
        command.arg("-L").arg(path);
    }
    for lib in &options.link_libs {
        command.arg(format!("-l{lib}"));
    }
    for arg in &options.link_args {
        command.arg(arg);
    }
    command.arg("-o").arg(output_path);

    let status = command.output().map_err(|source| BuildError::Io {
        context: format!("failed to run `{}`", clang.display()),
        source,
    })?;
    if !status.status.success() {
        return Err(BuildError::ToolFailed {
            tool: clang.display().to_string(),
            status: status.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&status.stderr).into_owned(),
        });
    }
    Ok(())
}

fn target_linker_env(target: &TargetSpec) -> Option<String> {
    let specific = format!("RUNE_LINKER_{}", sanitize_target_env_key(target.triple));
    env::var(&specific)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env::var("RUNE_LINKER")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
}

fn target_sdk_root_env(target: &TargetSpec) -> Option<String> {
    if !target.needs_macos_sdk {
        return None;
    }
    let specific = format!("RUNE_SDKROOT_{}", sanitize_target_env_key(target.triple));
    env::var(&specific)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env::var("SDKROOT")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
}

fn sanitize_target_env_key(target: &str) -> String {
    target
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn host_native_target_triple() -> Option<&'static str> {
    match (env::consts::OS, env::consts::ARCH) {
        ("windows", "x86_64") => Some("x86_64-pc-windows-gnu"),
        ("windows", "aarch64") => Some("aarch64-pc-windows-gnu"),
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-gnu"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        _ => None,
    }
}

fn rename_entry_symbol(asm: &str) -> String {
    asm.replace(".globl main", ".globl rune_entry_main")
        .replace("\nmain:\n", "\nrune_entry_main:\n")
        .replace("main.return", "rune_entry_main.return")
}

fn rename_functions_for_library(program: &Program, asm: &str) -> String {
    let mut renamed = asm.to_string();
    for item in &program.items {
        let Item::Function(function) = item else {
            continue;
        };
        let from = crate::codegen::native_internal_symbol_name(&function.name);
        let to = format!("rune_export_internal_{}", function.name);
        renamed = renamed.replace(&format!(".globl {from}"), &format!(".globl {to}"));
        renamed = renamed.replace(&format!("\n{from}:\n"), &format!("\n{to}:\n"));
        renamed = renamed.replace(&format!("{from}.return"), &format!("{to}.return"));
        renamed = renamed.replace(&format!("call {from}\n"), &format!("call {to}\n"));
    }
    renamed
}

fn rust_runtime_support_body() -> &'static str {
    r#"use std::cell::{Cell, RefCell};
use std::env;
use std::fs;
use std::io::{self, Write};
#[cfg(not(target_os = "wasi"))]
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::OnceLock;
use std::thread_local;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

thread_local! {
    static RUNE_OWNED_STRINGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static RUNE_LAST_STRING_LEN: Cell<i64> = const { Cell::new(0) };
}

fn rune_rt_store_string(value: String) -> *const u8 {
    let len = value.len();
    RUNE_OWNED_STRINGS.with(|strings| {
        let mut strings = strings.borrow_mut();
        strings.push(value);
        let ptr = strings
            .last()
            .expect("just pushed string should exist")
            .as_ptr();
        RUNE_LAST_STRING_LEN.with(|last_len| last_len.set(len as i64));
        ptr
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_print_i64(value: i64) {
    print!("{value}");
    let _ = io::stdout().flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_eprint_i64(value: i64) {
    eprint!("{value}");
    let _ = io::stderr().flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_print_str(ptr: *const u8, len: i64) {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let text = std::str::from_utf8(bytes).expect("Rune string literals must be valid UTF-8");
    print!("{text}");
    let _ = io::stdout().flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_eprint_str(ptr: *const u8, len: i64) {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let text = std::str::from_utf8(bytes).expect("Rune string literals must be valid UTF-8");
    eprint!("{text}");
    let _ = io::stderr().flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_print_newline() {
    let _ = io::stdout().write_all(b"\n");
    let _ = io::stdout().flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_eprint_newline() {
    let _ = io::stderr().write_all(b"\n");
    let _ = io::stderr().flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_flush_stdout() {
    let _ = io::stdout().flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_flush_stderr() {
    let _ = io::stderr().flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_input_line() -> *const u8 {
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .expect("failed to read Rune input line");
    while matches!(line.as_bytes().last(), Some(b'\n' | b'\r')) {
        line.pop();
    }
    rune_rt_store_string(line)
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_panic(
    msg_ptr: *const u8,
    msg_len: i64,
    ctx_ptr: *const u8,
    ctx_len: i64,
) {
    let msg = unsafe { std::slice::from_raw_parts(msg_ptr, msg_len as usize) };
    let msg = std::str::from_utf8(msg).expect("Rune panic messages must be valid UTF-8");
    let ctx = unsafe { std::slice::from_raw_parts(ctx_ptr, ctx_len as usize) };
    let ctx = std::str::from_utf8(ctx).expect("Rune panic context must be valid UTF-8");
    eprintln!("Rune panic: {msg}");
    eprintln!("  {ctx}");
    std::process::exit(101);
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_raise(
    msg_ptr: *const u8,
    msg_len: i64,
    meta_ptr: *const u8,
    meta_len: i64,
) {
    let msg = unsafe { std::slice::from_raw_parts(msg_ptr, msg_len as usize) };
    let msg = std::str::from_utf8(msg).expect("Rune raise messages must be valid UTF-8");
    let meta = unsafe { std::slice::from_raw_parts(meta_ptr, meta_len as usize) };
    let meta = std::str::from_utf8(meta).expect("Rune raise metadata must be valid UTF-8");
    eprintln!("Rune exception: {meta}: {msg}");
    std::process::exit(102);
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_print_dynamic(tag: i64, payload: i64, extra: i64) {
    match tag {
        0 => print!("unit"),
        1 => print!("{}", payload != 0),
        2 => print!("{}", payload as i32),
        3 => print!("{payload}"),
        4 => {
            let ptr = payload as *const u8;
            let len = extra as usize;
            let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
            let text = std::str::from_utf8(bytes).expect("Rune dynamic strings must be valid UTF-8");
            print!("{text}");
        }
        _ => print!("<dynamic:{tag}>"),
    }
    let _ = io::stdout().flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_eprint_dynamic(tag: i64, payload: i64, extra: i64) {
    match tag {
        0 => eprint!("unit"),
        1 => eprint!("{}", payload != 0),
        2 => eprint!("{}", payload as i32),
        3 => eprint!("{payload}"),
        4 => {
            let ptr = payload as *const u8;
            let len = extra as usize;
            let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
            let text = std::str::from_utf8(bytes).expect("Rune dynamic strings must be valid UTF-8");
            eprint!("{text}");
        }
        _ => eprint!("<dynamic:{tag}>"),
    }
    let _ = io::stderr().flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_last_string_len() -> i64 {
    RUNE_LAST_STRING_LEN.with(Cell::get)
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_string_concat(
    left_ptr: *const u8,
    left_len: i64,
    right_ptr: *const u8,
    right_len: i64,
) -> *const u8 {
    let left = unsafe { std::slice::from_raw_parts(left_ptr, left_len as usize) };
    let right = unsafe { std::slice::from_raw_parts(right_ptr, right_len as usize) };
    let left = std::str::from_utf8(left).expect("left Rune string must be valid UTF-8");
    let right = std::str::from_utf8(right).expect("right Rune string must be valid UTF-8");
    rune_rt_store_string(format!("{left}{right}"))
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_string_from_i64(value: i64) -> *const u8 {
    rune_rt_store_string(value.to_string())
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_string_from_bool(value: bool) -> *const u8 {
    rune_rt_store_string(value.to_string())
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_to_c_string(ptr: *const u8, len: i64) -> *const u8 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let mut owned = bytes.to_vec();
    owned.push(0);
    let text = String::from_utf8_lossy(bytes).to_string();
    RUNE_OWNED_STRINGS.with(|strings| {
        let mut strings = strings.borrow_mut();
        strings.push(text);
    });
    Box::leak(owned.into_boxed_slice()).as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_from_c_string(ptr: *const u8) -> *const u8 {
    if ptr.is_null() {
        return rune_rt_store_string(String::new());
    }
    let mut len = 0usize;
    unsafe {
        while *ptr.add(len) != 0 {
            len += 1;
        }
        let bytes = std::slice::from_raw_parts(ptr, len);
        let text = std::str::from_utf8(bytes)
            .expect("C strings crossing into Rune must be valid UTF-8");
        rune_rt_store_string(text.to_string())
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_dynamic_to_string(tag: i64, payload: i64, extra: i64) -> *const u8 {
    match tag {
        0 => rune_rt_store_string("unit".to_string()),
        1 => rune_rt_store_string((payload != 0).to_string()),
        2 => rune_rt_store_string((payload as i32).to_string()),
        3 => rune_rt_store_string(payload.to_string()),
        4 => {
            let ptr = payload as *const u8;
            let len = extra as usize;
            let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
            let text = std::str::from_utf8(bytes).expect("Rune dynamic strings must be valid UTF-8");
            rune_rt_store_string(text.to_string())
        }
        _ => rune_rt_store_string(format!("<dynamic:{tag}>")),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_string_to_i64(ptr: *const u8, len: i64) -> i64 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let text = std::str::from_utf8(bytes).expect("Rune string conversion input must be valid UTF-8");
    text.trim()
        .parse::<i64>()
        .unwrap_or_else(|_| panic!("failed to convert Rune string `{text}` to i64"))
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_dynamic_to_i64(tag: i64, payload: i64, extra: i64) -> i64 {
    match tag {
        0 => 0,
        1 => (payload != 0) as i64,
        2 => payload as i32 as i64,
        3 => payload,
        4 => rune_rt_string_to_i64(payload as *const u8, extra),
        _ => panic!("failed to convert dynamic Rune value with tag {tag} to i64"),
    }
}

fn rune_rt_dynamic_value_to_string(tag: i64, payload: i64, extra: i64) -> String {
    match tag {
        0 => "unit".to_string(),
        1 => (payload != 0).to_string(),
        2 => (payload as i32).to_string(),
        3 => payload.to_string(),
        4 => {
            let ptr = payload as *const u8;
            let len = extra as usize;
            let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
            let text = std::str::from_utf8(bytes).expect("Rune dynamic strings must be valid UTF-8");
            text.to_string()
        }
        _ => format!("<dynamic:{tag}>"),
    }
}

fn rune_rt_dynamic_value_to_i64_lossy(tag: i64, payload: i64, _extra: i64) -> Option<i64> {
    match tag {
        1 => Some((payload != 0) as i64),
        2 => Some(payload as i32 as i64),
        3 => Some(payload),
        _ => None,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_dynamic_binary(left: *const i64, right: *const i64, out: *mut i64, op: i64) {
    let left = unsafe { std::slice::from_raw_parts(left, 3) };
    let right = unsafe { std::slice::from_raw_parts(right, 3) };
    let out = unsafe { std::slice::from_raw_parts_mut(out, 3) };

    let left_tag = left[0];
    let left_payload = left[1];
    let left_extra = left[2];
    let right_tag = right[0];
    let right_payload = right[1];
    let right_extra = right[2];

    if op == 0 && (left_tag == 4 || right_tag == 4) {
        let ptr = rune_rt_store_string(format!(
            "{}{}",
            rune_rt_dynamic_value_to_string(left_tag, left_payload, left_extra),
            rune_rt_dynamic_value_to_string(right_tag, right_payload, right_extra)
        ));
        out[0] = 4;
        out[1] = ptr as i64;
        out[2] = rune_rt_last_string_len() as i64;
        return;
    }

    let Some(left_number) = rune_rt_dynamic_value_to_i64_lossy(left_tag, left_payload, left_extra) else {
        panic!("dynamic binary op does not support left operand tag {left_tag}");
    };
    let Some(right_number) = rune_rt_dynamic_value_to_i64_lossy(right_tag, right_payload, right_extra) else {
        panic!("dynamic binary op does not support right operand tag {right_tag}");
    };

    out[0] = 3;
    out[1] = match op {
        0 => left_number + right_number,
        1 => left_number - right_number,
        2 => left_number * right_number,
        3 => {
            if right_number == 0 {
                panic!("dynamic division by zero");
            }
            left_number / right_number
        }
        4 => {
            if right_number == 0 {
                panic!("dynamic modulo by zero");
            }
            left_number % right_number
        }
        _ => panic!("unknown dynamic binary opcode {op}"),
    };
    out[2] = 0;
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_dynamic_compare(left: *const i64, right: *const i64, op: i64) -> bool {
    let left = unsafe { std::slice::from_raw_parts(left, 3) };
    let right = unsafe { std::slice::from_raw_parts(right, 3) };

    let left_tag = left[0];
    let left_payload = left[1];
    let left_extra = left[2];
    let right_tag = right[0];
    let right_payload = right[1];
    let right_extra = right[2];

    match op {
        0 | 1 => {
            let equal = if left_tag == 4 || right_tag == 4 {
                rune_rt_dynamic_value_to_string(left_tag, left_payload, left_extra)
                    == rune_rt_dynamic_value_to_string(right_tag, right_payload, right_extra)
            } else {
                match (
                    rune_rt_dynamic_value_to_i64_lossy(left_tag, left_payload, left_extra),
                    rune_rt_dynamic_value_to_i64_lossy(right_tag, right_payload, right_extra),
                ) {
                    (Some(left_number), Some(right_number)) => left_number == right_number,
                    _ => left_tag == right_tag
                        && left_payload == right_payload
                        && left_extra == right_extra,
                }
            };
            if op == 0 { equal } else { !equal }
        }
        2 | 3 | 4 | 5 => {
            let Some(left_number) = rune_rt_dynamic_value_to_i64_lossy(left_tag, left_payload, left_extra) else {
                panic!("dynamic ordering does not support left operand tag {left_tag}");
            };
            let Some(right_number) = rune_rt_dynamic_value_to_i64_lossy(right_tag, right_payload, right_extra) else {
                panic!("dynamic ordering does not support right operand tag {right_tag}");
            };
            match op {
                2 => left_number > right_number,
                3 => left_number >= right_number,
                4 => left_number < right_number,
                5 => left_number <= right_number,
                _ => unreachable!(),
            }
        }
        _ => panic!("unknown dynamic comparison opcode {op}"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_dynamic_truthy(tag: i64, payload: i64, extra: i64) -> bool {
    match tag {
        0 => false,
        1 => payload != 0,
        2 => payload as i32 != 0,
        3 => payload != 0,
        4 => extra != 0,
        _ => panic!("unknown dynamic truthiness tag {tag}"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_time_now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_secs() as i64
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_time_monotonic_ms() -> i64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START
        .get_or_init(Instant::now)
        .elapsed()
        .as_millis() as i64
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_time_sleep_ms(ms: i64) {
    if ms > 0 {
        std::thread::sleep(Duration::from_millis(ms as u64));
    }
}

#[cfg(target_os = "wasi")]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_system_pid() -> i32 {
    1
}

#[cfg(not(target_os = "wasi"))]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_system_pid() -> i32 {
    std::process::id() as i32
}

#[cfg(target_os = "wasi")]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_system_cpu_count() -> i32 {
    1
}

#[cfg(not(target_os = "wasi"))]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_system_cpu_count() -> i32 {
    std::thread::available_parallelism()
        .map(|count| count.get() as i32)
        .unwrap_or(1)
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_system_exit(code: i32) {
    std::process::exit(code);
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_env_exists(ptr: *const u8, len: i64) -> bool {
    let key = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let key = std::str::from_utf8(key).expect("environment variable name must be valid UTF-8");
    env::var_os(key).is_some()
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_env_get_i32(ptr: *const u8, len: i64, default: i32) -> i32 {
    let key = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let key = std::str::from_utf8(key).expect("environment variable name must be valid UTF-8");
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(default)
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_env_get_bool(ptr: *const u8, len: i64, default: bool) -> bool {
    let key = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let key = std::str::from_utf8(key).expect("environment variable name must be valid UTF-8");
    match env::var(key) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            _ => default,
        },
        Err(_) => default,
    }
}

#[cfg(target_os = "wasi")]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_env_arg_count() -> i32 {
    env::args().skip(1).count() as i32
}

#[cfg(not(target_os = "wasi"))]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_env_arg_count() -> i32 {
    env::args().count() as i32
}

#[cfg(target_os = "wasi")]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_connect(_ptr: *const u8, _len: i64, _port: i32) -> bool {
    false
}

#[cfg(target_os = "wasi")]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_connect_timeout(_ptr: *const u8, _len: i64, _port: i32, _timeout_ms: i32) -> bool {
    false
}

#[cfg(not(target_os = "wasi"))]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_connect(ptr: *const u8, len: i64, port: i32) -> bool {
    rune_rt_network_tcp_connect_timeout(ptr, len, port, 250)
}

#[cfg(not(target_os = "wasi"))]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_connect_timeout(ptr: *const u8, len: i64, port: i32, timeout_ms: i32) -> bool {
    if port < 0 || port > u16::MAX as i32 {
        return false;
    }
    if timeout_ms < 0 {
        return false;
    }
    let host = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let host = std::str::from_utf8(host).expect("TCP host must be valid UTF-8");
    let address = format!("{host}:{}", port as u16);
    let resolved = match address.to_socket_addrs() {
        Ok(addrs) => addrs.collect::<Vec<SocketAddr>>(),
        Err(_) => return false,
    };
    resolved.into_iter().any(|addr| {
        TcpStream::connect_timeout(&addr, Duration::from_millis(timeout_ms as u64)).is_ok()
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_fs_exists(ptr: *const u8, len: i64) -> bool {
    let path = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let path = std::str::from_utf8(path).expect("filesystem path must be valid UTF-8");
    fs::metadata(path).is_ok()
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_fs_read_string(ptr: *const u8, len: i64) -> *const u8 {
    let path = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let path = std::str::from_utf8(path).expect("filesystem path must be valid UTF-8");
    let text = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("failed to read Rune file `{path}`: {error}"));
    rune_rt_store_string(text)
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_fs_write_string(
    path_ptr: *const u8,
    path_len: i64,
    content_ptr: *const u8,
    content_len: i64,
) -> bool {
    let path = unsafe { std::slice::from_raw_parts(path_ptr, path_len as usize) };
    let path = std::str::from_utf8(path).expect("filesystem path must be valid UTF-8");
    let content = unsafe { std::slice::from_raw_parts(content_ptr, content_len as usize) };
    let content = std::str::from_utf8(content).expect("filesystem content must be valid UTF-8");
    fs::write(path, content).is_ok()
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_terminal_clear() {
    let _ = io::stdout().write_all(b"\x1b[2J\x1b[H");
    let _ = io::stdout().flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_terminal_move_to(row: i32, col: i32) {
    let row = row.max(1);
    let col = col.max(1);
    let _ = write!(io::stdout(), "\x1b[{row};{col}H");
    let _ = io::stdout().flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_terminal_hide_cursor() {
    let _ = io::stdout().write_all(b"\x1b[?25l");
    let _ = io::stdout().flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_terminal_show_cursor() {
    let _ = io::stdout().write_all(b"\x1b[?25h");
    let _ = io::stdout().flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_terminal_set_title(ptr: *const u8, len: i64) {
    let title = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let title = std::str::from_utf8(title).expect("terminal title must be valid UTF-8");
    let _ = write!(io::stdout(), "\x1b]0;{title}\x07");
    let _ = io::stdout().flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_audio_bell() -> bool {
    io::stdout().write_all(b"\x07").is_ok() && io::stdout().flush().is_ok()
}
"#
}

fn rust_exe_wrapper_source() -> String {
    format!(
        "use std::arch::global_asm;\nglobal_asm!(include_str!(\"out.s\"));\n{}\nunsafe extern \"C\" {{\n    fn rune_entry_main() -> i64;\n}}\n\nfn main() {{\n    let code = unsafe {{ rune_entry_main() }} as i32;\n    std::process::exit(code);\n}}\n",
        rust_runtime_support_body()
    )
}

fn rust_llvm_exe_wrapper_source() -> String {
    format!(
        "{}\nunsafe extern \"C\" {{\n    fn rune_entry_main() -> i64;\n}}\n\nfn main() {{\n    let code = unsafe {{ rune_entry_main() }} as i32;\n    std::process::exit(code);\n}}\n",
        rust_runtime_support_body()
    )
}

fn rust_wasi_wrapper_source() -> String {
    format!(
        "{}\nunsafe extern \"C\" {{\n    fn rune_wasi_main() -> i32;\n}}\n\nfn main() {{\n    let code = unsafe {{ rune_wasi_main() }};\n    std::process::exit(code);\n}}\n",
        rust_runtime_support_body()
    )
}

fn rust_cdylib_wrapper_source(program: &Program) -> String {
    let mut out = String::new();
    out.push_str("use std::arch::global_asm;\nglobal_asm!(include_str!(\"out.s\"));\n");
    out.push_str(rust_runtime_support_body());
    out.push('\n');

    for item in &program.items {
        let Item::Function(function) = item else {
            continue;
        };
        let params = function
            .params
            .iter()
            .map(|param| format!("{}: {}", param.name, map_ffi_type(&param.ty)))
            .collect::<Vec<_>>();
        let arg_names = function
            .params
            .iter()
            .map(|param| param.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let ret = match &function.return_type {
            Some(ty) => map_ffi_type(ty),
            None => "()".to_string(),
        };
        let internal_name = format!("rune_export_internal_{}", function.name);

        out.push_str("unsafe extern \"C\" {\n");
        out.push_str(&format!(
            "    fn {internal_name}({}) -> {ret};\n",
            params.join(", ")
        ));
        out.push_str("}\n\n");
        out.push_str("#[unsafe(no_mangle)]\n");
        out.push_str(&format!(
            "pub extern \"C\" fn {}({}) -> {ret} {{\n",
            function.name,
            params.join(", ")
        ));
        if ret == "()" {
            out.push_str(&format!("    unsafe {{ {internal_name}({arg_names}) }};\n"));
        } else {
            out.push_str(&format!("    unsafe {{ {internal_name}({arg_names}) }}\n"));
        }
        out.push_str("}\n\n");
    }

    out
}

fn validate_ffi_signatures(program: &Program) -> Result<(), BuildError> {
    for item in &program.items {
        let Item::Function(function) = item else {
            continue;
        };
        for param in &function.params {
            if !is_supported_ffi_type(&param.ty) {
                return Err(BuildError::UnsupportedFfiSignature(format!(
                    "FFI export currently supports only i32, i64, bool, and unit types, found `{}` in parameter `{}` of function `{}`",
                    param.ty.name, param.name, function.name
                )));
            }
        }
        if let Some(ret) = &function.return_type
            && !is_supported_ffi_type(ret)
        {
            return Err(BuildError::UnsupportedFfiSignature(format!(
                "FFI export currently supports only i32, i64, bool, and unit types, found `{}` in return type of function `{}`",
                ret.name, function.name
            )));
        }
    }
    Ok(())
}

fn map_ffi_type(ty: &TypeRef) -> String {
    match ty.name.as_str() {
        "i32" => "i32".to_string(),
        "i64" => "i64".to_string(),
        "bool" => "bool".to_string(),
        "unit" => "()".to_string(),
        _ => unreachable!("unsupported FFI types should be rejected before wrapper generation"),
    }
}

fn is_supported_ffi_type(ty: &TypeRef) -> bool {
    matches!(ty.name.as_str(), "i32" | "i64" | "bool" | "unit")
}

fn map_ffi_c_type(ty: &TypeRef) -> String {
    match ty.name.as_str() {
        "i32" => "int32_t".to_string(),
        "i64" => "int64_t".to_string(),
        "bool" => "bool".to_string(),
        "unit" => "void".to_string(),
        _ => unreachable!("unsupported FFI types should be rejected before header generation"),
    }
}

fn write_c_header_for_library(program: &Program, library_path: &Path) -> Result<(), BuildError> {
    let header_path = library_path.with_extension("h");
    write_c_header(program, &header_path)
}

fn write_c_header(program: &Program, header_path: &Path) -> Result<(), BuildError> {
    let header = generate_c_header(program, header_path);
    fs::write(header_path, header).map_err(|source| BuildError::Io {
        context: format!("failed to write `{}`", header_path.display()),
        source,
    })?;
    Ok(())
}

fn generate_c_header(program: &Program, library_path: &Path) -> String {
    let stem = library_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("runeffi");
    let guard = format!(
        "RUNE_{}_H",
        stem.chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() {
                    ch.to_ascii_uppercase()
                } else {
                    '_'
                }
            })
            .collect::<String>()
    );

    let mut out = String::new();
    out.push_str(&format!("#ifndef {guard}\n#define {guard}\n\n"));
    out.push_str("#include <stdbool.h>\n#include <stdint.h>\n\n");
    out.push_str("#ifdef __cplusplus\nextern \"C\" {\n#endif\n\n");

    for item in &program.items {
        let Item::Function(function) = item else {
            continue;
        };
        let ret = match &function.return_type {
            Some(ty) => map_ffi_c_type(ty),
            None => "void".to_string(),
        };
        let params = if function.params.is_empty() {
            "void".to_string()
        } else {
            function
                .params
                .iter()
                .map(|param| format!("{} {}", map_ffi_c_type(&param.ty), param.name))
                .collect::<Vec<_>>()
                .join(", ")
        };
        out.push_str(&format!("{ret} {}({params});\n", function.name));
    }

    out.push_str("\n#ifdef __cplusplus\n}\n#endif\n\n");
    out.push_str(&format!("#endif /* {guard} */\n"));
    out
}

fn write_wrapper_files(
    temp_dir: &Path,
    asm: &str,
    wrapper_source: &str,
) -> Result<PathBuf, BuildError> {
    let asm_path = temp_dir.join("out.s");
    let wrapper_path = temp_dir.join("wrapper.rs");

    fs::write(&asm_path, asm).map_err(|source| BuildError::Io {
        context: format!("failed to write `{}`", asm_path.display()),
        source,
    })?;
    fs::write(&wrapper_path, wrapper_source).map_err(|source| BuildError::Io {
        context: format!("failed to write `{}`", wrapper_path.display()),
        source,
    })?;

    Ok(wrapper_path)
}

fn create_temp_dir() -> Result<PathBuf, BuildError> {
    let base = env::temp_dir();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let dir = base.join(format!("rune-build-{stamp}"));
    fs::create_dir_all(&dir).map_err(|source| BuildError::Io {
        context: format!("failed to create temporary directory `{}`", dir.display()),
        source,
    })?;
    Ok(dir)
}

pub fn default_library_extension() -> &'static str {
    if cfg!(target_os = "windows") {
        "dll"
    } else if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    }
}
