use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use std::collections::HashMap;

use crate::codegen::CodegenError;
use crate::llvm_backend::{emit_object_file, emit_object_file_from_ir};
use crate::llvm_ir::emit_llvm_ir;
use crate::module_loader::load_program_from_path;
use crate::optimize::{optimize_program, prune_program_for_executable};
use crate::parser::{BinaryOp, CallArg, Expr, ExprKind, Item, Program, Stmt, TypeRef, UnaryOp};
use crate::semantic::check_program;
use crate::toolchain::{
    find_arduino_avr_avrdude_conf, find_arduino_avrdude, find_arduino_avr_core_root,
    find_arduino_avr_gcc, find_arduino_avr_gpp, find_arduino_avr_objcopy,
    find_arduino_avr_runtime_header, find_arduino_avr_servo_library_root,
    find_packaged_llvm_cbe, find_packaged_llvm_tool, find_packaged_wasm_ld,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildOptions {
    pub link_search_paths: Vec<PathBuf>,
    pub link_libs: Vec<String>,
    pub link_args: Vec<String>,
    pub link_c_sources: Vec<PathBuf>,
    pub flash_after_build: bool,
    pub flash_port: Option<String>,
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
    Embedded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetSpec {
    pub triple: &'static str,
    pub platform: TargetPlatform,
    pub exe_extension: &'static str,
    pub library_extension: &'static str,
    pub static_library_extension: &'static str,
    pub object_extension: &'static str,
    pub needs_macos_sdk: bool,
}

const KNOWN_TARGETS: &[TargetSpec] = &[
    TargetSpec {
        triple: "x86_64-pc-windows-gnu",
        platform: TargetPlatform::Windows,
        exe_extension: "exe",
        library_extension: "dll",
        static_library_extension: "lib",
        object_extension: "obj",
        needs_macos_sdk: false,
    },
    TargetSpec {
        triple: "x86_64-pc-windows-msvc",
        platform: TargetPlatform::Windows,
        exe_extension: "exe",
        library_extension: "dll",
        static_library_extension: "lib",
        object_extension: "obj",
        needs_macos_sdk: false,
    },
    TargetSpec {
        triple: "aarch64-pc-windows-gnu",
        platform: TargetPlatform::Windows,
        exe_extension: "exe",
        library_extension: "dll",
        static_library_extension: "lib",
        object_extension: "obj",
        needs_macos_sdk: false,
    },
    TargetSpec {
        triple: "x86_64-unknown-linux-gnu",
        platform: TargetPlatform::Linux,
        exe_extension: "",
        library_extension: "so",
        static_library_extension: "a",
        object_extension: "o",
        needs_macos_sdk: false,
    },
    TargetSpec {
        triple: "aarch64-unknown-linux-gnu",
        platform: TargetPlatform::Linux,
        exe_extension: "",
        library_extension: "so",
        static_library_extension: "a",
        object_extension: "o",
        needs_macos_sdk: false,
    },
    TargetSpec {
        triple: "x86_64-apple-darwin",
        platform: TargetPlatform::MacOS,
        exe_extension: "",
        library_extension: "dylib",
        static_library_extension: "a",
        object_extension: "o",
        needs_macos_sdk: true,
    },
    TargetSpec {
        triple: "aarch64-apple-darwin",
        platform: TargetPlatform::MacOS,
        exe_extension: "",
        library_extension: "dylib",
        static_library_extension: "a",
        object_extension: "o",
        needs_macos_sdk: true,
    },
    TargetSpec {
        triple: "wasm32-unknown-unknown",
        platform: TargetPlatform::Wasm,
        exe_extension: "wasm",
        library_extension: "wasm",
        static_library_extension: "a",
        object_extension: "o",
        needs_macos_sdk: false,
    },
    TargetSpec {
        triple: "wasm32-wasip1",
        platform: TargetPlatform::Wasm,
        exe_extension: "wasm",
        library_extension: "wasm",
        static_library_extension: "a",
        object_extension: "o",
        needs_macos_sdk: false,
    },
    TargetSpec {
        triple: "avr-atmega328p-arduino-uno",
        platform: TargetPlatform::Embedded,
        exe_extension: "hex",
        library_extension: "a",
        static_library_extension: "a",
        object_extension: "o",
        needs_macos_sdk: false,
    },
    TargetSpec {
        triple: "thumbv6m-none-eabi",
        platform: TargetPlatform::Embedded,
        exe_extension: "",
        library_extension: "a",
        static_library_extension: "a",
        object_extension: "o",
        needs_macos_sdk: false,
    },
    TargetSpec {
        triple: "thumbv7em-none-eabihf",
        platform: TargetPlatform::Embedded,
        exe_extension: "",
        library_extension: "a",
        static_library_extension: "a",
        object_extension: "o",
        needs_macos_sdk: false,
    },
    TargetSpec {
        triple: "riscv32-unknown-elf",
        platform: TargetPlatform::Embedded,
        exe_extension: "",
        library_extension: "a",
        static_library_extension: "a",
        object_extension: "o",
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

pub fn build_object_file(
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

    if let Some(parent) = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|source| BuildError::Io {
            context: format!("failed to create `{}`", parent.display()),
            source,
        })?;
    }

    emit_object_file(&program, target_spec.triple, output_path)
        .map(|_| ())
        .map_err(|error| {
        BuildError::Codegen(CodegenError {
            message: error.message,
            span: crate::lexer::Span { line: 1, column: 1 },
        })
    })
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

pub fn emit_avr_precode(source_path: &Path, target: Option<&str>) -> Result<String, BuildError> {
    let target_spec = target_spec(target.or(Some("avr-atmega328p-arduino-uno")))?;
    if target_spec.triple != "avr-atmega328p-arduino-uno" {
        return Err(BuildError::UnsupportedTarget(format!(
            "`emit-avr-precode` currently supports only `avr-atmega328p-arduino-uno`, found `{}`",
            target_spec.triple
        )));
    }

    let mut program = load_program_from_path(source_path)
        .map_err(|error| BuildError::ModuleLoad(error.to_string()))?;
    check_program(&program).map_err(|error| {
        BuildError::Codegen(CodegenError {
            message: error.message,
            span: error.span,
        })
    })?;
    optimize_program(&mut program);
    prune_program_for_executable(&mut program);

    emit_arduino_uno_precode(&program)
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
    prune_program_for_executable(&mut program);
    if target_spec.triple == "avr-atmega328p-arduino-uno" {
        return build_arduino_uno_hex(&program, output_path, options);
    }
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

fn build_arduino_uno_hex(
    program: &Program,
    output_path: &Path,
    options: &BuildOptions,
) -> Result<(), BuildError> {
    if let Some(llvm_cbe) = find_packaged_llvm_cbe() {
        return build_arduino_uno_hex_via_llvm_cbe(program, output_path, options, &llvm_cbe);
    }

    let cpp_source = emit_arduino_uno_cpp(program).map_err(BuildError::Codegen)?;
    let gpp = find_arduino_avr_gpp().ok_or_else(|| {
        BuildError::ToolNotFound("packaged Arduino AVR g++ toolchain not found".into())
    })?;
    let gcc = find_arduino_avr_gcc().ok_or_else(|| {
        BuildError::ToolNotFound("packaged Arduino AVR gcc toolchain not found".into())
    })?;
    let objcopy = find_arduino_avr_objcopy().ok_or_else(|| {
        BuildError::ToolNotFound("packaged Arduino AVR objcopy not found".into())
    })?;
    let core_root = find_arduino_avr_core_root().ok_or_else(|| {
        BuildError::ToolNotFound("packaged Arduino AVR core sources not found".into())
    })?;
    let temp_dir = create_temp_dir()?;
    let cpp_path = temp_dir.join("rune_arduino_uno.cpp");
    let sketch_obj = temp_dir.join("rune_arduino_uno.o");
    let elf_path = temp_dir.join("rune_arduino_uno.elf");
    fs::write(&cpp_path, cpp_source).map_err(|source| BuildError::Io {
        context: format!("failed to write `{}`", cpp_path.display()),
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

    let core_dir = core_root.join("cores").join("arduino");
    let variant_dir = core_root.join("variants").join("standard");
    let servo_dir = find_arduino_avr_servo_library_root();
    let include_servo_sources = program_uses_arduino_servo(program);
    let servo_include_dir = if include_servo_sources {
        servo_dir.as_deref()
    } else {
        None
    };
    let common_args =
        arduino_uno_common_compile_args(&core_dir, &variant_dir, servo_include_dir);

    let sketch_compile = Command::new(&gpp)
        .args(&common_args)
        .arg("-std=gnu++11")
        .arg("-fno-exceptions")
        .arg("-fno-threadsafe-statics")
        .arg("-c")
        .arg(&cpp_path)
        .arg("-o")
        .arg(&sketch_obj)
        .output()
        .map_err(|source| BuildError::Io {
            context: format!("failed to run `{}`", gpp.display()),
            source,
        })?;
    if !sketch_compile.status.success() {
        return Err(BuildError::ToolFailed {
            tool: gpp.display().to_string(),
            status: sketch_compile.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&sketch_compile.stderr).into_owned(),
        });
    }

    let mut objects = vec![sketch_obj.clone()];
    objects.extend(compile_arduino_uno_core_sources(
        &temp_dir,
        &gcc,
        &gpp,
        &core_dir,
        &variant_dir,
        servo_include_dir,
        include_servo_sources,
    )?);

    let mut link = Command::new(&gpp);
    link.arg("-mmcu=atmega328p")
        .arg("-Os")
        .arg("-flto")
        .arg("-Wl,--gc-sections")
        .arg("-o")
        .arg(&elf_path);
    for object in &objects {
        link.arg(object);
    }
    let link = link.output().map_err(|source| BuildError::Io {
        context: format!("failed to run `{}`", gpp.display()),
        source,
    })?;
    if !link.status.success() {
        return Err(BuildError::ToolFailed {
            tool: gpp.display().to_string(),
            status: link.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&link.stderr).into_owned(),
        });
    }

    let hex = Command::new(&objcopy)
        .arg("-O")
        .arg("ihex")
        .arg("-R")
        .arg(".eeprom")
        .arg(&elf_path)
        .arg(output_path)
        .output()
        .map_err(|source| BuildError::Io {
            context: format!("failed to run `{}`", objcopy.display()),
            source,
        })?;
    if !hex.status.success() {
        return Err(BuildError::ToolFailed {
            tool: objcopy.display().to_string(),
            status: hex.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&hex.stderr).into_owned(),
        });
    }

    let elf_output = output_path.with_extension("elf");
    let _ = fs::copy(&elf_path, &elf_output);
    if options.flash_after_build {
        flash_arduino_uno_hex(output_path, options.flash_port.as_deref())?;
    }

    let _ = fs::remove_file(cpp_path);
    for object in objects {
        let _ = fs::remove_file(object);
    }
    let _ = fs::remove_file(elf_path);
    let _ = fs::remove_dir(temp_dir);
    Ok(())
}

fn emit_arduino_uno_precode(program: &Program) -> Result<String, BuildError> {
    if find_packaged_llvm_cbe().is_some() {
        let precode = emit_arduino_uno_precode_via_llvm_cbe(program)?;
        return Ok(format!(
            "// --- rune_arduino_uno.ll ---\n{}\n// --- rune_arduino_uno.c ---\n{}\n// --- rune_arduino_runtime.hpp ---\n{}\n// --- rune_arduino_uno_runtime.cpp ---\n{}",
            precode.llvm_ir, precode.c_source, precode.runtime_header, precode.runtime_cpp
        ));
    }

    let cpp_source = emit_arduino_uno_cpp(program).map_err(BuildError::Codegen)?;
    Ok(format!(
        "// --- rune_arduino_uno.cpp ---\n{cpp_source}"
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArduinoUnoEntrypointKind {
    Main,
    SetupLoop,
}

fn detect_arduino_uno_entrypoint_kind(
    program: &Program,
) -> Result<ArduinoUnoEntrypointKind, CodegenError> {
    let main = program.items.iter().find_map(|item| match item {
        Item::Function(function) if function.name == "main" => Some(function),
        _ => None,
    });
    let setup_fn = program.items.iter().find_map(|item| match item {
        Item::Function(function) if function.name == "setup" => Some(function),
        _ => None,
    });
    let loop_fn = program.items.iter().find_map(|item| match item {
        Item::Function(function) if function.name == "loop" => Some(function),
        _ => None,
    });

    if main.is_some() && (setup_fn.is_some() || loop_fn.is_some()) {
        return Err(CodegenError {
            message: "Arduino Uno target expects either `main()` or `setup()`/`loop()`, not both"
                .into(),
            span: main.expect("checked above").span,
        });
    }

    if let Some(main) = main {
        validate_arduino_uno_entry_fn(main, "main")?;
        return Ok(ArduinoUnoEntrypointKind::Main);
    }

    if let Some(setup_fn) = setup_fn {
        validate_arduino_uno_entry_fn(setup_fn, "setup")?;
    }
    if let Some(loop_fn) = loop_fn {
        validate_arduino_uno_entry_fn(loop_fn, "loop")?;
    }

    if setup_fn.is_some() || loop_fn.is_some() {
        Ok(ArduinoUnoEntrypointKind::SetupLoop)
    } else {
        Err(CodegenError {
            message: "Arduino Uno target requires `main()` or at least one of `setup()`/`loop()`"
                .into(),
            span: crate::lexer::Span { line: 1, column: 1 },
        })
    }
}

fn build_arduino_uno_hex_via_llvm_cbe(
    program: &Program,
    output_path: &Path,
    options: &BuildOptions,
    _llvm_cbe: &Path,
) -> Result<(), BuildError> {
    let precode = emit_arduino_uno_precode_via_llvm_cbe(program)?;
    let gcc = find_arduino_avr_gcc().ok_or_else(|| {
        BuildError::ToolNotFound("packaged Arduino AVR gcc toolchain not found".into())
    })?;
    let gpp = find_arduino_avr_gpp().ok_or_else(|| {
        BuildError::ToolNotFound("packaged Arduino AVR g++ toolchain not found".into())
    })?;
    let objcopy = find_arduino_avr_objcopy().ok_or_else(|| {
        BuildError::ToolNotFound("packaged Arduino AVR objcopy not found".into())
    })?;
    let core_root = find_arduino_avr_core_root().ok_or_else(|| {
        BuildError::ToolNotFound("packaged Arduino AVR core sources not found".into())
    })?;
    let runtime_header = find_arduino_avr_runtime_header().ok_or_else(|| {
        BuildError::ToolNotFound("packaged Rune Arduino AVR runtime header not found".into())
    })?;
    let temp_dir = create_temp_dir()?;
    let llvm_ir_path = temp_dir.join("rune_arduino_uno.ll");
    let c_path = temp_dir.join("rune_arduino_uno.c");
    let c_object = temp_dir.join("rune_arduino_uno_cbe.o");
    let shim_path = temp_dir.join("rune_arduino_uno_runtime.cpp");
    let shim_header_path = temp_dir.join("rune_arduino_runtime.hpp");
    let shim_object = temp_dir.join("rune_arduino_uno_runtime.o");
    let elf_path = temp_dir.join("rune_arduino_uno.elf");
    fs::write(&llvm_ir_path, precode.llvm_ir).map_err(|source| BuildError::Io {
        context: format!("failed to write `{}`", llvm_ir_path.display()),
        source,
    })?;
    fs::write(&c_path, precode.c_source).map_err(|source| BuildError::Io {
        context: format!("failed to write `{}`", c_path.display()),
        source,
    })?;

    fs::copy(&runtime_header, &shim_header_path).map_err(|source| BuildError::Io {
        context: format!(
            "failed to copy `{}` into `{}`",
            runtime_header.display(),
            shim_header_path.display()
        ),
        source,
    })?;
    fs::write(&shim_path, precode.runtime_cpp).map_err(|source| {
        BuildError::Io {
            context: format!("failed to write `{}`", shim_path.display()),
            source,
        }
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

    let core_dir = core_root.join("cores").join("arduino");
    let variant_dir = core_root.join("variants").join("standard");
    let servo_dir = find_arduino_avr_servo_library_root();
    let include_servo_sources = program_uses_arduino_servo(program);
    let servo_include_dir = if include_servo_sources {
        servo_dir.as_deref()
    } else {
        None
    };
    let common_args =
        arduino_uno_common_compile_args(&core_dir, &variant_dir, servo_include_dir);

    let c_compile = Command::new(&gcc)
        .args(&common_args)
        .arg("-std=gnu11")
        .arg("-c")
        .arg(&c_path)
        .arg("-o")
        .arg(&c_object)
        .output()
        .map_err(|source| BuildError::Io {
            context: format!("failed to run `{}`", gcc.display()),
            source,
        })?;
    if !c_compile.status.success() {
        return Err(BuildError::ToolFailed {
            tool: gcc.display().to_string(),
            status: c_compile.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&c_compile.stderr).into_owned(),
        });
    }

    let shim_compile = Command::new(&gpp)
        .args(&common_args)
        .arg("-std=gnu++11")
        .arg("-fno-exceptions")
        .arg("-fno-threadsafe-statics")
        .arg("-c")
        .arg(&shim_path)
        .arg("-o")
        .arg(&shim_object)
        .output()
        .map_err(|source| BuildError::Io {
            context: format!("failed to run `{}`", gpp.display()),
            source,
        })?;
    if !shim_compile.status.success() {
        return Err(BuildError::ToolFailed {
            tool: gpp.display().to_string(),
            status: shim_compile.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&shim_compile.stderr).into_owned(),
        });
    }

    let mut objects = vec![c_object.clone(), shim_object.clone()];
    objects.extend(compile_arduino_uno_core_sources(
        &temp_dir,
        &gcc,
        &gpp,
        &core_dir,
        &variant_dir,
        servo_include_dir,
        include_servo_sources,
    )?);

    let mut link = Command::new(&gpp);
    link.arg("-mmcu=atmega328p")
        .arg("-Os")
        .arg("-flto")
        .arg("-Wl,--gc-sections")
        .arg("-o")
        .arg(&elf_path);
    for object in &objects {
        link.arg(object);
    }
    let link = link.output().map_err(|source| BuildError::Io {
        context: format!("failed to run `{}`", gpp.display()),
        source,
    })?;
    if !link.status.success() {
        return Err(BuildError::ToolFailed {
            tool: gpp.display().to_string(),
            status: link.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&link.stderr).into_owned(),
        });
    }

    let hex = Command::new(&objcopy)
        .arg("-O")
        .arg("ihex")
        .arg("-R")
        .arg(".eeprom")
        .arg(&elf_path)
        .arg(output_path)
        .output()
        .map_err(|source| BuildError::Io {
            context: format!("failed to run `{}`", objcopy.display()),
            source,
        })?;
    if !hex.status.success() {
        return Err(BuildError::ToolFailed {
            tool: objcopy.display().to_string(),
            status: hex.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&hex.stderr).into_owned(),
        });
    }

    let elf_output = output_path.with_extension("elf");
    let _ = fs::copy(&elf_path, &elf_output);
    if options.flash_after_build {
        flash_arduino_uno_hex(output_path, options.flash_port.as_deref())?;
    }

    let _ = fs::remove_file(llvm_ir_path);
    let _ = fs::remove_file(c_path);
    let _ = fs::remove_file(shim_path);
    let _ = fs::remove_file(shim_header_path);
    for object in objects {
        let _ = fs::remove_file(object);
    }
    let _ = fs::remove_file(elf_path);
    let _ = fs::remove_dir(temp_dir);
    Ok(())
}

struct ArduinoUnoPrecodeArtifacts {
    llvm_ir: String,
    c_source: String,
    runtime_header: String,
    runtime_cpp: String,
}

fn emit_arduino_uno_precode_via_llvm_cbe(
    program: &Program,
) -> Result<ArduinoUnoPrecodeArtifacts, BuildError> {
    let llvm_cbe = find_packaged_llvm_cbe().ok_or_else(|| {
        BuildError::ToolNotFound("packaged LLVM C backend tool not found".into())
    })?;
    let runtime_header_path = find_arduino_avr_runtime_header().ok_or_else(|| {
        BuildError::ToolNotFound("packaged Rune Arduino AVR runtime header not found".into())
    })?;
    let entrypoint = detect_arduino_uno_entrypoint_kind(program).map_err(BuildError::Codegen)?;
    let include_servo_sources = program_uses_arduino_servo(program);
    let llvm_ir = emit_llvm_ir(program).map_err(|error| {
        BuildError::Codegen(CodegenError {
            message: error.message,
            span: crate::lexer::Span { line: 1, column: 1 },
        })
    })?;
    let llvm_ir = rewrite_arduino_uno_cbe_llvm_ir(&llvm_ir, entrypoint);

    let temp_dir = create_temp_dir()?;
    let llvm_ir_path = temp_dir.join("rune_arduino_uno.ll");
    let c_path = temp_dir.join("rune_arduino_uno.c");
    fs::write(&llvm_ir_path, &llvm_ir).map_err(|source| BuildError::Io {
        context: format!("failed to write `{}`", llvm_ir_path.display()),
        source,
    })?;

    let cbe = Command::new(&llvm_cbe)
        .arg(&llvm_ir_path)
        .arg("-o")
        .arg(&c_path)
        .output()
        .map_err(|source| BuildError::Io {
            context: format!("failed to run `{}`", llvm_cbe.display()),
            source,
        })?;
    if !cbe.status.success() {
        return Err(BuildError::ToolFailed {
            tool: llvm_cbe.display().to_string(),
            status: cbe.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&cbe.stderr).into_owned(),
        });
    }

    let c_source = fs::read_to_string(&c_path).map_err(|source| BuildError::Io {
        context: format!("failed to read `{}`", c_path.display()),
        source,
    })?;
    let c_source = rewrite_arduino_uno_cbe_source(&c_source, entrypoint);
    let runtime_header = fs::read_to_string(&runtime_header_path).map_err(|source| BuildError::Io {
        context: format!("failed to read `{}`", runtime_header_path.display()),
        source,
    })?;
    let runtime_cpp = emit_arduino_uno_cbe_runtime_cpp_with_features(entrypoint, include_servo_sources);

    let _ = fs::remove_file(&llvm_ir_path);
    let _ = fs::remove_file(&c_path);
    let _ = fs::remove_dir(temp_dir);

    Ok(ArduinoUnoPrecodeArtifacts {
        llvm_ir,
        c_source,
        runtime_header,
        runtime_cpp,
    })
}

fn rewrite_arduino_uno_cbe_source(
    c_source: &str,
    entrypoint: ArduinoUnoEntrypointKind,
) -> String {
    match entrypoint {
        ArduinoUnoEntrypointKind::Main => c_source.replace("int main(void)", "int rune_entry_main(void)"),
        ArduinoUnoEntrypointKind::SetupLoop => c_source
            .replace("void setup(void)", "void rune_entry_setup(void)")
            .replace("void loop(void)", "void rune_entry_loop(void)"),
    }
}

fn rewrite_arduino_uno_cbe_llvm_ir(
    llvm_ir: &str,
    entrypoint: ArduinoUnoEntrypointKind,
) -> String {
    let mut rename_map = HashMap::new();
    for line in llvm_ir.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("define ") {
            continue;
        }
        let Some(at_index) = trimmed.find('@') else {
            continue;
        };
        let name_start = at_index + 1;
        let name_end = trimmed[name_start..]
            .find('(')
            .map(|index| name_start + index)
            .unwrap_or(trimmed.len());
        let name = &trimmed[name_start..name_end];
        if name.starts_with("rune_rt_") {
            continue;
        }
        if matches!(
            (entrypoint, name),
            (ArduinoUnoEntrypointKind::Main, "main")
                | (ArduinoUnoEntrypointKind::SetupLoop, "setup")
                | (ArduinoUnoEntrypointKind::SetupLoop, "loop")
        ) {
            continue;
        }
        rename_map.insert(name.to_string(), format!("rune_cbe_{name}"));
    }

    rewrite_llvm_global_identifiers(llvm_ir, &rename_map)
}

fn rewrite_llvm_global_identifiers(source: &str, rename_map: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(source.len());
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'@' {
            let start = index + 1;
            let mut end = start;
            while end < bytes.len() {
                let ch = bytes[end];
                if ch.is_ascii_alphanumeric() || ch == b'_' || ch == b'.' {
                    end += 1;
                } else {
                    break;
                }
            }
            if end > start {
                let name = &source[start..end];
                if let Some(replacement) = rename_map.get(name) {
                    out.push('@');
                    out.push_str(replacement);
                    index = end;
                    continue;
                }
            }
        }
        out.push(bytes[index] as char);
        index += 1;
    }
    out
}

fn emit_arduino_uno_cbe_runtime_cpp_with_features(
    entrypoint: ArduinoUnoEntrypointKind,
    enable_servo: bool,
) -> String {
    let mut defines = vec![match entrypoint {
        ArduinoUnoEntrypointKind::Main => "#define RUNE_ARDUINO_ENTRY_MAIN 1",
        ArduinoUnoEntrypointKind::SetupLoop => "#define RUNE_ARDUINO_ENTRY_SETUP_LOOP 1",
    }];
    if enable_servo {
        defines.push("#define RUNE_ARDUINO_ENABLE_SERVO 1");
    }
    format!("{}\n#include \"rune_arduino_runtime.hpp\"\n", defines.join("\n"))
}

fn flash_arduino_uno_hex(hex_path: &Path, port: Option<&str>) -> Result<(), BuildError> {
    let avrdude = find_arduino_avrdude().ok_or_else(|| {
        BuildError::ToolNotFound("packaged Arduino AVR avrdude not found".into())
    })?;
    let conf = find_arduino_avr_avrdude_conf().ok_or_else(|| {
        BuildError::ToolNotFound("packaged Arduino AVR avrdude.conf not found".into())
    })?;
    let port = port.ok_or_else(|| {
        BuildError::ToolNotFound(
            "Arduino Uno flashing requires `--port <serial-port>` (for example `COM5`)".into(),
        )
    })?;

    let flash = Command::new(&avrdude)
        .arg("-C")
        .arg(&conf)
        .arg("-p")
        .arg("m328p")
        .arg("-c")
        .arg("arduino")
        .arg("-P")
        .arg(port)
        .arg("-b")
        .arg("115200")
        .arg("-D")
        .arg("-U")
        .arg(format!("flash:w:{}:i", hex_path.display()))
        .output()
        .map_err(|source| BuildError::Io {
            context: format!("failed to run `{}`", avrdude.display()),
            source,
        })?;
    if !flash.status.success() {
        return Err(BuildError::ToolFailed {
            tool: avrdude.display().to_string(),
            status: flash.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&flash.stderr).into_owned(),
        });
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ArduinoUnoType {
    I64,
    Bool,
    String,
    Dynamic,
    Struct(String),
}

#[derive(Debug, Clone)]
struct ArduinoUnoFunctionSig {
    params: Vec<(String, ArduinoUnoType)>,
    return_type: Option<ArduinoUnoType>,
}

#[derive(Debug, Clone)]
struct ArduinoUnoStructSig {
    fields: Vec<(String, ArduinoUnoType)>,
    methods: HashMap<String, ArduinoUnoFunctionSig>,
}

#[derive(Debug, Clone)]
enum ArduinoUnoReturnKind {
    Entry,
    Void,
    Value(ArduinoUnoType),
}

fn emit_arduino_uno_cpp(program: &Program) -> Result<String, CodegenError> {
    let include_servo_sources = program_uses_arduino_servo(program);
    let main = program.items.iter().find_map(|item| match item {
        Item::Function(function) if function.name == "main" => Some(function),
        _ => None,
    });
    let setup_fn = program.items.iter().find_map(|item| match item {
        Item::Function(function) if function.name == "setup" => Some(function),
        _ => None,
    });
    let loop_fn = program.items.iter().find_map(|item| match item {
        Item::Function(function) if function.name == "loop" => Some(function),
        _ => None,
    });

    if main.is_some() && (setup_fn.is_some() || loop_fn.is_some()) {
        return Err(CodegenError {
            message: "Arduino Uno target expects either `main()` or `setup()`/`loop()`, not both".into(),
            span: main.expect("checked above").span,
        });
    }

    let has_setup = setup_fn.is_some();
    let has_loop = loop_fn.is_some();
    let struct_signatures = collect_arduino_uno_structs(program)?;
    let helper_functions = collect_arduino_uno_helper_functions(program)?;
    let helper_signatures = helper_functions
        .iter()
        .map(|function| {
            Ok((
                function.name.clone(),
                ArduinoUnoFunctionSig {
                    params: function
                        .params
                        .iter()
                        .map(|param| {
                            Ok((param.name.clone(), arduino_uno_type_from_ref(&param.ty)?))
                        })
                        .collect::<Result<Vec<_>, CodegenError>>()?,
                    return_type: function
                        .return_type
                        .as_ref()
                        .map(arduino_uno_function_return_type_from_ref)
                        .transpose()?
                        .flatten(),
                },
            ))
        })
        .collect::<Result<HashMap<_, _>, CodegenError>>()?;
    let (setup_source, loop_source) = if let Some(main) = main {
        if main.is_extern || main.is_async {
            return Err(CodegenError {
                message: "Arduino Uno target does not support extern or async `main`".into(),
                span: main.span,
            });
        }
        if !main.params.is_empty() {
            return Err(CodegenError {
                message: "Arduino Uno target requires `main()` with no parameters".into(),
                span: main.span,
            });
        }
        let mut body = String::new();
        let mut scope = HashMap::new();
        emit_arduino_uno_block(
            &mut body,
            &main.body.statements,
            &mut scope,
            &helper_signatures,
            &struct_signatures,
            ArduinoUnoReturnKind::Entry,
            false,
            1,
        )?;
        (body, String::new())
    } else {
        let mut setup_body = String::new();
        let mut loop_body = String::new();

        if let Some(setup_fn) = setup_fn {
            validate_arduino_uno_entry_fn(setup_fn, "setup")?;
            let mut scope = HashMap::new();
            emit_arduino_uno_block(
                &mut setup_body,
                &setup_fn.body.statements,
                &mut scope,
                &helper_signatures,
                &struct_signatures,
                ArduinoUnoReturnKind::Entry,
                false,
                1,
            )?;
        }

        if let Some(loop_fn) = loop_fn {
            validate_arduino_uno_entry_fn(loop_fn, "loop")?;
            let mut scope = HashMap::new();
            emit_arduino_uno_block(
                &mut loop_body,
                &loop_fn.body.statements,
                &mut scope,
                &helper_signatures,
                &struct_signatures,
                ArduinoUnoReturnKind::Entry,
                false,
                1,
            )?;
        }

        if !has_setup && !has_loop {
            return Err(CodegenError {
                message: "Arduino Uno target requires `main()` or at least one of `setup()`/`loop()`".into(),
                span: crate::lexer::Span { line: 1, column: 1 },
            });
        }

        (setup_body, loop_body)
    };
    let struct_definitions = emit_arduino_uno_struct_definitions(&struct_signatures);
    let method_prototypes = emit_arduino_uno_method_prototypes(&struct_signatures);
    let helper_prototypes = emit_arduino_uno_function_prototypes(&helper_functions, &helper_signatures)?;
    let helper_definitions =
        emit_arduino_uno_function_definitions(&helper_functions, &helper_signatures, &struct_signatures)?;
    let method_definitions =
        emit_arduino_uno_method_definitions(program, &helper_signatures, &struct_signatures)?;
    let servo_include = if include_servo_sources {
        "#include <Servo.h>\n"
    } else {
        ""
    };
    let servo_globals = if include_servo_sources {
        "alignas(Servo) static unsigned char rune_servo_storage[20][sizeof(Servo)];\n\
static Servo* rune_servo_slots[20] = { nullptr };\n\
static bool rune_servo_constructed_flags[20] = { false };\n\
static bool rune_servo_attached_flags[20] = { false };\n\n\
"
    } else {
        ""
    };
    let servo_functions = if include_servo_sources {
        "static bool rune_rt_arduino_servo_attach(int64_t pin) {\n\
    if (pin < 0 || pin >= 20) {\n\
        return false;\n\
    }\n\
    uint8_t slot = (uint8_t)pin;\n\
    if (rune_servo_slots[slot] == nullptr) {\n\
        rune_servo_slots[slot] = new (&rune_servo_storage[slot][0]) Servo();\n\
        rune_servo_constructed_flags[slot] = true;\n\
    }\n\
    if (!rune_servo_attached_flags[slot]) {\n\
        rune_servo_slots[slot]->attach((int)pin);\n\
        rune_servo_attached_flags[slot] = true;\n\
    }\n\
    return rune_servo_attached_flags[slot];\n\
}\n\n\
static void rune_rt_arduino_servo_detach(int64_t pin) {\n\
    if (pin < 0 || pin >= 20) {\n\
        return;\n\
    }\n\
    uint8_t slot = (uint8_t)pin;\n\
    if (rune_servo_slots[slot] != nullptr && rune_servo_attached_flags[slot]) {\n\
        rune_servo_slots[slot]->detach();\n\
        rune_servo_attached_flags[slot] = false;\n\
    }\n\
}\n\n\
static void rune_rt_arduino_servo_write(int64_t pin, int64_t angle) {\n\
    if (!rune_rt_arduino_servo_attach(pin)) {\n\
        return;\n\
    }\n\
    if (angle < 0) {\n\
        angle = 0;\n\
    } else if (angle > 180) {\n\
        angle = 180;\n\
    }\n\
    rune_servo_slots[(uint8_t)pin]->write((int)angle);\n\
}\n\n\
static void rune_rt_arduino_servo_write_us(int64_t pin, int64_t pulse_us) {\n\
    if (!rune_rt_arduino_servo_attach(pin)) {\n\
        return;\n\
    }\n\
    rune_servo_slots[(uint8_t)pin]->writeMicroseconds((int)pulse_us);\n\
}\n\n\
"
    } else {
        ""
    };

    Ok(format!(
        "#include <Arduino.h>\n{servo_include}#include <new>\n#include <stdint.h>\n#include <stdlib.h>\n#include <string.h>\n\n\
static char rune_input_buffer[96];\n\n\
static char rune_dynamic_concat_buffer[160];\n\n\
#define RUNE_STRING_SLOT_COUNT 8\n\
#define RUNE_STRING_SLOT_SIZE 96\n\n\
static char rune_string_slots[RUNE_STRING_SLOT_COUNT][RUNE_STRING_SLOT_SIZE];\n\
static uint8_t rune_string_slot_index = 0;\n\n\
{servo_globals}\
typedef struct rune_dynamic_value {{\n\
    int64_t tag;\n\
    int64_t payload;\n\
    const char* text;\n\
}} rune_dynamic_value;\n\n\
 \n\
static char* rune_claim_string_slot(void) {{\n\
    char* slot = rune_string_slots[rune_string_slot_index];\n\
    rune_string_slot_index = (uint8_t)((rune_string_slot_index + 1) % RUNE_STRING_SLOT_COUNT);\n\
    slot[0] = '\\0';\n\
    return slot;\n\
}}\n\n\
static void rune_serial_write_cstr(const char* text) {{\n\
    Serial.print(text);\n\
}}\n\n\
static void rune_serial_write_bool(bool value) {{\n\
    Serial.print(value ? \"true\" : \"false\");\n\
}}\n\n\
static void rune_serial_write_i64(int64_t value) {{\n\
    char buffer[24];\n\
    uint8_t index = 0;\n\
    uint64_t magnitude = (value < 0) ? (uint64_t)(-value) : (uint64_t)value;\n\
    if (value == 0) {{\n\
        Serial.write('0');\n\
        return;\n\
    }}\n\
    if (value < 0) {{\n\
        Serial.write('-');\n\
    }}\n\
    while (magnitude > 0) {{\n\
        buffer[index++] = (char)('0' + (magnitude % 10));\n\
        magnitude /= 10;\n\
    }}\n\
    while (index > 0) {{\n\
        Serial.write(buffer[--index]);\n\
    }}\n\
}}\n\n\
static void rune_serial_newline(void) {{\n\
    Serial.write('\\r');\n\
    Serial.write('\\n');\n\
}}\n\n\
static const char* rune_serial_read_line(void) {{\n\
    size_t index = 0;\n\
    for (;;) {{\n\
        while (Serial.available() <= 0) {{}}\n\
        int value = Serial.read();\n\
        if (value < 0) {{\n\
            continue;\n\
        }}\n\
        if (value == '\\r') {{\n\
            continue;\n\
        }}\n\
        if (value == '\\n') {{\n\
            break;\n\
        }}\n\
        if (index + 1 < sizeof(rune_input_buffer)) {{\n\
            rune_input_buffer[index++] = (char)value;\n\
        }}\n\
    }}\n\
    rune_input_buffer[index] = '\\0';\n\
    return rune_input_buffer;\n\
}}\n\n\
static int64_t rune_parse_i64(const char* text) {{\n\
    if (text == nullptr) {{\n\
        return 0;\n\
    }}\n\
    bool negative = false;\n\
    if (*text == '-') {{\n\
        negative = true;\n\
        text++;\n\
    }}\n\
    int64_t value = 0;\n\
    while (*text >= '0' && *text <= '9') {{\n\
        value = (value * 10) + (int64_t)(*text - '0');\n\
        text++;\n\
    }}\n\
    return negative ? -value : value;\n\
}}\n\n\
static const char* rune_string_from_i64(int64_t value) {{\n\
    char* slot = rune_claim_string_slot();\n\
    uint8_t index = 0;\n\
    uint64_t magnitude = (value < 0) ? (uint64_t)(-value) : (uint64_t)value;\n\
    if (value == 0) {{\n\
        slot[0] = '0';\n\
        slot[1] = '\\0';\n\
        return slot;\n\
    }}\n\
    if (value < 0) {{\n\
        slot[index++] = '-';\n\
    }}\n\
    char reversed[21];\n\
    uint8_t digits = 0;\n\
    while (magnitude > 0 && digits + 1 < sizeof(reversed)) {{\n\
        reversed[digits++] = (char)('0' + (magnitude % 10));\n\
        magnitude /= 10;\n\
    }}\n\
    while (digits > 0 && index + 1 < RUNE_STRING_SLOT_SIZE) {{\n\
        slot[index++] = reversed[--digits];\n\
    }}\n\
    slot[index] = '\\0';\n\
    return slot;\n\
}}\n\n\
static const char* rune_string_from_bool(bool value) {{\n\
    return value ? \"true\" : \"false\";\n\
}}\n\n\
{servo_functions}\
static rune_dynamic_value rune_dynamic_from_i64(int64_t value) {{\n\
    rune_dynamic_value out = {{1, value, nullptr}};\n\
    return out;\n\
}}\n\n\
static rune_dynamic_value rune_dynamic_from_bool(bool value) {{\n\
    rune_dynamic_value out = {{2, value ? 1 : 0, nullptr}};\n\
    return out;\n\
}}\n\n\
static rune_dynamic_value rune_dynamic_from_string(const char* value) {{\n\
    rune_dynamic_value out = {{3, 0, value == nullptr ? \"\" : value}};\n\
    return out;\n\
}}\n\n\
static const char* rune_dynamic_to_string(rune_dynamic_value value) {{\n\
    switch (value.tag) {{\n\
        case 1:\n\
            return rune_string_from_i64(value.payload);\n\
        case 2:\n\
            return rune_string_from_bool(value.payload != 0);\n\
        case 3:\n\
            return value.text == nullptr ? \"\" : value.text;\n\
        default:\n\
            return \"<dynamic>\";\n\
    }}\n\
}}\n\n\
static int64_t rune_dynamic_to_i64(rune_dynamic_value value) {{\n\
    switch (value.tag) {{\n\
        case 1:\n\
            return value.payload;\n\
        case 2:\n\
            return value.payload != 0 ? 1 : 0;\n\
        case 3:\n\
            return rune_parse_i64(value.text == nullptr ? \"\" : value.text);\n\
        default:\n\
            return 0;\n\
    }}\n\
}}\n\n\
static bool rune_dynamic_truthy(rune_dynamic_value value) {{\n\
    switch (value.tag) {{\n\
        case 1:\n\
            return value.payload != 0;\n\
        case 2:\n\
            return value.payload != 0;\n\
        case 3:\n\
            return value.text != nullptr && value.text[0] != '\\0';\n\
        default:\n\
            return false;\n\
    }}\n\
}}\n\n\
static void rune_serial_write_dynamic(rune_dynamic_value value) {{\n\
    rune_serial_write_cstr(rune_dynamic_to_string(value));\n\
}}\n\n\
static rune_dynamic_value rune_dynamic_binary(rune_dynamic_value left, rune_dynamic_value right, int64_t op) {{\n\
    if (op == 0 && (left.tag == 3 || right.tag == 3)) {{\n\
        const char* left_text = rune_dynamic_to_string(left);\n\
        const char* right_text = rune_dynamic_to_string(right);\n\
        char* slot = rune_claim_string_slot();\n\
        strncat(slot, left_text, RUNE_STRING_SLOT_SIZE - 1);\n\
        size_t used = strlen(slot);\n\
        if (used + 1 < RUNE_STRING_SLOT_SIZE) {{\n\
            strncat(slot, right_text, RUNE_STRING_SLOT_SIZE - used - 1);\n\
        }}\n\
        return rune_dynamic_from_string(slot);\n\
    }}\n\
    int64_t left_number = rune_dynamic_to_i64(left);\n\
    int64_t right_number = rune_dynamic_to_i64(right);\n\
    switch (op) {{\n\
        case 0:\n\
            return rune_dynamic_from_i64(left_number + right_number);\n\
        case 1:\n\
            return rune_dynamic_from_i64(left_number - right_number);\n\
        case 2:\n\
            return rune_dynamic_from_i64(left_number * right_number);\n\
        case 3:\n\
            return rune_dynamic_from_i64(right_number == 0 ? 0 : (left_number / right_number));\n\
        case 4:\n\
            return rune_dynamic_from_i64(right_number == 0 ? 0 : (left_number % right_number));\n\
        default:\n\
            return rune_dynamic_from_i64(0);\n\
    }}\n\
}}\n\n\
static bool rune_dynamic_compare(rune_dynamic_value left, rune_dynamic_value right, int64_t op) {{\n\
    if (left.tag == 3 || right.tag == 3) {{\n\
        int cmp = strcmp(rune_dynamic_to_string(left), rune_dynamic_to_string(right));\n\
        switch (op) {{\n\
            case 0: return cmp == 0;\n\
            case 1: return cmp != 0;\n\
            case 2: return cmp > 0;\n\
            case 3: return cmp >= 0;\n\
            case 4: return cmp < 0;\n\
            case 5: return cmp <= 0;\n\
            default: return false;\n\
        }}\n\
    }}\n\
    int64_t left_number = rune_dynamic_to_i64(left);\n\
    int64_t right_number = rune_dynamic_to_i64(right);\n\
    switch (op) {{\n\
        case 0: return left_number == right_number;\n\
        case 1: return left_number != right_number;\n\
        case 2: return left_number > right_number;\n\
        case 3: return left_number >= right_number;\n\
        case 4: return left_number < right_number;\n\
        case 5: return left_number <= right_number;\n\
        default: return false;\n\
    }}\n\
}}\n\n\
static int64_t rune_sum_range(int64_t start, int64_t stop, int64_t step) {{\n\
    if (step == 0) {{\n\
        return 0;\n\
    }}\n\
    int64_t total = 0;\n\
    if (step > 0) {{\n\
        for (int64_t value = start; value < stop; value += step) {{\n\
            total += value;\n\
        }}\n\
    }} else {{\n\
        for (int64_t value = start; value > stop; value += step) {{\n\
            total += value;\n\
        }}\n\
    }}\n\
    return total;\n\
}}\n\n\
static bool rune_string_eq(const char* left, const char* right) {{\n\
    if (left == nullptr || right == nullptr) {{\n\
        return left == right;\n\
    }}\n\
    return strcmp(left, right) == 0;\n\
}}\n\n\
static const char* rune_string_concat(const char* left, const char* right) {{\n\
    const char* left_text = left == nullptr ? \"\" : left;\n\
    const char* right_text = right == nullptr ? \"\" : right;\n\
    char* slot = rune_claim_string_slot();\n\
    strncat(slot, left_text, RUNE_STRING_SLOT_SIZE - 1);\n\
    size_t used = strlen(slot);\n\
    if (used + 1 < RUNE_STRING_SLOT_SIZE) {{\n\
        strncat(slot, right_text, RUNE_STRING_SLOT_SIZE - used - 1);\n\
    }}\n\
    return slot;\n\
}}\n\n\
static int rune_mode_input(void) {{ return INPUT; }}\n\
static int rune_mode_output(void) {{ return OUTPUT; }}\n\
static int rune_mode_input_pullup(void) {{ return INPUT_PULLUP; }}\n\
static int rune_led_builtin(void) {{ return LED_BUILTIN; }}\n\n\
static int rune_high(void) {{ return HIGH; }}\n\
static int rune_low(void) {{ return LOW; }}\n\
static int rune_bit_order_lsb_first(void) {{ return LSBFIRST; }}\n\
static int rune_bit_order_msb_first(void) {{ return MSBFIRST; }}\n\
static int rune_analog_ref_default(void) {{ return DEFAULT; }}\n\
static int rune_analog_ref_internal(void) {{ return INTERNAL; }}\n\
static int rune_analog_ref_external(void) {{ return EXTERNAL; }}\n\n\
{struct_definitions}\
{method_prototypes}\
{helper_prototypes}\
{helper_definitions}\
{method_definitions}\
void setup() {{\n\
    Serial.begin(115200);\n\
{setup_source}\
}}\n\n\
void loop() {{\n\
{loop_source}\
}}\n"
    ))
}

fn validate_arduino_uno_entry_fn(function: &crate::parser::Function, name: &str) -> Result<(), CodegenError> {
    if function.is_extern || function.is_async {
        return Err(CodegenError {
            message: format!("Arduino Uno target does not support extern or async `{name}`"),
            span: function.span,
        });
    }
    if !function.params.is_empty() {
        return Err(CodegenError {
            message: format!("Arduino Uno target requires `{name}()` with no parameters"),
            span: function.span,
        });
    }
    Ok(())
}

fn emit_arduino_uno_block(
    out: &mut String,
    statements: &[Stmt],
    scope: &mut HashMap<String, ArduinoUnoType>,
    functions: &HashMap<String, ArduinoUnoFunctionSig>,
    structs: &HashMap<String, ArduinoUnoStructSig>,
    return_kind: ArduinoUnoReturnKind,
    in_loop: bool,
    indent: usize,
) -> Result<(), CodegenError> {
    for stmt in statements {
        emit_arduino_uno_stmt(
            out,
            stmt,
            scope,
            functions,
            structs,
            return_kind.clone(),
            in_loop,
            indent,
        )?;
    }
    Ok(())
}

fn emit_arduino_uno_stmt(
    out: &mut String,
    stmt: &Stmt,
    scope: &mut HashMap<String, ArduinoUnoType>,
    functions: &HashMap<String, ArduinoUnoFunctionSig>,
    structs: &HashMap<String, ArduinoUnoStructSig>,
    return_kind: ArduinoUnoReturnKind,
    in_loop: bool,
    indent: usize,
) -> Result<(), CodegenError> {
    let prefix = "    ".repeat(indent);
    match stmt {
        Stmt::Block(stmt) => {
            let mut block_scope = scope.clone();
            emit_arduino_uno_block(
                out,
                &stmt.block.statements,
                &mut block_scope,
                functions,
                structs,
                return_kind,
                in_loop,
                indent,
            )
        }
        Stmt::Let(stmt) => {
            let explicit_ty = stmt.ty.as_ref().map(arduino_uno_type_from_ref).transpose()?;
            let inferred_ty = emit_arduino_uno_expr(scope, functions, structs, &stmt.value)?;
            let inferred_value_ty = inferred_ty.1.clone();
            let ty = explicit_ty.unwrap_or_else(|| inferred_value_ty.clone());
            let value = arduino_uno_coerce_value(inferred_ty, &ty, stmt.span).map_err(|_| CodegenError {
                message: "Arduino Uno target requires matching scalar let types".into(),
                span: stmt.span,
            })?;
            if ty != inferred_value_ty && !(ty == ArduinoUnoType::Dynamic && arduino_uno_can_promote_to_dynamic(&inferred_value_ty)) {
                return Err(CodegenError {
                    message: "Arduino Uno target requires matching scalar let types".into(),
                    span: stmt.span,
                });
            }
            out.push_str(&format!(
                "{prefix}{} {} = {};\n",
                arduino_uno_c_type(&ty),
                stmt.name,
                value
            ));
            scope.insert(stmt.name.clone(), ty);
            Ok(())
        }
        Stmt::Assign(stmt) => {
            let Some(existing) = scope.get(&stmt.name).cloned() else {
                return Err(CodegenError {
                    message: format!(
                        "Arduino Uno target requires local `{}` to be declared before assignment",
                        stmt.name
                    ),
                    span: stmt.span,
                });
            };
            let rendered = emit_arduino_uno_expr(scope, functions, structs, &stmt.value)?;
            let value = arduino_uno_coerce_value(rendered, &existing, stmt.span).map_err(|_| CodegenError {
                message: "Arduino Uno target requires assignment types to stay concrete".into(),
                span: stmt.span,
            })?;
            out.push_str(&format!("{prefix}{} = {};\n", stmt.name, value));
            Ok(())
        }
        Stmt::Expr(expr_stmt) => emit_arduino_uno_stmt_expr(out, &expr_stmt.expr, scope, functions, structs, indent),
        Stmt::If(stmt) => {
            let condition = emit_arduino_uno_expr(scope, functions, structs, &stmt.condition)?;
            let condition_ty = condition.1.clone();
            let condition_expr = match condition_ty {
                ArduinoUnoType::Bool => condition.0,
                ArduinoUnoType::Dynamic => format!("rune_dynamic_truthy({})", condition.0),
                _ => {
                    return Err(CodegenError {
                        message: "Arduino Uno target requires boolean `if` conditions".into(),
                        span: stmt.span,
                    })
                }
            };
            out.push_str(&format!("{prefix}if ({}) {{\n", condition_expr));
            let mut then_scope = scope.clone();
            emit_arduino_uno_block(
                out,
                &stmt.then_block.statements,
                &mut then_scope,
                functions,
                structs,
                return_kind.clone(),
                in_loop,
                indent + 1,
            )?;
            out.push_str(&format!("{prefix}}}"));
            for elif in &stmt.elif_blocks {
                let cond = emit_arduino_uno_expr(scope, functions, structs, &elif.condition)?;
                let cond_ty = cond.1.clone();
                let cond_expr = match cond_ty {
                    ArduinoUnoType::Bool => cond.0,
                    ArduinoUnoType::Dynamic => format!("rune_dynamic_truthy({})", cond.0),
                    _ => {
                        return Err(CodegenError {
                            message: "Arduino Uno target requires boolean `elif` conditions".into(),
                            span: elif.span,
                        })
                    }
                };
                out.push_str(&format!(" else if ({}) {{\n", cond_expr));
                let mut elif_scope = scope.clone();
                emit_arduino_uno_block(
                    out,
                    &elif.block.statements,
                    &mut elif_scope,
                    functions,
                    structs,
                    return_kind.clone(),
                    in_loop,
                    indent + 1,
                )?;
                out.push_str(&format!("{prefix}}}"));
            }
            if let Some(block) = &stmt.else_block {
                out.push_str(" else {\n");
                let mut else_scope = scope.clone();
                emit_arduino_uno_block(
                    out,
                    &block.statements,
                    &mut else_scope,
                    functions,
                    structs,
                    return_kind.clone(),
                    in_loop,
                    indent + 1,
                )?;
                out.push_str(&format!("{prefix}}}"));
            }
            out.push('\n');
            Ok(())
        }
        Stmt::While(stmt) => {
            let condition = emit_arduino_uno_expr(scope, functions, structs, &stmt.condition)?;
            let condition_ty = condition.1.clone();
            let condition_expr = match condition_ty {
                ArduinoUnoType::Bool => condition.0,
                ArduinoUnoType::Dynamic => format!("rune_dynamic_truthy({})", condition.0),
                _ => {
                    return Err(CodegenError {
                        message: "Arduino Uno target requires boolean `while` conditions".into(),
                        span: stmt.span,
                    })
                }
            };
            out.push_str(&format!("{prefix}while ({}) {{\n", condition_expr));
            let mut body_scope = scope.clone();
            emit_arduino_uno_block(
                out,
                &stmt.body.statements,
                &mut body_scope,
                functions,
                structs,
                return_kind.clone(),
                true,
                indent + 1,
            )?;
            out.push_str(&format!("{prefix}}}\n"));
            Ok(())
        }
        Stmt::Break(stmt) => {
            if !in_loop {
                return Err(CodegenError {
                    message: "`break` is only allowed inside a loop".into(),
                    span: stmt.span,
                });
            }
            out.push_str(&format!("{prefix}break;\n"));
            Ok(())
        }
        Stmt::Continue(stmt) => {
            if !in_loop {
                return Err(CodegenError {
                    message: "`continue` is only allowed inside a loop".into(),
                    span: stmt.span,
                });
            }
            out.push_str(&format!("{prefix}continue;\n"));
            Ok(())
        }
        Stmt::Return(stmt) => match return_kind {
            ArduinoUnoReturnKind::Entry => {
                out.push_str(&format!("{prefix}return;\n"));
                Ok(())
            }
            ArduinoUnoReturnKind::Void => {
                if stmt.value.is_some() {
                    return Err(CodegenError {
                        message: "Arduino Uno target function with no return type cannot return a value".into(),
                        span: stmt.span,
                    });
                }
                out.push_str(&format!("{prefix}return;\n"));
                Ok(())
            }
            ArduinoUnoReturnKind::Value(expected) => {
                let Some(value) = &stmt.value else {
                    return Err(CodegenError {
                        message: "Arduino Uno target function returning a value requires `return <expr>`".into(),
                        span: stmt.span,
                    });
                };
                let rendered = emit_arduino_uno_expr(scope, functions, structs, value)?;
                let value_expr = arduino_uno_coerce_value(rendered, &expected, stmt.span).map_err(|_| CodegenError {
                    message: "Arduino Uno target function return type must stay concrete".into(),
                    span: stmt.span,
                })?;
                out.push_str(&format!("{prefix}return {};\n", value_expr));
                Ok(())
            }
        },
        Stmt::Raise(_) => Err(CodegenError {
            message: "Arduino Uno target does not support exceptions yet".into(),
            span: stmt_span(stmt),
        }),
        Stmt::Panic(stmt) => {
            let rendered = emit_arduino_uno_expr(scope, functions, structs, &stmt.value)?;
            let panic_expr = arduino_uno_coerce_value(rendered, &ArduinoUnoType::String, stmt.span).map_err(|_| CodegenError {
                message: "Arduino Uno target requires string panic messages".into(),
                span: stmt.span,
            })?;
            out.push_str(&format!("{prefix}rune_serial_write_cstr(\"Rune panic: \");\n"));
            out.push_str(&format!("{prefix}rune_serial_write_cstr({});\n", panic_expr));
            out.push_str(&format!("{prefix}rune_serial_newline();\n"));
            out.push_str(&format!("{prefix}for (;;) {{}}\n"));
            Ok(())
        }
    }
}

fn emit_arduino_uno_stmt_expr(
    out: &mut String,
    expr: &Expr,
    scope: &HashMap<String, ArduinoUnoType>,
    functions: &HashMap<String, ArduinoUnoFunctionSig>,
    structs: &HashMap<String, ArduinoUnoStructSig>,
    indent: usize,
) -> Result<(), CodegenError> {
    let prefix = "    ".repeat(indent);
    let ExprKind::Call { callee, args } = &expr.kind else {
        return Err(CodegenError {
            message: "Arduino Uno target supports only call statements in `main`".into(),
            span: expr.span,
        });
    };
    if let ExprKind::Field { base, name } = &callee.kind {
        let base_rendered = emit_arduino_uno_expr(scope, functions, structs, base)?;
        let ArduinoUnoType::Struct(struct_name) = &base_rendered.1 else {
            return Err(CodegenError {
                message: "Arduino Uno target method calls require a concrete class or struct receiver".into(),
                span: callee.span,
            });
        };
        let struct_sig = structs.get(struct_name).ok_or_else(|| CodegenError {
            message: format!("Arduino Uno target is missing struct layout for `{struct_name}`"),
            span: callee.span,
        })?;
        let method_sig = struct_sig.methods.get(name).ok_or_else(|| CodegenError {
            message: format!("Arduino Uno target struct `{struct_name}` has no method `{name}`"),
            span: callee.span,
        })?;
        let rendered = emit_arduino_uno_method_call(
            scope,
            functions,
            structs,
            struct_name,
            name,
            method_sig,
            &base_rendered,
            args,
            expr.span,
        )?;
        if method_sig.return_type.is_some() {
            out.push_str(&format!("{prefix}(void)({rendered});\n"));
        } else {
            out.push_str(&format!("{prefix}{rendered};\n"));
        }
        return Ok(());
    }
    let ExprKind::Identifier(name) = &callee.kind else {
        let rendered = emit_arduino_uno_expr(scope, functions, structs, expr)?;
        match rendered.1 {
            ArduinoUnoType::I64 | ArduinoUnoType::Bool => {
                out.push_str(&format!("{prefix}(void)({});\n", rendered.0));
                return Ok(());
            }
            ArduinoUnoType::String | ArduinoUnoType::Dynamic => {
                out.push_str(&format!("{prefix}(void)({});\n", rendered.0));
                return Ok(());
            }
            ArduinoUnoType::Struct(_) => {
                return Err(CodegenError {
                    message: "Arduino Uno target does not allow struct-valued call statements".into(),
                    span: expr.span,
                });
            }
        }
    };
    let dispatch_name = arduino_uno_builtin_alias(name);
    if !is_arduino_uno_builtin_dispatch_name(dispatch_name)
        && let Some(sig) = functions.get(dispatch_name)
    {
        let rendered =
            emit_arduino_uno_function_call(scope, functions, structs, dispatch_name, sig, args, expr.span)?;
        out.push_str(&format!("{prefix}{};\n", rendered));
        return Ok(());
    }
    match dispatch_name {
        "print" => {
            let [CallArg::Positional(value)] = args.as_slice() else {
                return Err(CodegenError {
                    message: "`print` expects exactly one positional argument on the Arduino Uno target".into(),
                    span: expr.span,
                });
            };
            emit_arduino_uno_print_expr(out, value, false, scope, functions, structs, indent)
        }
        "println" => {
            let [CallArg::Positional(value)] = args.as_slice() else {
                return Err(CodegenError {
                    message: "`println` expects exactly one positional argument on the Arduino Uno target".into(),
                    span: expr.span,
                });
            };
            emit_arduino_uno_print_expr(out, value, true, scope, functions, structs, indent)
        }
        "exit" => {
            let [CallArg::Positional(code_expr)] = args.as_slice() else {
                return Err(CodegenError {
                    message: "`exit` expects 1 positional argument on the Arduino Uno target".into(),
                    span: expr.span,
                });
            };
            let code = emit_arduino_uno_expr(scope, functions, structs, code_expr)?;
            if code.1 != ArduinoUnoType::I64 {
                return Err(CodegenError {
                    message: "Arduino Uno target requires integer exit code for `exit`".into(),
                    span: expr.span,
                });
            }
            out.push_str(&format!("{prefix}(void)({});\n", code.0));
            out.push_str(&format!("{prefix}for (;;) {{}}\n"));
            Ok(())
        }
        "pin_mode" => {
            let [CallArg::Positional(pin_expr), CallArg::Positional(mode_expr)] = args.as_slice() else {
                return Err(CodegenError {
                    message: "`pin_mode` expects 2 positional arguments on the Arduino Uno target".into(),
                    span: expr.span,
                });
            };
            let pin_rendered = emit_arduino_uno_expr(scope, functions, structs, pin_expr)?;
            let mode_rendered = emit_arduino_uno_expr(scope, functions, structs, mode_expr)?;
            if pin_rendered.1 != ArduinoUnoType::I64 || mode_rendered.1 != ArduinoUnoType::I64 {
                return Err(CodegenError {
                    message: "Arduino Uno target requires integer pin and mode for `pin_mode`".into(),
                    span: expr.span,
                });
            }
            out.push_str(&format!(
                "{prefix}pinMode((uint8_t)({}), (uint8_t)({}));\n",
                pin_rendered.0, mode_rendered.0
            ));
            Ok(())
        }
        "digital_write" => {
            let [CallArg::Positional(pin_expr), CallArg::Positional(value_expr)] = args.as_slice() else {
                return Err(CodegenError {
                    message: "`digital_write` expects 2 positional arguments on the Arduino Uno target".into(),
                    span: expr.span,
                });
            };
            let pin_rendered = emit_arduino_uno_expr(scope, functions, structs, pin_expr)?;
            let value_rendered = emit_arduino_uno_expr(scope, functions, structs, value_expr)?;
            if pin_rendered.1 != ArduinoUnoType::I64 || value_rendered.1 != ArduinoUnoType::Bool {
                return Err(CodegenError {
                    message: "Arduino Uno target requires integer pin and bool value for `digital_write`".into(),
                    span: expr.span,
                });
            }
            out.push_str(&format!(
                "{prefix}digitalWrite((uint8_t)({}), {} ? HIGH : LOW);\n",
                pin_rendered.0, value_rendered.0
            ));
            Ok(())
        }
        "analog_write" => {
            let [CallArg::Positional(pin_expr), CallArg::Positional(value_expr)] = args.as_slice() else {
                return Err(CodegenError {
                    message: "`analog_write` expects 2 positional arguments on the Arduino Uno target".into(),
                    span: expr.span,
                });
            };
            let pin_rendered = emit_arduino_uno_expr(scope, functions, structs, pin_expr)?;
            let value_rendered = emit_arduino_uno_expr(scope, functions, structs, value_expr)?;
            if pin_rendered.1 != ArduinoUnoType::I64 || value_rendered.1 != ArduinoUnoType::I64 {
                return Err(CodegenError {
                    message: "Arduino Uno target requires integer pin and integer PWM value for `analog_write`".into(),
                    span: expr.span,
                });
            }
            out.push_str(&format!(
                "{prefix}analogWrite((uint8_t)({}), (int)({}));\n",
                pin_rendered.0, value_rendered.0
            ));
            Ok(())
        }
        "analog_reference" => {
            let [CallArg::Positional(mode_expr)] = args.as_slice() else {
                return Err(CodegenError {
                    message: "`analog_reference` expects 1 positional argument on the Arduino Uno target".into(),
                    span: expr.span,
                });
            };
            let mode_rendered = emit_arduino_uno_expr(scope, functions, structs, mode_expr)?;
            if mode_rendered.1 != ArduinoUnoType::I64 {
                return Err(CodegenError {
                    message: "Arduino Uno target requires integer mode for `analog_reference`".into(),
                    span: expr.span,
                });
            }
            out.push_str(&format!(
                "{prefix}analogReference((uint8_t)({}));\n",
                mode_rendered.0
            ));
            Ok(())
        }
        "shift_out" => {
            let [CallArg::Positional(data_pin_expr), CallArg::Positional(clock_pin_expr), CallArg::Positional(bit_order_expr), CallArg::Positional(value_expr)] = args.as_slice() else {
                return Err(CodegenError {
                    message: "`shift_out` expects 4 positional arguments on the Arduino Uno target".into(),
                    span: expr.span,
                });
            };
            let data_pin = emit_arduino_uno_expr(scope, functions, structs, data_pin_expr)?;
            let clock_pin = emit_arduino_uno_expr(scope, functions, structs, clock_pin_expr)?;
            let bit_order = emit_arduino_uno_expr(scope, functions, structs, bit_order_expr)?;
            let value = emit_arduino_uno_expr(scope, functions, structs, value_expr)?;
            if data_pin.1 != ArduinoUnoType::I64
                || clock_pin.1 != ArduinoUnoType::I64
                || bit_order.1 != ArduinoUnoType::I64
                || value.1 != ArduinoUnoType::I64
            {
                return Err(CodegenError {
                    message: "Arduino Uno target requires integer arguments for `shift_out`".into(),
                    span: expr.span,
                });
            }
            out.push_str(&format!(
                "{prefix}shiftOut((uint8_t)({}), (uint8_t)({}), (uint8_t)({}), (uint8_t)({}));\n",
                data_pin.0, clock_pin.0, bit_order.0, value.0
            ));
            Ok(())
        }
        "interrupts_enable" | "interrupts_disable" => {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: format!("`{dispatch_name}` takes no arguments on the Arduino Uno target"),
                    span: expr.span,
                });
            }
            let runtime = if dispatch_name == "interrupts_enable" {
                "interrupts"
            } else {
                "noInterrupts"
            };
            out.push_str(&format!("{prefix}{runtime}();\n"));
            Ok(())
        }
        "random_seed" => {
            let [CallArg::Positional(seed_expr)] = args.as_slice() else {
                return Err(CodegenError {
                    message: "`random_seed` expects 1 positional argument on the Arduino Uno target".into(),
                    span: expr.span,
                });
            };
            let seed = emit_arduino_uno_expr(scope, functions, structs, seed_expr)?;
            if seed.1 != ArduinoUnoType::I64 {
                return Err(CodegenError {
                    message: "Arduino Uno target requires integer seed for `random_seed`".into(),
                    span: expr.span,
                });
            }
            out.push_str(&format!("{prefix}randomSeed((unsigned long)({}));\n", seed.0));
            Ok(())
        }
        "tone" => {
            let [CallArg::Positional(pin_expr), CallArg::Positional(freq_expr), CallArg::Positional(duration_expr)] = args.as_slice() else {
                return Err(CodegenError {
                    message: "`tone` expects 3 positional arguments on the Arduino Uno target".into(),
                    span: expr.span,
                });
            };
            let pin = emit_arduino_uno_expr(scope, functions, structs, pin_expr)?;
            let frequency = emit_arduino_uno_expr(scope, functions, structs, freq_expr)?;
            let duration = emit_arduino_uno_expr(scope, functions, structs, duration_expr)?;
            if pin.1 != ArduinoUnoType::I64
                || frequency.1 != ArduinoUnoType::I64
                || duration.1 != ArduinoUnoType::I64
            {
                return Err(CodegenError {
                    message: "Arduino Uno target requires integer arguments for `tone`".into(),
                    span: expr.span,
                });
            }
            out.push_str(&format!(
                "{prefix}tone((uint8_t)({}), (unsigned int)({}), (unsigned long)({}));\n",
                pin.0, frequency.0, duration.0
            ));
            Ok(())
        }
        "no_tone" => {
            let [CallArg::Positional(pin_expr)] = args.as_slice() else {
                return Err(CodegenError {
                    message: "`no_tone` expects 1 positional argument on the Arduino Uno target".into(),
                    span: expr.span,
                });
            };
            let pin = emit_arduino_uno_expr(scope, functions, structs, pin_expr)?;
            if pin.1 != ArduinoUnoType::I64 {
                return Err(CodegenError {
                    message: "Arduino Uno target requires integer pin for `no_tone`".into(),
                    span: expr.span,
                });
            }
            out.push_str(&format!("{prefix}noTone((uint8_t)({}));\n", pin.0));
            Ok(())
        }
        "servo_detach" => {
            let [CallArg::Positional(pin_expr)] = args.as_slice() else {
                return Err(CodegenError {
                    message: "`servo_detach` expects 1 positional argument on the Arduino Uno target".into(),
                    span: expr.span,
                });
            };
            let pin = emit_arduino_uno_expr(scope, functions, structs, pin_expr)?;
            if pin.1 != ArduinoUnoType::I64 {
                return Err(CodegenError {
                    message: "Arduino Uno target requires integer pin for `servo_detach`".into(),
                    span: expr.span,
                });
            }
            out.push_str(&format!("{}rune_rt_arduino_servo_detach({});\n", prefix, pin.0));
            Ok(())
        }
        "servo_write" | "servo_write_us" => {
            let [CallArg::Positional(pin_expr), CallArg::Positional(value_expr)] = args.as_slice() else {
                return Err(CodegenError {
                    message: format!("`{dispatch_name}` expects 2 positional arguments on the Arduino Uno target"),
                    span: expr.span,
                });
            };
            let pin = emit_arduino_uno_expr(scope, functions, structs, pin_expr)?;
            let value = emit_arduino_uno_expr(scope, functions, structs, value_expr)?;
            if pin.1 != ArduinoUnoType::I64 || value.1 != ArduinoUnoType::I64 {
                return Err(CodegenError {
                    message: format!("Arduino Uno target requires integer pin and integer value for `{dispatch_name}`"),
                    span: expr.span,
                });
            }
            let runtime = if dispatch_name == "servo_write" {
                "rune_rt_arduino_servo_write"
            } else {
                "rune_rt_arduino_servo_write_us"
            };
            out.push_str(&format!("{prefix}{runtime}({}, {});\n", pin.0, value.0));
            Ok(())
        }
        "delay_ms" => {
            let [CallArg::Positional(value)] = args.as_slice() else {
                return Err(CodegenError {
                    message: "`delay_ms` expects 1 positional argument on the Arduino Uno target".into(),
                    span: expr.span,
                });
            };
            let delay_rendered = emit_arduino_uno_expr(scope, functions, structs, value)?;
            if delay_rendered.1 != ArduinoUnoType::I64 {
                return Err(CodegenError {
                    message: "Arduino Uno target requires integer milliseconds for `delay_ms`".into(),
                    span: expr.span,
                });
            }
            out.push_str(&format!("{prefix}delay((unsigned long)({}));\n", delay_rendered.0));
            Ok(())
        }
        "delay_us" => {
            let [CallArg::Positional(value)] = args.as_slice() else {
                return Err(CodegenError {
                    message: "`delay_us` expects 1 positional argument on the Arduino Uno target".into(),
                    span: expr.span,
                });
            };
            let delay_rendered = emit_arduino_uno_expr(scope, functions, structs, value)?;
            if delay_rendered.1 != ArduinoUnoType::I64 {
                return Err(CodegenError {
                    message: "Arduino Uno target requires integer microseconds for `delay_us`".into(),
                    span: expr.span,
                });
            }
            out.push_str(&format!(
                "{prefix}delayMicroseconds((unsigned int)({}));\n",
                delay_rendered.0
            ));
            Ok(())
        }
        "uart_begin" => {
            let [CallArg::Positional(value)] = args.as_slice() else {
                return Err(CodegenError {
                    message: "`uart_begin` expects 1 positional argument on the Arduino Uno target".into(),
                    span: expr.span,
                });
            };
            let baud_rendered = emit_arduino_uno_expr(scope, functions, structs, value)?;
            if baud_rendered.1 != ArduinoUnoType::I64 {
                return Err(CodegenError {
                    message: "Arduino Uno target requires integer baud for `uart_begin`".into(),
                    span: expr.span,
                });
            }
            out.push_str(&format!(
                "{prefix}Serial.begin((unsigned long)({}));\n",
                baud_rendered.0
            ));
            Ok(())
        }
        "uart_write_byte" => {
            let [CallArg::Positional(value)] = args.as_slice() else {
                return Err(CodegenError {
                    message: "`uart_write_byte` expects 1 positional argument on the Arduino Uno target".into(),
                    span: expr.span,
                });
            };
            let value_rendered = emit_arduino_uno_expr(scope, functions, structs, value)?;
            if value_rendered.1 != ArduinoUnoType::I64 {
                return Err(CodegenError {
                    message: "Arduino Uno target requires integer byte value for `uart_write_byte`".into(),
                    span: expr.span,
                });
            }
            out.push_str(&format!(
                "{prefix}Serial.write((uint8_t)({}));\n",
                value_rendered.0
            ));
            Ok(())
        }
        "uart_write" => {
            let [CallArg::Positional(value)] = args.as_slice() else {
                return Err(CodegenError {
                    message: "`uart_write` expects 1 positional argument on the Arduino Uno target".into(),
                    span: expr.span,
                });
            };
            let rendered = emit_arduino_uno_expr(scope, functions, structs, value)?;
            if rendered.1 != ArduinoUnoType::String {
                return Err(CodegenError {
                    message: "Arduino Uno target requires String text for `uart_write`".into(),
                    span: expr.span,
                });
            }
            out.push_str(&format!("{prefix}Serial.print({});\n", rendered.0));
            Ok(())
        }
        "close" => {
            if !args.is_empty() {
                return Err(CodegenError {
                    message: "`close` takes no arguments on the Arduino Uno target".into(),
                    span: expr.span,
                });
            }
            Ok(())
        }
        _ => Err(CodegenError {
            message: "current Arduino Uno target supports `print`, `println`, `pin_mode`, `digital_write`, `analog_write`, `analog_reference`, `shift_out`, `interrupts_enable`, `interrupts_disable`, `random_seed`, `tone`, `no_tone`, `servo_detach`, `servo_write`, `servo_write_us`, `delay_ms`, `delay_us`, `uart_begin`, `uart_write_byte`, `uart_write`, `close`, `send`, and `send_line` statements".into(),
            span: callee.span,
        }),
    }
}

fn emit_arduino_uno_print_expr(
    out: &mut String,
    expr: &Expr,
    newline: bool,
    scope: &HashMap<String, ArduinoUnoType>,
    functions: &HashMap<String, ArduinoUnoFunctionSig>,
    structs: &HashMap<String, ArduinoUnoStructSig>,
    indent: usize,
) -> Result<(), CodegenError> {
    let prefix = "    ".repeat(indent);
    let rendered = emit_arduino_uno_expr(scope, functions, structs, expr)?;
    match rendered.1 {
        ArduinoUnoType::String => {
            out.push_str(&format!("{prefix}rune_serial_write_cstr({});\n", rendered.0));
        }
        ArduinoUnoType::Bool => {
            out.push_str(&format!("{prefix}rune_serial_write_bool({});\n", rendered.0));
        }
        ArduinoUnoType::I64 => {
            out.push_str(&format!("{prefix}rune_serial_write_i64({});\n", rendered.0));
        }
        ArduinoUnoType::Dynamic => {
            out.push_str(&format!("{prefix}rune_serial_write_dynamic({});\n", rendered.0));
        }
        ArduinoUnoType::Struct(_) => {
            let rendered = emit_arduino_uno_expr(
                scope,
                functions,
                structs,
                &build_str_call_expr(expr),
            )?;
            out.push_str(&format!("{prefix}rune_serial_write_cstr({});\n", rendered.0));
        }
    }
    if newline {
        out.push_str(&format!("{prefix}rune_serial_newline();\n"));
    }
    Ok(())
}

fn arduino_uno_common_compile_args(
    core_dir: &Path,
    variant_dir: &Path,
    servo_dir: Option<&Path>,
) -> Vec<String> {
    let mut args = vec![
        "-mmcu=atmega328p".into(),
        "-DF_CPU=16000000UL".into(),
        "-DARDUINO=10819".into(),
        "-DARDUINO_ARCH_AVR".into(),
        "-DARDUINO_AVR_UNO".into(),
        "-Os".into(),
        "-flto".into(),
        "-ffunction-sections".into(),
        "-fdata-sections".into(),
        format!("-I{}", core_dir.display()),
        format!("-I{}", variant_dir.display()),
    ];
    if let Some(servo_dir) = servo_dir {
        args.push(format!("-I{}", servo_dir.display()));
        let avr_dir = servo_dir.join("avr");
        if avr_dir.is_dir() {
            args.push(format!("-I{}", avr_dir.display()));
        }
    }
    args
}

fn compile_arduino_uno_core_sources(
    temp_dir: &Path,
    gcc: &Path,
    gpp: &Path,
    core_dir: &Path,
    variant_dir: &Path,
    servo_dir: Option<&Path>,
    include_servo_sources: bool,
) -> Result<Vec<PathBuf>, BuildError> {
    let mut sources = fs::read_dir(core_dir)
        .map_err(|source| BuildError::Io {
            context: format!("failed to read `{}`", core_dir.display()),
            source,
        })?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("c") | Some("cpp") | Some("S")
            )
        })
        .collect::<Vec<_>>();
    sources.sort();

    if include_servo_sources {
        if let Some(servo_dir) = servo_dir {
        let servo_cpp = servo_dir.join("avr").join("Servo.cpp");
        if servo_cpp.is_file() {
            sources.push(servo_cpp);
        }
    }
    }

    let common_args = arduino_uno_common_compile_args(
        core_dir,
        variant_dir,
        if include_servo_sources { servo_dir } else { None },
    );
    let mut objects = Vec::new();

    for source_path in sources {
        let extension = source_path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default();
        let object_name = format!(
            "core_{}_{}.o",
            source_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("arduino_core"),
            extension
        );
        let object_path = temp_dir.join(object_name);

        let mut command = if extension == "cpp" {
            let mut cmd = Command::new(gpp);
            cmd.args(&common_args)
                .arg("-std=gnu++11")
                .arg("-fno-exceptions")
                .arg("-fno-threadsafe-statics");
            cmd
        } else {
            let mut cmd = Command::new(gcc);
            cmd.args(&common_args);
            cmd
        };
        command
            .arg("-c")
            .arg(&source_path)
            .arg("-o")
            .arg(&object_path);

        let output = command.output().map_err(|source| BuildError::Io {
            context: format!("failed to run compiler for `{}`", source_path.display()),
            source,
        })?;
        if !output.status.success() {
            return Err(BuildError::ToolFailed {
                tool: if extension == "cpp" {
                    gpp.display().to_string()
                } else {
                    gcc.display().to_string()
                },
                status: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }
        objects.push(object_path);
    }

    Ok(objects)
}

fn program_uses_arduino_servo(program: &Program) -> bool {
    program.items.iter().any(item_uses_arduino_servo)
}

fn item_uses_arduino_servo(item: &Item) -> bool {
    match item {
        Item::Function(function) => block_uses_arduino_servo(&function.body),
        Item::Struct(struct_decl) => struct_decl
            .methods
            .iter()
            .any(|method| block_uses_arduino_servo(&method.body)),
        _ => false,
    }
}

fn block_uses_arduino_servo(block: &crate::parser::Block) -> bool {
    block.statements.iter().any(stmt_uses_arduino_servo)
}

fn stmt_uses_arduino_servo(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Block(block) => block_uses_arduino_servo(&block.block),
        Stmt::Let(let_stmt) => expr_uses_arduino_servo(&let_stmt.value),
        Stmt::Assign(assign_stmt) => expr_uses_arduino_servo(&assign_stmt.value),
        Stmt::Return(return_stmt) => return_stmt
            .value
            .as_ref()
            .is_some_and(expr_uses_arduino_servo),
        Stmt::If(if_stmt) => {
            expr_uses_arduino_servo(&if_stmt.condition)
                || block_uses_arduino_servo(&if_stmt.then_block)
                || if_stmt
                    .elif_blocks
                    .iter()
                    .any(|elif| {
                        expr_uses_arduino_servo(&elif.condition)
                            || block_uses_arduino_servo(&elif.block)
                    })
                || if_stmt
                    .else_block
                    .as_ref()
                    .is_some_and(block_uses_arduino_servo)
        }
        Stmt::While(while_stmt) => {
            expr_uses_arduino_servo(&while_stmt.condition)
                || block_uses_arduino_servo(&while_stmt.body)
        }
        Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::Raise(raise_stmt) => expr_uses_arduino_servo(&raise_stmt.value),
        Stmt::Panic(panic_stmt) => expr_uses_arduino_servo(&panic_stmt.value),
        Stmt::Expr(expr_stmt) => expr_uses_arduino_servo(&expr_stmt.expr),
    }
}

fn expr_uses_arduino_servo(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Identifier(_) | ExprKind::Integer(_) | ExprKind::String(_) | ExprKind::Bool(_) => {
            false
        }
        ExprKind::Unary { expr, .. } | ExprKind::Await { expr } => expr_uses_arduino_servo(expr),
        ExprKind::Binary { left, right, .. } => {
            expr_uses_arduino_servo(left) || expr_uses_arduino_servo(right)
        }
        ExprKind::Field { base, .. } => expr_uses_arduino_servo(base),
        ExprKind::Call { callee, args } => {
            let is_servo_builtin = match &callee.kind {
                ExprKind::Identifier(name) => {
                    matches!(
                        arduino_uno_builtin_alias(name),
                        "servo_attach" | "servo_detach" | "servo_write" | "servo_write_us"
                    )
                }
                _ => false,
            };
            is_servo_builtin
                || expr_uses_arduino_servo(callee)
                || args.iter().any(|arg| match arg {
                    CallArg::Positional(expr) => expr_uses_arduino_servo(expr),
                    CallArg::Keyword { value, .. } => expr_uses_arduino_servo(value),
                })
        }
    }
}

fn emit_arduino_uno_expr(
    scope: &HashMap<String, ArduinoUnoType>,
    functions: &HashMap<String, ArduinoUnoFunctionSig>,
    structs: &HashMap<String, ArduinoUnoStructSig>,
    expr: &Expr,
) -> Result<(String, ArduinoUnoType), CodegenError> {
    match &expr.kind {
        ExprKind::Identifier(name) => scope.get(name).cloned().map(|ty| (name.clone(), ty)).ok_or_else(
            || CodegenError {
                message: format!("Arduino Uno target does not know local `{name}`"),
                span: expr.span,
            },
        ),
        ExprKind::Integer(value) => Ok((format!("((int64_t)({value}))"), ArduinoUnoType::I64)),
        ExprKind::String(value) => Ok((format!("\"{}\"", c_escape(value)), ArduinoUnoType::String)),
        ExprKind::Bool(value) => Ok((
            if *value { "true".into() } else { "false".into() },
            ArduinoUnoType::Bool,
        )),
        ExprKind::Call { callee, args } => {
            if let ExprKind::Field { base, name } = &callee.kind {
                let base_rendered = emit_arduino_uno_expr(scope, functions, structs, base)?;
                let ArduinoUnoType::Struct(struct_name) = &base_rendered.1 else {
                    return Err(CodegenError {
                        message: "Arduino Uno target method calls require a concrete class or struct receiver".into(),
                        span: callee.span,
                    });
                };
                let struct_sig = structs.get(struct_name).ok_or_else(|| CodegenError {
                    message: format!("Arduino Uno target is missing struct layout for `{struct_name}`"),
                    span: callee.span,
                })?;
                let method_sig = struct_sig.methods.get(name).ok_or_else(|| CodegenError {
                    message: format!("Arduino Uno target struct `{struct_name}` has no method `{name}`"),
                    span: callee.span,
                })?;
                let Some(return_type) = method_sig.return_type.clone() else {
                    return Err(CodegenError {
                        message: format!(
                            "Arduino Uno target method `{struct_name}.{name}` does not return a value and cannot be used in an expression"
                        ),
                        span: expr.span,
                    });
                };
                let rendered = emit_arduino_uno_method_call(
                    scope,
                    functions,
                    structs,
                    struct_name,
                    name,
                    method_sig,
                    &base_rendered,
                    args,
                    expr.span,
                )?;
                return Ok((rendered, return_type));
            }
            let ExprKind::Identifier(name) = &callee.kind else {
                return Err(CodegenError {
                    message: "Arduino Uno target supports only direct builtin-style calls in expressions".into(),
                    span: callee.span,
                });
            };
            let dispatch_name = arduino_uno_builtin_alias(name);
            if !is_arduino_uno_builtin_dispatch_name(dispatch_name)
                && let Some(sig) = functions.get(dispatch_name)
            {
                let Some(return_type) = sig.return_type.clone() else {
                    return Err(CodegenError {
                        message: format!(
                            "Arduino Uno target function `{dispatch_name}` does not return a value and cannot be used in an expression"
                        ),
                        span: expr.span,
                    });
                };
                let rendered = emit_arduino_uno_function_call(
                    scope,
                    functions,
                    structs,
                    dispatch_name,
                    sig,
                    args,
                    expr.span,
                )?;
                return Ok((rendered, return_type));
            }
            if let Some(struct_sig) = structs.get(dispatch_name) {
                let rendered = emit_arduino_uno_constructor_call(
                    scope,
                    functions,
                    structs,
                    dispatch_name,
                    struct_sig,
                    args,
                    expr.span,
                )?;
                return Ok((rendered, ArduinoUnoType::Struct(dispatch_name.to_string())));
            }
            match dispatch_name {
                "digital_read" => {
                    let [CallArg::Positional(pin_expr)] = args.as_slice() else {
                        return Err(CodegenError {
                            message: "`digital_read` expects 1 positional argument on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    };
                    let pin_rendered = emit_arduino_uno_expr(scope, functions, structs, pin_expr)?;
                    if pin_rendered.1 != ArduinoUnoType::I64 {
                        return Err(CodegenError {
                            message: "Arduino Uno target requires integer pin for `digital_read`".into(),
                            span: expr.span,
                        });
                    }
                    Ok((format!("(digitalRead((uint8_t)({})) == HIGH)", pin_rendered.0), ArduinoUnoType::Bool))
                }
                "analog_read" => {
                    let [CallArg::Positional(pin_expr)] = args.as_slice() else {
                        return Err(CodegenError {
                            message: "`analog_read` expects 1 positional argument on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    };
                    let pin_rendered = emit_arduino_uno_expr(scope, functions, structs, pin_expr)?;
                    if pin_rendered.1 != ArduinoUnoType::I64 {
                        return Err(CodegenError {
                            message: "Arduino Uno target requires integer pin for `analog_read`".into(),
                            span: expr.span,
                        });
                    }
                    Ok((format!("((int64_t)analogRead((uint8_t)({})))", pin_rendered.0), ArduinoUnoType::I64))
                }
                "pulse_in" => {
                    let [CallArg::Positional(pin_expr), CallArg::Positional(state_expr), CallArg::Positional(timeout_expr)] = args.as_slice() else {
                        return Err(CodegenError {
                            message: "`pulse_in` expects 3 positional arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    };
                    let pin = emit_arduino_uno_expr(scope, functions, structs, pin_expr)?;
                    let state = emit_arduino_uno_expr(scope, functions, structs, state_expr)?;
                    let timeout = emit_arduino_uno_expr(scope, functions, structs, timeout_expr)?;
                    if pin.1 != ArduinoUnoType::I64 || state.1 != ArduinoUnoType::Bool || timeout.1 != ArduinoUnoType::I64 {
                        return Err(CodegenError {
                            message: "Arduino Uno target requires integer pin, bool state, and integer timeout for `pulse_in`".into(),
                            span: expr.span,
                        });
                    }
                    Ok((
                        format!(
                            "((int64_t)pulseIn((uint8_t)({}), {} ? HIGH : LOW, (unsigned long)({})))",
                            pin.0, state.0, timeout.0
                        ),
                        ArduinoUnoType::I64,
                    ))
                }
                "shift_in" => {
                    let [CallArg::Positional(data_pin_expr), CallArg::Positional(clock_pin_expr), CallArg::Positional(bit_order_expr)] = args.as_slice() else {
                        return Err(CodegenError {
                            message: "`shift_in` expects 3 positional arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    };
                    let data_pin = emit_arduino_uno_expr(scope, functions, structs, data_pin_expr)?;
                    let clock_pin = emit_arduino_uno_expr(scope, functions, structs, clock_pin_expr)?;
                    let bit_order = emit_arduino_uno_expr(scope, functions, structs, bit_order_expr)?;
                    if data_pin.1 != ArduinoUnoType::I64 || clock_pin.1 != ArduinoUnoType::I64 || bit_order.1 != ArduinoUnoType::I64 {
                        return Err(CodegenError {
                            message: "Arduino Uno target requires integer arguments for `shift_in`".into(),
                            span: expr.span,
                        });
                    }
                    Ok((
                        format!(
                            "((int64_t)shiftIn((uint8_t)({}), (uint8_t)({}), (uint8_t)({})))",
                            data_pin.0, clock_pin.0, bit_order.0
                        ),
                        ArduinoUnoType::I64,
                    ))
                }
                "servo_attach" => {
                    let [CallArg::Positional(pin_expr)] = args.as_slice() else {
                        return Err(CodegenError {
                            message: "`servo_attach` expects 1 positional argument on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    };
                    let pin = emit_arduino_uno_expr(scope, functions, structs, pin_expr)?;
                    if pin.1 != ArduinoUnoType::I64 {
                        return Err(CodegenError {
                            message: "Arduino Uno target requires integer pin for `servo_attach`".into(),
                            span: expr.span,
                        });
                    }
                    Ok((
                        format!("rune_rt_arduino_servo_attach({})", pin.0),
                        ArduinoUnoType::Bool,
                    ))
                }
                "millis" => {
                    if !args.is_empty() {
                        return Err(CodegenError {
                            message: "`millis` takes no arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    }
                    Ok(("((int64_t)millis())".into(), ArduinoUnoType::I64))
                }
                "micros" => {
                    if !args.is_empty() {
                        return Err(CodegenError {
                            message: "`micros` takes no arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    }
                    Ok(("((int64_t)micros())".into(), ArduinoUnoType::I64))
                }
                "random_i64" => {
                    let [CallArg::Positional(max_expr)] = args.as_slice() else {
                        return Err(CodegenError {
                            message: "`random_i64` expects 1 positional argument on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    };
                    let max_value = emit_arduino_uno_expr(scope, functions, structs, max_expr)?;
                    if max_value.1 != ArduinoUnoType::I64 {
                        return Err(CodegenError {
                            message: "Arduino Uno target requires integer max value for `random_i64`".into(),
                            span: expr.span,
                        });
                    }
                    Ok((
                        format!("((int64_t)random((long)({})))", max_value.0),
                        ArduinoUnoType::I64,
                    ))
                }
                "random_range" => {
                    let [CallArg::Positional(min_expr), CallArg::Positional(max_expr)] = args.as_slice() else {
                        return Err(CodegenError {
                            message: "`random_range` expects 2 positional arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    };
                    let min_value = emit_arduino_uno_expr(scope, functions, structs, min_expr)?;
                    let max_value = emit_arduino_uno_expr(scope, functions, structs, max_expr)?;
                    if min_value.1 != ArduinoUnoType::I64 || max_value.1 != ArduinoUnoType::I64 {
                        return Err(CodegenError {
                            message: "Arduino Uno target requires integer bounds for `random_range`".into(),
                            span: expr.span,
                        });
                    }
                    Ok((
                        format!("((int64_t)random((long)({}), (long)({})))", min_value.0, max_value.0),
                        ArduinoUnoType::I64,
                    ))
                }
                "pid" => {
                    if !args.is_empty() {
                        return Err(CodegenError {
                            message: "`pid` takes no arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    }
                    Ok(("((int64_t)1)".into(), ArduinoUnoType::I64))
                }
                "cpu_count" => {
                    if !args.is_empty() {
                        return Err(CodegenError {
                            message: "`cpu_count` takes no arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    }
                    Ok(("((int64_t)1)".into(), ArduinoUnoType::I64))
                }
                "input" | "read_line" => {
                    if !args.is_empty() {
                        return Err(CodegenError {
                            message: format!("`{name}` takes no arguments on the Arduino Uno target"),
                            span: expr.span,
                        });
                    }
                    Ok(("rune_serial_read_line()".into(), ArduinoUnoType::String))
                }
                "open" => {
                    let [CallArg::Positional(_port_expr), CallArg::Positional(baud_expr)] = args.as_slice() else {
                        return Err(CodegenError {
                            message: "`open` expects 2 positional arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    };
                    let baud = emit_arduino_uno_expr(scope, functions, structs, baud_expr)?;
                    if baud.1 != ArduinoUnoType::I64 {
                        return Err(CodegenError {
                            message: "Arduino Uno target requires integer baud for `open`".into(),
                            span: expr.span,
                        });
                    }
                    Ok((format!("(Serial.begin((unsigned long)({})), true)", baud.0), ArduinoUnoType::Bool))
                }
                "is_open" => {
                    if !args.is_empty() {
                        return Err(CodegenError {
                            message: "`is_open` takes no arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    }
                    Ok(("true".into(), ArduinoUnoType::Bool))
                }
                "recv_line" => {
                    if !args.is_empty() {
                        return Err(CodegenError {
                            message: "`recv_line` takes no arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    }
                    Ok(("rune_serial_read_line()".into(), ArduinoUnoType::String))
                }
                "send" => {
                    let [CallArg::Positional(value_expr)] = args.as_slice() else {
                        return Err(CodegenError {
                            message: "`send` expects 1 positional argument on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    };
                    let rendered = emit_arduino_uno_expr(scope, functions, structs, value_expr)?;
                    let text = arduino_uno_coerce_value(rendered, &ArduinoUnoType::String, expr.span)
                        .map_err(|_| CodegenError {
                            message: "Arduino Uno target requires String-convertible text for `send`".into(),
                            span: expr.span,
                        })?;
                    Ok((format!("(Serial.print({}), true)", text), ArduinoUnoType::Bool))
                }
                "send_line" => {
                    let [CallArg::Positional(value_expr)] = args.as_slice() else {
                        return Err(CodegenError {
                            message: "`send_line` expects 1 positional argument on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    };
                    let rendered = emit_arduino_uno_expr(scope, functions, structs, value_expr)?;
                    let text = arduino_uno_coerce_value(rendered, &ArduinoUnoType::String, expr.span)
                        .map_err(|_| CodegenError {
                            message: "Arduino Uno target requires String-convertible text for `send_line`".into(),
                            span: expr.span,
                        })?;
                    Ok((format!("(Serial.println({}), true)", text), ArduinoUnoType::Bool))
                }
                "str" => {
                    let [CallArg::Positional(value_expr)] = args.as_slice() else {
                        return Err(CodegenError {
                            message: "`str` expects 1 positional argument on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    };
                    let rendered = emit_arduino_uno_expr(scope, functions, structs, value_expr)?;
                    match rendered.1 {
                        ArduinoUnoType::String => Ok((rendered.0, ArduinoUnoType::String)),
                        ArduinoUnoType::I64 => Ok((
                            format!("rune_string_from_i64({})", rendered.0),
                            ArduinoUnoType::String,
                        )),
                        ArduinoUnoType::Bool => Ok((
                            format!("rune_string_from_bool({})", rendered.0),
                            ArduinoUnoType::String,
                        )),
                        ArduinoUnoType::Dynamic => Ok((
                            format!("rune_dynamic_to_string({})", rendered.0),
                            ArduinoUnoType::String,
                        )),
                        ArduinoUnoType::Struct(ref struct_name) => {
                            let struct_sig = structs.get(struct_name).ok_or_else(|| CodegenError {
                                message: format!("Arduino Uno target is missing struct layout for `{struct_name}`"),
                                span: expr.span,
                            })?;
                            if let Some(method_sig) = struct_sig.methods.get("__str__") {
                                if method_sig.params.len() != 1
                                    || method_sig.return_type != Some(ArduinoUnoType::String)
                                {
                                    return Err(CodegenError {
                                        message: format!(
                                            "Arduino Uno target `str` on `{struct_name}` requires `__str__`, when defined, to have signature `__str__(self) -> String`"
                                        ),
                                        span: expr.span,
                                    });
                                }
                                Ok((
                                    emit_arduino_uno_method_call(
                                        scope,
                                        functions,
                                        structs,
                                        &struct_name,
                                        "__str__",
                                        method_sig,
                                        &rendered,
                                        &[],
                                        expr.span,
                                    )?,
                                    ArduinoUnoType::String,
                                ))
                            } else {
                                emit_arduino_uno_expr(
                                    scope,
                                    functions,
                                    structs,
                                    &build_default_struct_string_expr(
                                        value_expr,
                                        struct_name,
                                        &struct_sig.fields,
                                    ),
                                )
                            }
                        }
                    }
                }
                "int" => {
                    let [CallArg::Positional(value_expr)] = args.as_slice() else {
                        return Err(CodegenError {
                            message: "`int` expects 1 positional argument on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    };
                    let rendered = emit_arduino_uno_expr(scope, functions, structs, value_expr)?;
                    match rendered.1 {
                        ArduinoUnoType::I64 => Ok((rendered.0, ArduinoUnoType::I64)),
                        ArduinoUnoType::Bool => Ok((
                            format!("((int64_t)({} ? 1 : 0))", rendered.0),
                            ArduinoUnoType::I64,
                        )),
                        ArduinoUnoType::String => Ok((
                            format!("rune_parse_i64({})", rendered.0),
                            ArduinoUnoType::I64,
                        )),
                        ArduinoUnoType::Dynamic => Ok((
                            format!("rune_dynamic_to_i64({})", rendered.0),
                            ArduinoUnoType::I64,
                        )),
                        ArduinoUnoType::Struct(_) => Err(CodegenError {
                            message: "Arduino Uno target does not convert structs to integers yet".into(),
                            span: expr.span,
                        }),
                    }
                }
                "sum_range" => {
                    let [
                        CallArg::Positional(start_expr),
                        CallArg::Positional(stop_expr),
                        CallArg::Positional(step_expr),
                    ] = args.as_slice()
                    else {
                        return Err(CodegenError {
                            message: "`sum(range(...))` expects exactly 3 positional arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    };
                    let start = emit_arduino_uno_expr(scope, functions, structs, start_expr)?;
                    let stop = emit_arduino_uno_expr(scope, functions, structs, stop_expr)?;
                    let step = emit_arduino_uno_expr(scope, functions, structs, step_expr)?;
                    if start.1 != ArduinoUnoType::I64
                        || stop.1 != ArduinoUnoType::I64
                        || step.1 != ArduinoUnoType::I64
                    {
                        return Err(CodegenError {
                            message: "Arduino Uno target requires integer arguments for `sum(range(...))`".into(),
                            span: expr.span,
                        });
                    }
                    Ok((
                        format!("rune_sum_range({}, {}, {})", start.0, stop.0, step.0),
                        ArduinoUnoType::I64,
                    ))
                }
                "uart_available" => {
                    if !args.is_empty() {
                        return Err(CodegenError {
                            message: "`uart_available` takes no arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    }
                    Ok(("((int64_t)Serial.available())".into(), ArduinoUnoType::I64))
                }
                "uart_read_byte" => {
                    if !args.is_empty() {
                        return Err(CodegenError {
                            message: "`uart_read_byte` takes no arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    }
                    Ok(("((int64_t)Serial.read())".into(), ArduinoUnoType::I64))
                }
                "mode_input" => {
                    if !args.is_empty() {
                        return Err(CodegenError {
                            message: "`mode_input` takes no arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    }
                    Ok(("((int64_t)rune_mode_input())".into(), ArduinoUnoType::I64))
                }
                "mode_output" => {
                    if !args.is_empty() {
                        return Err(CodegenError {
                            message: "`mode_output` takes no arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    }
                    Ok(("((int64_t)rune_mode_output())".into(), ArduinoUnoType::I64))
                }
                "mode_input_pullup" => {
                    if !args.is_empty() {
                        return Err(CodegenError {
                            message: "`mode_input_pullup` takes no arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    }
                    Ok(("((int64_t)rune_mode_input_pullup())".into(), ArduinoUnoType::I64))
                }
                "led_builtin" => {
                    if !args.is_empty() {
                        return Err(CodegenError {
                            message: "`led_builtin` takes no arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    }
                    Ok(("((int64_t)rune_led_builtin())".into(), ArduinoUnoType::I64))
                }
                "high" => {
                    if !args.is_empty() {
                        return Err(CodegenError {
                            message: "`high` takes no arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    }
                    Ok(("((int64_t)rune_high())".into(), ArduinoUnoType::I64))
                }
                "low" => {
                    if !args.is_empty() {
                        return Err(CodegenError {
                            message: "`low` takes no arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    }
                    Ok(("((int64_t)rune_low())".into(), ArduinoUnoType::I64))
                }
                "bit_order_lsb_first" => {
                    if !args.is_empty() {
                        return Err(CodegenError {
                            message: "`bit_order_lsb_first` takes no arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    }
                    Ok(("((int64_t)rune_bit_order_lsb_first())".into(), ArduinoUnoType::I64))
                }
                "bit_order_msb_first" => {
                    if !args.is_empty() {
                        return Err(CodegenError {
                            message: "`bit_order_msb_first` takes no arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    }
                    Ok(("((int64_t)rune_bit_order_msb_first())".into(), ArduinoUnoType::I64))
                }
                "analog_ref_default" => {
                    if !args.is_empty() {
                        return Err(CodegenError {
                            message: "`analog_ref_default` takes no arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    }
                    Ok(("((int64_t)rune_analog_ref_default())".into(), ArduinoUnoType::I64))
                }
                "analog_ref_internal" => {
                    if !args.is_empty() {
                        return Err(CodegenError {
                            message: "`analog_ref_internal` takes no arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    }
                    Ok(("((int64_t)rune_analog_ref_internal())".into(), ArduinoUnoType::I64))
                }
                "analog_ref_external" => {
                    if !args.is_empty() {
                        return Err(CodegenError {
                            message: "`analog_ref_external` takes no arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    }
                    Ok(("((int64_t)rune_analog_ref_external())".into(), ArduinoUnoType::I64))
                }
                "platform" => {
                    if !args.is_empty() {
                        return Err(CodegenError {
                            message: "`platform` takes no arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    }
                    Ok(("\"embedded\"".into(), ArduinoUnoType::String))
                }
                "arch" => {
                    if !args.is_empty() {
                        return Err(CodegenError {
                            message: "`arch` takes no arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    }
                    Ok(("\"avr\"".into(), ArduinoUnoType::String))
                }
                "target" => {
                    if !args.is_empty() {
                        return Err(CodegenError {
                            message: "`target` takes no arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    }
                    Ok((
                        "\"avr-atmega328p-arduino-uno\"".into(),
                        ArduinoUnoType::String,
                    ))
                }
                "board" => {
                    if !args.is_empty() {
                        return Err(CodegenError {
                            message: "`board` takes no arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    }
                    Ok(("\"arduino-uno\"".into(), ArduinoUnoType::String))
                }
                "is_embedded" => {
                    if !args.is_empty() {
                        return Err(CodegenError {
                            message: "`is_embedded` takes no arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    }
                    Ok(("true".into(), ArduinoUnoType::Bool))
                }
                "is_wasm" => {
                    if !args.is_empty() {
                        return Err(CodegenError {
                            message: "`is_wasm` takes no arguments on the Arduino Uno target".into(),
                            span: expr.span,
                        });
                    }
                    Ok(("false".into(), ArduinoUnoType::Bool))
                }
                _ => Err(CodegenError {
                    message: "current Arduino Uno target supports `digital_read`, `analog_read`, `pulse_in`, `shift_in`, `servo_attach`, `millis`, `micros`, `random_i64`, `random_range`, `input`, `read_line`, `open`, `is_open`, `recv_line`, `send`, `send_line`, `uart_available`, `uart_read_byte`, `mode_input`, `mode_output`, `mode_input_pullup`, `led_builtin`, `high`, `low`, `bit_order_lsb_first`, `bit_order_msb_first`, `analog_ref_default`, `analog_ref_internal`, `analog_ref_external`, `platform`, `arch`, `target`, `board`, `is_embedded`, and `is_wasm` expressions".into(),
                    span: expr.span,
                }),
            }
        }
        ExprKind::Field { base, name } => {
            let base_rendered = emit_arduino_uno_expr(scope, functions, structs, base)?;
            let ArduinoUnoType::Struct(struct_name) = &base_rendered.1 else {
                return Err(CodegenError {
                    message: "Arduino Uno target field access requires a concrete class or struct value".into(),
                    span: expr.span,
                });
            };
            let struct_sig = structs.get(struct_name).ok_or_else(|| CodegenError {
                message: format!("Arduino Uno target is missing struct layout for `{struct_name}`"),
                span: expr.span,
            })?;
            let Some((_, field_ty)) = struct_sig.fields.iter().find(|(field_name, _)| field_name == name) else {
                return Err(CodegenError {
                    message: format!("Arduino Uno target struct `{struct_name}` has no field `{name}`"),
                    span: expr.span,
                });
            };
            Ok((format!("({}).{}", base_rendered.0, name), field_ty.clone()))
        }
        ExprKind::Unary { op, expr: inner } => {
            let rendered = emit_arduino_uno_expr(scope, functions, structs, inner)?;
            match op {
                UnaryOp::Negate if rendered.1 == ArduinoUnoType::I64 => {
                    Ok((format!("(-{})", rendered.0), ArduinoUnoType::I64))
                }
                UnaryOp::Negate if rendered.1 == ArduinoUnoType::Dynamic => Ok((
                    format!(
                        "rune_dynamic_from_i64(-rune_dynamic_to_i64({}))",
                        rendered.0
                    ),
                    ArduinoUnoType::Dynamic,
                )),
                UnaryOp::Not if rendered.1 == ArduinoUnoType::Bool => {
                    Ok((format!("(!{})", rendered.0), ArduinoUnoType::Bool))
                }
                UnaryOp::Not if rendered.1 == ArduinoUnoType::Dynamic => Ok((
                    format!("rune_dynamic_from_bool(!rune_dynamic_truthy({}))", rendered.0),
                    ArduinoUnoType::Dynamic,
                )),
                _ => Err(CodegenError {
                    message: "Arduino Uno target received an unsupported unary operation".into(),
                    span: expr.span,
                }),
            }
        }
        ExprKind::Binary { left, op, right } => {
            let lhs = emit_arduino_uno_expr(scope, functions, structs, left)?;
            let rhs = emit_arduino_uno_expr(scope, functions, structs, right)?;
            if matches!(op, BinaryOp::EqualEqual | BinaryOp::NotEqual)
                && matches!((&lhs.1, &rhs.1), (ArduinoUnoType::Struct(a), ArduinoUnoType::Struct(b)) if a == b)
            {
                let ArduinoUnoType::Struct(struct_name) = &lhs.1 else {
                    unreachable!();
                };
                let struct_sig = structs.get(struct_name).ok_or_else(|| CodegenError {
                    message: format!("Arduino Uno target is missing struct layout for `{struct_name}`"),
                    span: expr.span,
                })?;
                if let Some(method_sig) = struct_sig.methods.get("__eq__") {
                    if method_sig.params.len() != 2
                        || method_sig.params[1].1 != ArduinoUnoType::Struct(struct_name.clone())
                        || method_sig.return_type != Some(ArduinoUnoType::Bool)
                    {
                        return Err(CodegenError {
                            message: format!(
                                "Arduino Uno target `__eq__` on `{struct_name}` requires signature `__eq__(self, other: {struct_name}) -> bool`"
                            ),
                            span: expr.span,
                        });
                    }
                    let rendered = emit_arduino_uno_method_call(
                        scope,
                        functions,
                        structs,
                        struct_name,
                        "__eq__",
                        method_sig,
                        &lhs,
                        &[CallArg::Positional((**right).clone())],
                        expr.span,
                    )?;
                    let rendered = if matches!(op, BinaryOp::NotEqual) {
                        format!("(!{rendered})")
                    } else {
                        rendered
                    };
                    return Ok((rendered, ArduinoUnoType::Bool));
                }
                return emit_arduino_uno_expr(
                    scope,
                    functions,
                    structs,
                    &build_default_struct_eq_expr(left, right, &struct_sig.fields, *op),
                );
            }
            match op {
                BinaryOp::Add
                | BinaryOp::Subtract
                | BinaryOp::Multiply
                | BinaryOp::Divide
                | BinaryOp::Modulo if lhs.1 == ArduinoUnoType::I64 && rhs.1 == ArduinoUnoType::I64 => {
                    Ok((
                        format!("({} {} {})", lhs.0, arduino_uno_binary_op(*op), rhs.0),
                        ArduinoUnoType::I64,
                    ))
                }
                BinaryOp::EqualEqual
                | BinaryOp::NotEqual
                | BinaryOp::Greater
                | BinaryOp::GreaterEqual
                | BinaryOp::Less
                | BinaryOp::LessEqual if lhs.1 == rhs.1 && lhs.1 != ArduinoUnoType::String => {
                    Ok((
                        format!("({} {} {})", lhs.0, arduino_uno_binary_op(*op), rhs.0),
                        ArduinoUnoType::Bool,
                    ))
                }
                BinaryOp::EqualEqual | BinaryOp::NotEqual
                    if lhs.1 == ArduinoUnoType::String && rhs.1 == ArduinoUnoType::String =>
                {
                    let base = format!("rune_string_eq({}, {})", lhs.0, rhs.0);
                    let rendered = match op {
                        BinaryOp::EqualEqual => base,
                        BinaryOp::NotEqual => format!("(!{base})"),
                        _ => unreachable!(),
                    };
                    Ok((rendered, ArduinoUnoType::Bool))
                }
                BinaryOp::Add
                    if lhs.1 == ArduinoUnoType::String && rhs.1 == ArduinoUnoType::String =>
                {
                    Ok((
                        format!("rune_string_concat({}, {})", lhs.0, rhs.0),
                        ArduinoUnoType::String,
                    ))
                }
                BinaryOp::And | BinaryOp::Or
                    if lhs.1 == ArduinoUnoType::Bool && rhs.1 == ArduinoUnoType::Bool =>
                {
                    Ok((
                        format!("({} {} {})", lhs.0, arduino_uno_binary_op(*op), rhs.0),
                        ArduinoUnoType::Bool,
                    ))
                }
                BinaryOp::And | BinaryOp::Or
                    if matches!(lhs.1, ArduinoUnoType::Bool | ArduinoUnoType::Dynamic)
                        && matches!(rhs.1, ArduinoUnoType::Bool | ArduinoUnoType::Dynamic) =>
                {
                    let left = arduino_uno_coerce_value(lhs, &ArduinoUnoType::Bool, expr.span)?;
                    let right = arduino_uno_coerce_value(rhs, &ArduinoUnoType::Bool, expr.span)?;
                    Ok((
                        format!("({} {} {})", left, arduino_uno_binary_op(*op), right),
                        ArduinoUnoType::Bool,
                    ))
                }
                BinaryOp::Add
                | BinaryOp::Subtract
                | BinaryOp::Multiply
                | BinaryOp::Divide
                | BinaryOp::Modulo
                    if arduino_uno_can_promote_to_dynamic(&lhs.1)
                        && arduino_uno_can_promote_to_dynamic(&rhs.1)
                        && (lhs.1 == ArduinoUnoType::Dynamic || rhs.1 == ArduinoUnoType::Dynamic) =>
                {
                    let left = arduino_uno_coerce_value(lhs, &ArduinoUnoType::Dynamic, expr.span)?;
                    let right = arduino_uno_coerce_value(rhs, &ArduinoUnoType::Dynamic, expr.span)?;
                    Ok((
                        format!(
                            "rune_dynamic_binary({}, {}, {})",
                            left,
                            right,
                            arduino_uno_dynamic_binary_opcode(*op)?
                        ),
                        ArduinoUnoType::Dynamic,
                    ))
                }
                BinaryOp::EqualEqual
                | BinaryOp::NotEqual
                | BinaryOp::Greater
                | BinaryOp::GreaterEqual
                | BinaryOp::Less
                | BinaryOp::LessEqual
                    if arduino_uno_can_promote_to_dynamic(&lhs.1)
                        && arduino_uno_can_promote_to_dynamic(&rhs.1)
                        && (lhs.1 == ArduinoUnoType::Dynamic || rhs.1 == ArduinoUnoType::Dynamic) =>
                {
                    let left = arduino_uno_coerce_value(lhs, &ArduinoUnoType::Dynamic, expr.span)?;
                    let right = arduino_uno_coerce_value(rhs, &ArduinoUnoType::Dynamic, expr.span)?;
                    Ok((
                        format!(
                            "rune_dynamic_compare({}, {}, {})",
                            left,
                            right,
                            arduino_uno_dynamic_compare_opcode(*op)?
                        ),
                        ArduinoUnoType::Bool,
                    ))
                }
                _ => Err(CodegenError {
                    message: "Arduino Uno target received an unsupported binary operation".into(),
                    span: expr.span,
                }),
            }
        }
        _ => Err(CodegenError {
            message: "current Arduino Uno target supports locals, literals, unary ops, binary ops, `if`, `while`, `print`, and `println`".into(),
            span: expr.span,
        }),
    }
}

fn collect_arduino_uno_helper_functions<'a>(
    program: &'a Program,
) -> Result<Vec<&'a crate::parser::Function>, CodegenError> {
    let mut helpers = Vec::new();
    for item in &program.items {
        let Item::Function(function) = item else {
            continue;
        };
        if matches!(function.name.as_str(), "main" | "setup" | "loop") {
            continue;
        }
        if function.is_extern || function.is_async {
            return Err(CodegenError {
                message: format!(
                    "Arduino Uno target does not support extern or async helper function `{}`",
                    function.name
                ),
                span: function.span,
            });
        }
        helpers.push(function);
    }
    Ok(helpers)
}

fn collect_arduino_uno_structs(
    program: &Program,
) -> Result<HashMap<String, ArduinoUnoStructSig>, CodegenError> {
    let mut structs = HashMap::new();
    for item in &program.items {
        let Item::Struct(decl) = item else {
            continue;
        };
        let mut fields = Vec::new();
        for field in &decl.fields {
            fields.push((field.name.clone(), arduino_uno_type_from_ref(&field.ty)?));
        }
        let mut methods = HashMap::new();
        for method in &decl.methods {
            if method.is_extern || method.is_async {
                return Err(CodegenError {
                    message: format!(
                        "Arduino Uno target does not support extern or async method `{}` on `{}`",
                        method.name, decl.name
                    ),
                    span: method.span,
                });
            }
            let mut params = Vec::new();
            for (index, param) in method.params.iter().enumerate() {
                if index == 0 && param.name == "self" {
                    params.push((param.name.clone(), ArduinoUnoType::Struct(decl.name.clone())));
                } else {
                    params.push((param.name.clone(), arduino_uno_type_from_ref(&param.ty)?));
                }
            }
            methods.insert(
                method.name.clone(),
                ArduinoUnoFunctionSig {
                    params,
                    return_type: method
                        .return_type
                        .as_ref()
                        .map(arduino_uno_function_return_type_from_ref)
                        .transpose()?
                        .flatten(),
                },
            );
        }
        structs.insert(decl.name.clone(), ArduinoUnoStructSig { fields, methods });
    }
    Ok(structs)
}

fn emit_arduino_uno_struct_definitions(
    structs: &HashMap<String, ArduinoUnoStructSig>,
) -> String {
    let mut names = structs.keys().cloned().collect::<Vec<_>>();
    names.sort();
    let mut out = String::new();
    for name in names {
        let sig = &structs[&name];
        out.push_str(&format!("typedef struct rune_struct_{name} {{\n"));
        for (field_name, field_ty) in &sig.fields {
            out.push_str(&format!("    {} {};\n", arduino_uno_c_type(field_ty), field_name));
        }
        out.push_str(&format!("}} rune_struct_{name};\n\n"));
    }
    out
}

fn emit_arduino_uno_method_prototypes(
    structs: &HashMap<String, ArduinoUnoStructSig>,
) -> String {
    let mut struct_names = structs.keys().cloned().collect::<Vec<_>>();
    struct_names.sort();
    let mut out = String::new();
    for struct_name in struct_names {
        let struct_sig = &structs[&struct_name];
        let mut method_names = struct_sig.methods.keys().cloned().collect::<Vec<_>>();
        method_names.sort();
        for method_name in method_names {
            let sig = &struct_sig.methods[&method_name];
            let ret = sig
                .return_type
                .as_ref()
                .map(arduino_uno_c_type)
                .unwrap_or("void");
            let params = sig
                .params
                .iter()
                .map(|(name, ty)| format!("{} {}", arduino_uno_c_type(ty), name))
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!(
                "{ret} rune_method_{}__{}({params});\n",
                struct_name, method_name
            ));
        }
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

fn emit_arduino_uno_function_prototypes(
    functions: &[&crate::parser::Function],
    signatures: &HashMap<String, ArduinoUnoFunctionSig>,
) -> Result<String, CodegenError> {
    let mut out = String::new();
    for function in functions {
        let sig = signatures.get(&function.name).ok_or_else(|| CodegenError {
            message: format!(
                "Arduino Uno target internal error: missing signature for `{}`",
                function.name
            ),
            span: function.span,
        })?;
        let ret = sig
            .return_type
            .as_ref()
            .map(arduino_uno_c_type)
            .unwrap_or("void");
        let params = sig
            .params
            .iter()
            .map(|(name, ty)| format!("{} {}", arduino_uno_c_type(ty), name))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("{ret} rune_fn_{}({params});\n", function.name));
    }
    if !out.is_empty() {
        out.push('\n');
    }
    Ok(out)
}

fn emit_arduino_uno_function_definitions(
    functions: &[&crate::parser::Function],
    signatures: &HashMap<String, ArduinoUnoFunctionSig>,
    structs: &HashMap<String, ArduinoUnoStructSig>,
) -> Result<String, CodegenError> {
    let mut out = String::new();
    for function in functions {
        let sig = signatures.get(&function.name).ok_or_else(|| CodegenError {
            message: format!(
                "Arduino Uno target internal error: missing signature for `{}`",
                function.name
            ),
            span: function.span,
        })?;
        let ret = sig
            .return_type
            .as_ref()
            .map(arduino_uno_c_type)
            .unwrap_or("void");
        let params = sig
            .params
            .iter()
            .map(|(name, ty)| format!("{} {}", arduino_uno_c_type(ty), name))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("{ret} rune_fn_{}({params}) {{\n", function.name));
        let mut scope = HashMap::new();
        for (name, ty) in &sig.params {
            scope.insert(name.clone(), ty.clone());
        }
        let return_kind = match sig.return_type.clone() {
            Some(ty) => ArduinoUnoReturnKind::Value(ty),
            None => ArduinoUnoReturnKind::Void,
        };
        emit_arduino_uno_block(
            &mut out,
            &function.body.statements,
            &mut scope,
            signatures,
            structs,
            return_kind.clone(),
            false,
            1,
        )?;
        out.push_str("}\n\n");
    }
    Ok(out)
}

fn emit_arduino_uno_method_definitions(
    program: &Program,
    functions: &HashMap<String, ArduinoUnoFunctionSig>,
    structs: &HashMap<String, ArduinoUnoStructSig>,
) -> Result<String, CodegenError> {
    let mut out = String::new();
    for item in &program.items {
        let Item::Struct(decl) = item else {
            continue;
        };
        for method in &decl.methods {
            let sig = structs
                .get(&decl.name)
                .and_then(|struct_sig| struct_sig.methods.get(&method.name))
                .ok_or_else(|| CodegenError {
                    message: format!(
                        "Arduino Uno target internal error: missing method signature for `{}.{}`",
                        decl.name, method.name
                    ),
                    span: method.span,
                })?;
            let ret = sig
                .return_type
                .as_ref()
                .map(arduino_uno_c_type)
                .unwrap_or("void");
            let params = sig
                .params
                .iter()
                .map(|(name, ty)| format!("{} {}", arduino_uno_c_type(ty), name))
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!(
                "{ret} rune_method_{}__{}({params}) {{\n",
                decl.name, method.name
            ));
            let mut scope = HashMap::new();
            for (name, ty) in &sig.params {
                scope.insert(name.clone(), ty.clone());
            }
            let return_kind = match sig.return_type.clone() {
                Some(ty) => ArduinoUnoReturnKind::Value(ty),
                None => ArduinoUnoReturnKind::Void,
            };
            emit_arduino_uno_block(
                &mut out,
                &method.body.statements,
                &mut scope,
                functions,
                structs,
                return_kind,
                false,
                1,
            )?;
            out.push_str("}\n\n");
        }
    }
    Ok(out)
}

fn emit_arduino_uno_function_call(
    scope: &HashMap<String, ArduinoUnoType>,
    functions: &HashMap<String, ArduinoUnoFunctionSig>,
    structs: &HashMap<String, ArduinoUnoStructSig>,
    name: &str,
    sig: &ArduinoUnoFunctionSig,
    args: &[CallArg],
    span: crate::lexer::Span,
) -> Result<String, CodegenError> {
    let resolved_args =
        resolve_arduino_uno_call_args(&format!("function `{name}`"), &sig.params, args, span)?;
    if resolved_args.len() != sig.params.len() {
        return Err(CodegenError {
            message: format!(
                "Arduino Uno target function `{name}` expects {} positional arguments",
                sig.params.len()
            ),
            span,
        });
    }
    let mut rendered_args = Vec::with_capacity(resolved_args.len());
    for (expr, (_, expected_ty)) in resolved_args.iter().zip(sig.params.iter()) {
        let rendered = emit_arduino_uno_expr(scope, functions, structs, expr)?;
        let value = arduino_uno_coerce_value(rendered, expected_ty, span).map_err(|_| CodegenError {
            message: format!(
                "Arduino Uno target function `{name}` requires concrete argument type matches"
            ),
            span,
        })?;
        rendered_args.push(value);
    }
    Ok(format!("rune_fn_{}({})", name, rendered_args.join(", ")))
}

fn emit_arduino_uno_method_call(
    scope: &HashMap<String, ArduinoUnoType>,
    functions: &HashMap<String, ArduinoUnoFunctionSig>,
    structs: &HashMap<String, ArduinoUnoStructSig>,
    struct_name: &str,
    method_name: &str,
    sig: &ArduinoUnoFunctionSig,
    base_rendered: &(String, ArduinoUnoType),
    args: &[CallArg],
    span: crate::lexer::Span,
) -> Result<String, CodegenError> {
    if sig.params.is_empty() {
        return Err(CodegenError {
            message: format!(
                "Arduino Uno target method `{struct_name}.{method_name}` is missing `self`"
            ),
            span,
        });
    }
    let resolved_args = resolve_arduino_uno_call_args(
        &format!("method `{struct_name}.{method_name}`"),
        &sig.params.iter().skip(1).cloned().collect::<Vec<_>>(),
        args,
        span,
    )?;
    if resolved_args.len() + 1 != sig.params.len() {
        return Err(CodegenError {
            message: format!(
                "Arduino Uno target method `{struct_name}.{method_name}` expects {} positional arguments",
                sig.params.len() - 1
            ),
            span,
        });
    }
    let mut rendered_args = vec![base_rendered.0.clone()];
    for (expr, (_, expected_ty)) in resolved_args.iter().zip(sig.params.iter().skip(1)) {
        let rendered = emit_arduino_uno_expr(scope, functions, structs, expr)?;
        let value = arduino_uno_coerce_value(rendered, expected_ty, span).map_err(|_| CodegenError {
            message: format!(
                "Arduino Uno target method `{struct_name}.{method_name}` requires concrete argument type matches"
            ),
            span,
        })?;
        rendered_args.push(value);
    }
    Ok(format!(
        "rune_method_{}__{}({})",
        struct_name,
        method_name,
        rendered_args.join(", ")
    ))
}

fn emit_arduino_uno_constructor_call(
    scope: &HashMap<String, ArduinoUnoType>,
    functions: &HashMap<String, ArduinoUnoFunctionSig>,
    structs: &HashMap<String, ArduinoUnoStructSig>,
    name: &str,
    sig: &ArduinoUnoStructSig,
    args: &[CallArg],
    span: crate::lexer::Span,
) -> Result<String, CodegenError> {
    if args.len() != sig.fields.len() {
        return Err(CodegenError {
            message: format!("Arduino Uno target struct `{name}` expects {} keyword arguments", sig.fields.len()),
            span,
        });
    }
    let mut rendered_values = Vec::with_capacity(sig.fields.len());
    for (field_name, field_ty) in &sig.fields {
        let Some(value_expr) = args.iter().find_map(|arg| match arg {
            CallArg::Keyword { name, value, .. } if name == field_name => Some(value),
            _ => None,
        }) else {
            return Err(CodegenError {
                message: format!("Arduino Uno target struct `{name}` is missing field `{field_name}`"),
                span,
            });
        };
        let rendered = emit_arduino_uno_expr(scope, functions, structs, value_expr)?;
        let value = arduino_uno_coerce_value(rendered, field_ty, span).map_err(|_| CodegenError {
            message: format!("Arduino Uno target struct `{name}` field `{field_name}` requires a concrete matching type"),
            span,
        })?;
        rendered_values.push(value);
    }
    Ok(format!("({}){{{}}}", arduino_uno_c_type(&ArduinoUnoType::Struct(name.to_string())), rendered_values.join(", ")))
}

fn resolve_arduino_uno_call_args<'a>(
    callable_name: &str,
    params: &[(String, ArduinoUnoType)],
    args: &'a [CallArg],
    span: crate::lexer::Span,
) -> Result<Vec<&'a Expr>, CodegenError> {
    let mut resolved: Vec<Option<&Expr>> = vec![None; params.len()];
    let mut positional_index = 0usize;
    let mut saw_keyword = false;

    for arg in args {
        match arg {
            CallArg::Positional(expr) => {
                if saw_keyword {
                    return Err(CodegenError {
                        message: format!(
                            "positional arguments cannot appear after keyword arguments in {callable_name}"
                        ),
                        span: expr.span,
                    });
                }
                if positional_index >= params.len() {
                    return Err(CodegenError {
                        message: format!(
                            "{callable_name} expects {} arguments but got {}",
                            params.len(),
                            args.len()
                        ),
                        span: expr.span,
                    });
                }
                resolved[positional_index] = Some(expr);
                positional_index += 1;
            }
            CallArg::Keyword {
                name,
                value,
                span: kw_span,
            } => {
                saw_keyword = true;
                let Some(index) = params.iter().position(|(param_name, _)| param_name == name) else {
                    return Err(CodegenError {
                        message: format!("{callable_name} has no parameter named `{name}`"),
                        span: *kw_span,
                    });
                };
                if resolved[index].is_some() {
                    return Err(CodegenError {
                        message: format!("parameter `{name}` was provided more than once"),
                        span: *kw_span,
                    });
                }
                resolved[index] = Some(value);
            }
        }
    }

    if resolved.iter().any(|arg| arg.is_none()) {
        return Err(CodegenError {
            message: format!(
                "{callable_name} expects {} arguments but got {}",
                params.len(),
                args.len()
            ),
            span,
        });
    }

    Ok(resolved
        .into_iter()
        .map(|arg| arg.expect("checked above"))
        .collect())
}

fn build_string_expr(span: crate::lexer::Span, value: impl Into<String>) -> Expr {
    Expr {
        kind: ExprKind::String(value.into()),
        span,
    }
}

fn build_bool_expr(span: crate::lexer::Span, value: bool) -> Expr {
    Expr {
        kind: ExprKind::Bool(value),
        span,
    }
}

fn build_identifier_expr(span: crate::lexer::Span, name: &str) -> Expr {
    Expr {
        kind: ExprKind::Identifier(name.to_string()),
        span,
    }
}

fn build_binary_add_expr(span: crate::lexer::Span, left: Expr, right: Expr) -> Expr {
    build_binary_expr(span, left, BinaryOp::Add, right)
}

fn build_binary_expr(span: crate::lexer::Span, left: Expr, op: BinaryOp, right: Expr) -> Expr {
    Expr {
        kind: ExprKind::Binary {
            left: Box::new(left),
            op,
            right: Box::new(right),
        },
        span,
    }
}

fn build_str_call_expr(expr: &Expr) -> Expr {
    Expr {
        kind: ExprKind::Call {
            callee: Box::new(build_identifier_expr(expr.span, "str")),
            args: vec![CallArg::Positional(expr.clone())],
        },
        span: expr.span,
    }
}

fn build_default_struct_string_expr(
    base: &Expr,
    struct_name: &str,
    fields: &[(String, ArduinoUnoType)],
) -> Expr {
    let span = base.span;
    let mut rendered = build_string_expr(span, format!("{struct_name}("));
    for (index, (field_name, _)) in fields.iter().enumerate() {
        if index > 0 {
            rendered = build_binary_add_expr(span, rendered, build_string_expr(span, ", "));
        }
        rendered = build_binary_add_expr(
            span,
            rendered,
            build_string_expr(span, format!("{field_name}=")),
        );
        let field_expr = Expr {
            kind: ExprKind::Field {
                base: Box::new(base.clone()),
                name: field_name.clone(),
            },
            span,
        };
        rendered = build_binary_add_expr(span, rendered, build_str_call_expr(&field_expr));
    }
    build_binary_add_expr(span, rendered, build_string_expr(span, ")"))
}

fn build_default_struct_eq_expr(
    left: &Expr,
    right: &Expr,
    fields: &[(String, ArduinoUnoType)],
    op: BinaryOp,
) -> Expr {
    let span = left.span;
    let mut rendered = build_bool_expr(span, true);
    for (field_name, _) in fields {
        let left_field = Expr {
            kind: ExprKind::Field {
                base: Box::new(left.clone()),
                name: field_name.clone(),
            },
            span,
        };
        let right_field = Expr {
            kind: ExprKind::Field {
                base: Box::new(right.clone()),
                name: field_name.clone(),
            },
            span,
        };
        let field_eq = build_binary_expr(span, left_field, BinaryOp::EqualEqual, right_field);
        rendered = build_binary_expr(span, rendered, BinaryOp::And, field_eq);
    }
    if matches!(op, BinaryOp::NotEqual) {
        Expr {
            kind: ExprKind::Unary {
                op: UnaryOp::Not,
                expr: Box::new(rendered),
            },
            span,
        }
    } else {
        rendered
    }
}

fn stmt_span(stmt: &Stmt) -> crate::lexer::Span {
    match stmt {
        Stmt::Block(stmt) => stmt.span,
        Stmt::Let(stmt) => stmt.span,
        Stmt::Assign(stmt) => stmt.span,
        Stmt::Return(stmt) => stmt.span,
        Stmt::If(stmt) => stmt.span,
        Stmt::While(stmt) => stmt.span,
        Stmt::Break(stmt) => stmt.span,
        Stmt::Continue(stmt) => stmt.span,
        Stmt::Raise(stmt) => stmt.span,
        Stmt::Panic(stmt) => stmt.span,
        Stmt::Expr(stmt) => stmt.expr.span,
    }
}

fn c_escape(text: &str) -> String {
    let mut escaped = String::new();
    for ch in text.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn arduino_uno_can_promote_to_dynamic(ty: &ArduinoUnoType) -> bool {
    matches!(
        ty,
        ArduinoUnoType::I64 | ArduinoUnoType::Bool | ArduinoUnoType::String | ArduinoUnoType::Dynamic
    )
}

fn arduino_uno_coerce_value(
    rendered: (String, ArduinoUnoType),
    expected: &ArduinoUnoType,
    span: crate::lexer::Span,
) -> Result<String, CodegenError> {
    let (expr, actual) = rendered;
    if &actual == expected {
        return Ok(expr);
    }
    match (actual, expected) {
        (ArduinoUnoType::I64, ArduinoUnoType::String) => Ok(format!("rune_string_from_i64({expr})")),
        (ArduinoUnoType::Bool, ArduinoUnoType::String) => {
            Ok(format!("rune_string_from_bool({expr})"))
        }
        (ArduinoUnoType::I64, ArduinoUnoType::Dynamic) => {
            Ok(format!("rune_dynamic_from_i64({expr})"))
        }
        (ArduinoUnoType::Bool, ArduinoUnoType::Dynamic) => {
            Ok(format!("rune_dynamic_from_bool({expr})"))
        }
        (ArduinoUnoType::String, ArduinoUnoType::Dynamic) => {
            Ok(format!("rune_dynamic_from_string({expr})"))
        }
        (ArduinoUnoType::Dynamic, ArduinoUnoType::String) => {
            Ok(format!("rune_dynamic_to_string({expr})"))
        }
        (ArduinoUnoType::Dynamic, ArduinoUnoType::I64) => Ok(format!("rune_dynamic_to_i64({expr})")),
        (ArduinoUnoType::Dynamic, ArduinoUnoType::Bool) => {
            Ok(format!("rune_dynamic_truthy({expr})"))
        }
        _ => Err(CodegenError {
            message: "Arduino Uno target requires concrete argument type matches".into(),
            span,
        }),
    }
}

fn arduino_uno_type_from_ref(ty: &TypeRef) -> Result<ArduinoUnoType, CodegenError> {
    match ty.name.as_str() {
        "i32" | "i64" | "int" => Ok(ArduinoUnoType::I64),
        "bool" => Ok(ArduinoUnoType::Bool),
        "String" | "string" => Ok(ArduinoUnoType::String),
        "dynamic" => Ok(ArduinoUnoType::Dynamic),
        _ => Ok(ArduinoUnoType::Struct(ty.name.clone())),
    }
}

fn arduino_uno_function_return_type_from_ref(
    ty: &TypeRef,
) -> Result<Option<ArduinoUnoType>, CodegenError> {
    match ty.name.as_str() {
        "unit" | "Unit" => Ok(None),
        _ => arduino_uno_type_from_ref(ty).map(Some),
    }
}

fn arduino_uno_c_type(ty: &ArduinoUnoType) -> &str {
    match ty {
        ArduinoUnoType::I64 => "int64_t",
        ArduinoUnoType::Bool => "bool",
        ArduinoUnoType::String => "const char*",
        ArduinoUnoType::Dynamic => "rune_dynamic_value",
        ArduinoUnoType::Struct(name) => Box::leak(format!("rune_struct_{name}").into_boxed_str()),
    }
}

fn arduino_uno_dynamic_binary_opcode(op: BinaryOp) -> Result<i64, CodegenError> {
    match op {
        BinaryOp::Add => Ok(0),
        BinaryOp::Subtract => Ok(1),
        BinaryOp::Multiply => Ok(2),
        BinaryOp::Divide => Ok(3),
        BinaryOp::Modulo => Ok(4),
        _ => Err(CodegenError {
            message: "Arduino Uno target received an unsupported dynamic binary operation".into(),
            span: crate::lexer::Span { line: 1, column: 1 },
        }),
    }
}

fn arduino_uno_dynamic_compare_opcode(op: BinaryOp) -> Result<i64, CodegenError> {
    match op {
        BinaryOp::EqualEqual => Ok(0),
        BinaryOp::NotEqual => Ok(1),
        BinaryOp::Greater => Ok(2),
        BinaryOp::GreaterEqual => Ok(3),
        BinaryOp::Less => Ok(4),
        BinaryOp::LessEqual => Ok(5),
        _ => Err(CodegenError {
            message: "Arduino Uno target received an unsupported dynamic comparison".into(),
            span: crate::lexer::Span { line: 1, column: 1 },
        }),
    }
}

fn arduino_uno_binary_op(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Subtract => "-",
        BinaryOp::Multiply => "*",
        BinaryOp::Divide => "/",
        BinaryOp::Modulo => "%",
        BinaryOp::EqualEqual => "==",
        BinaryOp::NotEqual => "!=",
        BinaryOp::Greater => ">",
        BinaryOp::GreaterEqual => ">=",
        BinaryOp::Less => "<",
        BinaryOp::LessEqual => "<=",
        BinaryOp::And => "&&",
        BinaryOp::Or => "||",
    }
}

fn arduino_uno_builtin_alias(name: &str) -> &str {
    match name {
        "__rune_builtin_arduino_pin_mode" => "pin_mode",
        "__rune_builtin_arduino_digital_write" => "digital_write",
        "__rune_builtin_arduino_digital_read" => "digital_read",
        "__rune_builtin_arduino_analog_write" => "analog_write",
        "__rune_builtin_arduino_analog_read" => "analog_read",
        "__rune_builtin_arduino_analog_reference" => "analog_reference",
        "__rune_builtin_arduino_pulse_in" => "pulse_in",
        "__rune_builtin_arduino_shift_out" => "shift_out",
        "__rune_builtin_arduino_shift_in" => "shift_in",
        "__rune_builtin_arduino_tone" => "tone",
        "__rune_builtin_arduino_no_tone" => "no_tone",
        "__rune_builtin_arduino_servo_attach" => "servo_attach",
        "__rune_builtin_arduino_servo_detach" => "servo_detach",
        "__rune_builtin_arduino_servo_write" => "servo_write",
        "__rune_builtin_arduino_servo_write_us" => "servo_write_us",
        "__rune_builtin_arduino_delay_ms" => "delay_ms",
        "__rune_builtin_arduino_delay_us" => "delay_us",
        "__rune_builtin_arduino_millis" => "millis",
        "__rune_builtin_arduino_micros" => "micros",
        "__rune_builtin_arduino_read_line" => "read_line",
        "__rune_builtin_arduino_mode_input" => "mode_input",
        "__rune_builtin_arduino_mode_output" => "mode_output",
        "__rune_builtin_arduino_mode_input_pullup" => "mode_input_pullup",
        "__rune_builtin_arduino_led_builtin" => "led_builtin",
        "__rune_builtin_arduino_high" => "high",
        "__rune_builtin_arduino_low" => "low",
        "__rune_builtin_arduino_bit_order_lsb_first" => "bit_order_lsb_first",
        "__rune_builtin_arduino_bit_order_msb_first" => "bit_order_msb_first",
        "__rune_builtin_arduino_analog_ref_default" => "analog_ref_default",
        "__rune_builtin_arduino_analog_ref_internal" => "analog_ref_internal",
        "__rune_builtin_arduino_analog_ref_external" => "analog_ref_external",
        "__rune_builtin_arduino_uart_begin" => "uart_begin",
        "__rune_builtin_arduino_uart_available" => "uart_available",
        "__rune_builtin_arduino_uart_read_byte" => "uart_read_byte",
        "__rune_builtin_arduino_uart_write_byte" => "uart_write_byte",
        "__rune_builtin_arduino_uart_write" => "uart_write",
        "__rune_builtin_arduino_interrupts_enable" => "interrupts_enable",
        "__rune_builtin_arduino_interrupts_disable" => "interrupts_disable",
        "__rune_builtin_arduino_random_seed" => "random_seed",
        "__rune_builtin_arduino_random_i64" => "random_i64",
        "__rune_builtin_arduino_random_range" => "random_range",
        "__rune_builtin_serial_open" => "open",
        "__rune_builtin_serial_is_open" => "is_open",
        "__rune_builtin_serial_close" => "close",
        "__rune_builtin_serial_read_line" => "recv_line",
        "__rune_builtin_serial_write" => "send",
        "__rune_builtin_serial_write_line" => "send_line",
        "__rune_builtin_sum_range" => "sum_range",
        "__rune_builtin_system_pid" => "pid",
        "__rune_builtin_system_cpu_count" => "cpu_count",
        "__rune_builtin_system_exit" => "exit",
        "__rune_builtin_system_platform" => "platform",
        "__rune_builtin_system_arch" => "arch",
        "__rune_builtin_system_target" => "target",
        "__rune_builtin_system_board" => "board",
        "__rune_builtin_system_is_embedded" => "is_embedded",
        "__rune_builtin_system_is_wasm" => "is_wasm",
        _ => name,
    }
}

fn is_arduino_uno_builtin_dispatch_name(name: &str) -> bool {
    matches!(
        name,
        "print"
            | "println"
            | "pin_mode"
            | "digital_write"
            | "digital_read"
            | "analog_write"
            | "analog_read"
            | "analog_reference"
            | "pulse_in"
            | "shift_out"
            | "shift_in"
            | "interrupts_enable"
            | "interrupts_disable"
            | "random_seed"
            | "random_i64"
            | "random_range"
            | "tone"
            | "no_tone"
            | "servo_attach"
            | "servo_detach"
            | "servo_write"
            | "servo_write_us"
            | "delay_ms"
            | "delay_us"
            | "millis"
            | "micros"
            | "input"
            | "read_line"
            | "open"
            | "is_open"
            | "close"
            | "recv_line"
            | "send"
            | "send_line"
            | "str"
            | "int"
            | "sum_range"
            | "uart_begin"
            | "uart_available"
            | "uart_read_byte"
            | "uart_write_byte"
            | "uart_write"
            | "mode_input"
            | "mode_output"
            | "mode_input_pullup"
            | "led_builtin"
            | "high"
            | "low"
            | "bit_order_lsb_first"
            | "bit_order_msb_first"
            | "analog_ref_default"
            | "analog_ref_internal"
            | "analog_ref_external"
            | "pid"
            | "cpu_count"
            | "exit"
            | "platform"
            | "arch"
            | "target"
            | "board"
            | "is_embedded"
            | "is_wasm"
    )
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
    if target_spec.platform == TargetPlatform::Embedded {
        return Err(BuildError::UnsupportedBackendForTarget(
            "freestanding embedded targets currently support `rune build --object` and `rune build --static-lib`, not executable linking".into(),
        ));
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
    let wrapper_path = temp_dir.join("main_wrapper.rs");
    let wrapper_obj_path = temp_dir.join("main_wrapper.o");
    let obj_path = temp_dir.join(object_file_name(target_spec));
    fs::write(&wrapper_path, rust_unix_llvm_wrapper_object_source()).map_err(|source| BuildError::Io {
        context: format!("failed to write `{}`", wrapper_path.display()),
        source,
    })?;
    compile_rust_object(&wrapper_path, &wrapper_obj_path)?;

    emit_object_file(program, target_spec.triple, &obj_path).map_err(|error| {
        BuildError::Codegen(CodegenError {
            message: error.message,
            span: crate::lexer::Span { line: 1, column: 1 },
        })
    })?;

    let compiled_c_objects = compile_c_sources(&temp_dir, target_spec, &options.link_c_sources)?;

    let mut link_objects = Vec::with_capacity(2 + compiled_c_objects.len());
    link_objects.push(obj_path.clone());
    link_objects.push(wrapper_obj_path.clone());
    link_objects.extend(compiled_c_objects.iter().cloned());
    link_with_packaged_clang(target_spec, &link_objects, output_path, false, options)?;

    let _ = fs::remove_file(wrapper_path);
    let _ = fs::remove_file(wrapper_obj_path);
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
            object_extension: target_spec.object_extension,
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

  function runeStringFromI64(value) {{
    return allocString(BigInt(value).toString());
  }}

  function runeStringFromBool(value) {{
    return allocString((value !== 0n && value !== 0) ? "true" : "false");
  }}

  function runeStringToI64(ptr, len) {{
    const text = readString(ptr, len).trim();
    const parsed = Number.parseInt(text || "0", 10);
    return Number.isFinite(parsed) ? BigInt(parsed) : 0n;
  }}

  function runeStringCompare(leftPtr, leftLen, rightPtr, rightLen) {{
    const left = readString(leftPtr, leftLen);
    const right = readString(rightPtr, rightLen);
    if (left < right) {{
      return -1;
    }}
    if (left > right) {{
      return 1;
    }}
    return 0;
  }}

  function runeStringConcat(leftPtr, leftLen, rightPtr, rightLen) {{
    return allocString(readString(leftPtr, leftLen) + readString(rightPtr, rightLen));
  }}

  function runeDynamicToString(tag, payload, extra) {{
    switch (Number(tag)) {{
      case 0:
        return allocString("unit");
      case 1:
        return allocString((payload !== 0n && payload !== 0) ? "true" : "false");
      case 2:
      case 3:
        return allocString(BigInt(payload).toString());
      case 4:
        return allocString(readString(payload, extra));
      default:
        return allocString(`<dynamic:${{Number(tag)}}>`);
    }}
  }}

  function runeDynamicTruthy(tag, payload, extra) {{
    switch (Number(tag)) {{
      case 0:
        return false;
      case 1:
        return payload !== 0n && payload !== 0;
      case 2:
      case 3:
        return BigInt(payload) !== 0n;
      case 4:
        return Number(extra) !== 0;
      default:
        return false;
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

  function tcpListen(host, port) {{
    if (!isNode) {{
      return false;
    }}
    const probe = [
      "const net = require('net');",
      "const host = process.argv[1];",
      "const port = Number(process.argv[2]);",
      "const server = net.createServer();",
      "let done = false;",
      "function finish(ok) {{ if (!done) {{ done = true; try {{ server.close(); }} catch (_) {{}} process.exit(ok ? 0 : 1); }} }}",
      "server.once('error', () => finish(false));",
      "server.listen(port, host, () => finish(true));"
    ].join("");
    const result = childProcess.spawnSync(process.execPath, ["-e", probe, host, String(port)], {{
      stdio: "ignore"
    }});
    return result.status === 0;
  }}

  function udpBind(host, port) {{
    if (!isNode) {{
      return false;
    }}
    const probe = [
      "const dgram = require('dgram');",
      "const host = process.argv[1];",
      "const port = Number(process.argv[2]);",
      "const socket = dgram.createSocket('udp4');",
      "let done = false;",
      "function finish(ok) {{ if (!done) {{ done = true; try {{ socket.close(); }} catch (_) {{}} process.exit(ok ? 0 : 1); }} }}",
      "socket.once('error', () => finish(false));",
      "socket.bind(port, host, () => finish(true));"
    ].join("");
    const result = childProcess.spawnSync(process.execPath, ["-e", probe, host, String(port)], {{
      stdio: "ignore"
    }});
    return result.status === 0;
  }}

  function tcpSend(host, port, payload) {{
    if (!isNode) {{
      return false;
    }}
    const probe = [
      "const net = require('net');",
      "const host = process.argv[1];",
      "const port = Number(process.argv[2]);",
      "const payload = process.argv[3] ?? '';",
      "const socket = new net.Socket();",
      "let done = false;",
      "function finish(ok) {{ if (!done) {{ done = true; try {{ socket.destroy(); }} catch (_) {{}} process.exit(ok ? 0 : 1); }} }}",
      "socket.setTimeout(500);",
      "socket.once('connect', () => {{ socket.end(payload, 'utf8', () => finish(true)); }});",
      "socket.once('timeout', () => finish(false));",
      "socket.once('error', () => finish(false));",
      "socket.connect(port, host);"
    ].join("");
    const result = childProcess.spawnSync(process.execPath, ["-e", probe, host, String(port), payload], {{
      stdio: "ignore"
    }});
    return result.status === 0;
  }}


  function udpSend(host, port, payload) {{
    if (!isNode) {{
      return false;
    }}
    const probe = [
      "const dgram = require('dgram');",
      "const host = process.argv[1];",
      "const port = Number(process.argv[2]);",
      "const payload = process.argv[3] ?? '';",
      "const socket = dgram.createSocket('udp4');",
      "let done = false;",
      "function finish(ok) {{ if (!done) {{ done = true; try {{ socket.close(); }} catch (_) {{}} process.exit(ok ? 0 : 1); }} }}",
      "socket.once('error', () => finish(false));",
      "socket.send(Buffer.from(payload, 'utf8'), port, host, (err) => finish(!err));"
    ].join("");
    const result = childProcess.spawnSync(process.execPath, ["-e", probe, host, String(port), payload], {{
      stdio: "ignore"
    }});
    return result.status === 0;
  }}

  const imports = {{
    env: {{
      rune_rt_print_i64(value) {{ writeText("stdout", value.toString()); }},
      rune_rt_eprint_i64(value) {{ writeText("stderr", value.toString()); }},
      rune_rt_print_bool(value) {{ writeText("stdout", value !== 0n && value !== 0 ? "true" : "false"); }},
      rune_rt_eprint_bool(value) {{ writeText("stderr", value !== 0n && value !== 0 ? "true" : "false"); }},
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
      rune_rt_string_compare(leftPtr, leftLen, rightPtr, rightLen) {{
        return runeStringCompare(leftPtr, leftLen, rightPtr, rightLen);
      }},
      rune_rt_string_concat(leftPtr, leftLen, rightPtr, rightLen) {{
        return runeStringConcat(leftPtr, leftLen, rightPtr, rightLen);
      }},
      rune_rt_string_from_i64(value) {{
        return runeStringFromI64(value);
      }},
      rune_rt_string_from_bool(value) {{
        return runeStringFromBool(value);
      }},
      rune_rt_string_to_i64(ptr, len) {{
        return runeStringToI64(ptr, len);
      }},
      rune_rt_dynamic_to_string(tag, payload, extra) {{
        return runeDynamicToString(tag, payload, extra);
      }},
      rune_rt_dynamic_truthy(tag, payload, extra) {{
        return runeDynamicTruthy(tag, payload, extra);
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
      rune_rt_time_monotonic_us() {{
        const now = (perf && typeof perf.now === "function") ? perf.now() : Date.now();
        return BigInt(Math.floor((now - monotonicStart) * 1000));
      }},
      rune_rt_time_sleep_ms(ms) {{
        sleepMs(ms);
      }},
      rune_rt_time_sleep_us(us) {{
        const millis = Math.ceil(Number(us) / 1000);
        sleepMs(millis);
      }},
      rune_rt_system_pid() {{
        return isNode ? (process.pid | 0) : 0;
      }},
      rune_rt_system_platform() {{
        return allocString("wasm");
      }},
      rune_rt_system_arch() {{
        return allocString("wasm32");
      }},
      rune_rt_system_target() {{
        return allocString("wasm32-unknown-unknown");
      }},
      rune_rt_system_board() {{
        return allocString("wasm");
      }},
      rune_rt_system_is_embedded() {{
        return false;
      }},
      rune_rt_system_is_wasm() {{
        return true;
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
      rune_rt_env_get_string(ptr, len, defaultPtr, defaultLen) {{
        if (!isNode) {{
          return allocString(readString(defaultPtr, defaultLen));
        }}
        const key = readString(ptr, len);
        const raw = process.env[key];
        if (raw == null) {{
          return allocString(readString(defaultPtr, defaultLen));
        }}
        return allocString(String(raw));
      }},
      rune_rt_env_arg_count() {{
        return hostArgv.length | 0;
      }},
      rune_rt_env_arg(index) {{
        const argIndex = Number(index) | 0;
        const value = argIndex >= 0 && argIndex < hostArgv.length ? hostArgv[argIndex] : "";
        return allocString(value);
      }},
      rune_rt_network_tcp_connect(ptr, len, port) {{
        return tcpConnect(readString(ptr, len), Number(port), 250);
      }},
      rune_rt_network_tcp_listen(ptr, len, port) {{
        return tcpListen(readString(ptr, len), Number(port));
      }},
      rune_rt_network_tcp_send(hostPtr, hostLen, port, dataPtr, dataLen) {{
        return tcpSend(readString(hostPtr, hostLen), Number(port), readString(dataPtr, dataLen));
      }},
      rune_rt_network_tcp_recv(ptr, len, port, maxBytes) {{
        throw new Error("Rune network receive builtins are not supported for wasm32-unknown-unknown");
      }},
      rune_rt_network_tcp_recv_timeout(ptr, len, port, maxBytes, timeoutMs) {{
        throw new Error("Rune network receive builtins are not supported for wasm32-unknown-unknown");
      }},
      rune_rt_network_tcp_accept_once(ptr, len, port, maxBytes, timeoutMs) {{
        throw new Error("Rune network server builtins are not supported for wasm32-unknown-unknown");
      }},
      rune_rt_network_tcp_reply_once(hostPtr, hostLen, port, dataPtr, dataLen, maxBytes, timeoutMs) {{
        throw new Error("Rune network server builtins are not supported for wasm32-unknown-unknown");
      }},
      rune_rt_network_tcp_server_open(ptr, len, port) {{
        throw new Error("Rune persistent network server builtins are not supported for wasm32-unknown-unknown");
      }},
      rune_rt_network_tcp_client_open(ptr, len, port, timeoutMs) {{
        throw new Error("Rune persistent network client builtins are not supported for wasm32-unknown-unknown");
      }},
      rune_rt_network_tcp_server_accept(handle, maxBytes, timeoutMs) {{
        throw new Error("Rune persistent network server builtins are not supported for wasm32-unknown-unknown");
      }},
      rune_rt_network_tcp_server_reply(handle, dataPtr, dataLen, maxBytes, timeoutMs) {{
        throw new Error("Rune persistent network server builtins are not supported for wasm32-unknown-unknown");
      }},
      rune_rt_network_tcp_server_close(handle) {{
        throw new Error("Rune persistent network server builtins are not supported for wasm32-unknown-unknown");
      }},
      rune_rt_network_tcp_client_send(handle, dataPtr, dataLen) {{
        throw new Error("Rune persistent network client builtins are not supported for wasm32-unknown-unknown");
      }},
      rune_rt_network_tcp_client_recv(handle, maxBytes, timeoutMs) {{
        throw new Error("Rune persistent network client builtins are not supported for wasm32-unknown-unknown");
      }},
      rune_rt_network_tcp_client_close(handle) {{
        throw new Error("Rune persistent network client builtins are not supported for wasm32-unknown-unknown");
      }},
      rune_rt_network_last_error_code() {{
        return 0;
      }},
      rune_rt_network_last_error_message() {{
        return allocString("");
      }},
      rune_rt_network_clear_error_state() {{
        return;
      }},
      rune_rt_network_tcp_request(hostPtr, hostLen, port, dataPtr, dataLen, maxBytes, timeoutMs) {{
        throw new Error("Rune network request builtins are not supported for wasm32-unknown-unknown");
      }},
      rune_rt_network_tcp_connect_timeout(ptr, len, port, timeoutMs) {{
        return tcpConnect(readString(ptr, len), Number(port), Number(timeoutMs));
      }},
      rune_rt_network_udp_bind(ptr, len, port) {{
        return udpBind(readString(ptr, len), Number(port));
      }},
      rune_rt_network_udp_send(hostPtr, hostLen, port, dataPtr, dataLen) {{
        return udpSend(readString(hostPtr, hostLen), Number(port), readString(dataPtr, dataLen));
      }},
      rune_rt_network_udp_recv(ptr, len, port, maxBytes, timeoutMs) {{
        throw new Error("Rune network receive builtins are not supported for wasm32-unknown-unknown");
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
        TargetPlatform::Linux
        | TargetPlatform::MacOS
        | TargetPlatform::Wasm
        | TargetPlatform::Embedded => {
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
    match target_spec.object_extension {
        "obj" => "out.obj",
        _ => "out.o",
    }
}

fn find_rustc() -> Option<PathBuf> {
    if let Some(explicit) = env::var_os("RUSTC") {
        let candidate = PathBuf::from(explicit);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    let path_names: &[&str] = if cfg!(target_os = "windows") {
        &["rustc.exe", "rustc"]
    } else {
        &["rustc"]
    };
    if let Some(path_var) = env::var_os("PATH") {
        for dir in env::split_paths(&path_var) {
            for name in path_names {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }

    let home = env::var_os(if cfg!(target_os = "windows") {
        "USERPROFILE"
    } else {
        "HOME"
    })?;
    let rustup_bin = if cfg!(target_os = "windows") {
        PathBuf::from(&home)
            .join(".cargo")
            .join("bin")
            .join("rustc.exe")
    } else {
        PathBuf::from(&home).join(".cargo").join("bin").join("rustc")
    };
    if rustup_bin.is_file() {
        return Some(rustup_bin);
    }

    None
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
void rune_rt_print_bool(int64_t value) { rune_rt_init_io(); fputs(value ? "true" : "false", stdout); fflush(stdout); }
void rune_rt_eprint_bool(int64_t value) { rune_rt_init_io(); fputs(value ? "true" : "false", stderr); fflush(stderr); }
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
int32_t rune_rt_string_compare(const char* left_ptr, int64_t left_len, const char* right_ptr, int64_t right_len) {
    int64_t limit = left_len < right_len ? left_len : right_len;
    int cmp = memcmp(left_ptr, right_ptr, (size_t)limit);
    if (cmp != 0) {
        return cmp < 0 ? -1 : 1;
    }
    if (left_len < right_len) {
        return -1;
    }
    if (left_len > right_len) {
        return 1;
    }
    return 0;
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
char* rune_rt_system_platform(void) {
#if defined(_WIN32)
    return rune_rt_store_copied_string("windows");
#elif defined(__APPLE__)
    return rune_rt_store_copied_string("macos");
#elif defined(__wasi__)
    return rune_rt_store_copied_string("wasi");
#elif defined(__linux__)
    return rune_rt_store_copied_string("linux");
#elif defined(__AVR__)
    return rune_rt_store_copied_string("embedded");
#else
    return rune_rt_store_copied_string("unknown");
#endif
}
char* rune_rt_system_arch(void) {
#if defined(__x86_64__) || defined(_M_X64)
    return rune_rt_store_copied_string("x86_64");
#elif defined(__aarch64__) || defined(_M_ARM64)
    return rune_rt_store_copied_string("aarch64");
#elif defined(__wasm32__)
    return rune_rt_store_copied_string("wasm32");
#elif defined(__AVR__)
    return rune_rt_store_copied_string("avr");
#elif defined(__riscv) && (__riscv_xlen == 32)
    return rune_rt_store_copied_string("riscv32");
#elif defined(__arm__) || defined(_M_ARM)
    return rune_rt_store_copied_string("arm");
#else
    return rune_rt_store_copied_string("unknown");
#endif
}
char* rune_rt_system_target(void) {
#if defined(__AVR_ATmega328P__)
    return rune_rt_store_copied_string("avr-atmega328p-arduino-uno");
#elif defined(__wasi__)
    return rune_rt_store_copied_string("wasm32-wasip1");
#elif defined(__wasm32__)
    return rune_rt_store_copied_string("wasm32-unknown-unknown");
#elif defined(_WIN32) && (defined(__aarch64__) || defined(_M_ARM64))
    return rune_rt_store_copied_string("aarch64-pc-windows-gnu");
#elif defined(_WIN32)
    return rune_rt_store_copied_string("x86_64-pc-windows-gnu");
#elif defined(__APPLE__) && defined(__aarch64__)
    return rune_rt_store_copied_string("aarch64-apple-darwin");
#elif defined(__APPLE__)
    return rune_rt_store_copied_string("x86_64-apple-darwin");
#elif defined(__linux__) && defined(__aarch64__)
    return rune_rt_store_copied_string("aarch64-unknown-linux-gnu");
#elif defined(__linux__) && defined(__x86_64__)
    return rune_rt_store_copied_string("x86_64-unknown-linux-gnu");
#else
    return rune_rt_store_copied_string("unknown");
#endif
}
char* rune_rt_system_board(void) {
#if defined(__AVR_ATmega328P__)
    return rune_rt_store_copied_string("arduino-uno");
#elif defined(__wasi__) || defined(__wasm32__)
    return rune_rt_store_copied_string("wasm");
#else
    return rune_rt_store_copied_string("host");
#endif
}
_Bool rune_rt_system_is_embedded(void) {
#if defined(__AVR__)
    return 1;
#else
    return 0;
#endif
}
_Bool rune_rt_system_is_wasm(void) {
#if defined(__wasm32__)
    return 1;
#else
    return 0;
#endif
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

fn rust_unix_llvm_wrapper_object_source() -> String {
    format!(
        "{}\nunsafe extern \"C\" {{\n    fn rune_entry_main() -> i64;\n}}\n\n#[unsafe(no_mangle)]\npub extern \"C\" fn main() -> i32 {{\n    unsafe {{ rune_entry_main() as i32 }}\n}}\n",
        rust_runtime_support_body()
    )
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

fn compile_rust_object(source_path: &Path, output_path: &Path) -> Result<(), BuildError> {
    let rustc = find_rustc().ok_or(BuildError::RustcNotFound)?;
    let status = Command::new(&rustc)
        .arg(source_path)
        .arg("--edition=2024")
        .arg("--crate-type=lib")
        .arg("--emit=obj")
        .arg("-C")
        .arg("opt-level=3")
        .arg("-C")
        .arg("codegen-units=1")
        .arg("-C")
        .arg("panic=abort")
        .arg("-o")
        .arg(output_path)
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

    Ok(())
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
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Read, Write};
#[cfg(not(target_os = "wasi"))]
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::process::Command;
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread_local;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

thread_local! {
    static RUNE_OWNED_STRINGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static RUNE_LAST_STRING_LEN: Cell<i64> = const { Cell::new(0) };
    static RUNE_ARDUINO_DIGITAL_PINS: RefCell<[u8; 64]> = const { RefCell::new([0; 64]) };
    static RUNE_ARDUINO_ANALOG_PINS: RefCell<[i64; 64]> = const { RefCell::new([0; 64]) };
    static RUNE_ARDUINO_SERVO_ATTACHED: RefCell<[u8; 64]> = const { RefCell::new([0; 64]) };
    static RUNE_ARDUINO_SERVO_ANGLE: RefCell<[i64; 64]> = const { RefCell::new([0; 64]) };
}

static RUNE_ARDUINO_START: OnceLock<Instant> = OnceLock::new();
static RUNE_ARDUINO_RANDOM_STATE: AtomicU64 = AtomicU64::new(0);
static RUNE_HOST_SERIAL: OnceLock<std::sync::Mutex<Option<std::fs::File>>> = OnceLock::new();
static RUNE_NETWORK_ERROR: OnceLock<Mutex<(i32, String)>> = OnceLock::new();
#[cfg(not(target_os = "wasi"))]
static RUNE_NETWORK_SERVER_HANDLES: OnceLock<Mutex<HashMap<i32, TcpListener>>> = OnceLock::new();
#[cfg(not(target_os = "wasi"))]
static RUNE_NETWORK_SERVER_NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);
#[cfg(not(target_os = "wasi"))]
static RUNE_NETWORK_CLIENT_HANDLES: OnceLock<Mutex<HashMap<i32, TcpStream>>> = OnceLock::new();
#[cfg(not(target_os = "wasi"))]
static RUNE_NETWORK_CLIENT_NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

const RUNE_NETWORK_OK: i32 = 0;
const RUNE_NETWORK_ERR_INVALID_ARGUMENT: i32 = 1;
const RUNE_NETWORK_ERR_UNSUPPORTED_TARGET: i32 = 2;
const RUNE_NETWORK_ERR_ADDRESS_RESOLUTION: i32 = 3;
const RUNE_NETWORK_ERR_BIND: i32 = 4;
const RUNE_NETWORK_ERR_CONNECT: i32 = 5;
const RUNE_NETWORK_ERR_ACCEPT_TIMEOUT: i32 = 6;
const RUNE_NETWORK_ERR_ACCEPT: i32 = 7;
const RUNE_NETWORK_ERR_READ: i32 = 8;
const RUNE_NETWORK_ERR_WRITE: i32 = 9;
const RUNE_NETWORK_ERR_SOCKET_OPTION: i32 = 10;

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

fn rune_rt_serial_handle() -> &'static std::sync::Mutex<Option<std::fs::File>> {
    RUNE_HOST_SERIAL.get_or_init(|| std::sync::Mutex::new(None))
}

fn rune_rt_network_error_state() -> &'static Mutex<(i32, String)> {
    RUNE_NETWORK_ERROR.get_or_init(|| Mutex::new((RUNE_NETWORK_OK, String::new())))
}

#[cfg(not(target_os = "wasi"))]
fn rune_rt_network_server_handles() -> &'static Mutex<HashMap<i32, TcpListener>> {
    RUNE_NETWORK_SERVER_HANDLES.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(not(target_os = "wasi"))]
fn rune_rt_network_client_handles() -> &'static Mutex<HashMap<i32, TcpStream>> {
    RUNE_NETWORK_CLIENT_HANDLES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn rune_rt_network_clear_error_state() {
    let mut state = rune_rt_network_error_state()
        .lock()
        .expect("network error mutex poisoned");
    state.0 = RUNE_NETWORK_OK;
    state.1.clear();
}

fn rune_rt_network_set_error(code: i32, message: impl Into<String>) {
    let mut state = rune_rt_network_error_state()
        .lock()
        .expect("network error mutex poisoned");
    state.0 = code;
    state.1 = message.into();
}

fn rune_rt_configure_serial_port(port_name: &str, baud: i64) -> bool {
    if baud <= 0 {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        let status = Command::new("cmd")
            .args([
                "/C",
                "mode",
                port_name,
                &format!("BAUD={baud}"),
                "PARITY=n",
                "DATA=8",
                "STOP=1",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        return matches!(status, Ok(exit) if exit.success());
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let port_flag = if cfg!(target_os = "macos") { "-f" } else { "-F" };
        let status = Command::new("stty")
            .args([
                port_flag,
                port_name,
                &baud.to_string(),
                "raw",
                "-echo",
                "cs8",
                "-cstopb",
                "-parenb",
                "min",
                "0",
                "time",
                "1",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        return matches!(status, Ok(exit) if exit.success());
    }
    #[allow(unreachable_code)]
    false
}

fn rune_rt_open_serial_file(port_name: &str) -> io::Result<std::fs::File> {
    #[cfg(target_os = "windows")]
    let normalized = if port_name.starts_with(r"\\.\") {
        port_name.to_string()
    } else {
        format!(r"\\.\{port_name}")
    };
    #[cfg(not(target_os = "windows"))]
    let normalized = port_name.to_string();

    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(normalized)
}

fn rune_rt_host_platform_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "wasi") {
        "wasi"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_arch = "avr") {
        "embedded"
    } else {
        "unknown"
    }
}

fn rune_rt_host_arch_name() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else if cfg!(target_arch = "wasm32") {
        "wasm32"
    } else if cfg!(target_arch = "avr") {
        "avr"
    } else if cfg!(target_arch = "riscv32") {
        "riscv32"
    } else if cfg!(target_arch = "arm") {
        "arm"
    } else {
        "unknown"
    }
}

fn rune_rt_host_target_name() -> &'static str {
    if cfg!(all(target_arch = "avr")) {
        "avr-atmega328p-arduino-uno"
    } else if cfg!(target_os = "wasi") {
        "wasm32-wasip1"
    } else if cfg!(all(target_arch = "wasm32")) {
        "wasm32-unknown-unknown"
    } else if cfg!(all(target_os = "windows", target_arch = "aarch64")) {
        "aarch64-pc-windows-gnu"
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "x86_64-pc-windows-gnu"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "aarch64-apple-darwin"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "x86_64-apple-darwin"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "aarch64-unknown-linux-gnu"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "x86_64-unknown-linux-gnu"
    } else {
        "unknown"
    }
}

fn rune_rt_host_board_name() -> &'static str {
    if cfg!(target_arch = "avr") {
        "arduino-uno"
    } else if cfg!(target_arch = "wasm32") {
        "wasm"
    } else {
        "host"
    }
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
pub extern "C" fn rune_rt_print_bool(value: i64) {
    print!("{}", if value != 0 { "true" } else { "false" });
    let _ = io::stdout().flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_eprint_bool(value: i64) {
    eprint!("{}", if value != 0 { "true" } else { "false" });
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
pub extern "C" fn rune_rt_system_platform() -> *const u8 {
    rune_rt_store_string(rune_rt_host_platform_name().to_string())
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_system_arch() -> *const u8 {
    rune_rt_store_string(rune_rt_host_arch_name().to_string())
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_system_target() -> *const u8 {
    rune_rt_store_string(rune_rt_host_target_name().to_string())
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_system_board() -> *const u8 {
    rune_rt_store_string(rune_rt_host_board_name().to_string())
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_system_is_embedded() -> bool {
    cfg!(target_arch = "avr")
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_system_is_wasm() -> bool {
    cfg!(target_arch = "wasm32")
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_pin_mode(_pin: i64, _mode: i64) {}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_digital_write(pin: i64, value: bool) {
    if let Ok(index) = usize::try_from(pin) {
        RUNE_ARDUINO_DIGITAL_PINS.with(|pins| {
            let mut pins = pins.borrow_mut();
            if index < pins.len() {
                pins[index] = value as u8;
            }
        });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_digital_read(pin: i64) -> bool {
    usize::try_from(pin)
        .ok()
        .and_then(|index| {
            RUNE_ARDUINO_DIGITAL_PINS.with(|pins| pins.borrow().get(index).copied())
        })
        .unwrap_or(0)
        != 0
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_analog_write(pin: i64, value: i64) {
    if let Ok(index) = usize::try_from(pin) {
        RUNE_ARDUINO_ANALOG_PINS.with(|pins| {
            let mut pins = pins.borrow_mut();
            if index < pins.len() {
                pins[index] = value as i64;
            }
        });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_analog_read(pin: i64) -> i64 {
    usize::try_from(pin)
        .ok()
        .and_then(|index| {
            RUNE_ARDUINO_ANALOG_PINS.with(|pins| pins.borrow().get(index).copied())
        })
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_analog_reference(_mode: i64) {}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_pulse_in(_pin: i64, _state: bool, _timeout_us: i64) -> i64 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_shift_out(
    _data_pin: i64,
    _clock_pin: i64,
    _bit_order: i64,
    _value: i64,
) {
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_tone(_pin: i64, _frequency_hz: i64, _duration_ms: i64) {}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_no_tone(_pin: i64) {}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_servo_attach(pin: i64) -> bool {
    if let Ok(index) = usize::try_from(pin) {
        return RUNE_ARDUINO_SERVO_ATTACHED.with(|pins| {
            let mut pins = pins.borrow_mut();
            if index < pins.len() {
                pins[index] = 1;
                true
            } else {
                false
            }
        });
    }
    false
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_servo_detach(pin: i64) {
    if let Ok(index) = usize::try_from(pin) {
        RUNE_ARDUINO_SERVO_ATTACHED.with(|pins| {
            let mut pins = pins.borrow_mut();
            if index < pins.len() {
                pins[index] = 0;
            }
        });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_servo_write(pin: i64, angle: i64) {
    if !rune_rt_arduino_servo_attach(pin) {
        return;
    }
    if let Ok(index) = usize::try_from(pin) {
        RUNE_ARDUINO_SERVO_ANGLE.with(|angles| {
            let mut angles = angles.borrow_mut();
            if index < angles.len() {
                angles[index] = angle.clamp(0, 180);
            }
        });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_servo_write_us(pin: i64, pulse_us: i64) {
    if !rune_rt_arduino_servo_attach(pin) {
        return;
    }
    if let Ok(index) = usize::try_from(pin) {
        RUNE_ARDUINO_SERVO_ANGLE.with(|angles| {
            let mut angles = angles.borrow_mut();
            if index < angles.len() {
                angles[index] = pulse_us;
            }
        });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_delay_ms(ms: i64) {
    if ms > 0 {
        std::thread::sleep(Duration::from_millis(ms as u64));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_delay_us(us: i64) {
    if us > 0 {
        std::thread::sleep(Duration::from_micros(us as u64));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_millis() -> i64 {
    let start = RUNE_ARDUINO_START.get_or_init(Instant::now);
    start.elapsed().as_millis() as i64
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_micros() -> i64 {
    let start = RUNE_ARDUINO_START.get_or_init(Instant::now);
    start.elapsed().as_micros() as i64
}

fn rune_rt_arduino_random_next() -> u64 {
    let mut state = RUNE_ARDUINO_RANDOM_STATE.load(Ordering::Relaxed);
    if state == 0 {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos() as u64)
            .unwrap_or(0x9e37_79b9_7f4a_7c15);
        state = if seed == 0 { 1 } else { seed };
    }
    let next = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    RUNE_ARDUINO_RANDOM_STATE.store(next, Ordering::Relaxed);
    next
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_read_line() -> *const u8 {
    rune_rt_input_line()
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_mode_input() -> i64 { 0 }

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_mode_output() -> i64 { 1 }

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_mode_input_pullup() -> i64 { 2 }

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_led_builtin() -> i64 { 13 }

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_high() -> i64 { 1 }

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_low() -> i64 { 0 }

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_bit_order_lsb_first() -> i64 { 0 }

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_bit_order_msb_first() -> i64 { 1 }

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_analog_ref_default() -> i64 { 1 }

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_analog_ref_internal() -> i64 { 3 }

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_analog_ref_external() -> i64 { 0 }

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_uart_begin(_baud: i64) {}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_uart_available() -> i64 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_uart_read_byte() -> i64 {
    -1
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_uart_write_byte(value: i64) {
    let byte = [value as u8];
    let _ = io::stdout().write_all(&byte);
    let _ = io::stdout().flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_uart_write(ptr: *const u8, len: i64) {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let _ = io::stdout().write_all(bytes);
    let _ = io::stdout().flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_interrupts_enable() {}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_interrupts_disable() {}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_shift_in(data_pin: i64, clock_pin: i64, _bit_order: i64) -> i64 {
    let data_bit = rune_rt_arduino_digital_read(data_pin) as i64;
    rune_rt_arduino_digital_write(clock_pin, true);
    rune_rt_arduino_digital_write(clock_pin, false);
    data_bit & 0xFF
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_random_seed(seed: i64) {
    let seed = if seed == 0 { 1 } else { seed as u64 };
    RUNE_ARDUINO_RANDOM_STATE.store(seed, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_random_i64(max_value: i64) -> i64 {
    if max_value <= 0 {
        return 0;
    }
    (rune_rt_arduino_random_next() % (max_value as u64)) as i64
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_arduino_random_range(min_value: i64, max_value: i64) -> i64 {
    if max_value <= min_value {
        return min_value;
    }
    let span = (max_value - min_value) as u64;
    min_value + (rune_rt_arduino_random_next() % span) as i64
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_serial_open(port_ptr: *const u8, port_len: i64, baud: i64) -> bool {
    if port_ptr.is_null() || port_len < 0 || baud <= 0 {
        return false;
    }
    let bytes = unsafe { std::slice::from_raw_parts(port_ptr, port_len as usize) };
    let Ok(port_name) = std::str::from_utf8(bytes) else {
        return false;
    };
    if !rune_rt_configure_serial_port(port_name, baud) {
        return false;
    }
    let Ok(port) = rune_rt_open_serial_file(port_name) else {
        return false;
    };
    let mut guard = rune_rt_serial_handle()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    *guard = Some(port);
    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
    std::thread::sleep(Duration::from_millis(1200));
    true
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_serial_is_open() -> bool {
    rune_rt_serial_handle()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .is_some()
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_serial_close() {
    let mut guard = rune_rt_serial_handle()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    *guard = None;
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_serial_write(ptr: *const u8, len: i64) -> bool {
    if ptr.is_null() || len < 0 {
        return false;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let mut guard = rune_rt_serial_handle()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let Some(port) = guard.as_mut() else {
        return false;
    };
    if port.write_all(bytes).is_err() {
        return false;
    }
    port.flush().is_ok()
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_serial_write_line(ptr: *const u8, len: i64) -> bool {
    if ptr.is_null() || len < 0 {
        return false;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let mut guard = rune_rt_serial_handle()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let Some(port) = guard.as_mut() else {
        return false;
    };
    if port.write_all(bytes).is_err() {
        return false;
    }
    if port.write_all(b"\n").is_err() {
        return false;
    }
    port.flush().is_ok()
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_serial_read_line() -> *const u8 {
    let mut guard = rune_rt_serial_handle()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let Some(port) = guard.as_mut() else {
        return rune_rt_store_string(String::new());
    };

    let mut bytes = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match port.read(&mut byte) {
            Ok(0) => {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            Ok(_) => {
                if byte[0] == b'\n' {
                    break;
                }
                if byte[0] != b'\r' {
                    bytes.push(byte[0]);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::TimedOut => {
                if !bytes.is_empty() {
                    break;
                }
            }
            Err(_) => return rune_rt_store_string(String::new()),
        }
    }

    let line = String::from_utf8_lossy(&bytes).to_string();
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
        5 => {
            let ptr = rune_rt_json_stringify(payload);
            let len = rune_rt_last_string_len() as usize;
            let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
            let text = std::str::from_utf8(bytes).expect("Rune JSON strings must be valid UTF-8");
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
        5 => {
            let ptr = rune_rt_json_stringify(payload);
            let len = rune_rt_last_string_len() as usize;
            let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
            let text = std::str::from_utf8(bytes).expect("Rune JSON strings must be valid UTF-8");
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
pub extern "C" fn rune_rt_string_compare(
    left_ptr: *const u8,
    left_len: i64,
    right_ptr: *const u8,
    right_len: i64,
) -> i32 {
    let left = unsafe { std::slice::from_raw_parts(left_ptr, left_len as usize) };
    let right = unsafe { std::slice::from_raw_parts(right_ptr, right_len as usize) };
    match left.cmp(right) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
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
        5 => rune_rt_json_to_i64(payload),
        _ => panic!("failed to convert dynamic Rune value with tag {tag} to i64"),
    }
}

fn rune_rt_string_from_handle(ptr: *const u8) -> String {
    RUNE_OWNED_STRINGS.with(|strings| {
        strings
            .borrow()
            .iter()
            .find(|value| value.as_ptr() == ptr)
            .cloned()
            .unwrap_or_else(|| panic!("unknown Rune string handle {ptr:p}"))
    })
}

fn rune_rt_json_skip_ws(bytes: &[u8], index: &mut usize) {
    while *index < bytes.len() && matches!(bytes[*index], b' ' | b'\n' | b'\r' | b'\t') {
        *index += 1;
    }
}

fn rune_rt_json_parse_hex(bytes: &[u8], index: &mut usize) -> char {
    let mut value = 0u32;
    for _ in 0..4 {
        if *index >= bytes.len() {
            panic!("invalid JSON unicode escape");
        }
        value = value * 16
            + match bytes[*index] {
                b'0'..=b'9' => (bytes[*index] - b'0') as u32,
                b'a'..=b'f' => (bytes[*index] - b'a' + 10) as u32,
                b'A'..=b'F' => (bytes[*index] - b'A' + 10) as u32,
                _ => panic!("invalid JSON unicode escape"),
            };
        *index += 1;
    }
    char::from_u32(value).unwrap_or('\u{FFFD}')
}

fn rune_rt_json_parse_string_end(bytes: &[u8], index: &mut usize) {
    if bytes.get(*index) != Some(&b'"') {
        panic!("expected JSON string");
    }
    *index += 1;
    while *index < bytes.len() {
        match bytes[*index] {
            b'"' => {
                *index += 1;
                return;
            }
            b'\\' => {
                *index += 1;
                let Some(escape) = bytes.get(*index).copied() else {
                    panic!("unterminated JSON escape");
                };
                *index += 1;
                match escape {
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {}
                    b'u' => {
                        let _ = rune_rt_json_parse_hex(bytes, index);
                    }
                    _ => panic!("invalid JSON escape"),
                }
            }
            value if value < 0x20 => panic!("control characters are not valid in JSON strings"),
            _ => *index += 1,
        }
    }
    panic!("unterminated JSON string");
}

fn rune_rt_json_decode_string(literal: &str) -> String {
    let bytes = literal.as_bytes();
    let mut index = 0usize;
    if bytes.get(index) != Some(&b'"') {
        panic!("expected JSON string literal");
    }
    index += 1;
    let mut out = String::new();
    while index < bytes.len() {
        match bytes[index] {
            b'"' => return out,
            b'\\' => {
                index += 1;
                let Some(escape) = bytes.get(index).copied() else {
                    panic!("unterminated JSON escape");
                };
                index += 1;
                match escape {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{0008}'),
                    b'f' => out.push('\u{000C}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => out.push(rune_rt_json_parse_hex(bytes, &mut index)),
                    _ => panic!("invalid JSON escape"),
                }
            }
            _ => {
                let ch = std::str::from_utf8(&bytes[index..])
                    .expect("JSON strings must be UTF-8")
                    .chars()
                    .next()
                    .expect("character should exist");
                out.push(ch);
                index += ch.len_utf8();
            }
        }
    }
    panic!("unterminated JSON string literal");
}

fn rune_rt_json_parse_number_end(bytes: &[u8], index: &mut usize) {
    if bytes.get(*index) == Some(&b'-') {
        *index += 1;
    }
    match bytes.get(*index) {
        Some(b'0') => *index += 1,
        Some(b'1'..=b'9') => {
            *index += 1;
            while matches!(bytes.get(*index), Some(b'0'..=b'9')) {
                *index += 1;
            }
        }
        _ => panic!("invalid JSON number"),
    }
    if bytes.get(*index) == Some(&b'.') {
        *index += 1;
        let start = *index;
        while matches!(bytes.get(*index), Some(b'0'..=b'9')) {
            *index += 1;
        }
        if *index == start {
            panic!("invalid JSON fractional part");
        }
    }
    if matches!(bytes.get(*index), Some(b'e' | b'E')) {
        *index += 1;
        if matches!(bytes.get(*index), Some(b'+' | b'-')) {
            *index += 1;
        }
        let start = *index;
        while matches!(bytes.get(*index), Some(b'0'..=b'9')) {
            *index += 1;
        }
        if *index == start {
            panic!("invalid JSON exponent");
        }
    }
}

fn rune_rt_json_parse_value_end(bytes: &[u8], index: &mut usize) {
    rune_rt_json_skip_ws(bytes, index);
    match bytes.get(*index).copied() {
        Some(b'"') => rune_rt_json_parse_string_end(bytes, index),
        Some(b'{') => {
            *index += 1;
            rune_rt_json_skip_ws(bytes, index);
            if bytes.get(*index) == Some(&b'}') {
                *index += 1;
                return;
            }
            loop {
                rune_rt_json_parse_string_end(bytes, index);
                rune_rt_json_skip_ws(bytes, index);
                if bytes.get(*index) != Some(&b':') {
                    panic!("expected `:` in JSON object");
                }
                *index += 1;
                rune_rt_json_parse_value_end(bytes, index);
                rune_rt_json_skip_ws(bytes, index);
                match bytes.get(*index) {
                    Some(b',') => {
                        *index += 1;
                        rune_rt_json_skip_ws(bytes, index);
                    }
                    Some(b'}') => {
                        *index += 1;
                        return;
                    }
                    _ => panic!("expected `,` or `}}` in JSON object"),
                }
            }
        }
        Some(b'[') => {
            *index += 1;
            rune_rt_json_skip_ws(bytes, index);
            if bytes.get(*index) == Some(&b']') {
                *index += 1;
                return;
            }
            loop {
                rune_rt_json_parse_value_end(bytes, index);
                rune_rt_json_skip_ws(bytes, index);
                match bytes.get(*index) {
                    Some(b',') => {
                        *index += 1;
                        rune_rt_json_skip_ws(bytes, index);
                    }
                    Some(b']') => {
                        *index += 1;
                        return;
                    }
                    _ => panic!("expected `,` or `]` in JSON array"),
                }
            }
        }
        Some(b't') if bytes.get(*index..(*index + 4)) == Some(b"true") => *index += 4,
        Some(b'f') if bytes.get(*index..(*index + 5)) == Some(b"false") => *index += 5,
        Some(b'n') if bytes.get(*index..(*index + 4)) == Some(b"null") => *index += 4,
        Some(b'-' | b'0'..=b'9') => rune_rt_json_parse_number_end(bytes, index),
        _ => panic!("invalid JSON value"),
    }
}

fn rune_rt_json_trimmed(text: &str) -> &str {
    text.trim_matches(|ch| matches!(ch, ' ' | '\n' | '\r' | '\t'))
}

fn rune_rt_json_handle_string(handle: i64) -> String {
    rune_rt_string_from_handle(handle as *const u8)
}

fn rune_rt_json_kind_name(text: &str) -> &'static str {
    match rune_rt_json_trimmed(text).as_bytes().first().copied() {
        Some(b'{') => "object",
        Some(b'[') => "array",
        Some(b'"') => "string",
        Some(b't' | b'f') => "bool",
        Some(b'n') => "null",
        Some(b'-' | b'0'..=b'9') => "number",
        _ => panic!("invalid JSON handle"),
    }
}

fn rune_rt_json_store_slice(slice: &str) -> *const u8 {
    rune_rt_store_string(rune_rt_json_trimmed(slice).to_string())
}

fn rune_rt_json_find_object_value(text: &str, key: &str) -> Option<String> {
    let trimmed = rune_rt_json_trimmed(text);
    let bytes = trimmed.as_bytes();
    let mut index = 0usize;
    rune_rt_json_skip_ws(bytes, &mut index);
    if bytes.get(index) != Some(&b'{') {
        return None;
    }
    index += 1;
    rune_rt_json_skip_ws(bytes, &mut index);
    if bytes.get(index) == Some(&b'}') {
        return None;
    }
    loop {
        let key_start = index;
        rune_rt_json_parse_string_end(bytes, &mut index);
        let parsed_key = rune_rt_json_decode_string(&trimmed[key_start..index]);
        rune_rt_json_skip_ws(bytes, &mut index);
        if bytes.get(index) != Some(&b':') {
            panic!("expected `:` in JSON object");
        }
        index += 1;
        let value_start = index;
        rune_rt_json_parse_value_end(bytes, &mut index);
        if parsed_key == key {
            return Some(rune_rt_json_trimmed(&trimmed[value_start..index]).to_string());
        }
        rune_rt_json_skip_ws(bytes, &mut index);
        match bytes.get(index) {
            Some(b',') => {
                index += 1;
                rune_rt_json_skip_ws(bytes, &mut index);
            }
            Some(b'}') => return None,
            _ => panic!("expected `,` or `}}` in JSON object"),
        }
    }
}

fn rune_rt_json_index_value(text: &str, wanted: usize) -> Option<String> {
    let trimmed = rune_rt_json_trimmed(text);
    let bytes = trimmed.as_bytes();
    let mut index = 0usize;
    rune_rt_json_skip_ws(bytes, &mut index);
    if bytes.get(index) != Some(&b'[') {
        return None;
    }
    index += 1;
    rune_rt_json_skip_ws(bytes, &mut index);
    if bytes.get(index) == Some(&b']') {
        return None;
    }
    let mut current = 0usize;
    loop {
        let value_start = index;
        rune_rt_json_parse_value_end(bytes, &mut index);
        if current == wanted {
            return Some(rune_rt_json_trimmed(&trimmed[value_start..index]).to_string());
        }
        current += 1;
        rune_rt_json_skip_ws(bytes, &mut index);
        match bytes.get(index) {
            Some(b',') => {
                index += 1;
                rune_rt_json_skip_ws(bytes, &mut index);
            }
            Some(b']') => return None,
            _ => panic!("expected `,` or `]` in JSON array"),
        }
    }
}

fn rune_rt_json_len_value(text: &str) -> i64 {
    let trimmed = rune_rt_json_trimmed(text);
    match rune_rt_json_kind_name(trimmed) {
        "array" => {
            let bytes = trimmed.as_bytes();
            let mut index = 1usize;
            let mut count = 0i64;
            rune_rt_json_skip_ws(bytes, &mut index);
            if bytes.get(index) == Some(&b']') {
                return 0;
            }
            loop {
                rune_rt_json_parse_value_end(bytes, &mut index);
                count += 1;
                rune_rt_json_skip_ws(bytes, &mut index);
                match bytes.get(index) {
                    Some(b',') => {
                        index += 1;
                        rune_rt_json_skip_ws(bytes, &mut index);
                    }
                    Some(b']') => return count,
                    _ => panic!("expected `,` or `]` in JSON array"),
                }
            }
        }
        "object" => {
            let bytes = trimmed.as_bytes();
            let mut index = 1usize;
            let mut count = 0i64;
            rune_rt_json_skip_ws(bytes, &mut index);
            if bytes.get(index) == Some(&b'}') {
                return 0;
            }
            loop {
                rune_rt_json_parse_string_end(bytes, &mut index);
                rune_rt_json_skip_ws(bytes, &mut index);
                if bytes.get(index) != Some(&b':') {
                    panic!("expected `:` in JSON object");
                }
                index += 1;
                rune_rt_json_parse_value_end(bytes, &mut index);
                count += 1;
                rune_rt_json_skip_ws(bytes, &mut index);
                match bytes.get(index) {
                    Some(b',') => {
                        index += 1;
                        rune_rt_json_skip_ws(bytes, &mut index);
                    }
                    Some(b'}') => return count,
                    _ => panic!("expected `,` or `}}` in JSON object"),
                }
            }
        }
        "string" => rune_rt_json_decode_string(trimmed).chars().count() as i64,
        _ => 0,
    }
}

fn rune_rt_json_equal_values(left: &str, right: &str) -> bool {
    let left_trimmed = rune_rt_json_trimmed(left);
    let right_trimmed = rune_rt_json_trimmed(right);
    let left_kind = rune_rt_json_kind_name(left_trimmed);
    let right_kind = rune_rt_json_kind_name(right_trimmed);
    if left_kind != right_kind {
        return false;
    }

    match left_kind {
        "null" => true,
        "bool" => left_trimmed == right_trimmed,
        "number" => match (left_trimmed.parse::<f64>(), right_trimmed.parse::<f64>()) {
            (Ok(left_value), Ok(right_value)) => left_value == right_value,
            _ => false,
        },
        "string" => {
            rune_rt_json_decode_string(left_trimmed) == rune_rt_json_decode_string(right_trimmed)
        }
        "array" => {
            let left_len = rune_rt_json_len_value(left_trimmed);
            let right_len = rune_rt_json_len_value(right_trimmed);
            if left_len != right_len {
                return false;
            }
            for index in 0..left_len as usize {
                let Some(left_value) = rune_rt_json_index_value(left_trimmed, index) else {
                    return false;
                };
                let Some(right_value) = rune_rt_json_index_value(right_trimmed, index) else {
                    return false;
                };
                if !rune_rt_json_equal_values(&left_value, &right_value) {
                    return false;
                }
            }
            true
        }
        "object" => {
            let left_len = rune_rt_json_len_value(left_trimmed);
            let right_len = rune_rt_json_len_value(right_trimmed);
            if left_len != right_len {
                return false;
            }

            let bytes = left_trimmed.as_bytes();
            let mut index = 0usize;
            rune_rt_json_skip_ws(bytes, &mut index);
            if bytes.get(index) != Some(&b'{') {
                return false;
            }
            index += 1;
            rune_rt_json_skip_ws(bytes, &mut index);
            if bytes.get(index) == Some(&b'}') {
                return true;
            }

            loop {
                if bytes.get(index) != Some(&b'"') {
                    return false;
                }
                let key_start = index;
                rune_rt_json_parse_string_end(bytes, &mut index);
                let key_text = match std::str::from_utf8(&bytes[key_start..index]) {
                    Ok(text) => text,
                    Err(_) => return false,
                };
                let key = rune_rt_json_decode_string(key_text);

                rune_rt_json_skip_ws(bytes, &mut index);
                if bytes.get(index) != Some(&b':') {
                    return false;
                }
                index += 1;
                rune_rt_json_skip_ws(bytes, &mut index);

                let value_start = index;
                rune_rt_json_parse_value_end(bytes, &mut index);
                let left_value = match std::str::from_utf8(&bytes[value_start..index]) {
                    Ok(text) => text.to_string(),
                    Err(_) => return false,
                };
                let Some(right_value) = rune_rt_json_find_object_value(right_trimmed, &key) else {
                    return false;
                };
                if !rune_rt_json_equal_values(&left_value, &right_value) {
                    return false;
                }

                rune_rt_json_skip_ws(bytes, &mut index);
                match bytes.get(index) {
                    Some(b',') => {
                        index += 1;
                        rune_rt_json_skip_ws(bytes, &mut index);
                    }
                    Some(b'}') => return true,
                    _ => return false,
                }
            }
        }
        _ => false,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_json_parse(ptr: *const u8, len: i64) -> i64 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let text = std::str::from_utf8(bytes).expect("JSON source must be valid UTF-8");
    let trimmed = rune_rt_json_trimmed(text);
    let mut index = 0usize;
    rune_rt_json_parse_value_end(trimmed.as_bytes(), &mut index);
    rune_rt_json_skip_ws(trimmed.as_bytes(), &mut index);
    if index != trimmed.len() {
        panic!("invalid trailing characters after JSON value");
    }
    rune_rt_store_string(trimmed.to_string()) as i64
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_json_stringify(handle: i64) -> *const u8 {
    let text = rune_rt_json_handle_string(handle);
    rune_rt_store_string(text)
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_json_kind(handle: i64) -> *const u8 {
    rune_rt_store_string(rune_rt_json_kind_name(&rune_rt_json_handle_string(handle)).to_string())
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_json_is_null(handle: i64) -> bool {
    rune_rt_json_kind_name(&rune_rt_json_handle_string(handle)) == "null"
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_json_len(handle: i64) -> i64 {
    rune_rt_json_len_value(&rune_rt_json_handle_string(handle))
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_json_get(handle: i64, key_ptr: *const u8, key_len: i64) -> i64 {
    let key_bytes = unsafe { std::slice::from_raw_parts(key_ptr, key_len as usize) };
    let key = std::str::from_utf8(key_bytes).expect("JSON object key must be valid UTF-8");
    let text = rune_rt_json_handle_string(handle);
    rune_rt_store_string(
        rune_rt_json_find_object_value(&text, key).unwrap_or_else(|| "null".to_string()),
    ) as i64
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_json_index(handle: i64, index: i64) -> i64 {
    if index < 0 {
        return rune_rt_store_string("null".to_string()) as i64;
    }
    let text = rune_rt_json_handle_string(handle);
    rune_rt_store_string(
        rune_rt_json_index_value(&text, index as usize).unwrap_or_else(|| "null".to_string()),
    ) as i64
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_json_to_string(handle: i64) -> *const u8 {
    let text = rune_rt_json_handle_string(handle);
    let trimmed = rune_rt_json_trimmed(&text);
    if rune_rt_json_kind_name(trimmed) == "string" {
        rune_rt_store_string(rune_rt_json_decode_string(trimmed))
    } else {
        rune_rt_store_string(trimmed.to_string())
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_json_to_i64(handle: i64) -> i64 {
    let text = rune_rt_json_handle_string(handle);
    let trimmed = rune_rt_json_trimmed(&text);
    match rune_rt_json_kind_name(trimmed) {
        "null" => 0,
        "bool" => (trimmed == "true") as i64,
        "number" => trimmed
            .parse::<i64>()
            .unwrap_or_else(|_| panic!("failed to convert JSON number `{trimmed}` to i64")),
        "string" => rune_rt_json_decode_string(trimmed)
            .trim()
            .parse::<i64>()
            .unwrap_or_else(|_| panic!("failed to convert JSON string `{trimmed}` to i64")),
        other => panic!("cannot convert JSON {other} to i64"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_json_to_bool(handle: i64) -> bool {
    let text = rune_rt_json_handle_string(handle);
    let trimmed = rune_rt_json_trimmed(&text);
    match rune_rt_json_kind_name(trimmed) {
        "null" => false,
        "bool" => trimmed == "true",
        "number" => trimmed.parse::<f64>().map(|value| value != 0.0).unwrap_or(false),
        "string" => !rune_rt_json_decode_string(trimmed).is_empty(),
        "array" | "object" => rune_rt_json_len_value(trimmed) > 0,
        _ => false,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_json_equal(left_handle: i64, right_handle: i64) -> bool {
    rune_rt_json_equal_values(
        &rune_rt_json_handle_string(left_handle),
        &rune_rt_json_handle_string(right_handle),
    )
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
        5 => rune_rt_string_from_handle(rune_rt_json_to_string(payload) as *const u8),
        _ => format!("<dynamic:{tag}>"),
    }
}

fn rune_rt_dynamic_value_to_i64_lossy(tag: i64, payload: i64, _extra: i64) -> Option<i64> {
    match tag {
        1 => Some((payload != 0) as i64),
        2 => Some(payload as i32 as i64),
        3 => Some(payload),
        5 => Some(rune_rt_json_to_i64(payload)),
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
            let equal = if left_tag == 5 && right_tag == 5 {
                rune_rt_json_equal(left_payload, right_payload)
            } else if left_tag == 4 || right_tag == 4 {
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
        5 => rune_rt_json_to_bool(payload),
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
pub extern "C" fn rune_rt_time_monotonic_us() -> i64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START
        .get_or_init(Instant::now)
        .elapsed()
        .as_micros() as i64
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_time_sleep_ms(ms: i64) {
    if ms > 0 {
        std::thread::sleep(Duration::from_millis(ms as u64));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_time_sleep_us(us: i64) {
    if us > 0 {
        std::thread::sleep(Duration::from_micros(us as u64));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_sum_range(start: i64, stop: i64, step: i64) -> i64 {
    if step == 0 {
        return 0;
    }

    let mut total = 0i64;
    let mut value = start;
    if step > 0 {
        while value < stop {
            total += value;
            value += step;
        }
    } else {
        while value > stop {
            total += value;
            value += step;
        }
    }
    total
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
pub extern "C" fn rune_rt_env_get_string(
    ptr: *const u8,
    len: i64,
    default_ptr: *const u8,
    default_len: i64,
) -> *const u8 {
    let key = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let key = std::str::from_utf8(key).expect("environment variable name must be valid UTF-8");
    let default = unsafe { std::slice::from_raw_parts(default_ptr, default_len as usize) };
    let default =
        std::str::from_utf8(default).expect("default environment value must be valid UTF-8");
    let value = env::var(key).unwrap_or_else(|_| default.to_string());
    rune_rt_store_string(value)
}

#[cfg(not(target_os = "wasi"))]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_env_get_string(
    ptr: *const u8,
    len: i64,
    default_ptr: *const u8,
    default_len: i64,
) -> *const u8 {
    let key = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let key = std::str::from_utf8(key).expect("environment variable name must be valid UTF-8");
    let default = unsafe { std::slice::from_raw_parts(default_ptr, default_len as usize) };
    let default =
        std::str::from_utf8(default).expect("default environment value must be valid UTF-8");
    let value = env::var(key).unwrap_or_else(|_| default.to_string());
    rune_rt_store_string(value)
}

#[cfg(target_os = "wasi")]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_env_arg_count() -> i32 {
    env::args().skip(1).count() as i32
}

#[cfg(not(target_os = "wasi"))]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_env_arg_count() -> i32 {
    env::args().skip(1).count() as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_env_arg(index: i32) -> *const u8 {
    if index < 0 {
        return rune_rt_store_string(String::new());
    }
    let value = env::args()
        .skip(1)
        .nth(index as usize)
        .unwrap_or_default();
    rune_rt_store_string(value)
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_last_error_code() -> i32 {
    rune_rt_network_error_state()
        .lock()
        .expect("network error mutex poisoned")
        .0
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_last_error_message() -> *const u8 {
    let message = rune_rt_network_error_state()
        .lock()
        .expect("network error mutex poisoned")
        .1
        .clone();
    rune_rt_store_string(message)
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_clear_error() {
    rune_rt_network_clear_error_state();
}

#[cfg(target_os = "wasi")]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_connect(_ptr: *const u8, _len: i64, _port: i32) -> bool {
    rune_rt_network_set_error(
        RUNE_NETWORK_ERR_UNSUPPORTED_TARGET,
        "network is not supported on this target",
    );
    false
}

#[cfg(target_os = "wasi")]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_listen(_ptr: *const u8, _len: i64, _port: i32) -> bool {
    rune_rt_network_set_error(
        RUNE_NETWORK_ERR_UNSUPPORTED_TARGET,
        "network is not supported on this target",
    );
    false
}

#[cfg(target_os = "wasi")]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_send(
    _host_ptr: *const u8,
    _host_len: i64,
    _port: i32,
    _data_ptr: *const u8,
    _data_len: i64,
) -> bool {
    rune_rt_network_set_error(
        RUNE_NETWORK_ERR_UNSUPPORTED_TARGET,
        "network is not supported on this target",
    );
    false
}

#[cfg(target_os = "wasi")]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_recv(
    _ptr: *const u8,
    _len: i64,
    _port: i32,
    _max_bytes: i32,
) -> *const u8 {
    rune_rt_network_set_error(
        RUNE_NETWORK_ERR_UNSUPPORTED_TARGET,
        "network is not supported on this target",
    );
    rune_rt_store_string(String::new())
}

#[cfg(target_os = "wasi")]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_recv_timeout(
    _ptr: *const u8,
    _len: i64,
    _port: i32,
    _max_bytes: i32,
    _timeout_ms: i32,
) -> *const u8 {
    rune_rt_network_set_error(
        RUNE_NETWORK_ERR_UNSUPPORTED_TARGET,
        "network is not supported on this target",
    );
    rune_rt_store_string(String::new())
}

#[cfg(target_os = "wasi")]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_accept_once(
    _ptr: *const u8,
    _len: i64,
    _port: i32,
    _max_bytes: i32,
    _timeout_ms: i32,
) -> *const u8 {
    rune_rt_network_set_error(
        RUNE_NETWORK_ERR_UNSUPPORTED_TARGET,
        "network is not supported on this target",
    );
    rune_rt_store_string(String::new())
}

#[cfg(target_os = "wasi")]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_reply_once(
    _host_ptr: *const u8,
    _host_len: i64,
    _port: i32,
    _data_ptr: *const u8,
    _data_len: i64,
    _max_bytes: i32,
    _timeout_ms: i32,
) -> *const u8 {
    rune_rt_network_set_error(
        RUNE_NETWORK_ERR_UNSUPPORTED_TARGET,
        "network is not supported on this target",
    );
    rune_rt_store_string(String::new())
}

#[cfg(target_os = "wasi")]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_server_open(_ptr: *const u8, _len: i64, _port: i32) -> i32 {
    rune_rt_network_set_error(
        RUNE_NETWORK_ERR_UNSUPPORTED_TARGET,
        "network is not supported on this target",
    );
    0
}

#[cfg(target_os = "wasi")]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_client_open(
    _ptr: *const u8,
    _len: i64,
    _port: i32,
    _timeout_ms: i32,
) -> i32 {
    rune_rt_network_set_error(
        RUNE_NETWORK_ERR_UNSUPPORTED_TARGET,
        "network is not supported on this target",
    );
    0
}

#[cfg(target_os = "wasi")]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_server_accept(
    _handle: i32,
    _max_bytes: i32,
    _timeout_ms: i32,
) -> *const u8 {
    rune_rt_network_set_error(
        RUNE_NETWORK_ERR_UNSUPPORTED_TARGET,
        "network is not supported on this target",
    );
    rune_rt_store_string(String::new())
}

#[cfg(target_os = "wasi")]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_server_reply(
    _handle: i32,
    _data_ptr: *const u8,
    _data_len: i64,
    _max_bytes: i32,
    _timeout_ms: i32,
) -> *const u8 {
    rune_rt_network_set_error(
        RUNE_NETWORK_ERR_UNSUPPORTED_TARGET,
        "network is not supported on this target",
    );
    rune_rt_store_string(String::new())
}

#[cfg(target_os = "wasi")]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_server_close(_handle: i32) -> bool {
    rune_rt_network_set_error(
        RUNE_NETWORK_ERR_UNSUPPORTED_TARGET,
        "network is not supported on this target",
    );
    false
}

#[cfg(target_os = "wasi")]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_client_send(
    _handle: i32,
    _data_ptr: *const u8,
    _data_len: i64,
) -> bool {
    rune_rt_network_set_error(
        RUNE_NETWORK_ERR_UNSUPPORTED_TARGET,
        "network is not supported on this target",
    );
    false
}

#[cfg(target_os = "wasi")]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_client_recv(
    _handle: i32,
    _max_bytes: i32,
    _timeout_ms: i32,
) -> *const u8 {
    rune_rt_network_set_error(
        RUNE_NETWORK_ERR_UNSUPPORTED_TARGET,
        "network is not supported on this target",
    );
    rune_rt_store_string(String::new())
}

#[cfg(target_os = "wasi")]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_client_close(_handle: i32) -> bool {
    rune_rt_network_set_error(
        RUNE_NETWORK_ERR_UNSUPPORTED_TARGET,
        "network is not supported on this target",
    );
    false
}

#[cfg(target_os = "wasi")]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_request(
    _host_ptr: *const u8,
    _host_len: i64,
    _port: i32,
    _data_ptr: *const u8,
    _data_len: i64,
    _max_bytes: i32,
    _timeout_ms: i32,
) -> *const u8 {
    rune_rt_network_set_error(
        RUNE_NETWORK_ERR_UNSUPPORTED_TARGET,
        "network is not supported on this target",
    );
    rune_rt_store_string(String::new())
}

#[cfg(target_os = "wasi")]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_connect_timeout(_ptr: *const u8, _len: i64, _port: i32, _timeout_ms: i32) -> bool {
    rune_rt_network_set_error(
        RUNE_NETWORK_ERR_UNSUPPORTED_TARGET,
        "network is not supported on this target",
    );
    false
}

#[cfg(not(target_os = "wasi"))]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_connect(ptr: *const u8, len: i64, port: i32) -> bool {
    rune_rt_network_tcp_connect_timeout(ptr, len, port, 250)
}

#[cfg(not(target_os = "wasi"))]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_listen(ptr: *const u8, len: i64, port: i32) -> bool {
    if port < 0 || port > u16::MAX as i32 {
        rune_rt_network_set_error(
            RUNE_NETWORK_ERR_INVALID_ARGUMENT,
            format!("invalid TCP port `{port}`"),
        );
        return false;
    }
    let host = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let host = std::str::from_utf8(host).expect("TCP listen host must be valid UTF-8");
    match TcpListener::bind((host, port as u16)) {
        Ok(_) => {
            rune_rt_network_clear_error_state();
            true
        }
        Err(error) => {
            rune_rt_network_set_error(
                RUNE_NETWORK_ERR_BIND,
                format!("failed to listen on {host}:{port}: {error}"),
            );
            false
        }
    }
}

#[cfg(not(target_os = "wasi"))]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_send(
    host_ptr: *const u8,
    host_len: i64,
    port: i32,
    data_ptr: *const u8,
    data_len: i64,
) -> bool {
    if port < 0 || port > u16::MAX as i32 {
        rune_rt_network_set_error(
            RUNE_NETWORK_ERR_INVALID_ARGUMENT,
            format!("invalid TCP port `{port}`"),
        );
        return false;
    }
    let host = unsafe { std::slice::from_raw_parts(host_ptr, host_len as usize) };
    let host = std::str::from_utf8(host).expect("TCP send host must be valid UTF-8");
    let data = unsafe { std::slice::from_raw_parts(data_ptr, data_len as usize) };
    let data = std::str::from_utf8(data).expect("TCP send data must be valid UTF-8");
    match TcpStream::connect((host, port as u16)) {
        Ok(mut stream) => match std::io::Write::write_all(&mut stream, data.as_bytes()) {
            Ok(_) => {
                rune_rt_network_clear_error_state();
                true
            }
            Err(error) => {
                rune_rt_network_set_error(
                    RUNE_NETWORK_ERR_WRITE,
                    format!("failed to send TCP payload to {host}:{port}: {error}"),
                );
                false
            }
        },
        Err(error) => {
            rune_rt_network_set_error(
                RUNE_NETWORK_ERR_CONNECT,
                format!("failed to connect to {host}:{port}: {error}"),
            );
            false
        }
    }
}

#[cfg(not(target_os = "wasi"))]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_recv(
    ptr: *const u8,
    len: i64,
    port: i32,
    max_bytes: i32,
) -> *const u8 {
    rune_rt_network_tcp_recv_timeout(ptr, len, port, max_bytes, 250)
}

#[cfg(not(target_os = "wasi"))]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_recv_timeout(
    ptr: *const u8,
    len: i64,
    port: i32,
    max_bytes: i32,
    timeout_ms: i32,
) -> *const u8 {
    if port < 0 || port > u16::MAX as i32 || max_bytes < 0 || timeout_ms < 0 {
        rune_rt_network_set_error(
            RUNE_NETWORK_ERR_INVALID_ARGUMENT,
            "tcp_recv_timeout requires non-negative port, max_bytes, and timeout_ms",
        );
        return rune_rt_store_string(String::new());
    }
    let host = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let host = std::str::from_utf8(host).expect("TCP recv host must be valid UTF-8");
    let address = format!("{host}:{}", port as u16);
    let resolved = match address.to_socket_addrs() {
        Ok(addrs) => addrs.collect::<Vec<SocketAddr>>(),
        Err(error) => {
            rune_rt_network_set_error(
                RUNE_NETWORK_ERR_ADDRESS_RESOLUTION,
                format!("failed to resolve {address}: {error}"),
            );
            return rune_rt_store_string(String::new());
        }
    };
    for addr in resolved {
        if let Ok(mut stream) =
            TcpStream::connect_timeout(&addr, Duration::from_millis(timeout_ms as u64))
        {
            if let Err(error) = stream.set_read_timeout(Some(Duration::from_millis(timeout_ms as u64)))
            {
                rune_rt_network_set_error(
                    RUNE_NETWORK_ERR_SOCKET_OPTION,
                    format!("failed to set TCP read timeout for {address}: {error}"),
                );
                return rune_rt_store_string(String::new());
            }
            let mut buffer = vec![0u8; max_bytes as usize];
            match std::io::Read::read(&mut stream, &mut buffer) {
                Ok(read) => {
                    buffer.truncate(read);
                    let text = String::from_utf8_lossy(&buffer).to_string();
                    rune_rt_network_clear_error_state();
                    return rune_rt_store_string(text);
                }
                Err(error) => {
                    rune_rt_network_set_error(
                        RUNE_NETWORK_ERR_READ,
                        format!("failed to read TCP payload from {address}: {error}"),
                    );
                    return rune_rt_store_string(String::new());
                }
            }
        }
    }
    rune_rt_network_set_error(
        RUNE_NETWORK_ERR_CONNECT,
        format!("failed to connect to {address} within {timeout_ms}ms"),
    );
    rune_rt_store_string(String::new())
}

#[cfg(not(target_os = "wasi"))]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_accept_once(
    ptr: *const u8,
    len: i64,
    port: i32,
    max_bytes: i32,
    timeout_ms: i32,
) -> *const u8 {
    if port < 0 || port > u16::MAX as i32 || max_bytes < 0 || timeout_ms < 0 {
        rune_rt_network_set_error(
            RUNE_NETWORK_ERR_INVALID_ARGUMENT,
            "tcp_accept_once requires non-negative port, max_bytes, and timeout_ms",
        );
        return rune_rt_store_string(String::new());
    }
    let host = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let host = std::str::from_utf8(host).expect("TCP accept host must be valid UTF-8");
    let listener = match TcpListener::bind((host, port as u16)) {
        Ok(listener) => listener,
        Err(error) => {
            rune_rt_network_set_error(
                RUNE_NETWORK_ERR_BIND,
                format!("failed to bind TCP listener on {host}:{port}: {error}"),
            );
            return rune_rt_store_string(String::new());
        }
    };
    if listener.set_nonblocking(true).is_err() {
        rune_rt_network_set_error(
            RUNE_NETWORK_ERR_SOCKET_OPTION,
            format!("failed to set TCP listener on {host}:{port} nonblocking"),
        );
        return rune_rt_store_string(String::new());
    }
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms as u64);
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_read_timeout(Some(Duration::from_millis(timeout_ms as u64)));
                let mut buffer = vec![0u8; max_bytes as usize];
                return match std::io::Read::read(&mut stream, &mut buffer) {
                    Ok(read) => {
                        buffer.truncate(read);
                        rune_rt_network_clear_error_state();
                        rune_rt_store_string(String::from_utf8_lossy(&buffer).to_string())
                    }
                    Err(error) => {
                        rune_rt_network_set_error(
                            RUNE_NETWORK_ERR_READ,
                            format!("failed to read accepted TCP payload on {host}:{port}: {error}"),
                        );
                        rune_rt_store_string(String::new())
                    }
                };
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    rune_rt_network_set_error(
                        RUNE_NETWORK_ERR_ACCEPT_TIMEOUT,
                        format!("timed out waiting for TCP client on {host}:{port}"),
                    );
                    return rune_rt_store_string(String::new());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => {
                rune_rt_network_set_error(
                    RUNE_NETWORK_ERR_ACCEPT,
                    format!("failed to accept TCP client on {host}:{port}: {error}"),
                );
                return rune_rt_store_string(String::new());
            }
        }
    }
}

#[cfg(not(target_os = "wasi"))]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_reply_once(
    host_ptr: *const u8,
    host_len: i64,
    port: i32,
    data_ptr: *const u8,
    data_len: i64,
    max_bytes: i32,
    timeout_ms: i32,
) -> *const u8 {
    if port < 0 || port > u16::MAX as i32 || max_bytes < 0 || timeout_ms < 0 {
        rune_rt_network_set_error(
            RUNE_NETWORK_ERR_INVALID_ARGUMENT,
            "tcp_reply_once requires non-negative port, max_bytes, and timeout_ms",
        );
        return rune_rt_store_string(String::new());
    }
    let host = unsafe { std::slice::from_raw_parts(host_ptr, host_len as usize) };
    let host = std::str::from_utf8(host).expect("TCP reply host must be valid UTF-8");
    let data = unsafe { std::slice::from_raw_parts(data_ptr, data_len as usize) };
    let data = std::str::from_utf8(data).expect("TCP reply data must be valid UTF-8");
    let listener = match TcpListener::bind((host, port as u16)) {
        Ok(listener) => listener,
        Err(error) => {
            rune_rt_network_set_error(
                RUNE_NETWORK_ERR_BIND,
                format!("failed to bind TCP reply listener on {host}:{port}: {error}"),
            );
            return rune_rt_store_string(String::new());
        }
    };
    if listener.set_nonblocking(true).is_err() {
        rune_rt_network_set_error(
            RUNE_NETWORK_ERR_SOCKET_OPTION,
            format!("failed to set TCP reply listener on {host}:{port} nonblocking"),
        );
        return rune_rt_store_string(String::new());
    }
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms as u64);
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_read_timeout(Some(Duration::from_millis(timeout_ms as u64)));
                let mut buffer = vec![0u8; max_bytes as usize];
                let request = match std::io::Read::read(&mut stream, &mut buffer) {
                    Ok(read) => {
                        buffer.truncate(read);
                        String::from_utf8_lossy(&buffer).to_string()
                    }
                    Err(error) => {
                        rune_rt_network_set_error(
                            RUNE_NETWORK_ERR_READ,
                            format!("failed to read TCP request on {host}:{port}: {error}"),
                        );
                        return rune_rt_store_string(String::new());
                    }
                };
                if let Err(error) = std::io::Write::write_all(&mut stream, data.as_bytes()) {
                    rune_rt_network_set_error(
                        RUNE_NETWORK_ERR_WRITE,
                        format!("failed to write TCP reply on {host}:{port}: {error}"),
                    );
                    return rune_rt_store_string(String::new());
                }
                rune_rt_network_clear_error_state();
                return rune_rt_store_string(request);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    rune_rt_network_set_error(
                        RUNE_NETWORK_ERR_ACCEPT_TIMEOUT,
                        format!("timed out waiting for TCP client on {host}:{port}"),
                    );
                    return rune_rt_store_string(String::new());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => {
                rune_rt_network_set_error(
                    RUNE_NETWORK_ERR_ACCEPT,
                    format!("failed to accept TCP client on {host}:{port}: {error}"),
                );
                return rune_rt_store_string(String::new());
            }
        }
    }
}

#[cfg(not(target_os = "wasi"))]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_server_open(ptr: *const u8, len: i64, port: i32) -> i32 {
    if port < 0 || port > u16::MAX as i32 {
        rune_rt_network_set_error(
            RUNE_NETWORK_ERR_INVALID_ARGUMENT,
            format!("invalid TCP port `{port}`"),
        );
        return 0;
    }
    let host = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let host = std::str::from_utf8(host).expect("TCP server host must be valid UTF-8");
    let listener = match TcpListener::bind((host, port as u16)) {
        Ok(listener) => listener,
        Err(error) => {
            rune_rt_network_set_error(
                RUNE_NETWORK_ERR_BIND,
                format!("failed to bind TCP server on {host}:{port}: {error}"),
            );
            return 0;
        }
    };
    if let Err(error) = listener.set_nonblocking(true) {
        rune_rt_network_set_error(
            RUNE_NETWORK_ERR_SOCKET_OPTION,
            format!("failed to configure TCP server on {host}:{port}: {error}"),
        );
        return 0;
    }
    let handle = RUNE_NETWORK_SERVER_NEXT_HANDLE.fetch_add(1, Ordering::Relaxed) as i32;
    rune_rt_network_server_handles()
        .lock()
        .expect("network server handle mutex poisoned")
        .insert(handle, listener);
    rune_rt_network_clear_error_state();
    handle
}

#[cfg(not(target_os = "wasi"))]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_client_open(
    ptr: *const u8,
    len: i64,
    port: i32,
    timeout_ms: i32,
) -> i32 {
    if port < 0 || port > u16::MAX as i32 || timeout_ms < 0 {
        rune_rt_network_set_error(
            RUNE_NETWORK_ERR_INVALID_ARGUMENT,
            "tcp_client_open requires a valid port and non-negative timeout_ms",
        );
        return 0;
    }
    let host = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let host = std::str::from_utf8(host).expect("TCP client host must be valid UTF-8");
    let address = format!("{host}:{}", port as u16);
    let resolved = match address.to_socket_addrs() {
        Ok(addrs) => addrs.collect::<Vec<SocketAddr>>(),
        Err(error) => {
            rune_rt_network_set_error(
                RUNE_NETWORK_ERR_ADDRESS_RESOLUTION,
                format!("failed to resolve {address}: {error}"),
            );
            return 0;
        }
    };
    for addr in resolved {
        match TcpStream::connect_timeout(&addr, Duration::from_millis(timeout_ms as u64)) {
            Ok(stream) => {
                let handle = RUNE_NETWORK_CLIENT_NEXT_HANDLE.fetch_add(1, Ordering::Relaxed) as i32;
                rune_rt_network_client_handles()
                    .lock()
                    .expect("network client handle mutex poisoned")
                    .insert(handle, stream);
                rune_rt_network_clear_error_state();
                return handle;
            }
            Err(_) => continue,
        }
    }
    rune_rt_network_set_error(
        RUNE_NETWORK_ERR_CONNECT,
        format!("failed to connect to {address} within {timeout_ms}ms"),
    );
    0
}

#[cfg(not(target_os = "wasi"))]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_server_accept(
    handle: i32,
    max_bytes: i32,
    timeout_ms: i32,
) -> *const u8 {
    if handle <= 0 || max_bytes < 0 || timeout_ms < 0 {
        rune_rt_network_set_error(
            RUNE_NETWORK_ERR_INVALID_ARGUMENT,
            "tcp_server_accept requires a positive handle and non-negative max_bytes/timeout_ms",
        );
        return rune_rt_store_string(String::new());
    }
    let listener: TcpListener = {
        let handles = rune_rt_network_server_handles()
            .lock()
            .expect("network server handle mutex poisoned");
        let Some(listener) = handles.get(&handle) else {
            rune_rt_network_set_error(
                RUNE_NETWORK_ERR_INVALID_ARGUMENT,
                format!("unknown TCP server handle `{handle}`"),
            );
            return rune_rt_store_string(String::new());
        };
        match listener.try_clone() {
            Ok(listener) => listener,
            Err(error) => {
                rune_rt_network_set_error(
                    RUNE_NETWORK_ERR_ACCEPT,
                    format!("failed to clone TCP listener for handle `{handle}`: {error}"),
                );
                return rune_rt_store_string(String::new());
            }
        }
    };
    let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                if let Err(error) =
                    stream.set_read_timeout(Some(Duration::from_millis(timeout_ms as u64)))
                {
                    rune_rt_network_set_error(
                        RUNE_NETWORK_ERR_SOCKET_OPTION,
                        format!("failed to set TCP read timeout for handle `{handle}`: {error}"),
                    );
                    return rune_rt_store_string(String::new());
                }
                let mut buffer = vec![0u8; max_bytes as usize];
                match std::io::Read::read(&mut stream, &mut buffer) {
                    Ok(read) => {
                        buffer.truncate(read);
                        rune_rt_network_clear_error_state();
                        return rune_rt_store_string(String::from_utf8_lossy(&buffer).to_string());
                    }
                    Err(error) => {
                        rune_rt_network_set_error(
                            RUNE_NETWORK_ERR_READ,
                            format!("failed to read from TCP server handle `{handle}`: {error}"),
                        );
                        return rune_rt_store_string(String::new());
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    rune_rt_network_set_error(
                        RUNE_NETWORK_ERR_ACCEPT_TIMEOUT,
                        format!("timed out waiting for TCP client on handle `{handle}`"),
                    );
                    return rune_rt_store_string(String::new());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => {
                rune_rt_network_set_error(
                    RUNE_NETWORK_ERR_ACCEPT,
                    format!("failed to accept TCP client on handle `{handle}`: {error}"),
                );
                return rune_rt_store_string(String::new());
            }
        }
    }
}

#[cfg(not(target_os = "wasi"))]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_server_reply(
    handle: i32,
    data_ptr: *const u8,
    data_len: i64,
    max_bytes: i32,
    timeout_ms: i32,
) -> *const u8 {
    if handle <= 0 || max_bytes < 0 || timeout_ms < 0 {
        rune_rt_network_set_error(
            RUNE_NETWORK_ERR_INVALID_ARGUMENT,
            "tcp_server_reply requires a positive handle and non-negative max_bytes/timeout_ms",
        );
        return rune_rt_store_string(String::new());
    }
    let listener: TcpListener = {
        let handles = rune_rt_network_server_handles()
            .lock()
            .expect("network server handle mutex poisoned");
        let Some(listener) = handles.get(&handle) else {
            rune_rt_network_set_error(
                RUNE_NETWORK_ERR_INVALID_ARGUMENT,
                format!("unknown TCP server handle `{handle}`"),
            );
            return rune_rt_store_string(String::new());
        };
        match listener.try_clone() {
            Ok(listener) => listener,
            Err(error) => {
                rune_rt_network_set_error(
                    RUNE_NETWORK_ERR_ACCEPT,
                    format!("failed to clone TCP listener for handle `{handle}`: {error}"),
                );
                return rune_rt_store_string(String::new());
            }
        }
    };
    let data = unsafe { std::slice::from_raw_parts(data_ptr, data_len as usize) };
    let data = std::str::from_utf8(data).expect("TCP server reply data must be valid UTF-8");
    let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                if let Err(error) =
                    stream.set_read_timeout(Some(Duration::from_millis(timeout_ms as u64)))
                {
                    rune_rt_network_set_error(
                        RUNE_NETWORK_ERR_SOCKET_OPTION,
                        format!("failed to set TCP read timeout for handle `{handle}`: {error}"),
                    );
                    return rune_rt_store_string(String::new());
                }
                let mut buffer = vec![0u8; max_bytes as usize];
                let request = match std::io::Read::read(&mut stream, &mut buffer) {
                    Ok(read) => {
                        buffer.truncate(read);
                        String::from_utf8_lossy(&buffer).to_string()
                    }
                    Err(error) => {
                        rune_rt_network_set_error(
                            RUNE_NETWORK_ERR_READ,
                            format!("failed to read from TCP server handle `{handle}`: {error}"),
                        );
                        return rune_rt_store_string(String::new());
                    }
                };
                if let Err(error) = std::io::Write::write_all(&mut stream, data.as_bytes()) {
                    rune_rt_network_set_error(
                        RUNE_NETWORK_ERR_WRITE,
                        format!("failed to write reply on TCP server handle `{handle}`: {error}"),
                    );
                    return rune_rt_store_string(String::new());
                }
                rune_rt_network_clear_error_state();
                return rune_rt_store_string(request);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    rune_rt_network_set_error(
                        RUNE_NETWORK_ERR_ACCEPT_TIMEOUT,
                        format!("timed out waiting for TCP client on handle `{handle}`"),
                    );
                    return rune_rt_store_string(String::new());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => {
                rune_rt_network_set_error(
                    RUNE_NETWORK_ERR_ACCEPT,
                    format!("failed to accept TCP client on handle `{handle}`: {error}"),
                );
                return rune_rt_store_string(String::new());
            }
        }
    }
}

#[cfg(not(target_os = "wasi"))]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_server_close(handle: i32) -> bool {
    if handle <= 0 {
        rune_rt_network_set_error(
            RUNE_NETWORK_ERR_INVALID_ARGUMENT,
            format!("invalid TCP server handle `{handle}`"),
        );
        return false;
    }
    let removed = rune_rt_network_server_handles()
        .lock()
        .expect("network server handle mutex poisoned")
        .remove(&handle)
        .is_some();
    if removed {
        rune_rt_network_clear_error_state();
        true
    } else {
        rune_rt_network_set_error(
            RUNE_NETWORK_ERR_INVALID_ARGUMENT,
            format!("unknown TCP server handle `{handle}`"),
        );
        false
    }
}

#[cfg(not(target_os = "wasi"))]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_client_send(
    handle: i32,
    data_ptr: *const u8,
    data_len: i64,
) -> bool {
    if handle <= 0 {
        rune_rt_network_set_error(
            RUNE_NETWORK_ERR_INVALID_ARGUMENT,
            format!("invalid TCP client handle `{handle}`"),
        );
        return false;
    }
    let data = unsafe { std::slice::from_raw_parts(data_ptr, data_len as usize) };
    let data = std::str::from_utf8(data).expect("TCP client send data must be valid UTF-8");
    let mut handles = rune_rt_network_client_handles()
        .lock()
        .expect("network client handle mutex poisoned");
    let Some(stream) = handles.get_mut(&handle) else {
        rune_rt_network_set_error(
            RUNE_NETWORK_ERR_INVALID_ARGUMENT,
            format!("unknown TCP client handle `{handle}`"),
        );
        return false;
    };
    match std::io::Write::write_all(stream, data.as_bytes()) {
        Ok(_) => {
            rune_rt_network_clear_error_state();
            true
        }
        Err(error) => {
            rune_rt_network_set_error(
                RUNE_NETWORK_ERR_WRITE,
                format!("failed to write to TCP client handle `{handle}`: {error}"),
            );
            false
        }
    }
}

#[cfg(not(target_os = "wasi"))]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_client_recv(
    handle: i32,
    max_bytes: i32,
    timeout_ms: i32,
) -> *const u8 {
    if handle <= 0 || max_bytes < 0 || timeout_ms < 0 {
        rune_rt_network_set_error(
            RUNE_NETWORK_ERR_INVALID_ARGUMENT,
            "tcp_client_recv requires a positive handle and non-negative max_bytes/timeout_ms",
        );
        return rune_rt_store_string(String::new());
    }
    let mut handles = rune_rt_network_client_handles()
        .lock()
        .expect("network client handle mutex poisoned");
    let Some(stream) = handles.get_mut(&handle) else {
        rune_rt_network_set_error(
            RUNE_NETWORK_ERR_INVALID_ARGUMENT,
            format!("unknown TCP client handle `{handle}`"),
        );
        return rune_rt_store_string(String::new());
    };
    if let Err(error) = stream.set_read_timeout(Some(Duration::from_millis(timeout_ms as u64))) {
        rune_rt_network_set_error(
            RUNE_NETWORK_ERR_SOCKET_OPTION,
            format!("failed to set TCP read timeout for client handle `{handle}`: {error}"),
        );
        return rune_rt_store_string(String::new());
    }
    let mut buffer = vec![0u8; max_bytes as usize];
    match std::io::Read::read(stream, &mut buffer) {
        Ok(read) => {
            buffer.truncate(read);
            rune_rt_network_clear_error_state();
            rune_rt_store_string(String::from_utf8_lossy(&buffer).to_string())
        }
        Err(error) => {
            rune_rt_network_set_error(
                RUNE_NETWORK_ERR_READ,
                format!("failed to read from TCP client handle `{handle}`: {error}"),
            );
            rune_rt_store_string(String::new())
        }
    }
}

#[cfg(not(target_os = "wasi"))]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_client_close(handle: i32) -> bool {
    if handle <= 0 {
        rune_rt_network_set_error(
            RUNE_NETWORK_ERR_INVALID_ARGUMENT,
            format!("invalid TCP client handle `{handle}`"),
        );
        return false;
    }
    let removed = rune_rt_network_client_handles()
        .lock()
        .expect("network client handle mutex poisoned")
        .remove(&handle)
        .is_some();
    if removed {
        rune_rt_network_clear_error_state();
        true
    } else {
        rune_rt_network_set_error(
            RUNE_NETWORK_ERR_INVALID_ARGUMENT,
            format!("unknown TCP client handle `{handle}`"),
        );
        false
    }
}

#[cfg(not(target_os = "wasi"))]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_request(
    host_ptr: *const u8,
    host_len: i64,
    port: i32,
    data_ptr: *const u8,
    data_len: i64,
    max_bytes: i32,
    timeout_ms: i32,
) -> *const u8 {
    if port < 0 || port > u16::MAX as i32 || max_bytes < 0 || timeout_ms < 0 {
        rune_rt_network_set_error(
            RUNE_NETWORK_ERR_INVALID_ARGUMENT,
            "tcp_request requires non-negative port, max_bytes, and timeout_ms",
        );
        return rune_rt_store_string(String::new());
    }
    let host = unsafe { std::slice::from_raw_parts(host_ptr, host_len as usize) };
    let host = std::str::from_utf8(host).expect("TCP request host must be valid UTF-8");
    let data = unsafe { std::slice::from_raw_parts(data_ptr, data_len as usize) };
    let data = std::str::from_utf8(data).expect("TCP request data must be valid UTF-8");
    let address = format!("{host}:{}", port as u16);
    let resolved = match address.to_socket_addrs() {
        Ok(addrs) => addrs.collect::<Vec<SocketAddr>>(),
        Err(error) => {
            rune_rt_network_set_error(
                RUNE_NETWORK_ERR_ADDRESS_RESOLUTION,
                format!("failed to resolve {address}: {error}"),
            );
            return rune_rt_store_string(String::new());
        }
    };
    for addr in resolved {
        if let Ok(mut stream) =
            TcpStream::connect_timeout(&addr, Duration::from_millis(timeout_ms as u64))
        {
            if let Err(error) = stream.set_read_timeout(Some(Duration::from_millis(timeout_ms as u64)))
            {
                rune_rt_network_set_error(
                    RUNE_NETWORK_ERR_SOCKET_OPTION,
                    format!("failed to set TCP read timeout for {address}: {error}"),
                );
                return rune_rt_store_string(String::new());
            }
            if let Err(error) = std::io::Write::write_all(&mut stream, data.as_bytes()) {
                rune_rt_network_set_error(
                    RUNE_NETWORK_ERR_WRITE,
                    format!("failed to write TCP request to {address}: {error}"),
                );
                return rune_rt_store_string(String::new());
            }
            let _ = std::net::Shutdown::Write;
            let _ = stream.shutdown(std::net::Shutdown::Write);
            let mut buffer = vec![0u8; max_bytes as usize];
            match std::io::Read::read(&mut stream, &mut buffer) {
                Ok(read) => {
                    buffer.truncate(read);
                    let text = String::from_utf8_lossy(&buffer).to_string();
                    rune_rt_network_clear_error_state();
                    return rune_rt_store_string(text);
                }
                Err(error) => {
                    rune_rt_network_set_error(
                        RUNE_NETWORK_ERR_READ,
                        format!("failed to read TCP response from {address}: {error}"),
                    );
                    return rune_rt_store_string(String::new());
                }
            }
        }
    }
    rune_rt_network_set_error(
        RUNE_NETWORK_ERR_CONNECT,
        format!("failed to connect to {address} within {timeout_ms}ms"),
    );
    rune_rt_store_string(String::new())
}

#[cfg(not(target_os = "wasi"))]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_tcp_connect_timeout(ptr: *const u8, len: i64, port: i32, timeout_ms: i32) -> bool {
    if port < 0 || port > u16::MAX as i32 {
        rune_rt_network_set_error(
            RUNE_NETWORK_ERR_INVALID_ARGUMENT,
            format!("invalid TCP port `{port}`"),
        );
        return false;
    }
    if timeout_ms < 0 {
        rune_rt_network_set_error(
            RUNE_NETWORK_ERR_INVALID_ARGUMENT,
            format!("invalid TCP timeout `{timeout_ms}`"),
        );
        return false;
    }
    let host = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let host = std::str::from_utf8(host).expect("TCP host must be valid UTF-8");
    let address = format!("{host}:{}", port as u16);
    let resolved = match address.to_socket_addrs() {
        Ok(addrs) => addrs.collect::<Vec<SocketAddr>>(),
        Err(error) => {
            rune_rt_network_set_error(
                RUNE_NETWORK_ERR_ADDRESS_RESOLUTION,
                format!("failed to resolve {address}: {error}"),
            );
            return false;
        }
    };
    if resolved.into_iter().any(|addr| {
        TcpStream::connect_timeout(&addr, Duration::from_millis(timeout_ms as u64)).is_ok()
    }) {
        rune_rt_network_clear_error_state();
        true
    } else {
        rune_rt_network_set_error(
            RUNE_NETWORK_ERR_CONNECT,
            format!("failed to connect to {address} within {timeout_ms}ms"),
        );
        false
    }
}

#[cfg(target_os = "wasi")]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_udp_bind(_ptr: *const u8, _len: i64, _port: i32) -> bool {
    rune_rt_network_set_error(
        RUNE_NETWORK_ERR_UNSUPPORTED_TARGET,
        "network is not supported on this target",
    );
    false
}

#[cfg(not(target_os = "wasi"))]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_udp_bind(ptr: *const u8, len: i64, port: i32) -> bool {
    if port < 0 || port > u16::MAX as i32 {
        rune_rt_network_set_error(
            RUNE_NETWORK_ERR_INVALID_ARGUMENT,
            format!("invalid UDP port `{port}`"),
        );
        return false;
    }
    let host = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let host = std::str::from_utf8(host).expect("UDP bind host must be valid UTF-8");
    match UdpSocket::bind((host, port as u16)) {
        Ok(_) => {
            rune_rt_network_clear_error_state();
            true
        }
        Err(error) => {
            rune_rt_network_set_error(
                RUNE_NETWORK_ERR_BIND,
                format!("failed to bind UDP socket on {host}:{port}: {error}"),
            );
            false
        }
    }
}

#[cfg(target_os = "wasi")]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_udp_send(
    _host_ptr: *const u8,
    _host_len: i64,
    _port: i32,
    _data_ptr: *const u8,
    _data_len: i64,
) -> bool {
    rune_rt_network_set_error(
        RUNE_NETWORK_ERR_UNSUPPORTED_TARGET,
        "network is not supported on this target",
    );
    false
}

#[cfg(target_os = "wasi")]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_udp_recv(
    _ptr: *const u8,
    _len: i64,
    _port: i32,
    _max_bytes: i32,
    _timeout_ms: i32,
) -> *const u8 {
    rune_rt_network_set_error(
        RUNE_NETWORK_ERR_UNSUPPORTED_TARGET,
        "network is not supported on this target",
    );
    rune_rt_store_string(String::new())
}

#[cfg(not(target_os = "wasi"))]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_udp_send(
    host_ptr: *const u8,
    host_len: i64,
    port: i32,
    data_ptr: *const u8,
    data_len: i64,
) -> bool {
    if port < 0 || port > u16::MAX as i32 {
        rune_rt_network_set_error(
            RUNE_NETWORK_ERR_INVALID_ARGUMENT,
            format!("invalid UDP port `{port}`"),
        );
        return false;
    }
    let host = unsafe { std::slice::from_raw_parts(host_ptr, host_len as usize) };
    let host = std::str::from_utf8(host).expect("UDP send host must be valid UTF-8");
    let data = unsafe { std::slice::from_raw_parts(data_ptr, data_len as usize) };
    let data = std::str::from_utf8(data).expect("UDP send data must be valid UTF-8");
    match UdpSocket::bind(("0.0.0.0", 0)) {
        Ok(socket) => match socket.send_to(data.as_bytes(), (host, port as u16)) {
            Ok(_) => {
                rune_rt_network_clear_error_state();
                true
            }
            Err(error) => {
                rune_rt_network_set_error(
                    RUNE_NETWORK_ERR_WRITE,
                    format!("failed to send UDP payload to {host}:{port}: {error}"),
                );
                false
            }
        },
        Err(error) => {
            rune_rt_network_set_error(
                RUNE_NETWORK_ERR_BIND,
                format!("failed to allocate UDP socket: {error}"),
            );
            false
        }
    }
}

#[cfg(not(target_os = "wasi"))]
#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_network_udp_recv(
    ptr: *const u8,
    len: i64,
    port: i32,
    max_bytes: i32,
    timeout_ms: i32,
) -> *const u8 {
    if port < 0 || port > u16::MAX as i32 || max_bytes < 0 || timeout_ms < 0 {
        rune_rt_network_set_error(
            RUNE_NETWORK_ERR_INVALID_ARGUMENT,
            "udp_recv requires non-negative port, max_bytes, and timeout_ms",
        );
        return rune_rt_store_string(String::new());
    }
    let host = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let host = std::str::from_utf8(host).expect("UDP recv host must be valid UTF-8");
    match UdpSocket::bind((host, port as u16)) {
        Ok(socket) => {
            if let Err(error) = socket.set_read_timeout(Some(Duration::from_millis(timeout_ms as u64))) {
                rune_rt_network_set_error(
                    RUNE_NETWORK_ERR_SOCKET_OPTION,
                    format!("failed to set UDP read timeout for {host}:{port}: {error}"),
                );
                return rune_rt_store_string(String::new());
            }
            let mut buffer = vec![0u8; max_bytes as usize];
            match socket.recv_from(&mut buffer) {
                Ok((read, _)) => {
                    buffer.truncate(read);
                    rune_rt_network_clear_error_state();
                    rune_rt_store_string(String::from_utf8_lossy(&buffer).to_string())
                }
                Err(error) => {
                    rune_rt_network_set_error(
                        RUNE_NETWORK_ERR_READ,
                        format!("failed to receive UDP payload on {host}:{port}: {error}"),
                    );
                    rune_rt_store_string(String::new())
                }
            }
        }
        Err(error) => {
            rune_rt_network_set_error(
                RUNE_NETWORK_ERR_BIND,
                format!("failed to bind UDP socket on {host}:{port}: {error}"),
            );
            rune_rt_store_string(String::new())
        }
    }
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
pub extern "C" fn rune_rt_fs_remove(ptr: *const u8, len: i64) -> bool {
    let path = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let path = std::str::from_utf8(path).expect("filesystem path must be valid UTF-8");
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path).is_ok(),
        Ok(_) => fs::remove_file(path).is_ok(),
        Err(_) => false,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_fs_rename(
    from_ptr: *const u8,
    from_len: i64,
    to_ptr: *const u8,
    to_len: i64,
) -> bool {
    let from = unsafe { std::slice::from_raw_parts(from_ptr, from_len as usize) };
    let from = std::str::from_utf8(from).expect("filesystem source path must be valid UTF-8");
    let to = unsafe { std::slice::from_raw_parts(to_ptr, to_len as usize) };
    let to = std::str::from_utf8(to).expect("filesystem destination path must be valid UTF-8");
    fs::rename(from, to).is_ok()
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_fs_copy(
    from_ptr: *const u8,
    from_len: i64,
    to_ptr: *const u8,
    to_len: i64,
) -> bool {
    let from = unsafe { std::slice::from_raw_parts(from_ptr, from_len as usize) };
    let from = std::str::from_utf8(from).expect("filesystem source path must be valid UTF-8");
    let to = unsafe { std::slice::from_raw_parts(to_ptr, to_len as usize) };
    let to = std::str::from_utf8(to).expect("filesystem destination path must be valid UTF-8");
    fs::copy(from, to).is_ok()
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_fs_create_dir(ptr: *const u8, len: i64) -> bool {
    let path = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let path = std::str::from_utf8(path).expect("filesystem path must be valid UTF-8");
    fs::create_dir(path).is_ok()
}

#[unsafe(no_mangle)]
pub extern "C" fn rune_rt_fs_create_dir_all(ptr: *const u8, len: i64) -> bool {
    let path = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let path = std::str::from_utf8(path).expect("filesystem path must be valid UTF-8");
    fs::create_dir_all(path).is_ok()
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
    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
    let base = env::temp_dir();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let unique = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = base.join(format!("rune-build-{}-{stamp}-{unique}", std::process::id()));
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
